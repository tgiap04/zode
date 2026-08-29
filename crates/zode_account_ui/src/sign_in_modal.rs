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

        let AccountStatus::WaitingForApproval {
            user_code,
            verification_uri,
            verification_uri_complete,
        } = status
        else {
            // A frame can land between the account leaving the waiting state
            // and the dismiss above being processed.
            return v_flex().w(rems(24.)).p_4();
        };

        let code_for_clipboard = user_code.clone();
        let link = verification_uri_complete;

        v_flex()
            .w(rems(24.))
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .key_context("SignInModal")
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .child(Label::new("Sign in to Zode").size(LabelSize::Large))
            .child(
                Label::new("Signing in is optional — the editor works either way.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .py_3()
                    .child(Label::new(user_code).size(LabelSize::Large).buffer_font(cx)),
            )
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
                // The way out for a session with no browser to open — a remote
                // or SSH window, which is the reason this flow was chosen over
                // a loopback redirect in the first place.
                Label::new(format!("Or open {verification_uri} and enter the code."))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .child(
                Button::new("account-cancel-sign-in", "Cancel")
                    .full_width()
                    .tooltip(Tooltip::text("Stops waiting and stops polling"))
                    .on_click(cx.listener(|this, _, _window, cx| this.cancel(cx))),
            )
    }
}
