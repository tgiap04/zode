use extension_host::ExtensionStore;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Subscription,
    Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_sync::{Kind, SyncSession, SyncStatus};

/// Reads the installed extension identifiers.
///
/// Done here rather than inside `zode_sync` because `extension_host` can reach
/// `telemetry`, and the sync crates are held to a graph rule saying they
/// cannot — see `script/check-account-no-telemetry`. The list crosses the
/// boundary as plain strings.
fn installed_extensions(cx: &App) -> Vec<String> {
    ExtensionStore::global(cx)
        .read(cx)
        .installed_extensions()
        .keys()
        .map(|id| id.to_string())
        .collect()
}

/// Push and pull, one artifact at a time.
///
/// Deliberately not an "enable sync" switch. Nothing here happens on its own:
/// no sync at startup, no reconciliation in the background, no timer. Every
/// transfer is one button press, and the window shows exactly three rows so it
/// is always obvious what is about to move.
pub struct SyncModal {
    session: Entity<SyncSession>,
    focus_handle: FocusHandle,
    _observation: Subscription,
}

impl EventEmitter<DismissEvent> for SyncModal {}
impl ModalView for SyncModal {}

impl Focusable for SyncModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SyncModal {
    pub fn new(session: Entity<SyncSession>, cx: &mut Context<Self>) -> Self {
        let observation = cx.observe(&session, |_this, _session, cx| cx.notify());
        Self {
            session,
            focus_handle: cx.focus_handle(),
            _observation: observation,
        }
    }

    /// The extensions this account has that this machine does not, and the one
    /// button that installs them.
    ///
    /// Invariant 7 lives here: pulling produced this list and nothing else.
    /// Installing is a separate, explicit press — a sync payload that installed
    /// code on arrival would be a supply-chain hole with the user's own account
    /// as the key. There is deliberately no bulk-uninstall counterpart:
    /// removing the wrong extension costs more than keeping a spare one.
    fn missing_extensions_row(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let missing = self.session.read(cx).missing_extensions().to_vec();
        if missing.is_empty() {
            return None;
        }
        let colors = cx.theme().colors().clone();
        let count = missing.len();
        let listed = missing.join(", ");

        Some(
            v_flex()
                .w_full()
                .px_3()
                .py_2()
                .gap_1()
                .bg(colors.editor_background)
                .border_b_1()
                .border_color(colors.border_variant)
                .child(
                    Label::new(format!(
                        "{count} extension(s) in your account are not installed here"
                    ))
                    .size(LabelSize::Small),
                )
                .child(
                    Label::new(listed)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .buffer_font(cx),
                )
                .child(
                    Button::new("sync-install-missing", "Install all")
                        .label_size(LabelSize::Small)
                        .tooltip(Tooltip::text("Installs from the registry, one at a time"))
                        .on_click(cx.listener(move |_this, _, _window, cx| {
                            let store = ExtensionStore::global(cx);
                            store.update(cx, |store, cx| {
                                for id in &missing {
                                    store.install_latest_extension(id.as_str().into(), cx);
                                }
                            });
                        })),
                )
                .into_any_element(),
        )
    }

    fn row(
        &self,
        kind: Kind,
        busy: bool,
        border: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .justify_between()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                v_flex().child(Label::new(title_for(kind))).child(
                    Label::new(subtitle_for(kind))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new(("sync-pull", kind as usize), "Pull")
                            .label_size(LabelSize::Small)
                            .disabled(busy)
                            .tooltip(Tooltip::text(
                                "Fetch the stored copy and show what would change",
                            ))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                // Extensions are derived from what is
                                // installed rather than read from a file, so
                                // the list is gathered here and handed across.
                                if kind == Kind::Extensions {
                                    let installed = installed_extensions(cx);
                                    this.session.update(cx, |session, cx| {
                                        session.pull_extensions(installed, cx)
                                    });
                                } else {
                                    this.session
                                        .update(cx, |session, cx| session.pull(kind, cx));
                                }
                            })),
                    )
                    .child(
                        Button::new(("sync-push", kind as usize), "Push")
                            .style(ButtonStyle::Filled)
                            .label_size(LabelSize::Small)
                            .disabled(busy)
                            .tooltip(Tooltip::text("Send this machine's copy"))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                if kind == Kind::Extensions {
                                    let installed = installed_extensions(cx);
                                    this.session.update(cx, |session, cx| {
                                        session.push_extensions(installed, cx)
                                    });
                                } else {
                                    this.session
                                        .update(cx, |session, cx| session.push(kind, cx));
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn title_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Settings => "Settings",
        Kind::Keymap => "Key bindings",
        Kind::Extensions => "Extensions",
    }
}

fn subtitle_for(kind: Kind) -> &'static str {
    match kind {
        Kind::Settings => "settings.json, encrypted before it leaves this machine",
        Kind::Keymap => "keymap.json",
        // Says what pulling will and will not do, on the row itself.
        Kind::Extensions => "the list only — pulling never installs anything",
    }
}

impl Render for SyncModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors().clone();
        let status = self.session.read(cx).status().clone();
        let has_key = self.session.read(cx).has_key();
        let busy = status == SyncStatus::Working;

        let (message, tone) = match &status {
            SyncStatus::Idle => (String::new(), Color::Muted),
            SyncStatus::Working => ("Working…".into(), Color::Muted),
            SyncStatus::Done(text) => (text.to_string(), Color::Success),
            SyncStatus::NeedsKey => (
                "This machine has no recovery key yet.".into(),
                Color::Warning,
            ),
            SyncStatus::KeyMismatch => (
                "The stored data was encrypted with a different recovery key.".into(),
                Color::Warning,
            ),
            SyncStatus::Failed(text) => (text.to_string(), Color::Error),
        };

        let disabled = busy || !has_key;
        let mut rows: Vec<_> = Kind::ALL
            .into_iter()
            .map(|kind| self.row(kind, disabled, colors.border_variant, cx))
            .collect();
        rows.extend(self.missing_extensions_row(cx));

        v_flex()
            .key_context("SyncModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(34.))
            .overflow_hidden()
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .child(Label::new("Sync settings").weight(FontWeight::MEDIUM))
                    .child(
                        Label::new(
                            "Encrypted on this machine. The server never sees the contents.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
            .child(
                v_flex()
                    .border_t_1()
                    .border_color(colors.border_variant)
                    // Built before the container so the borrow of `cx` ends
                    // before the container's own styling reads the theme.
                    .children(rows),
            )
            .child(
                h_flex()
                    .w_full()
                    .p_2()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .bg(colors.editor_background)
                    .child(Label::new(message).size(LabelSize::XSmall).color(tone))
                    .child(
                        Button::new("sync-close", "Close")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
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

    fn session(cx: &mut TestAppContext) -> Entity<SyncSession> {
        let account = cx.update(|cx| {
            cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: Some("Ada".into()),
                    avatar_url: None,
                }))
            })
        });
        cx.update(|cx| cx.new(|_| SyncSession::new(account)))
    }

    /// Draws the window for real at each status.
    ///
    /// This catches a panic in the render path — including the borrow of the
    /// theme that had to be untangled from the row builder. It does NOT catch
    /// a missing background: a modal without `elevation_3` renders perfectly
    /// and simply looks wrong, which is why phase-09 also carries a
    /// look-at-it-on-a-real-machine cell in the matrix.
    fn draw_with(status: SyncStatus, cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(cx);
        session.update(cx, |session, cx| session.set_status_for_test(status, cx));

        let (_modal, cx) = cx.add_window_view(|_window, cx| SyncModal::new(session, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_draws_when_idle(cx: &mut TestAppContext) {
        draw_with(SyncStatus::Idle, cx);
    }

    #[gpui::test]
    fn the_window_draws_without_a_key(cx: &mut TestAppContext) {
        draw_with(SyncStatus::NeedsKey, cx);
    }

    #[gpui::test]
    fn the_window_draws_after_a_failure(cx: &mut TestAppContext) {
        draw_with(
            SyncStatus::Failed("the sync service is unreachable".into()),
            cx,
        );
    }

    /// Push and pull must be unavailable while this machine has no key —
    /// otherwise the first button press produces an error instead of an
    /// explanation.
    #[gpui::test]
    fn the_transfer_buttons_are_disabled_without_a_key(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(cx);
        session.update(cx, |session, _| {
            assert!(!session.has_key(), "a fresh session must start with no key");
        });
    }
}
