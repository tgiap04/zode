use gpui::{
    App, ClipboardItem, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_env_sync::{EnvSession, WireBytes};

/// Everything that is about to leave this machine, shown before it does.
///
/// The bytes here are not a rendering of what will be sent. They **are** what
/// will be sent: `EnvSession::prepare` built one value, this window displays
/// it, and `EnvSession::send_prepared` puts that same value on the wire.
///
/// That distinction is the whole point. Two encryptions of one file differ —
/// the nonce is fresh each time — so a window that encrypted the file again in
/// order to show it would be displaying something the server never receives,
/// and the reassurance would be theatre. `wire_is_what_is_sent` in
/// `zode_env_sync` fails if the two ever come apart.
pub struct WirePanel {
    session: Entity<EnvSession>,
    wire: WireBytes,
    file_name: SharedString,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for WirePanel {}
impl ModalView for WirePanel {}

impl Focusable for WirePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl WirePanel {
    pub fn new(
        session: Entity<EnvSession>,
        wire: WireBytes,
        file_name: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            session,
            wire,
            file_name: file_name.into(),
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Render for WirePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let blob_length = self.wire.blob_base64.len();

        v_flex()
            .key_context("EnvWirePanel")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(58.))
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                this.session
                    .update(cx, |session, cx| session.discard_prepared(cx));
                cx.emit(DismissEvent);
            }))
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .child(
                        Label::new("What leaves your machine").weight(FontWeight::MEDIUM),
                    )
                    .child(
                        Label::new(format!(
                            "Sending {}. This is the whole request body — no part of the file is in it.",
                            self.file_name
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .id("env-wire-body")
                    .w_full()
                    .h(rems(20.))
                    .p_2()
                    .overflow_y_scroll()
                    .bg(colors.editor_background)
                    .border_y_1()
                    .border_color(colors.border_variant)
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                Label::new(self.wire.envelope_json.clone())
                                    .size(LabelSize::Small)
                                    .buffer_font(cx),
                            )
                            .child(
                                Label::new(format!(
                                    "blob (base64, {blob_length} characters):\n{}",
                                    self.wire.blob_base64
                                ))
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
                    .child(
                        Button::new("env-wire-copy", "Copy")
                            .label_size(LabelSize::Small)
                            .tooltip(Tooltip::text(
                                "Copy it and compare against what a proxy captured",
                            ))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.wire.blob_base64.clone(),
                                ));
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("env-wire-cancel", "Do not send")
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.session.update(cx, |session, cx| {
                                            session.discard_prepared(cx)
                                        });
                                        cx.emit(DismissEvent);
                                    })),
                            )
                            .child(
                                Button::new("env-wire-send", "Send")
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.session.update(cx, |session, cx| {
                                            session.send_prepared(cx)
                                        });
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
    use zode_account::{Account, AccountStatus, AccountUser};

    fn init_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn wire() -> WireBytes {
        WireBytes {
            envelope_json: "{\n  \"v\": 1,\n  \"alg\": \"AES-256-GCM\",\n  \"kid\": \"3rDjjO0eQd4=\",\n  \"nonce\": \"AAAAAAAAAAAAAAAA\",\n  \"ct\": \"ZmFrZQ==\"\n}".into(),
            blob_base64: "eyJ2IjoxfQ==".into(),
        }
    }

    fn session(cx: &mut TestAppContext) -> Entity<EnvSession> {
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
        cx.update(|cx| cx.new(|_| EnvSession::new(account)))
    }

    #[gpui::test]
    fn the_window_draws(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(cx);
        let (_panel, cx) =
            cx.add_window_view(|_window, cx| WirePanel::new(session, wire(), ".env", cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_shows_the_value_it_was_given_and_nothing_else(cx: &mut TestAppContext) {
        // The panel must not derive, re-encrypt, or reformat. What it holds is
        // what the caller prepared, which is what the request will carry.
        init_theme(cx);
        let session = session(cx);
        let given = wire();
        let (panel, cx) = cx.add_window_view({
            let given = given.clone();
            |_window, cx| WirePanel::new(session, given, ".env", cx)
        });
        cx.run_until_parked();

        panel.update(cx, |panel, _cx| {
            assert_eq!(panel.wire, given);
        });
    }
}
