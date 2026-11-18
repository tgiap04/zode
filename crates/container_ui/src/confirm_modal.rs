//! The last gate before something is gone.
//!
//! The list of what will be lost is what informs; the name typed, or pasted,
//! into the field is what says it was looked at. A button labelled OK is pressed
//! by the same reflex whether the row underneath it was read or not, and Enter
//! here goes through the same check as the Remove button.
//!
//! It takes a [`DestructivePlan`], which cannot be built without the list of
//! what will be lost. So there is no way to open this dialog over a removal
//! nobody has enumerated.

use std::sync::Arc;

use container::resource::row_key;
use container::{ComposeTarget, ContainerBackend, DestructivePlan, FORCE_CONFIRMATION, Intent};
use gpui::{
    AnyElement, App, ClickEvent, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, Task, Window,
};
use ui::{
    Checkbox, CopyButton, KeyBinding, Modal, ModalFooter, ModalHeader, Section, ToggleState,
    prelude::*,
};
use ui_input::InputField;

use crate::container_panel::{Busy, ContainerPanel};

/// What a `compose down` dialog needs to list the project's volumes too: the
/// engine to ask again, and the project to ask it about.
#[derive(Clone)]
pub(crate) struct Replan {
    pub(crate) backend: Arc<dyn ContainerBackend>,
    pub(crate) target: ComposeTarget,
}

pub struct ConfirmDestructive {
    plan: DestructivePlan,
    /// Set for a `compose down`, which offers to take the volumes along.
    replan: Option<Replan>,
    /// The tick the person left, which is ahead of `plan` while the engine is
    /// being asked again.
    volumes_wanted: bool,
    /// The tick that removes what is still in use, offered only when the plan
    /// says something is *and* the engine says forcing does something for this
    /// kind. While it is on, the word to type is [`FORCE_CONFIRMATION`] and the
    /// plan that leaves is the forced one.
    force_wanted: bool,
    /// What forcing does on this engine, in its own words, or `None` when it
    /// does nothing and so is not offered: a tick that promises more than the
    /// engine delivers is a removal that fails after the person consented to it.
    force_effect: Option<String>,
    /// Asking the engine for the plan with or without volumes. Held so closing
    /// the dialog drops the question.
    planning: Option<Task<()>>,
    /// Why the last such question failed, when it did.
    planning_error: Option<String>,
    input: Entity<InputField>,
    focus_handle: FocusHandle,
    /// Called with the plan once the typed text matches.
    confirmed: Option<Box<dyn FnOnce(DestructivePlan, &mut Window, &mut App) + 'static>>,
}

impl EventEmitter<DismissEvent> for ConfirmDestructive {}

/// A modal, so it dims what is behind it and takes the keyboard.
///
/// `fade_out_background` is on: this is the one dialog in the panel where the
/// thing behind it is about to change irreversibly, and the dimming says the rest
/// of the window is not what is being answered.
impl workspace::ModalView for ConfirmDestructive {
    fn fade_out_background(&self) -> bool {
        true
    }
}

/// The field, so that the dialog opens ready to be typed into, and Enter typed
/// there reaches `menu::Confirm` below.
impl Focusable for ConfirmDestructive {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl ConfirmDestructive {
    pub fn new(
        plan: DestructivePlan,
        confirmed: impl FnOnce(DestructivePlan, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = format!("Type {}", plan.confirmation());
        let input = cx.new(|cx| InputField::new(window, cx, &placeholder));
        Self {
            plan,
            replan: None,
            volumes_wanted: false,
            force_wanted: false,
            force_effect: None,
            planning: None,
            planning_error: None,
            input,
            focus_handle: cx.focus_handle(),
            confirmed: Some(Box::new(confirmed)),
        }
    }

    /// Offers the force tick, with what the engine says forcing does, when it
    /// does anything.
    pub(crate) fn forcing(mut self, effect: Option<String>) -> Self {
        self.force_effect = effect;
        self
    }

    /// Whether the force tick is on offer: something uses a target, and forcing
    /// would make the engine remove it anyway.
    fn offers_force(&self) -> bool {
        self.plan.has_dependents() && self.force_effect.is_some()
    }

    /// Offers the "also remove volumes" tick, which asks `replan` for a plan
    /// that lists them.
    pub(crate) fn replanning(mut self, replan: Replan) -> Self {
        self.replan = Some(replan);
        self
    }

    #[cfg(test)]
    pub(crate) fn volumes_wanted(&self) -> bool {
        self.volumes_wanted
    }

    #[cfg(test)]
    pub(crate) fn planning_failure(&self) -> Option<&str> {
        self.planning_error.as_deref()
    }

    fn lists_volumes(&self) -> bool {
        matches!(
            self.plan.intent(),
            Intent::ComposeDown { with_volumes: true }
        )
    }

    /// Asks the engine for the plan with or without the project's volumes, and
    /// takes the answer in place of the one on screen.
    ///
    /// The Remove button is off until the answer is in: confirming while the
    /// list shows one thing and the plan holds another is the one mistake this
    /// dialog exists to prevent.
    pub(crate) fn set_volumes(
        &mut self,
        with_volumes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(replan) = self.replan.clone() else {
            return;
        };
        self.volumes_wanted = with_volumes;
        self.planning_error = None;
        self.planning = Some(cx.spawn_in(window, async move |this, cx| {
            let planned = replan
                .backend
                .compose_down_plan(&replan.target, with_volumes)
                .await;
            let applied = this.update_in(cx, |this, window, cx| {
                this.planning = None;
                match planned {
                    Ok(Some(plan)) => {
                        this.plan = plan;
                        // The name was typed for the list that was on screen, and
                        // this one says something else: one Enter must not turn
                        // an answer to the old list into consent to the new.
                        this.clear_answer(window, cx);
                    }
                    Ok(None) => {
                        this.planning_error = Some("Nothing is left to take down.".to_string())
                    }
                    Err(error) => this.planning_error = Some(error.to_string()),
                }
                // The tick follows the plan that is really held, so a failed
                // question does not leave it claiming something the list is not.
                this.volumes_wanted = this.lists_volumes();
                cx.notify();
            });
            if let Err(error) = applied {
                log::debug!("dialog closed before the plan came back: {error}");
            }
        }));
        cx.notify();
    }

    /// Empties the field and rewrites its placeholder for the word that is
    /// expected now.
    fn clear_answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = format!("Type {}", self.expected());
        let editor = self.input.read(cx).editor().clone();
        editor.set_placeholder_text(&placeholder, window, cx);
        editor.clear(window, cx);
    }

    #[cfg(test)]
    pub(crate) fn plan(&self) -> &DestructivePlan {
        &self.plan
    }

    fn typed(&self, cx: &App) -> String {
        self.input.read(cx).text(cx)
    }

    /// What has to be typed right now.
    fn expected(&self) -> &str {
        if self.force_wanted {
            FORCE_CONFIRMATION
        } else {
            self.plan.confirmation()
        }
    }

    /// Ticks or unticks forcing. Forcing starts a second, deliberate answer, so
    /// whatever was typed for the first is cleared: a name typed for a plain
    /// removal must not carry over as consent to a forced one.
    pub(crate) fn set_force(&mut self, wanted: bool, window: &mut Window, cx: &mut Context<Self>) {
        if wanted && !self.offers_force() {
            return;
        }
        self.force_wanted = wanted;
        self.clear_answer(window, cx);
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn force_effect(&self) -> Option<&str> {
        self.force_effect.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn force_wanted(&self) -> bool {
        self.force_wanted
    }

    /// Types into the field, for tests of the gate itself.
    ///
    /// `cfg(test)` rather than the crate's `test-support` feature: this modal is
    /// private to the crate, so a helper offered to other crates could not be
    /// called from one anyway -- it would only compile into their builds as dead
    /// code.
    #[cfg(test)]
    pub(crate) fn type_confirmation(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The editor is pulled out before `cx` is borrowed mutably: reading the
        // input and writing through it in one expression borrows `cx` twice.
        let editor = self.input.read(cx).editor().clone();
        editor.set_text(text, window, cx);
    }

    /// Presses Remove, for tests of the gate itself. `cfg(test)` for the reason
    /// given on `type_confirmation`.
    #[cfg(test)]
    pub(crate) fn press_remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm(window, cx);
    }

    fn matches(&self, cx: &App) -> bool {
        self.planning.is_none() && self.typed(cx).trim() == self.expected()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Checked here and not only by the button's enabled state: a keybinding
        // or a future call site must meet the same gate, and an enabled flag is
        // not a gate.
        if !self.matches(cx) {
            return;
        }
        let plan = if self.force_wanted {
            // The plan itself checks the word again: a forced plan cannot exist
            // without it.
            let Some(forced) = self.plan.clone().forced(&self.typed(cx)) else {
                return;
            };
            forced
        } else {
            self.plan.clone()
        };
        if let Some(confirmed) = self.confirmed.take() {
            confirmed(plan, window, cx);
        }
        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl ConfirmDestructive {
    /// What is still using the targets, and the tick that removes them anyway
    /// where the engine has one. Nothing at all when nothing is in use.
    fn render_dependents(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.plan.has_dependents() {
            return None;
        }
        let this = cx.entity().downgrade();
        let force = self.force_wanted;
        let notice = if self.offers_force() {
            "Still in use. The engine refuses to remove these unless forced."
        } else {
            "Still in use. The engine will not remove these until nothing uses them."
        };
        Some(
            v_flex()
                .debug_selector(|| "confirm-destructive-dependents".into())
                .gap_1()
                .child(
                    Label::new(notice)
                        .size(LabelSize::Small)
                        .color(Color::Warning),
                )
                .children(self.plan.dependents().iter().filter_map(|entry| {
                    if entry.containers.is_empty() {
                        return None;
                    }
                    let target = self
                        .plan
                        .targets()
                        .iter()
                        .find(|target| target.id == entry.target_id)
                        .map_or(entry.target_id.as_str(), |target| target.name.as_str());
                    let users = entry
                        .containers
                        .iter()
                        .map(|container| container.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Some(
                        Label::new(format!("{target}: {users}"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                }))
                .children(
                    self.force_effect
                        .clone()
                        .filter(|_| self.offers_force())
                        .map(|effect| {
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .debug_selector(|| "confirm-destructive-force-tick".into())
                                        .child(
                                            Checkbox::new(
                                                "confirm-destructive-force",
                                                if force {
                                                    ToggleState::Selected
                                                } else {
                                                    ToggleState::Unselected
                                                },
                                            )
                                            .label("Force removal")
                                            .label_size(LabelSize::Small)
                                            .on_click(
                                                move |state, window, cx| {
                                                    let wanted = *state == ToggleState::Selected;
                                                    this.update(cx, |this, cx| {
                                                        this.set_force(wanted, window, cx)
                                                    })
                                                    .ok();
                                                },
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .debug_selector(|| {
                                            "confirm-destructive-force-effect".into()
                                        })
                                        .child(
                                            Label::new(effect)
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                )
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Render for ConfirmDestructive {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let matches = self.matches(cx);
        let confirmation = self.expected().to_string();
        let targets = self.plan.targets();
        let focus_handle = self.focus_handle.clone();
        let (title, verb) = match self.plan.intent() {
            Intent::ComposeDown { .. } => (
                format!(
                    "Take down {}",
                    self.plan
                        .compose_target()
                        .map_or("the project", |target| target.project.as_str())
                ),
                "Take Down",
            ),
            _ => (format!("Remove {} item(s)", targets.len()), "Remove"),
        };
        let volumes = self.replan.is_some().then(|| {
            let this = cx.entity().downgrade();
            let wanted = self.volumes_wanted;
            Checkbox::new(
                "confirm-destructive-volumes",
                if wanted {
                    ToggleState::Selected
                } else {
                    ToggleState::Unselected
                },
            )
            .label("Also remove volumes")
            .label_size(LabelSize::Small)
            .on_click(move |state, window, cx| {
                let with_volumes = *state == ToggleState::Selected;
                this.update(cx, |this, cx| this.set_volumes(with_volumes, window, cx))
                    .ok();
            })
        });

        div()
            .key_context("ConfirmDestructive")
            .track_focus(&self.focus_handle)
            // Enter goes through `confirm`, so it meets the same check as the
            // button: with the wrong text typed it does nothing.
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "confirm-destructive".into())
            .w(rems(28.))
            .max_h(rems(40.))
            .child(
                Modal::new("confirm-destructive", None)
                    .header(
                        ModalHeader::new().child(Headline::new(title).size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                // The enumerated list, and it is the part that
                                // actually informs. Without it, typing a word is
                                // a ritual rather than a decision.
                                .child(
                                    v_flex()
                                        .id("confirm-destructive-targets")
                                        .max_h(rems(11.25))
                                        .overflow_y_scroll()
                                        .gap_1()
                                        .children(targets.iter().map(|target| {
                                            Label::new(format!(
                                                "{}  ({:?})",
                                                target.name, target.kind
                                            ))
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                        })),
                                )
                                .children(volumes)
                                .children(self.render_dependents(cx))
                                .when_some(self.planning_error.clone(), |element, error| {
                                    element.child(
                                        Label::new(error)
                                            .size(LabelSize::Small)
                                            .color(Color::Error),
                                    )
                                })
                                .when_some(self.plan.warning(), |element, warning| {
                                    element.child(
                                        Label::new(warning.to_string())
                                            .size(LabelSize::Small)
                                            .color(Color::Error),
                                    )
                                })
                                .child(
                                    v_flex()
                                        .w_full()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .child(
                                                    Label::new(format!(
                                                        "Type \"{confirmation}\" to confirm"
                                                    ))
                                                    .size(LabelSize::Small)
                                                    .color(Color::Muted),
                                                )
                                                // Copying is offered because the name has
                                                // to be read off the list above either way,
                                                // and a long generated name retyped by hand
                                                // only invites a typo that blocks a removal
                                                // the person already decided on.
                                                .child(
                                                    CopyButton::new(
                                                        "confirm-destructive-copy",
                                                        confirmation,
                                                    )
                                                    .tooltip_label("Copy the text to type"),
                                                ),
                                        )
                                        .child(self.input.clone()),
                                ),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("confirm-destructive-cancel", "Cancel").on_click(
                                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                                            this.cancel(cx)
                                        }),
                                    ),
                                )
                                .child(
                                    Button::new("confirm-destructive-go", verb)
                                        .style(ButtonStyle::Tinted(ui::TintColor::Error))
                                        // Disabled until the text matches. Not the gate
                                        // -- `confirm` checks again -- but the visible
                                        // half of it.
                                        .disabled(!matches)
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

/// Opening the dialog, from the panel.
///
/// These two are the **only** ways a removal begins. Both build a plan first and
/// hand it to the dialog; neither can reach `ContainerPanel::destroy` without one.
impl ContainerPanel {
    /// Whether the engine has a prune at all.
    pub(crate) fn prune_available(&self) -> bool {
        self.backend()
            .is_some_and(|backend| backend.kind() != container::BackendKind::Kubernetes)
    }

    pub(crate) fn start_removal(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.start_removal_of(vec![key.to_string()], window, cx);
    }

    /// Removing several rows: one plan listing every one of them, one dialog.
    ///
    /// Never one dialog per row and never a removal that skips the list: the
    /// names in the dialog are what the person consents to, so it has to be all
    /// of them. Rows that are gone, hidden or busy are not in the plan.
    ///
    /// Asks the engine what depends on the rows *before* the dialog opens, for
    /// the reason a prune does: a dialog that could not say "these containers
    /// use it" would be one to answer with a reflex.
    pub(crate) fn start_removal_of(
        &mut self,
        keys: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(engine) = self.backend().cloned() else {
            return;
        };
        let targets = self.removal_targets(&keys, cx);
        if targets.is_empty() {
            return;
        }
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        // Marked for as long as the engine is being asked, which for a volume or
        // an image means one lookup per row: the rows say so, and a second
        // Delete on them finds them busy and plans nothing.
        let marked: Vec<String> = targets.iter().map(row_key).collect();
        self.mark_busy(marked.iter().cloned(), Busy::Planning, action_id);
        cx.notify();
        let planning = self.plan_removal_for(engine.clone(), targets, cx);
        self.actions.insert(
            action_id,
            cx.spawn_in(window, async move |this, cx| {
                let planned = planning.await;
                if let Err(error) = this.update_in(cx, |this, window, cx| {
                    // The rows were only waiting for the question, not for an
                    // answer: whatever it was, they are free again.
                    this.finish_busy(&marked, action_id, true);
                    match planned {
                        Ok(Some(plan)) => this.open_confirmation(plan, engine, window, cx),
                        Ok(None) => cx.notify(),
                        Err(error) => {
                            this.last_error = Some(error);
                            cx.notify();
                        }
                    }
                }) {
                    log::debug!("panel closed before the removal dialog could open: {error}");
                }
                // See the matching comment in `ContainerPanel::act`: removing
                // our own map entry as the very last statement is safe.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its planning could self-remove: {error}");
                }
            }),
        );
    }

    /// Finds what a prune would delete, *then* asks.
    ///
    /// The order is the whole point: `docker system prune` has no `--dry-run`, so
    /// a dialog opened before the list was gathered could only say "everything
    /// unused", which is not something anybody can consent to.
    pub(crate) fn start_prune(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Volumes are NOT included. Turning them on is a separate, explicit act;
        // a stopped database's volume counts as unused.
        let Some(engine) = self.backend().cloned() else {
            return;
        };
        let finding = self.plan_prune(container::PruneScope::Reclaimable, cx);
        // `spawn_in` and not `spawn`: opening a modal needs a `Window`, and only
        // an `AsyncWindowContext` carries one across the await.
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        self.actions.insert(
            action_id,
            cx.spawn_in(window, async move |this, cx| {
                if let Some(plan) = finding.await {
                    if let Err(error) = this.update_in(cx, |this, window, cx| {
                        this.open_confirmation(plan, engine, window, cx);
                    }) {
                        log::debug!("panel closed before the prune dialog could open: {error}");
                    }
                }
                // See the matching comment in `ContainerPanel::act`: removing
                // our own map entry as the very last statement is safe.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its prune finding could self-remove: {error}");
                }
            }),
        );
    }

    fn open_confirmation(
        &mut self,
        plan: container::DestructivePlan,
        engine: Arc<dyn ContainerBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_confirmation_with(plan, engine, None, window, cx);
    }

    /// The `compose down` dialog: the same gate, with the tick that adds the
    /// project's volumes to the list and to the plan.
    pub(crate) fn open_confirmation_replanning(
        &mut self,
        plan: container::DestructivePlan,
        replan: Replan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let engine = replan.backend.clone();
        self.open_confirmation_with(plan, engine, Some(replan), window, cx);
    }

    /// Opens the dialog for a plan built on `engine`, and hands the answer back
    /// to that same engine: a dialog is open for as long as somebody takes to
    /// read it, and the panel may be on another engine by then.
    fn open_confirmation_with(
        &mut self,
        plan: container::DestructivePlan,
        engine: Arc<dyn ContainerBackend>,
        replan: Option<Replan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            // No workspace, no modal layer. Refusing is right: the alternative is
            // removing something with no confirmation at all.
            log::warn!("no workspace to show a removal confirmation in");
            return;
        };
        if !self.is_active_engine(&engine) {
            self.refuse_stale_plan(cx);
            return;
        }
        let panel = cx.entity().downgrade();
        // Only a removal can be forced, and only an engine that does something
        // different when forced gets to say so.
        let force_effect = match plan.intent() {
            Intent::Remove { kind } => engine.force_effect(kind),
            Intent::Prune { .. } | Intent::ComposeDown { .. } => None,
        };
        workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    let modal = ConfirmDestructive::new(
                        plan,
                        move |plan, _window, cx| {
                            panel
                                .update(cx, |panel, cx| panel.destroy(engine, plan, cx))
                                .ok();
                        },
                        window,
                        cx,
                    )
                    .forcing(force_effect);
                    match replan {
                        Some(replan) => modal.replanning(replan),
                        None => modal,
                    }
                });
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use container::{ResourceKind, RunState};
    use gpui::TestAppContext;
    use std::cell::Cell;
    use std::rc::Rc;

    fn a_plan() -> DestructivePlan {
        DestructivePlan::remove(
            ResourceKind::Container,
            vec![container::Resource {
                kind: ResourceKind::Container,
                id: "id".into(),
                name: "zode-postgres".into(),
                state: RunState::Stopped,
                detail: Vec::new(),
                parent: None,
                compose: None,
            }],
        )
        .expect("one target")
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
    }

    /// `confirm` must check the typed text itself, not trust the button.
    ///
    /// The disabled button is a rendering state. A keybinding, a future call
    /// site, or a mutation that drops the check would all bypass it -- and what
    /// is bypassed here is the last thing standing between a wrong answer and
    /// something being gone.
    #[gpui::test]
    async fn pressing_remove_with_the_wrong_text_does_nothing(cx: &mut TestAppContext) {
        init(cx);
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(
                a_plan(),
                move |_plan, _window, _cx| counter.set(counter.get() + 1),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        for wrong in ["", "yes", "ZODE-POSTGRES", "zode-postgre"] {
            modal.update_in(cx, |modal, window, cx| {
                modal.type_confirmation(wrong, window, cx);
                modal.press_remove(window, cx);
            });
            cx.run_until_parked();
            assert_eq!(
                fired.get(),
                0,
                "pressing Remove with {wrong:?} typed must do nothing"
            );
        }

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("zode-postgres", window, cx);
            modal.press_remove(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            fired.get(),
            1,
            "and the right text must let it through, once"
        );
    }

    /// Opens the dialog with Enter and Escape bound as the default keymap binds
    /// them: `menu::*` everywhere, and Escape first offered to the field's
    /// editor, which passes it on when it has nothing of its own to cancel.
    fn open_with_keys(
        fired: Rc<Cell<usize>>,
        cx: &mut TestAppContext,
    ) -> (Entity<ConfirmDestructive>, &mut gpui::VisualTestContext) {
        init(cx);
        cx.update(|cx| {
            cx.bind_keys([
                gpui::KeyBinding::new("enter", menu::Confirm, None),
                gpui::KeyBinding::new("escape", menu::Cancel, None),
                gpui::KeyBinding::new("escape", editor::actions::Cancel, Some("Editor")),
            ]);
        });
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(
                a_plan(),
                move |_plan, _window, _cx| fired.set(fired.get() + 1),
                window,
                cx,
            )
        });
        modal.update_in(cx, |modal, window, cx| {
            let focus = modal.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (modal, cx)
    }

    #[gpui::test]
    async fn enter_in_the_field_removes_only_once_the_text_matches(cx: &mut TestAppContext) {
        let fired = Rc::new(Cell::new(0usize));
        let (_modal, cx) = open_with_keys(fired.clone(), cx);

        cx.simulate_input("zode-postgre");
        cx.simulate_keystrokes("enter");
        assert_eq!(fired.get(), 0, "Enter with the wrong text must do nothing");

        cx.simulate_input("s");
        cx.simulate_keystrokes("enter");
        assert_eq!(fired.get(), 1, "Enter with the right text removes");
    }

    #[gpui::test]
    async fn escape_closes_without_removing(cx: &mut TestAppContext) {
        let fired = Rc::new(Cell::new(0usize));
        let (modal, cx) = open_with_keys(fired.clone(), cx);
        let dismissed = Rc::new(Cell::new(false));
        let _subscription = cx.update(|_window, cx| {
            let dismissed = dismissed.clone();
            cx.subscribe(&modal, move |_, _: &DismissEvent, _| dismissed.set(true))
        });

        cx.simulate_input("zode-postgres");
        cx.simulate_keystrokes("escape");
        assert!(dismissed.get());
        assert_eq!(fired.get(), 0);
    }

    /// The dialog is answered by typing, so it must open with the field ready.
    #[gpui::test]
    async fn the_dialog_opens_with_the_field_focused(cx: &mut TestAppContext) {
        init(cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(a_plan(), |_plan, _window, _cx| {}, window, cx)
        });
        modal.update_in(cx, |modal, window, cx| {
            let focus = modal.focus_handle(cx);
            window.focus(&focus, cx);
            assert!(
                modal
                    .input
                    .read(cx)
                    .editor()
                    .focus_handle(cx)
                    .is_focused(window),
                "what the modal layer focuses is the field itself"
            );
        });
    }

    /// A confirmation is the small kind of modal, however long the name it
    /// asks for.
    #[gpui::test]
    async fn the_dialog_is_a_small_modal(cx: &mut TestAppContext) {
        init(cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(a_plan(), |_plan, _window, _cx| {}, window, cx)
        });
        cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(800.)));
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("confirm-destructive")
            .expect("the dialog was not drawn");
        let small = cx.update(|window, _| window.rem_size() * 28.);
        assert_eq!(bounds.size.width, small);
    }

    /// Pressing it twice must not run twice: the callback is taken, not cloned.
    #[gpui::test]
    async fn a_confirmed_removal_runs_exactly_once(cx: &mut TestAppContext) {
        init(cx);
        let fired = Rc::new(Cell::new(0usize));
        let counter = fired.clone();
        let (modal, cx) = cx.add_window_view(|window, cx| {
            ConfirmDestructive::new(
                a_plan(),
                move |_plan, _window, _cx| counter.set(counter.get() + 1),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("zode-postgres", window, cx);
            modal.press_remove(window, cx);
            modal.press_remove(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(
            fired.get(),
            1,
            "a second press must not remove a second time"
        );
    }
}
