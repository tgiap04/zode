use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window};
use ui::{CommonAnimationExt, Modal, ModalFooter, ModalHeader, Section, Tooltip, prelude::*};
use workspace::ModalView;
use zode_env_sync::{EnvSession, EnvStatus};

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
    file_name: SharedString,
    focus_handle: FocusHandle,
    /// Set once a divergence has actually been shown.
    ///
    /// Without it, "nothing is pending" cannot be told apart from "the answer
    /// has not arrived yet", and a fetch is always the second one first.
    saw_pending: bool,
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
    pub fn new(
        session: Entity<EnvSession>,
        file_name: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Closes itself once the difference it was opened for has been
        // decided -- but only once there was one. This used to dismiss on any
        // notification that found nothing pending, and since a fetch is
        // asynchronous, nothing is pending at the moment it starts: the window
        // closed itself before the request had even been sent, which is
        // exactly what "I fetched and nothing happened" looks like.
        let observation = cx.observe(&session, |this: &mut Self, session, cx| {
            if session.read(cx).pending().is_some() {
                this.saw_pending = true;
            } else if this.saw_pending {
                cx.emit(DismissEvent);
            }
            cx.notify();
        });
        Self {
            session,
            file_name: file_name.into(),
            focus_handle: cx.focus_handle(),
            saw_pending: false,
            revealed: false,
            support: os_auth::support(),
            _observation: observation,
        }
    }

    /// What the session has to say when there is no difference to show.
    ///
    /// A fetch that changes nothing is still an answer, and the window has to
    /// give it. Every branch returns a sentence, so a new status cannot be
    /// added without one.
    fn outcome_line(&self, cx: &App) -> (SharedString, Color) {
        match self.session.read(cx).status() {
            EnvStatus::Working => ("Asking your account what it holds…".into(), Color::Muted),
            EnvStatus::Done(message) => (message.clone(), Color::Muted),
            EnvStatus::Idle => ("There is nothing waiting for this file.".into(), Color::Muted),
            EnvStatus::NeedsRecoveryKey => (
                "Enter your recovery key first — Account → Enter Recovery Key…".into(),
                Color::Warning,
            ),
            EnvStatus::KeyMismatch => (
                "This file was encrypted with a different key, probably rotated elsewhere.".into(),
                Color::Error,
            ),
            EnvStatus::Rollback { seen, got } => (
                format!(
                    "The server offered version {got} of a file this machine already has at {seen}. Nothing was written."
                )
                .into(),
                Color::Error,
            ),
            EnvStatus::Failed(message) => (message.clone(), Color::Error),
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

        let shell = div()
            .key_context("EnvDiffModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(58.))
            .max_h(rems(44.))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                this.session
                    .update(cx, |session, cx| session.dismiss_pending(cx));
                cx.emit(DismissEvent);
            }));

        let working = matches!(self.session.read(cx).status(), EnvStatus::Working);
        let (outcome, outcome_color) = self.outcome_line(cx);
        let file_name = self.file_name.clone();

        let session = self.session.read(cx);
        let Some(pending) = session.pending() else {
            // Not an empty box. Either the answer has not arrived, or it
            // arrived and changed nothing -- both are things to say, and
            // saying neither is what made a fetch look like it did nothing.
            return shell.child(
                Modal::new("env-diff-outcome", None)
                    .header(
                        ModalHeader::new()
                            .icon(
                                Icon::new(IconName::CloudDownload)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .headline(format!("Fetching {file_name}"))
                            .description(if working {
                                "Nothing on this machine has been touched yet."
                            } else {
                                "Your account was asked. Nothing on this machine was changed."
                            }),
                    )
                    .section(
                        Section::new().child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .when(working, |this| {
                                    this.child(
                                        Icon::new(IconName::LoadCircle)
                                            .size(IconSize::Small)
                                            .color(Color::Muted)
                                            .with_rotate_animation(3),
                                    )
                                })
                                .child(
                                    Label::new(outcome)
                                        .size(LabelSize::Small)
                                        .color(outcome_color),
                                ),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            Button::new("env-diff-outcome-close", "Close")
                                .label_size(LabelSize::Small)
                                .on_click(
                                    cx.listener(|_this, _, _window, cx| cx.emit(DismissEvent)),
                                ),
                        ),
                    ),
            );
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

        // A unified diff rendered in one colour asks the reader to parse the
        // leading character of every line themselves. The sign still carries
        // the meaning and the colour only repeats it, so the diff stays
        // readable where colour does not.
        // Nothing was written, and the file is still the old one. Shown
        // beside the diff rather than only in a status bar, because this is
        // the window the user is looking at when they press the button.
        let write_failure = (outcome_color == Color::Error).then(|| outcome.clone());

        let added_background = cx.theme().status().created_background;
        let removed_background = cx.theme().status().deleted_background;
        let mut diff_body = v_flex().w_full();
        for line in body.lines() {
            let (text, background) = match line.as_bytes().first() {
                Some(b'+') => (Color::Created, Some(added_background)),
                Some(b'-') => (Color::Deleted, Some(removed_background)),
                _ if line.starts_with("@@") => (Color::Accent, None),
                _ => (Color::Muted, None),
            };
            diff_body = diff_body.child(
                div()
                    .w_full()
                    .px_2()
                    .when_some(background, |this, background| this.bg(background))
                    .child(
                        Label::new(if line.is_empty() {
                            " ".to_string()
                        } else {
                            line.to_string()
                        })
                        .size(LabelSize::Small)
                        .color(text)
                        .buffer_font(cx),
                    ),
            );
        }

        shell.child(
            Modal::new("env-diff", None)
                .header(
                    ModalHeader::new()
                        .icon(
                            Icon::new(if safe {
                                IconName::CloudDownload
                            } else {
                                IconName::Warning
                            })
                            .size(IconSize::Small)
                            .color(if safe { Color::Muted } else { Color::Warning }),
                        )
                        .headline(format!("{name} differs from the server"))
                        .description(if safe {
                            "This machine has not changed it since the last sync, so taking the server's copy loses nothing."
                        } else {
                            "Both sides changed since the last sync. Whichever you choose, the other is replaced."
                        }),
                )
                .section(
                    Section::new()
                        .meta(if self.revealed {
                            "Values shown"
                        } else {
                            "Values masked — names and structure only"
                        })
                        .child(
                            div()
                                .id("env-diff-body")
                                .w_full()
                                .h(rems(22.))
                                .py_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(colors.border_variant)
                                .bg(colors.editor_background)
                                .overflow_y_scroll()
                                .child(diff_body),
                        )
                        .when_some(write_failure, |this, reason| {
                            // The window stays open for exactly this line.
                            this.child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        Icon::new(IconName::XCircle)
                                            .size(IconSize::XSmall)
                                            .color(Color::Error),
                                    )
                                    .child(
                                        Label::new(reason)
                                            .size(LabelSize::XSmall)
                                            .color(Color::Error),
                                    ),
                            )
                        })
                        .when(unverified_reveal, |this| {
                            this.child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        Icon::new(IconName::Warning)
                                            .size(IconSize::XSmall)
                                            .color(Color::Warning),
                                    )
                                    .child(
                                        Label::new(format!(
                                            "Shown without any check — {support_note}."
                                        ))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Warning),
                                    ),
                            )
                        }),
                )
                .footer(
                    ModalFooter::new()
                        .start_slot(
                            h_flex()
                                .gap_2()
                                .items_center()
                                // The sign carries the meaning and the colour
                                // only reinforces it, so the count still reads
                                // where colour does not.
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(
                                            Label::new(format!("+{added}"))
                                                .size(LabelSize::XSmall)
                                                .color(Color::Created),
                                        )
                                        .child(
                                            Label::new(format!("\u{2212}{removed}"))
                                                .size(LabelSize::XSmall)
                                                .color(Color::Deleted),
                                        ),
                                )
                                .child(
                                    Button::new("env-diff-reveal", self.reveal_label())
                                        .label_size(LabelSize::Small)
                                        .start_icon(
                                            Icon::new(IconName::Eye).size(IconSize::Small),
                                        )
                                        .tooltip(Tooltip::text(if self.support.is_available() {
                                            "Values are hidden until you confirm it is you"
                                        } else {
                                            "This machine cannot confirm it is you"
                                        }))
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| this.reveal(cx)),
                                        ),
                                ),
                        )
                        .end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("env-diff-cancel", "Cancel")
                                        .label_size(LabelSize::Small)
                                        .tooltip(Tooltip::text("Change nothing on either side"))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.session.update(cx, |session, cx| {
                                                session.dismiss_pending(cx)
                                            });
                                            cx.emit(DismissEvent);
                                        })),
                                )
                                .child(
                                    Button::new("env-diff-apply", "Write the Server's Copy")
                                        .style(ButtonStyle::Filled)
                                        .label_size(LabelSize::Small)
                                        .tooltip(Tooltip::text(
                                            "The current file is copied aside first, outside this project",
                                        ))
                                        // Dismissed only on a real write: a
                                        // failure keeps the window, because
                                        // this is where the reason can be read
                                        // and the file is still the old one.
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            let written = this.session.update(cx, |session, cx| {
                                                session.apply_pending(cx)
                                            });
                                            if written {
                                                cx.emit(DismissEvent);
                                            }
                                        })),
                                ),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;
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
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, ".env", cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_draws_a_conflict(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(false)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, ".env", cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// Watches for the window closing itself.
    fn dismissals(modal: &Entity<EnvDiffModal>, cx: &mut VisualTestContext) -> Rc<Cell<usize>> {
        let seen = Rc::new(Cell::new(0));
        cx.update(|_window, cx| {
            cx.subscribe(modal, {
                let seen = seen.clone();
                move |_modal, _: &DismissEvent, _cx| seen.set(seen.get() + 1)
            })
            .detach();
        });
        seen
    }

    #[gpui::test]
    fn a_fetch_that_has_not_answered_yet_keeps_the_window_open(cx: &mut TestAppContext) {
        // The bug this locks down: the window dismissed itself on any
        // notification that found nothing pending. Fetching is asynchronous,
        // so nothing IS pending when it starts -- the window closed before the
        // request had even been sent, and a fetch looked like it did nothing.
        init_theme(cx);
        let session = session_with(None, cx);
        let (modal, cx) = cx.add_window_view({
            let session = session.clone();
            |_window, cx| EnvDiffModal::new(session, ".env", cx)
        });
        cx.run_until_parked();
        let dismissed = dismissals(&modal, cx);

        for status in [
            EnvStatus::Working,
            EnvStatus::Done("already up to date".into()),
        ] {
            session.update(cx, |session, cx| {
                session.set_status_for_test(status.clone(), cx)
            });
            cx.run_until_parked();
            assert_eq!(
                dismissed.get(),
                0,
                "{status:?} closed the window before anything had diverged",
            );
            modal.update(cx, |modal, cx| {
                assert!(
                    !modal.outcome_line(cx).0.is_empty(),
                    "{status:?} leaves the window with nothing to say",
                );
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    fn the_window_closes_once_the_difference_it_showed_is_decided(cx: &mut TestAppContext) {
        // The other half of the same contract: once a divergence HAS been
        // shown, it going away means the user answered, and the window must go.
        init_theme(cx);
        let session = session_with(None, cx);
        let (modal, cx) = cx.add_window_view({
            let session = session.clone();
            |_window, cx| EnvDiffModal::new(session, ".env", cx)
        });
        cx.run_until_parked();
        let dismissed = dismissals(&modal, cx);

        session.update(cx, |session, cx| {
            session.set_pending_for_test(divergence(true), cx)
        });
        cx.run_until_parked();
        assert_eq!(
            dismissed.get(),
            0,
            "a difference to show is not a dismissal"
        );

        session.update(cx, |session, cx| session.dismiss_pending(cx));
        cx.run_until_parked();
        assert_eq!(
            dismissed.get(),
            1,
            "deciding the difference must close the window",
        );
    }

    /// The frame between the decision landing and the window dismissing.
    #[gpui::test]
    fn the_window_draws_with_nothing_pending(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(None, cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, ".env", cx));
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
        let (modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, ".env", cx));
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
        let (modal, cx) = cx.add_window_view(|_window, cx| EnvDiffModal::new(session, ".env", cx));
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
