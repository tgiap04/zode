//! The searchable list of places a checkout can be moved to.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task,
    WeakEntity, Window,
};
use picker::{Picker, PickerDelegate};
use ui::{ListItem, ListItemSpacing, prelude::*};

use crate::compose_checkout::{self, GitFailure, Target};
use crate::container_panel::ContainerPanel;

/// How long a fetch may take before the picker gives up on it and shows what it
/// already has. A hung network must not keep a dialog open for ever.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Where the picker is in reading the repository.
enum Status {
    Fetching,
    /// No remote configured: only worktrees can be offered.
    NoRemote,
    Ready,
    /// The fetch failed; what was fetched before is still listed. Worktrees do
    /// not need the network, so the picker stays usable.
    FetchFailed(String),
    ListFailed(String),
}

pub(crate) struct BranchPicker {
    picker: Entity<Picker<BranchPickerDelegate>>,
    /// Held so closing the dialog drops the load, which drops the `git` it is
    /// running: every command is spawned with `kill_on_drop`.
    _load: Task<()>,
}

impl EventEmitter<DismissEvent> for BranchPicker {}
impl workspace::ModalView for BranchPicker {}

impl Focusable for BranchPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for BranchPicker {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ContainerBranchPicker")
            .debug_selector(|| "container-branch-picker".into())
            .w(rems(34.))
            .child(self.picker.clone())
    }
}

impl BranchPicker {
    pub(crate) fn new(
        panel: WeakEntity<ContainerPanel>,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = BranchPickerDelegate {
            modal: cx.entity().downgrade(),
            panel,
            root: root.clone(),
            remote: None,
            targets: Vec::new(),
            matches: Vec::new(),
            selected_index: 0,
            status: Status::Fetching,
        };
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));
        let picker_for_load = picker.clone();
        let load = cx.spawn_in(window, async move |_this, cx| {
            let remote_root = root.clone();
            let remote = cx
                .background_spawn(
                    async move { compose_checkout::select_remote(&remote_root).await },
                )
                .await;
            let (remote, fetched) = match remote {
                Ok(Some(remote)) => {
                    let fetch_root = root.clone();
                    let fetch_remote = remote.clone();
                    let fetching = cx.background_spawn(async move {
                        compose_checkout::fetch(&fetch_root, &fetch_remote).await
                    });
                    let timer = cx.background_executor().timer(FETCH_TIMEOUT);
                    let fetched = compose_checkout::within(fetching, timer, "fetch").await;
                    (Some(remote), fetched.err())
                }
                Ok(None) => (None, None),
                Err(failure) => (None, Some(failure)),
            };
            let list_remote = remote.clone();
            let listed = cx
                .background_spawn(async move {
                    compose_checkout::list_targets(&root, list_remote.as_deref()).await
                })
                .await;
            let outcome = picker_for_load.update_in(cx, |picker, window, cx| {
                picker.delegate.finish(remote, fetched, listed);
                picker.refresh(window, cx);
            });
            if let Err(error) = outcome {
                log::debug!("branch picker closed before it finished loading: {error}");
            }
        });
        Self {
            picker,
            _load: load,
        }
    }

    /// What the list shows, in order, for tests.
    #[cfg(test)]
    pub(crate) fn listed_titles(&self, cx: &App) -> Vec<String> {
        self.picker
            .read(cx)
            .delegate
            .matches
            .iter()
            .map(Target::title)
            .collect()
    }

    /// Hands the load to the test, so it can await the git commands instead of
    /// guessing how long they take.
    #[cfg(test)]
    pub(crate) fn take_load(&mut self) -> Task<()> {
        std::mem::replace(&mut self._load, Task::ready(()))
    }

    #[cfg(test)]
    pub(crate) fn search(&self, query: &str, window: &mut Window, cx: &mut App) {
        self.picker
            .update(cx, |picker, cx| picker.set_query(query, window, cx));
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self, cx: &App) -> Option<String> {
        self.picker.read(cx).delegate.status_text()
    }

    #[cfg(test)]
    pub(crate) fn choose(&self, title: &str, window: &mut Window, cx: &mut App) {
        self.picker.update(cx, |picker, cx| {
            if let Some(index) = picker
                .delegate
                .matches
                .iter()
                .position(|target| target.title() == title)
            {
                picker.delegate.selected_index = index;
                picker.delegate.confirm(false, window, cx);
            }
        });
    }
}

struct BranchPickerDelegate {
    modal: WeakEntity<BranchPicker>,
    panel: WeakEntity<ContainerPanel>,
    /// The checkout that will move, shown so nobody has to guess which one.
    root: PathBuf,
    /// The remote actually used, once known.
    remote: Option<String>,
    targets: Vec<Target>,
    matches: Vec<Target>,
    selected_index: usize,
    status: Status,
}

impl BranchPickerDelegate {
    fn finish(
        &mut self,
        remote: Option<String>,
        fetch_failure: Option<GitFailure>,
        listed: Result<Vec<Target>, GitFailure>,
    ) {
        self.remote = remote;
        match listed {
            Ok(targets) => {
                self.targets = targets;
                self.status = match (&self.remote, fetch_failure) {
                    (None, _) => Status::NoRemote,
                    (Some(_), Some(failure)) => Status::FetchFailed(failure.to_string()),
                    (Some(_), None) => Status::Ready,
                };
            }
            Err(failure) => self.status = Status::ListFailed(failure.to_string()),
        }
    }

    fn remote_name(&self) -> &str {
        self.remote.as_deref().unwrap_or("the remote")
    }

    fn status_text(&self) -> Option<String> {
        match &self.status {
            Status::Fetching => Some("Fetching\u{2026}".to_string()),
            Status::NoRemote => {
                Some("This repository has no remote, so only worktrees are listed.".to_string())
            }
            Status::Ready => None,
            Status::FetchFailed(message) => Some(format!(
                "Could not fetch {}, showing what was fetched before: {message}",
                self.remote_name()
            )),
            Status::ListFailed(message) => Some(format!("Could not list branches: {message}")),
        }
    }
}

impl PickerDelegate for BranchPickerDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Switch to a remote branch or a worktree\u{2026}".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some(match self.status {
            Status::Fetching => "Fetching\u{2026}".into(),
            _ => "No branch or worktree matches".into(),
        })
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.matches = self
            .targets
            .iter()
            .filter(|target| {
                let haystack = match target {
                    Target::RemoteBranch { .. } => target.title(),
                    Target::Worktree { path, .. } => {
                        format!("{} {}", target.title(), path.display())
                    }
                }
                .to_lowercase();
                words.iter().all(|word| haystack.contains(word.as_str()))
            })
            .cloned()
            .collect();
        self.selected_index = 0;
        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(target) = self.matches.get(self.selected_index).cloned() else {
            return;
        };
        // The dialog is replaced by the confirmation, not dismissed first: the
        // confirmation opens only once the repository's state has been read,
        // and the picker staying up until then is what says something is
        // happening.
        if let Err(error) = self.panel.update(cx, |panel, cx| {
            panel.open_switch_confirmation(target, window, cx)
        }) {
            log::debug!("panel closed before the branch was confirmed: {error}");
            self.dismissed(window, cx);
        }
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Err(error) = self.modal.update(cx, |_, cx| cx.emit(DismissEvent)) {
            log::debug!("branch picker already gone: {error}");
        }
    }

    fn render_header(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<gpui::AnyElement> {
        let colour = match self.status {
            Status::Fetching | Status::NoRemote | Status::Ready => Color::Muted,
            Status::FetchFailed(_) => Color::Warning,
            Status::ListFailed(_) => Color::Error,
        };
        Some(
            v_flex()
                .px_2()
                .py_1()
                .gap_0p5()
                .child(
                    div()
                        .debug_selector(|| "container-branch-picker-root".into())
                        .child(
                            Label::new(format!("Moves {}", self.root.display()))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .children(self.status_text().map(|text| {
                    div()
                        .debug_selector(|| "container-branch-picker-status".into())
                        .child(Label::new(text).size(LabelSize::Small).color(colour))
                }))
                .into_any_element(),
        )
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let target = self.matches.get(ix)?;
        let icon = match target {
            Target::RemoteBranch { .. } => IconName::GitBranch,
            Target::Worktree { .. } => IconName::Folder,
        };
        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .start_slot(Icon::new(icon).size(IconSize::Small).color(Color::Muted))
                .child(Label::new(target.title()))
                .end_slot(
                    Label::new(target.kind_label())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
    }
}
