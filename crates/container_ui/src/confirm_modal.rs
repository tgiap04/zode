//! The last gate before something is gone.
//!
//! Typing, not clicking, and for a reason that is not ceremony: typing the name
//! is what makes somebody read *which* resource they are removing. A button
//! labelled OK is pressed by the same reflex whether the row underneath it was
//! read or not.
//!
//! It takes a [`DestructivePlan`], which cannot be built without the list of
//! what will be lost. So there is no way to open this dialog over a removal
//! nobody has enumerated.

use container::DestructivePlan;
use gpui::{
    App, ClickEvent, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Window,
};
use ui::prelude::*;
use ui_input::InputField;

use crate::container_panel::ContainerPanel;

pub struct ConfirmDestructive {
    plan: DestructivePlan,
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

impl Focusable for ConfirmDestructive {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        let _ = cx;
        self.focus_handle.clone()
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
            input,
            focus_handle: cx.focus_handle(),
            confirmed: Some(Box::new(confirmed)),
        }
    }

    fn typed(&self, cx: &App) -> String {
        self.input.read(cx).text(cx)
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
        self.plan.is_confirmed_by(&self.typed(cx))
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Checked here and not only by the button's enabled state: a keybinding
        // or a future call site must meet the same gate, and an enabled flag is
        // not a gate.
        if !self.matches(cx) {
            return;
        }
        if let Some(confirmed) = self.confirmed.take() {
            let plan = self.plan.clone();
            confirmed(plan, window, cx);
        }
        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl Render for ConfirmDestructive {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let matches = self.matches(cx);
        let confirmation = self.plan.confirmation().to_string();
        let targets = self.plan.targets();

        v_flex()
            .key_context("ConfirmDestructive")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(px(460.))
            .p_4()
            .gap_3()
            .child(Label::new(format!("Remove {} item(s)", targets.len())).size(LabelSize::Large))
            // The enumerated list, and it is the part that actually informs.
            // Without it, typing a word is a ritual rather than a decision.
            .child(
                v_flex()
                    .id("confirm-destructive-targets")
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .gap_1()
                    .children(targets.iter().map(|target| {
                        Label::new(format!("{}  ({:?})", target.name, target.kind))
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                    })),
            )
            .when_some(self.plan.warning(), |element, warning| {
                element.child(
                    Label::new(warning.to_string())
                        .size(LabelSize::Small)
                        .color(Color::Error),
                )
            })
            .child(
                Label::new(format!("Type \"{confirmation}\" to confirm"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(self.input.clone())
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("confirm-destructive-cancel", "Cancel").on_click(
                            cx.listener(|this, _: &ClickEvent, _window, cx| this.cancel(cx)),
                        ),
                    )
                    .child(
                        Button::new("confirm-destructive-go", "Remove")
                            .style(ButtonStyle::Tinted(ui::TintColor::Error))
                            // Disabled until the text matches. Not the gate --
                            // `confirm` checks again -- but the visible half of
                            // it.
                            .disabled(!matches)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.confirm(window, cx)
                            })),
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

    pub(crate) fn start_removal(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(plan) = self.plan_removal(id) else {
            return;
        };
        self.open_confirmation(plan, window, cx);
    }

    /// Finds what a prune would delete, *then* asks.
    ///
    /// The order is the whole point: `docker system prune` has no `--dry-run`, so
    /// a dialog opened before the list was gathered could only say "everything
    /// unused", which is not something anybody can consent to.
    pub(crate) fn start_prune(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Volumes are NOT included. Turning them on is a separate, explicit act;
        // a stopped database's volume counts as unused.
        let finding = self.plan_prune(container::PruneScope::Reclaimable, cx);
        // `spawn_in` and not `spawn`: opening a modal needs a `Window`, and only
        // an `AsyncWindowContext` carries one across the await.
        self.actions
            .push(cx.spawn_in(window, async move |this, cx| {
                let Some(plan) = finding.await else {
                    return;
                };
                this.update_in(cx, |this, window, cx| {
                    this.open_confirmation(plan, window, cx);
                })
                .ok();
            }));
    }

    fn open_confirmation(
        &mut self,
        plan: container::DestructivePlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            // No workspace, no modal layer. Refusing is right: the alternative is
            // removing something with no confirmation at all.
            log::warn!("no workspace to show a removal confirmation in");
            return;
        };
        let panel = cx.entity().downgrade();
        workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    ConfirmDestructive::new(
                        plan,
                        move |plan, _window, cx| {
                            panel.update(cx, |panel, cx| panel.destroy(plan, cx)).ok();
                        },
                        window,
                        cx,
                    )
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
