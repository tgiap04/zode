use std::time::Duration;

use gpui::{
    App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, Focusable, FontWeight,
    SharedString, Subscription, Task, Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_account::{Account, AccountStatus};

/// How long "Copied" stays on the button before it goes back to offering.
///
/// Long enough to be read, short enough that the button stays honest about
/// what pressing it will do next.
const COPIED_FEEDBACK: Duration = Duration::from_secs(2);

/// The window shown while a device sign-in is waiting for the user.
///
/// The layout answers one question: what does the person actually have to do?
/// At their own machine it is press one button — the code is already in the
/// link. In a window running over SSH there is no browser to open, so the code
/// and the address are written out to be read across to another machine.
///
/// So there is exactly one filled button. Copying is a quiet action attached to
/// the code it copies, the SSH route sits underneath at lower weight, and
/// Cancel is in the footer where a dismissal belongs. An earlier version
/// stacked all three as equal full-width buttons and left the reader to work
/// out the hierarchy themselves.
pub struct SignInModal {
    account: Entity<Account>,
    focus_handle: gpui::FocusHandle,
    /// Set while the "Copied" confirmation shows. Held as a task so the timer
    /// is cancelled if the modal closes first.
    copied_reset: Option<Task<()>>,
    _observation: Subscription,
}

impl EventEmitter<DismissEvent> for SignInModal {}
impl ModalView for SignInModal {}

impl Focusable for SignInModal {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl SignInModal {
    pub fn new(account: Entity<Account>, cx: &mut Context<Self>) -> Self {
        // Closes itself the moment the account leaves the waiting state —
        // approval, denial, or a code that timed out. The modal does not decide
        // when it is finished; the account does.
        let observation = cx.observe(&account, |_this: &mut Self, account, cx| {
            if !matches!(
                account.read(cx).status(),
                AccountStatus::WaitingForApproval { .. }
            ) {
                cx.emit(DismissEvent);
            }
            cx.notify();
        });

        Self {
            account,
            focus_handle: cx.focus_handle(),
            copied_reset: None,
            _observation: observation,
        }
    }

    /// Dismissing must also stop the polling — otherwise the flow keeps asking
    /// after the user has visibly walked away from it.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.account
            .update(cx, |account, cx| account.cancel_sign_in(cx));
        cx.emit(DismissEvent);
    }

    fn copy_code(&mut self, code: SharedString, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(code.to_string()));

        // Every action answers. A copy button that does nothing visible leaves
        // the reader pressing it again to find out whether it worked.
        self.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FEEDBACK).await;
            _ = this.update(cx, |this, cx| {
                this.copied_reset = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn showing_copied(&self) -> bool {
        self.copied_reset.is_some()
    }
}

impl Render for SignInModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.account.read(cx).status().clone();
        let colors = cx.theme().colors();

        // `elevation_3` carries the background, border, radius and shadow.
        // Without it the modal is transparent and whatever is behind it reads
        // straight through the middle of the code.
        let shell = v_flex()
            .key_context("SignInModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .w(rems(22.))
            .overflow_hidden();

        let AccountStatus::WaitingForApproval {
            user_code,
            verification_uri,
            verification_uri_complete,
        } = status
        else {
            // A frame can land between the account leaving the waiting state
            // and the dismiss above being processed.
            return shell;
        };

        let code_to_copy = user_code.clone();
        let link = verification_uri_complete;
        let copied = self.showing_copied();

        shell
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .child(Label::new("Sign in to Zode").weight(FontWeight::MEDIUM))
                    .child(
                        Label::new("Optional — the editor works either way.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                // The code is the one thing that has to travel to another
                // device, so it gets the largest type, its own surface, and
                // nothing competing beside it.
                v_flex()
                    .w_full()
                    .py_4()
                    .gap_1p5()
                    .items_center()
                    .bg(colors.editor_background)
                    .border_y_1()
                    .border_color(colors.border_variant)
                    .child(
                        Label::new(user_code)
                            .size(LabelSize::Large)
                            .weight(FontWeight::MEDIUM)
                            .buffer_font(cx),
                    )
                    .child(
                        Button::new(
                            "account-copy-code",
                            if copied { "Copied" } else { "Copy code" },
                        )
                        .label_size(LabelSize::Small)
                        .color(if copied { Color::Success } else { Color::Muted })
                        .start_icon(Icon::new(if copied {
                            IconName::Check
                        } else {
                            IconName::Copy
                        }))
                        .on_click(cx.listener(
                            move |this, _, _window, cx| this.copy_code(code_to_copy.clone(), cx),
                        )),
                    ),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        // The only filled button in the window.
                        Button::new("account-open-browser", "Open browser to approve")
                            .full_width()
                            .style(ButtonStyle::Filled)
                            .end_icon(Icon::new(IconName::ArrowUpRight))
                            .on_click(move |_, _window, cx| cx.open_url(&link)),
                    )
                    .child(
                        // The route for a window with no browser to open — a
                        // remote or SSH session, which is why this flow was
                        // chosen over a loopback redirect. Offered, not hidden,
                        // but plainly the second choice.
                        v_flex()
                            .gap_0p5()
                            .child(
                                Label::new("No browser here? Open this and enter the code:")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(verification_uri)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .buffer_font(cx),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .p_2()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .bg(colors.editor_background)
                    .border_t_1()
                    .border_color(colors.border_variant)
                    .child(
                        // Says the window is doing something. Without it a
                        // modal that only polls looks frozen.
                        Label::new("Waiting for approval…")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Button::new("account-cancel-sign-in", "Cancel")
                            .label_size(LabelSize::Small)
                            .tooltip(Tooltip::text("Stops waiting and stops polling"))
                            .on_click(cx.listener(|this, _, _window, cx| this.cancel(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn init_theme(cx: &mut TestAppContext) {
        // Rendering reads `cx.theme().colors()`, so the theme global has to
        // exist — the same two lines `sidebar::sidebar_tests::init_test` runs.
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn waiting() -> AccountStatus {
        AccountStatus::WaitingForApproval {
            user_code: "A1B2-C3D4".into(),
            verification_uri: "https://zodekit.site/activate".into(),
            verification_uri_complete: "https://zodekit.site/activate?code=A1B2-C3D4".into(),
        }
    }

    /// Draws the modal for real in both branches.
    ///
    /// This does NOT catch the class of bug that prompted the rework — a modal
    /// with no background renders perfectly and simply looks wrong, and no
    /// assertion here would have known. It catches the other thing: a panic in
    /// the render path, including the `let-else` frame where the account has
    /// left the waiting state but the dismiss has not been processed yet.
    fn draw_modal_with(status: AccountStatus, cx: &mut TestAppContext) {
        init_theme(cx);
        let account = cx.update(|cx| cx.new(|_| Account::for_test(status)));
        let (_modal, cx) = cx.add_window_view(|_window, cx| SignInModal::new(account.clone(), cx));

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_modal_draws_while_waiting(cx: &mut TestAppContext) {
        draw_modal_with(waiting(), cx);
    }

    /// The frame between approval landing and the modal dismissing itself.
    #[gpui::test]
    fn the_modal_draws_in_the_frame_after_the_wait_ends(cx: &mut TestAppContext) {
        draw_modal_with(AccountStatus::SignedOut, cx);
    }

    /// The copy button confirms and then reverts, so its label never offers
    /// something other than what pressing it will do.
    #[gpui::test]
    async fn copying_the_code_confirms_and_then_reverts(cx: &mut TestAppContext) {
        init_theme(cx);
        let account = cx.update(|cx| cx.new(|_| Account::for_test(waiting())));
        let (modal, cx) = cx.add_window_view(|_window, cx| SignInModal::new(account.clone(), cx));

        modal.update(cx, |modal: &mut SignInModal, _| {
            assert!(!modal.showing_copied())
        });

        modal.update(cx, |modal: &mut SignInModal, cx| {
            modal.copy_code("A1B2-C3D4".into(), cx)
        });
        modal.update(cx, |modal: &mut SignInModal, _| {
            assert!(modal.showing_copied(), "the button must confirm the copy")
        });

        cx.executor().advance_clock(COPIED_FEEDBACK * 2);
        cx.run_until_parked();

        modal.update(cx, |modal: &mut SignInModal, _| {
            assert!(
                !modal.showing_copied(),
                "the confirmation must clear, or the button keeps offering something it no longer does"
            )
        });
    }
}
