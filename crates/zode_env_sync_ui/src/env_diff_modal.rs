use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Subscription,
    Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_env_sync::EnvSession;

use crate::masking;

/// What would change in an environment file, before anything changes.
///
/// Sibling of the settings diff window, with one difference that is the whole
/// reason it is a separate type: **values are masked by default**. A diff of a
/// `.env` is the single most screenshot-and-paste-into-chat artefact this
/// editor can produce, and a window that renders `STRIPE_SECRET_KEY=sk_live_…`
/// in full has already leaked the credential by the time anyone notices.
///
/// The default button is Cancel. Two of the three ways out overwrite
/// something; the one that overwrites nothing is where a stray Return lands.
pub struct EnvDiffModal {
    session: Entity<EnvSession>,
    focus_handle: FocusHandle,
    /// Flipped only by the platform confirming who is at the keyboard, and
    /// never persisted: closing the window puts the values back.
    revealed: bool,
    support: os_auth::Support,
    _observation: Subscription,
}

impl EventEmitter<DismissEvent> for EnvDiffModal {}
impl ModalView for EnvDiffModal {}

impl Focusable for EnvDiffModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EnvDiffModal {
    pub fn new(session: Entity<EnvSession>, cx: &mut Context<Self>) -> Self {
        // Closes itself once there is nothing left to decide — the session
        // decides when the question is answered, not this window.
        let observation = cx.observe(&session, |_this, session, cx| {
            if session.read(cx).pending().is_none() {
                cx.emit(DismissEvent);
            }
            cx.notify();
        });
        Self {
            session,
            focus_handle: cx.focus_handle(),
            revealed: false,
            support: os_auth::support(),
            _observation: observation,
        }
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if self.revealed {
            self.revealed = false;
            cx.notify();
            return;
        }

        if !self.support.is_available() {
            // Nothing to ask with. Shown, not silently skipped and not faked:
            // the user is told the values are about to appear with no check
            // behind it, and the button label says so too.
            self.revealed = true;
            cx.notify();
            return;
        }

        cx.spawn(async move |this, cx| {
            let outcome = os_auth::authenticate("reveal the values in this environment file").await;
            _ = this.update(cx, |this, cx| {
                this.revealed = outcome == os_auth::Outcome::Confirmed;
                cx.notify();
            });
        })
        .detach();
    }

    /// The reveal button's label, which must never overstate the protection.
    fn reveal_label(&self) -> &'static str {
        match (self.revealed, self.support.is_available()) {
            (true, _) => "Hide values",
            (false, true) => "Reveal values…",
            // No qualifier, no ellipsis: nothing is going to be asked.
            (false, false) => "Reveal values",
        }
    }
}

impl Render for EnvDiffModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        let shell = v_flex()
            .key_context("EnvDiffModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(60.))
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                this.session
                    .update(cx, |session, cx| session.dismiss_pending(cx));
                cx.emit(DismissEvent);
            }));

        let session = self.session.read(cx);
        let Some(pending) = session.pending() else {
            // A frame can land between the decision being taken and the
            // dismiss above being processed.
            return shell;
        };

        let added = pending.diff.added;
        let removed = pending.diff.removed;
        let safe = pending.safe_to_apply;
        let name = pending
            .local_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "this environment file".into());

        let body = if self.revealed {
            pending.diff.unified.clone()
        } else {
            masking::mask_block(&pending.diff.unified)
        };

        let unverified_reveal = self.revealed && !self.support.is_available();
        let support_note = self.support.describe();

        shell
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .child(
                        Label::new(format!("{name} differs from the server"))
                            .weight(FontWeight::MEDIUM),
                    )
                    .child(
                        Label::new(if safe {
                            "This machine has not changed it since the last sync, so taking the server's copy loses nothing."
                        } else {
                            "Both sides changed since the last sync. Whichever you choose, the other is replaced."
                        })
                        .size(LabelSize::Small)
                        .color(if safe { Color::Muted } else { Color::Warning }),
                    ),
            )
            .child(
                div()
                    .id("env-diff-body")
                    .w_full()
                    .h(rems(24.))
                    .p_2()
                    .overflow_y_scroll()
                    .bg(colors.editor_background)
                    .border_y_1()
                    .border_color(colors.border_variant)
                    .child(Label::new(body).size(LabelSize::Small).buffer_font(cx)),
            )
            .when(unverified_reveal, |this| {
                this.child(
                    div().px_2().pt_1().child(
                        Label::new(format!("Shown without any check — {support_note}."))
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .p_2()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .bg(colors.editor_background)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Label::new(format!("+{added} −{removed}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Button::new("env-diff-reveal", self.reveal_label())
                                    .label_size(LabelSize::Small)
                                    .tooltip(Tooltip::text(if self.support.is_available() {
                                        "Values are hidden until you confirm it is you"
                                    } else {
                                        "This machine cannot confirm it is you"
                                    }))
                                    .on_click(cx.listener(|this, _, _window, cx| this.reveal(cx))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("env-diff-cancel", "Cancel")
                                    .label_size(LabelSize::Small)
                                    .tooltip(Tooltip::text("Change nothing on either side"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.session
                                            .update(cx, |session, cx| session.dismiss_pending(cx));
                                        cx.emit(DismissEvent);
                                    })),
                            )
                            .child(
                                Button::new("env-diff-apply", "Write the server's copy")
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::Small)
                                    .tooltip(Tooltip::text(
                                        "The current file is copied aside first, outside this project",
                                    ))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.session
                                            .update(cx, |session, cx| session.apply_pending(cx));
                                        cx.emit(DismissEvent);
                                    })),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::path::PathBuf;
    use zode_account::{Account, AccountStatus, AccountUser};
    use zode_env_sync::{EntryId, PendingEnvDivergence};

    fn init_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn session_with(
        pending: Option<PendingEnvDivergence>,
        cx: &mut TestAppContext,
    ) -> Entity<EnvSession> {
        let account = cx.update(|cx| {
            cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }))
            })
        });
        let session = cx.update(|cx| cx.new(|_| EnvSession::new(account)));
        if let Some(pending) = pending {
            session.update(cx, |session, cx| session.set_pending_for_test(pending, cx));
        }
        session
    }

    fn divergence(safe_to_apply: bool) -> PendingEnvDivergence {
        let local = "# staging\nAPI_KEY=old_secret_value\n";
        let remote = "# staging\nAPI_KEY=new_secret_value\n";
        PendingEnvDivergence {
            entry: EntryId::parse("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1").unwrap(),
            local_path: PathBuf::from("/work/acme/.env"),
            diff: zode_sync::diff::between(local, remote),
            remote: remote.into(),
            revision: "rev-1".into(),
            seq: 2,
            safe_to_apply,
        }
    }

    #[gpui::test]
    fn the_window_draws_a_safe_divergence(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(true)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_draws_a_conflict(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(false)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The frame between the decision landing and the window dismissing.
    #[gpui::test]
    fn the_window_draws_with_nothing_pending(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(None, cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_body_starts_masked_and_holds_no_secret(cx: &mut TestAppContext) {
        // The assertion this whole window exists for, made against the exact
        // string that reaches the label.
        init_theme(cx);
        let session = session_with(Some(divergence(true)), cx);
        let (modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, cx));
        cx.run_until_parked();

        modal.update(cx, |modal, cx| {
            assert!(!modal.revealed, "values must start hidden");
            let pending = modal.session.read(cx).pending().expect("a pending diff");
            let shown = masking::mask_block(&pending.diff.unified);
            assert!(!shown.contains("old_secret_value"), "{shown}");
            assert!(!shown.contains("new_secret_value"), "{shown}");
            assert!(shown.contains("API_KEY"), "the name must survive: {shown}");
        });
    }

    #[gpui::test]
    fn the_reveal_label_never_promises_a_check_that_will_not_happen(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(true)), cx);
        let (modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, cx));
        cx.run_until_parked();

        modal.update(cx, |modal, _cx| {
            // The ellipsis is the promise that something will be asked. It may
            // appear only where something actually will be.
            modal.support = os_auth::Support::Unsupported { reason: "test" };
            assert_eq!(modal.reveal_label(), "Reveal values");
            modal.support = os_auth::Support::Available { method: "test" };
            assert_eq!(modal.reveal_label(), "Reveal values…");
        });
    }
}
