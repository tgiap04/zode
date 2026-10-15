use gpui::{App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Subscription, Window};
use ui::{Modal, ModalFooter, ModalHeader, Tooltip, prelude::*};
use workspace::ModalView;
use zode_sync::SyncSession;

/// What would change, before anything changes.
///
/// Decision D2: nothing is ever written without this window first. The file on
/// the other side of it can hold API keys, terminal environment, and remote
/// paths that are correct on one machine and wrong on another, so "just take
/// the newer one" is not a safe default and is not offered as one.
///
/// The default button is Cancel. Of the three ways out, two overwrite
/// something, and the one that overwrites nothing is the one a stray Return
/// should land on.
pub struct DiffModal {
    session: Entity<SyncSession>,
    focus_handle: FocusHandle,
    _observation: Subscription,
}

impl EventEmitter<DismissEvent> for DiffModal {}
impl ModalView for DiffModal {}

impl Focusable for DiffModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DiffModal {
    pub fn new(session: Entity<SyncSession>, cx: &mut Context<Self>) -> Self {
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
            _observation: observation,
        }
    }
}

impl Render for DiffModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        let shell = div()
            .key_context("SyncDiffModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(60.))
            .max_h(rems(40.))
            .debug_selector(|| "account-sync-diff-modal".into())
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

        let kind = pending.kind;
        let added = pending.diff.added;
        let removed = pending.diff.removed;
        let safe = pending.safe_to_apply;
        let unified = pending.diff.unified.clone();

        shell
            .child(
                Modal::new("account-sync-diff", None)
                    .header(
                        // `ModalHeader::description` is always muted, and this
                        // sentence turns to a warning when both sides changed.
                        // So it borrows the description's size and spacing and
                        // keeps its own colour.
                        ModalHeader::new().child(
                            v_flex()
                                .child(
                                    Headline::new(format!("{kind} differs from the server"))
                                        .size(HeadlineSize::Small),
                                )
                                .child(
                                    Label::new(if safe {
                                        "This machine has not changed it since the last sync, so taking the server's copy loses nothing."
                                    } else {
                                        "Both sides changed since the last sync. Whichever you choose, the other is replaced."
                                    })
                                    .color(if safe { Color::Muted } else { Color::Warning })
                                    .mb_2(),
                                ),
                        ),
                    )
                    .child(
                        // Wide and scrollable in both directions. A cramped
                        // diff is a diff people accept without reading, which
                        // defeats the point of showing it.
                        div()
                            .id("sync-diff-body")
                            .w_full()
                            .h(rems(24.))
                            .p_2()
                            .overflow_y_scroll()
                            .bg(colors.editor_background)
                            .border_t_1()
                            .border_color(colors.border_variant)
                            .child(Label::new(unified).size(LabelSize::Small).buffer_font(cx)),
                    ),
            )
            .child(
                // `ModalFooter` has no fill of its own, so this band gives it the
                // editor background the body above it uses.
                div().w_full().bg(colors.editor_background).child(
                    ModalFooter::new()
                        .start_slot(
                            Label::new(format!("+{added} −{removed}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                        .end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("sync-diff-cancel", "Cancel")
                                        .tooltip(Tooltip::text("Change nothing on either side"))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.session.update(cx, |session, cx| {
                                                session.dismiss_pending(cx)
                                            });
                                            cx.emit(DismissEvent);
                                        })),
                                )
                                .child(
                                    Button::new("sync-diff-keep-local", "Keep This Machine's Copy")
                                        .tooltip(Tooltip::text("Overwrite the server"))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.session
                                                .update(cx, |session, cx| session.keep_local(cx));
                                            cx.emit(DismissEvent);
                                        })),
                                )
                                .child(
                                    Button::new("sync-diff-take-remote", "Take the Server's Copy")
                                        .style(ButtonStyle::Filled)
                                        .tooltip(Tooltip::text(
                                            "The current file is copied aside first",
                                        ))
                                        .on_click(cx.listener(|this, _, _window, cx| {
                                            this.session
                                                .update(cx, |session, cx| session.apply_pending(cx));
                                            cx.emit(DismissEvent);
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
    use gpui::TestAppContext;
    use zode_account::{Account, AccountStatus, AccountUser};
    use zode_sync::{Kind, PendingDivergence, diff};

    fn init_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn session_with(
        pending: Option<PendingDivergence>,
        cx: &mut TestAppContext,
    ) -> Entity<SyncSession> {
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
        let session = cx.update(|cx| cx.new(|_| SyncSession::new(account)));
        if let Some(pending) = pending {
            session.update(cx, |session, cx| session.set_pending_for_test(pending, cx));
        }
        session
    }

    fn divergence(safe_to_apply: bool) -> PendingDivergence {
        let local = "{\n  \"theme\": \"One Dark\"\n}\n";
        let remote = "{\n  \"theme\": \"Ayu Light\"\n}\n";
        PendingDivergence {
            kind: Kind::Settings,
            diff: diff::between(local, remote),
            remote: remote.into(),
            revision: "rev-1".into(),
            safe_to_apply,
        }
    }

    #[gpui::test]
    fn the_window_draws_a_safe_divergence(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(true)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| DiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// A window that shows a diff is the large tier of modal widths.
    #[gpui::test]
    fn the_window_is_the_large_width(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(true)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| DiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        let bounds = cx
            .debug_bounds("account-sync-diff-modal")
            .expect("the modal must be drawn");
        let large = cx.update(|window, _| rems(60.).to_pixels(window.rem_size()));
        assert_eq!(bounds.size.width, large);
    }

    #[gpui::test]
    fn the_window_draws_a_conflict(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(Some(divergence(false)), cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| DiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The frame between the decision landing and the window dismissing
    /// itself — the `let-else` branch in `render`.
    #[gpui::test]
    fn the_window_draws_with_nothing_pending(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session_with(None, cx);
        let (_modal, cx) = cx.add_window_view(|_window, cx| DiffModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }
}
