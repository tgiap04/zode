//! The three dialogs of "Switch Branch...": pick where to go (in
//! `branch_switch_picker`), confirm what will happen, and say why it did not.

use std::path::PathBuf;

use gpui::{
    App, ClickEvent, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, SharedString,
    WeakEntity, Window,
};
use ui::{Checkbox, KeyBinding, Modal, ModalFooter, ModalHeader, Section, TintColor, prelude::*};

use crate::branch_switch::{ProjectContainer, SwitchRequest};
use crate::compose_checkout::Target;
use crate::container_panel::ContainerPanel;

pub(crate) struct ConfirmBranchSwitch {
    panel: WeakEntity<ContainerPanel>,
    root: PathBuf,
    target: Target,
    changes: usize,
    /// The short id of a detached HEAD that no branch or tag contains.
    orphan: Option<String>,
    containers: Vec<(ProjectContainer, bool)>,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for ConfirmBranchSwitch {}

impl workspace::ModalView for ConfirmBranchSwitch {
    fn fade_out_background(&self) -> bool {
        true
    }
}

impl Focusable for ConfirmBranchSwitch {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ConfirmBranchSwitch {
    /// `containers` start ticked where they are running: a stopped one was
    /// stopped on purpose, and restarting it would start it.
    pub(crate) fn new(
        panel: WeakEntity<ContainerPanel>,
        root: PathBuf,
        target: Target,
        changes: usize,
        orphan: Option<String>,
        containers: Vec<ProjectContainer>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            panel,
            root,
            target,
            changes,
            orphan,
            containers: containers
                .into_iter()
                .map(|container| {
                    let running = container.running;
                    (container, running)
                })
                .collect(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// The line about uncommitted work, when there is any.
    pub(crate) fn carried_note(&self) -> Option<String> {
        (self.changes > 0).then(|| {
            let noun = if self.changes == 1 {
                "change"
            } else {
                "changes"
            };
            format!(
                "{} uncommitted {noun} will be carried over. If they clash with the target, \
                 git refuses and nothing is restarted.",
                self.changes
            )
        })
    }

    /// Leaving a detached HEAD that nothing names strands its commits.
    pub(crate) fn orphan_note(&self) -> Option<String> {
        self.orphan.as_ref().map(|sha| {
            format!(
                "Commits at {sha} are on no branch; switching away leaves them reachable \
                 only through the reflog."
            )
        })
    }

    /// The line about what a worktree does and does not bring along.
    pub(crate) fn worktree_note(&self) -> Option<&'static str> {
        matches!(self.target, Target::Worktree { .. }).then_some(
            "Only this worktree's committed HEAD is taken. Its uncommitted changes are not \
             carried over.",
        )
    }

    pub(crate) fn restart_ids(&self) -> Vec<String> {
        self.containers
            .iter()
            .filter(|(_, checked)| *checked)
            .map(|(container, _)| container.id.clone())
            .collect()
    }

    pub(crate) fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some((_, checked)) = self.containers.get_mut(index) {
            *checked = !*checked;
            cx.notify();
        }
    }

    pub(crate) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = SwitchRequest {
            root: self.root.clone(),
            target: self.target.clone(),
            restart: self
                .containers
                .iter()
                .filter(|(_, checked)| *checked)
                .map(|(container, _)| container.clone())
                .collect(),
        };
        cx.emit(DismissEvent);
        if let Err(error) = self.panel.update(cx, |panel, cx| {
            panel.start_branch_switch(request, window, cx)
        }) {
            log::debug!("panel closed before the switch could start: {error}");
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl Render for ConfirmBranchSwitch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        let restart_count = self.restart_ids().len();
        let go_label = match restart_count {
            0 => "Switch".to_string(),
            count => format!("Switch and restart {count}"),
        };
        let this = cx.entity().downgrade();

        div()
            .key_context("ConfirmBranchSwitch")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "confirm-branch-switch".into())
            .w(rems(32.))
            .max_h(rems(40.))
            .child(
                Modal::new("confirm-branch-switch", None)
                    .header(
                        ModalHeader::new().child(
                            Headline::new(format!("Switch to {}", self.target.title()))
                                .size(HeadlineSize::Small),
                        ),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .child(
                                    Label::new(format!(
                                        "Detaches {} at this commit.",
                                        self.root.display()
                                    ))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                                )
                                .when_some(self.worktree_note(), |element, note| {
                                    element.child(
                                        Label::new(note)
                                            .size(LabelSize::Small)
                                            .color(Color::Warning),
                                    )
                                })
                                .when_some(self.orphan_note(), |element, note| {
                                    element.child(
                                        div()
                                            .debug_selector(|| {
                                                "confirm-branch-switch-orphan".into()
                                            })
                                            .child(
                                                Label::new(note)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Warning),
                                            ),
                                    )
                                })
                                .when_some(self.carried_note(), |element, note| {
                                    element.child(
                                        div()
                                            .debug_selector(|| {
                                                "confirm-branch-switch-carried".into()
                                            })
                                            .child(
                                                Label::new(note)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Warning),
                                            ),
                                    )
                                })
                                .child(Label::new("Restart afterwards").size(LabelSize::Small))
                                .child(
                                    v_flex()
                                        .id("confirm-branch-switch-containers")
                                        .max_h(rems(11.25))
                                        .overflow_y_scroll()
                                        .gap_1()
                                        .children(self.containers.iter().enumerate().map(
                                            |(index, (container, checked))| {
                                                let this = this.clone();
                                                Checkbox::new(
                                                    SharedString::from(format!(
                                                        "confirm-branch-switch-container-{index}"
                                                    )),
                                                    (*checked).into(),
                                                )
                                                .label(container.name.clone())
                                                .label_size(LabelSize::Small)
                                                .on_click(move |_state, _window, cx| {
                                                    this.update(cx, |this, cx| {
                                                        this.toggle(index, cx)
                                                    })
                                                    .ok();
                                                })
                                            },
                                        )),
                                )
                                .when(restart_count == 0, |element| {
                                    element.child(
                                        Label::new(
                                            "No container is ticked: only the checkout changes.",
                                        )
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                    )
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("confirm-branch-switch-cancel", "Cancel").on_click(
                                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                                            this.cancel(cx)
                                        }),
                                    ),
                                )
                                .child(
                                    Button::new("confirm-branch-switch-go", go_label)
                                        .style(ButtonStyle::Tinted(TintColor::Accent))
                                        .key_binding(
                                            KeyBinding::for_action_in(
                                                &menu::Confirm,
                                                &focus_handle,
                                                cx,
                                            )
                                            .size(rems_from_px(12.)),
                                        )
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.confirm(window, cx)
                                            },
                                        )),
                                ),
                        ),
                    ),
            )
    }
}

/// Why a switch did not go through, in git's or the engine's own words.
pub(crate) struct BranchSwitchError {
    title: String,
    message: String,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for BranchSwitchError {}
impl workspace::ModalView for BranchSwitchError {}

impl Focusable for BranchSwitchError {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BranchSwitchError {
    pub(crate) fn new(title: String, message: String, cx: &mut Context<Self>) -> Self {
        Self {
            title,
            message,
            focus_handle: cx.focus_handle(),
        }
    }

    #[cfg(test)]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[cfg(test)]
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl Render for BranchSwitchError {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("BranchSwitchError")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, _window, cx| this.dismiss(cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.dismiss(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "branch-switch-error".into())
            .w(rems(32.))
            .max_h(rems(40.))
            .child(
                Modal::new("branch-switch-error", None)
                    .header(
                        ModalHeader::new()
                            .child(Headline::new(self.title.clone()).size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new().child(
                            div()
                                .id("branch-switch-error-message")
                                .w_full()
                                .max_h(rems(18.))
                                .overflow_y_scroll()
                                .child(
                                    Label::new(self.message.clone())
                                        .size(LabelSize::Small)
                                        .color(Color::Error),
                                ),
                        ),
                    )
                    .footer(ModalFooter::new().end_slot(
                        Button::new("branch-switch-error-ok", "OK").on_click(
                            cx.listener(|this, _: &ClickEvent, _window, cx| this.dismiss(cx)),
                        ),
                    )),
            )
    }
}
