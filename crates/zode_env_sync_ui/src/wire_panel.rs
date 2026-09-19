use gpui::{
    App, ClipboardItem, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Window,
};
use ui::{Modal, ModalFooter, ModalHeader, Section, Tooltip, prelude::*};
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

        // Two blocks rather than one, because they answer different
        // questions: the envelope is what the server can read, the blob is
        // what it cannot.
        let block = |id: &'static str, height: Rems| {
            div()
                .id(id)
                .w_full()
                .max_h(height)
                .p_2()
                .rounded_sm()
                .border_1()
                .border_color(colors.border_variant)
                .bg(colors.editor_background)
                .overflow_y_scroll()
        };

        div()
            .key_context("EnvWirePanel")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(54.))
            .max_h(rems(44.))
            // Escape discards. Enter is deliberately not bound to Send: this
            // window exists to be read, and a keystroke that skips the reading
            // would remove the only thing it is for.
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| {
                this.session
                    .update(cx, |session, cx| session.discard_prepared(cx));
                cx.emit(DismissEvent);
            }))
            .child(
                Modal::new("env-wire", None)
                    .header(
                        ModalHeader::new()
                            .icon(
                                Icon::new(IconName::ArrowUp)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .headline("What leaves your machine")
                            .description(format!(
                                "Sending {}. This is the whole request body, and no part of the file is readable in it.",
                                self.file_name
                            )),
                    )
                    .section(
                        Section::new()
                            .meta("Envelope — everything the server can read")
                            .child(block("env-wire-envelope", rems(11.)).child(
                                Label::new(self.wire.envelope_json.clone())
                                    .size(LabelSize::Small)
                                    .buffer_font(cx),
                            )),
                    )
                    .section(
                        Section::new()
                            .meta(format!(
                                "Ciphertext — {blob_length} characters of base64, and the only place your values are"
                            ))
                            .child(block("env-wire-blob", rems(9.)).child(
                                Label::new(self.wire.blob_base64.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .buffer_font(cx),
                            )),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                Button::new("env-wire-copy", "Copy Ciphertext")
                                    .label_size(LabelSize::Small)
                                    .start_icon(
                                        Icon::new(IconName::Copy).size(IconSize::Small),
                                    )
                                    .tooltip(Tooltip::text(
                                        "Copy it and compare against what a proxy captured",
                                    ))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            this.wire.blob_base64.clone(),
                                        ));
                                    })),
                            )
                            .end_slot(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("env-wire-cancel", "Do Not Send")
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
