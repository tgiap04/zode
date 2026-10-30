//! "Switch Branch...": moving the checkout a compose project was built from to
//! another branch or worktree, then restarting the containers that run it.
//!
//! Lives in the open container's view, not on the list row: the row's buttons
//! are the engine's own actions, and this is a change to the *source* the
//! containers run from. It needs the detail view's room to say what the
//! checkout is on and what is happening to it.
//!
//! The order is the point. The picker fetches, the confirmation says what will
//! happen, and only then does anything move: verify the target, detach the
//! checkout, restart the containers that were ticked. If git refuses, nothing is
//! restarted -- the containers keep running the files they have.

use std::path::PathBuf;

use container::{ContainerError, ResourceAction, ResourceKind, RunState};
use gpui::{AnyElement, Context, Window};
use ui::{CommonAnimationExt as _, prelude::*};

use crate::branch_switch_modals::{BranchSwitchError, ConfirmBranchSwitch};
use crate::branch_switch_picker::BranchPicker;
use crate::compose_checkout::{self, Checkout, GitFailure, Target, short};
use crate::container_panel::{Busy, ContainerPanel, ListState};

/// What a switch in progress is doing, for the label and the disabled button.
///
/// The fetch has no stage here: it happens inside the picker, which says so
/// itself, before there is any decision to be in the middle of.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SwitchStage {
    Switching,
    Restarting,
}

impl SwitchStage {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SwitchStage::Switching => "Switching\u{2026}",
            SwitchStage::Restarting => "Restarting\u{2026}",
        }
    }
}

/// A container of the project, as the confirmation lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectContainer {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) running: bool,
}

/// Everything a confirmed switch needs, decided in the dialog.
pub(crate) struct SwitchRequest {
    pub(crate) root: PathBuf,
    pub(crate) target: Target,
    /// The containers that were left ticked, in list order.
    pub(crate) restart: Vec<ProjectContainer>,
}

impl ContainerPanel {
    /// The checkout of the open container, when a switch can be offered for it.
    ///
    /// Needs a workspace because both dialogs are modals, and none for a
    /// floating window: refusing to draw the button is better than a button
    /// that cannot open its own dialog.
    pub(crate) fn switchable_checkout(&self) -> Option<&Checkout> {
        self.workspace.as_ref()?;
        self.detail.as_ref()?.checkout.as_ref()
    }

    /// Every container of the open one's compose project, and whether it is up.
    ///
    /// Matched on project *and* directory: two projects of the same name run
    /// from different checkouts, and restarting the other one's containers
    /// would be restarting something this switch did not touch.
    pub(crate) fn project_containers(&self) -> Vec<ProjectContainer> {
        let Some(detail) = self.detail.as_ref() else {
            return Vec::new();
        };
        let Some(labels) = detail.resource.compose.as_ref() else {
            return Vec::new();
        };
        let ListState::Ready(listing) = &self.state else {
            return vec![project_container(&detail.resource)];
        };
        listing
            .iter()
            .filter(|resource| {
                resource.kind == ResourceKind::Container
                    && resource
                        .compose
                        .as_ref()
                        .is_some_and(|other| labels.same_project(other))
            })
            .map(project_container)
            .collect()
    }

    /// Reads what the open container's checkout is on.
    ///
    /// Run again whenever the list is re-read, so a branch deleted or a checkout
    /// moved by hand shows up without reopening the view. Held in a field so
    /// closing the view drops the git commands it was waiting for.
    pub(crate) fn probe_checkout(&mut self, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_ref() else {
            self.checkout_probe = None;
            return;
        };
        let Some(directory) = detail
            .resource
            .compose
            .as_ref()
            .and_then(|labels| labels.working_dir.clone())
        else {
            return;
        };
        let opened_for = detail.resource.id.clone();
        self.checkout_probe = Some(cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(compose_checkout::inspect(PathBuf::from(directory)))
                .await;
            let applied = this.update(cx, |this, cx| {
                this.apply_checkout_probe(&opened_for, outcome, cx);
            });
            if let Err(error) = applied {
                log::debug!("panel closed before its checkout was read: {error}");
            }
        }));
    }

    /// Takes what a probe found, without letting a hiccup blank the row.
    ///
    /// A failed read keeps the last good checkout: git being briefly locked or
    /// busy says nothing about whether this is still a checkout. "Not a
    /// repository" clears it, except while a switch is running, when the
    /// directory is being rewritten and any answer but a good one is noise.
    pub(crate) fn apply_checkout_probe(
        &mut self,
        opened_for: &str,
        outcome: Result<Option<Checkout>, GitFailure>,
        cx: &mut Context<Self>,
    ) {
        let switching = self.branch_switch.is_some();
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if detail.resource.id != opened_for {
            return;
        }
        match outcome {
            Ok(Some(checkout)) => detail.checkout = Some(checkout),
            Ok(None) if !switching => detail.checkout = None,
            Ok(None) => {}
            Err(failure) => {
                log::warn!("could not read the checkout, keeping the last reading: {failure}");
                return;
            }
        }
        cx.notify();
    }

    pub(crate) fn open_branch_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branch_switch.is_some() {
            return;
        }
        let Some(root) = self
            .switchable_checkout()
            .map(|checkout| checkout.root.clone())
        else {
            return;
        };
        let panel = cx.entity().downgrade();
        self.show_modal(window, cx, move |window, cx| {
            BranchPicker::new(panel, root, window, cx)
        });
    }

    /// Counts what is uncommitted, then asks.
    ///
    /// The count is read first so the dialog can say what will be carried over
    /// instead of asking about something it has not looked at.
    pub(crate) fn open_switch_confirmation(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.branch_switch.is_some() {
            return;
        }
        let Some(root) = self
            .switchable_checkout()
            .map(|checkout| checkout.root.clone())
        else {
            return;
        };
        let containers = self.project_containers();
        let counting_root = root.clone();
        self.confirmation_prep = Some(cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    let changes = compose_checkout::uncommitted_changes(&counting_root).await?;
                    let orphan = compose_checkout::orphaned_head(&counting_root).await?;
                    Ok::<_, GitFailure>((changes, orphan))
                })
                .await;
            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok((changes, orphan)) => {
                    let panel = cx.entity().downgrade();
                    this.show_modal(window, cx, move |window, cx| {
                        ConfirmBranchSwitch::new(
                            panel, root, target, changes, orphan, containers, window, cx,
                        )
                    });
                }
                Err(failure) => this.show_switch_error(
                    "Could not read the checkout's state".into(),
                    failure.to_string(),
                    window,
                    cx,
                ),
            });
            if let Err(error) = outcome {
                log::debug!("panel closed before the confirmation could open: {error}");
            }
        }));
    }

    /// Switches the checkout, then restarts what was ticked.
    ///
    /// Restarts go through the backend's own `act`, the same path as the row's
    /// Restart button, one after another: a database restarting while the app
    /// that talks to it also restarts is a race, and one at a time keeps the
    /// failure that comes first the failure that is reported first.
    pub(crate) fn start_branch_switch(
        &mut self,
        request: SwitchRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend().cloned() else {
            self.show_switch_error(
                "Could not switch".into(),
                "There is no container engine to restart containers with, so nothing was changed."
                    .into(),
                window,
                cx,
            );
            return;
        };
        if self.branch_switch.is_some() {
            return;
        }
        self.branch_switch = Some(SwitchStage::Switching);
        self.last_error = None;
        cx.notify();

        self.switch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let SwitchRequest {
                root,
                target,
                restart,
            } = request;
            let switching = cx.background_spawn({
                let target = target.clone();
                async move { compose_checkout::switch(&root, &target).await }
            });
            let commit = match switching.await {
                Ok(commit) => commit,
                Err(failure) => {
                    let outcome = this.update_in(cx, |this, window, cx| {
                        this.branch_switch = None;
                        this.probe_checkout(cx);
                        this.show_switch_error(
                            format!("Could not switch to {}", target.title()),
                            failure.to_string(),
                            window,
                            cx,
                        );
                    });
                    if let Err(error) = outcome {
                        log::warn!(
                            "panel closed before the failed switch to {} was shown: {error}",
                            target.title()
                        );
                    }
                    return;
                }
            };

            let ids: Vec<String> = restart
                .iter()
                .map(|container| container.id.clone())
                .collect();
            let token = match this.update(cx, |this, cx| {
                let token = this.next_action_id;
                this.next_action_id += 1;
                this.branch_switch = Some(SwitchStage::Restarting);
                this.mark_busy(
                    ids.iter().cloned(),
                    Busy::Action(ResourceAction::Restart),
                    token,
                );
                cx.notify();
                token
            }) {
                Ok(token) => token,
                Err(error) => {
                    log::warn!(
                        "panel closed after the checkout moved to {}; its containers were not restarted: {error}",
                        target.title()
                    );
                    return;
                }
            };

            let mut failures: Vec<(String, ContainerError)> = Vec::new();
            for container in &restart {
                if let Err(error) = backend
                    .act(
                        ResourceKind::Container,
                        ResourceAction::Restart,
                        &container.id,
                    )
                    .await
                {
                    failures.push((container.name.clone(), error));
                }
            }

            let outcome = this.update_in(cx, |this, window, cx| {
                this.finish_busy(&ids, token, false);
                this.branch_switch = None;
                this.reload(cx);
                this.probe_checkout(cx);
                if !failures.is_empty() {
                    let lines: Vec<String> = failures
                        .iter()
                        .map(|(name, error)| format!("{name}: {error}"))
                        .collect();
                    this.show_switch_error(
                        format!("Switched to {}, but a restart failed", target.title()),
                        format!(
                            "The checkout moved to {} ({}), but these containers failed to \
                             restart:\n{}",
                            target.title(),
                            short(&commit),
                            lines.join("\n")
                        ),
                        window,
                        cx,
                    );
                }
            });
            if let Err(error) = outcome {
                log::warn!(
                    "panel closed after the checkout moved to {}, before the restarts were reported: {error}",
                    target.title()
                );
            }
        }));
    }

    pub(crate) fn show_switch_error(
        &mut self,
        title: String,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_modal(window, cx, move |_window, cx| {
            BranchSwitchError::new(title, message, cx)
        });
    }

    /// Puts a dialog up in the workspace, or says why it could not.
    fn show_modal<V: workspace::ModalView>(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            log::warn!("no workspace to show a branch switch dialog in");
            return;
        };
        if let Err(error) = workspace.update(cx, |workspace, cx| {
            // `toggle_modal` closes a modal of the same type instead of
            // replacing it, which would swallow a second error shown while the
            // first is still up.
            if workspace.active_modal::<V>(cx).is_some() {
                workspace.hide_modal(window, cx);
            }
            workspace.toggle_modal(window, cx, build);
        }) {
            log::warn!("could not show a branch switch dialog: {error}");
        }
    }

    /// What the checkout is on, and the button that moves it -- or, while a
    /// switch is running, what it is doing.
    pub(crate) fn render_checkout_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let checkout = self.switchable_checkout()?;
        let label = checkout.state.label();
        let colour = if checkout.state.points_nowhere() {
            Color::Warning
        } else {
            Color::Default
        };
        let stage = self.branch_switch;

        Some(
            h_flex()
                .debug_selector(|| "container-checkout-row".into())
                .flex_none()
                .px_2()
                .py_1()
                .gap_1p5()
                .justify_between()
                .border_b_1()
                .border_color(cx.theme().colors().border)
                .child(
                    h_flex()
                        .gap_1()
                        .min_w_0()
                        .child(
                            Icon::new(IconName::GitBranch)
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            div()
                                .debug_selector(|| "container-checkout-state".into())
                                .child(Label::new(label).size(LabelSize::Small).color(colour)),
                        ),
                )
                .child(match stage {
                    Some(stage) => h_flex()
                        .debug_selector(|| "container-switch-busy".into())
                        .gap_1()
                        .child(
                            Icon::new(IconName::LoadCircle)
                                .size(IconSize::Small)
                                .color(Color::Accent)
                                .with_keyed_rotate_animation("container-switch-spinner", 2),
                        )
                        .child(
                            Label::new(stage.label())
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .into_any_element(),
                    None => div()
                        .debug_selector(|| "container-switch-branch".into())
                        .child(
                            Button::new("container-switch-branch-button", "Switch Branch\u{2026}")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_branch_picker(window, cx)
                                })),
                        )
                        .into_any_element(),
                })
                .into_any_element(),
        )
    }
}

fn project_container(resource: &container::Resource) -> ProjectContainer {
    ProjectContainer {
        id: resource.id.clone(),
        name: resource.name.clone(),
        running: resource.state == RunState::Running,
    }
}
