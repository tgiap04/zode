//! Acting on the selected rows, and the keys that move through them.
//!
//! Every function here works on the selection *as seen through the filters*:
//! a row the search or a quick filter is hiding is never acted on, however it
//! came to be selected. A bulk remove that quietly took in rows nobody could see
//! would be the one failure this panel cannot afford.

use std::collections::HashMap;
use std::sync::Arc;

use container::resource::row_key;
use container::{
    ContainerBackend, ContainerError, DestructivePlan, Resource, ResourceAction, ResourceKind,
};
use futures::StreamExt as _;
use gpui::{AnyElement, Context, ScrollStrategy, Task, Window};
use ui::prelude::*;

use crate::container_panel::{Busy, ContainerPanel, ListState};
use crate::render::{action_icon, action_is_useful};

/// How many rows of one bulk action have their command running at once.
///
/// Stopping twenty containers is twenty `docker stop`s of up to ten seconds
/// each: all at once is twenty processes on the engine's daemon and, for a
/// remote project, twenty ssh sessions. A few at a time finishes in about the
/// same time without that.
const BULK_CONCURRENCY: usize = 4;

/// The actions the selection bar offers, in the order it draws them.
const BAR_ACTIONS: [ResourceAction; 3] = [
    ResourceAction::Start,
    ResourceAction::Stop,
    ResourceAction::Restart,
];

impl ContainerPanel {
    /// The rows the list shows, in the order it shows them.
    pub(crate) fn visible_resources(&self, cx: &App) -> Vec<Resource> {
        match &self.state {
            ListState::Ready(listing) => self.visible(listing, cx).into_iter().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The keys of the rows the list shows, in the order it shows them.
    pub(crate) fn visible_keys(&self, cx: &App) -> Vec<String> {
        match &self.state {
            ListState::Ready(listing) => {
                self.visible(listing, cx).into_iter().map(row_key).collect()
            }
            _ => Vec::new(),
        }
    }

    /// The selected rows that are on screen, in display order.
    pub(crate) fn selected_resources(&self, cx: &App) -> Vec<Resource> {
        let ListState::Ready(listing) = &self.state else {
            return Vec::new();
        };
        if self.selection.is_empty() {
            return Vec::new();
        }
        // Only the selected rows are cloned: this runs on every frame a
        // selection exists, and a busy row's spinner draws every frame.
        self.visible(listing, cx)
            .into_iter()
            .filter(|resource| self.selection.contains(&row_key(resource)))
            .cloned()
            .collect()
    }

    /// Runs `action` on every row of `targets` it makes sense for and that is
    /// not already busy, a few at a time.
    ///
    /// `targets` is what the caller saw when it was built -- a menu can sit open
    /// for as long as somebody likes -- so each row is looked up in the list as
    /// it is now, and what it is doing now decides: a container that has since
    /// stopped is not restarted, and one that has gone is skipped.
    ///
    /// Several rows go through one task, so each still gets its own spinner and
    /// its own refusal, and one container the engine will not stop does not
    /// hide what happened to the rest.
    pub(crate) fn act_many(
        &mut self,
        action: ResourceAction,
        targets: &[Resource],
        cx: &mut Context<Self>,
    ) {
        let ListState::Ready(listing) = &self.state else {
            return;
        };
        let current: HashMap<String, &Resource> = listing
            .iter()
            .map(|resource| (row_key(resource), resource))
            .collect();
        let mut ids: Vec<String> = targets
            .iter()
            .filter_map(|target| current.get(&row_key(target)))
            .filter(|row| {
                self.busy_for(&row_key(row)).is_none() && action_is_useful(action, row.state)
            })
            .map(|row| row.id.clone())
            .collect();
        if ids.len() > 1 {
            self.act_batch(action, ids, cx);
        } else if let Some(id) = ids.pop() {
            self.act(action, id, cx);
        }
    }

    /// `action` on every one of `ids`, [`BULK_CONCURRENCY`] at a time, each row
    /// settling as its own command does.
    fn act_batch(&mut self, action: ResourceAction, ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        if !self.available_actions().contains(&action) {
            return;
        }
        let kind = self.active_kind;
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        self.mark_busy(ids.iter().cloned(), Busy::Action(action), action_id);
        self.last_error = None;
        cx.notify();
        self.actions.insert(
            action_id,
            cx.spawn(async move |this, cx| {
                let mut outcomes = futures::stream::iter(ids.into_iter().map(|id| {
                    let backend = backend.clone();
                    async move {
                        let outcome = backend.act(kind, action, &id).await;
                        (id, outcome)
                    }
                }))
                .buffer_unordered(BULK_CONCURRENCY);
                while let Some((id, outcome)) = outcomes.next().await {
                    let applied = this.update(cx, |this, cx| {
                        this.finish_busy(&[id], action_id, outcome.is_err());
                        if let Err(error) = outcome {
                            this.last_error = Some(error);
                        }
                        // As in `act`: the engine alone knows the resulting state.
                        this.reload(cx);
                        cx.notify();
                    });
                    if let Err(error) = applied {
                        log::debug!("panel closed before its bulk action finished: {error}");
                        return;
                    }
                }
                // See the matching comment in `act`: removing our own map entry
                // as the very last statement is safe.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its bulk action could self-remove: {error}");
                }
            }),
        );
    }

    /// The rows of `keys` that are on screen and not busy, in list order. Keys
    /// that are not in the list are dropped, so a plan can only ever name what
    /// the engine just listed -- and what a person could see: a row the search
    /// or a filter hides is not one they agreed to remove.
    ///
    /// The open row counts as on screen even when the filters would hide it: its
    /// own Remove button is drawn above the list, and the container may well
    /// have stopped, under a running-only filter, from that very view.
    pub(crate) fn removal_targets(&self, keys: &[String], cx: &App) -> Vec<Resource> {
        if !self.removable() {
            return Vec::new();
        }
        let ListState::Ready(listing) = &self.state else {
            return Vec::new();
        };
        let open = self.detail.as_ref().map(|detail| row_key(&detail.resource));
        let shown: Vec<String> = self.visible(listing, cx).into_iter().map(row_key).collect();
        listing
            .iter()
            .filter(|resource| {
                let key = row_key(resource);
                keys.contains(&key)
                    && (shown.contains(&key) || open.as_ref() == Some(&key))
                    && self.busy_for(&key).is_none()
            })
            .cloned()
            .collect()
    }

    /// One plan listing every row of `keys` that is on screen and not busy, with
    /// what depends on each of them, or `None` when there is nothing to remove.
    ///
    /// Runs nothing: whoever holds the plan still has to get it past the
    /// confirmation. Asked of the backend's `removal_plan` and not built here,
    /// so an image, volume or network that is still in use says by whom -- and a
    /// lookup that failed is an error, not a plan that quietly says nobody uses
    /// it.
    ///
    /// What a removal starts from is [`ContainerPanel::start_removal_of`], which
    /// does this in two steps so the rows can say they are being planned; this
    /// is the same thing in one, for tests of what ends up in a plan.
    #[cfg(test)]
    pub(crate) fn plan_removal_of(
        &self,
        keys: &[String],
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<DestructivePlan>, ContainerError>> {
        let targets = self.removal_targets(keys, cx);
        let Some(engine) = self.backend().cloned() else {
            return Task::ready(Ok(None));
        };
        self.plan_removal_for(engine, targets, cx)
    }

    /// The plan for `targets` on `engine`, asked in the background.
    pub(crate) fn plan_removal_for(
        &self,
        engine: Arc<dyn ContainerBackend>,
        targets: Vec<Resource>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<DestructivePlan>, ContainerError>> {
        if targets.is_empty() {
            return Task::ready(Ok(None));
        }
        let kind: ResourceKind = self.active_kind;
        cx.background_spawn(async move { engine.removal_plan(kind, targets).await })
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            cx.propagate();
            return;
        }
        self.selection.clear();
        cx.notify();
    }

    pub(crate) fn select_all_rows(&mut self, cx: &mut Context<Self>) {
        let order = self.visible_keys(cx);
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        self.selection.select_all(&order);
        cx.notify();
    }

    pub(crate) fn move_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        let order = self.visible_keys(cx);
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        if let Some(index) = self.selection.move_cursor(delta, &order) {
            self.list_scroll
                .read(cx)
                .scroll_handle
                .scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    pub(crate) fn toggle_cursor_row(&mut self, cx: &mut Context<Self>) {
        let Some(cursor) = self.selection.cursor().map(str::to_string) else {
            return;
        };
        self.selection.toggle(&cursor);
        cx.notify();
    }

    /// Enter: opens the row under the cursor, where the kind has a detail view.
    pub(crate) fn open_cursor_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.rows_open_a_detail() {
            return;
        }
        let Some(cursor) = self.selection.cursor() else {
            return;
        };
        let Some(resource) = self
            .visible_resources(cx)
            .into_iter()
            .find(|resource| row_key(resource) == cursor)
        else {
            return;
        };
        self.selection.clear();
        self.open_detail(resource, window, cx);
    }

    /// Delete: removes the selection, or the row under the cursor when nothing
    /// is selected. Opens the confirmation either way.
    pub(crate) fn remove_selected_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut keys: Vec<String> = self.selected_resources(cx).iter().map(row_key).collect();
        if keys.is_empty() {
            let visible = self.visible_keys(cx);
            keys.extend(
                self.selection
                    .cursor()
                    .filter(|cursor| visible.iter().any(|key| key == cursor))
                    .map(str::to_string),
            );
        }
        self.start_removal_of(keys, window, cx);
    }

    /// "3 selected" and what can be done to all of them at once, or `None` when
    /// nothing visible is selected.
    pub(crate) fn render_selection_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.detail.is_some() {
            return None;
        }
        let selected = self.selected_resources(cx);
        if selected.is_empty() {
            return None;
        }
        let supported = self.available_actions();
        let removable = self.removable();
        let count = selected.len();
        let busy = selected
            .iter()
            .all(|resource| self.busy_for(&row_key(resource)).is_some());

        Some(
            h_flex()
                .debug_selector(|| "container-selection-bar".into())
                .flex_shrink_0()
                .gap_1()
                .px_1p5()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().colors().border)
                .child(
                    Label::new(format!("{count} selected"))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .children(
                    BAR_ACTIONS
                        .into_iter()
                        .filter(|action| supported.contains(action))
                        .filter(|action| {
                            selected
                                .iter()
                                .any(|resource| action_is_useful(*action, resource.state))
                        })
                        .map(|action| {
                            let targets = selected.clone();
                            let label = action.label();
                            div()
                                .debug_selector(move || format!("container-bulk:{label}"))
                                .child(
                                    Button::new(
                                        SharedString::from(format!(
                                            "container-bulk-{}",
                                            action.label()
                                        )),
                                        action.label(),
                                    )
                                    .label_size(LabelSize::Small)
                                    .disabled(busy)
                                    .start_icon(
                                        Icon::new(action_icon(action))
                                            .size(IconSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _window, cx| {
                                            this.act_many(action, &targets, cx)
                                        },
                                    )),
                                )
                        }),
                )
                .when(removable, |bar| {
                    let keys: Vec<String> = selected.iter().map(row_key).collect();
                    bar.child(
                        div()
                            .debug_selector(|| "container-bulk:Remove".into())
                            .child(
                                Button::new("container-bulk-remove", "Remove\u{2026}")
                                    .label_size(LabelSize::Small)
                                    .disabled(busy)
                                    .start_icon(
                                        Icon::new(IconName::Trash)
                                            .size(IconSize::XSmall)
                                            .color(Color::Error),
                                    )
                                    // Opens the confirmation and nothing else: the plan it
                                    // builds lists every selected row, and only passing the
                                    // dialog runs it.
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.start_removal_of(keys.clone(), window, cx)
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .debug_selector(|| "container-bulk:Clear".into())
                        .child(
                            Button::new("container-bulk-clear", "Clear")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.selection.clear();
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
