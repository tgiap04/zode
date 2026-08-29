use gpui::{
    App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, Focusable, Subscription,
    Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_account::{Account, AccountStatus};

/// The window shown while a device sign-in is waiting for the user.
///
/// It is doing two jobs at once, and the layout follows from that. For someone
/// at their own machine, the "Open browser" button is the whole interaction —
/// the code is already in the link. For someone whose editor is running over
/// SSH, there is no browser to open, so the code and the address are also
/// written out plainly to be read across to another machine.
pub struct SignInModal {
    account: Entity<Account>,
    focus_handle: gpui::FocusHandle,
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
        // whether that is approval, denial, or a code that timed out. The modal
        // does not decide when it is finished; the account does.
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
}

impl Render for SignInModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.account.read(cx).status().clone();

        let colors = cx.theme().colors();

        // `elevation_3` is what makes this a window rather than floating text:
        // it carries the background, border, corner radius and shadow. Without
        // it the modal is transparent and whatever is behind it — a terminal,
        // usually — reads straight through the middle of the code the user is
        // supposed to be copying.
        let shell = v_flex()
            .key_context("SignInModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .w_80()
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

        let code_for_clipboard = user_code.clone();
        let link = verification_uri_complete;

        shell
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(Label::new("Sign in to Zode"))
                    .child(
                        Label::new("Signing in is optional — the editor works either way.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                // The one thing on screen the user has to read across to
                // another device, so it gets a surface of its own rather than
                // sitting in the flow of the sentences around it.
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_4()
                    .bg(colors.editor_background)
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .child(Label::new(user_code).size(LabelSize::Large).buffer_font(cx)),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        Button::new("account-open-browser", "Open browser to approve")
                            .full_width()
                            .style(ButtonStyle::Filled)
                            .on_click(move |_, _window, cx| cx.open_url(&link)),
                    )
                    .child(
                        Button::new("account-copy-code", "Copy code")
                            .full_width()
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    code_for_clipboard.to_string(),
                                ))
                            }),
                    )
                    .child(
                        // The way out for a session with no browser to open — a
                        // remote or SSH window, which is the reason this flow
                        // was chosen over a loopback redirect in the first place.
                        Label::new(format!("Or open {verification_uri} and enter the code."))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .p_2()
                    .justify_end()
                    .bg(colors.editor_background)
                    .border_t_1()
                    .border_color(colors.border_variant)
                    .child(
                        Button::new("account-cancel-sign-in", "Cancel")
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

    /// Draws the modal for real in both branches.
    ///
    /// This does NOT catch the bug that prompted it — a modal with no
    /// background renders perfectly and looks wrong, and no assertion here
    /// would have known. It catches the other thing: a panic in the render
    /// path, including the `let-else` frame where the account has already
    /// left the waiting state but the dismiss has not been processed yet.
    fn draw_modal_with(status: AccountStatus, cx: &mut TestAppContext) {
        // Rendering reads `cx.theme().colors()`, so the theme global has to
        // exist — same two lines `sidebar::sidebar_tests::init_test` runs.
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });

        let account = cx.update(|cx| cx.new(|_| Account::for_test(status)));
        let (_modal, cx) = cx.add_window_view(|_window, cx| SignInModal::new(account.clone(), cx));

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_modal_draws_while_waiting(cx: &mut TestAppContext) {
        draw_modal_with(
            AccountStatus::WaitingForApproval {
                user_code: "A1B2-C3D4".into(),
                verification_uri: "https://zodekit.site/activate".into(),
                verification_uri_complete: "https://zodekit.site/activate?code=A1B2-C3D4".into(),
            },
            cx,
        );
    }

    /// The frame between approval landing and the modal dismissing itself.
    #[gpui::test]
    fn the_modal_draws_in_the_frame_after_the_wait_ends(cx: &mut TestAppContext) {
        draw_modal_with(AccountStatus::SignedOut, cx);
    }
}
