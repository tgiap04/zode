//! The QR code a phone scans to pair, and how long it stays good.
//!
//! The code is the confirmation that the phone and this screen are in the same
//! place, so it is shown only while this modal is open: closing it takes the
//! code down, and the host refuses a proof made from it. While it is open the
//! code is replaced as soon as it runs out, so the person never has to ask.

use std::time::Duration;

use gpui::{
    App, Bounds, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    Subscription, Task, Window, black, canvas, fill, point, px, size, white,
};
use qrcode::{Color as ModuleColor, EcLevel, QrCode};
use ui::{Modal, ModalFooter, ModalHeader, Section, prelude::*};
use workspace::{ModalView, Workspace};

use crate::remote_host::{HostState, QrPairing, RemoteHost, RemoteHostEvent, problem_message};

/// How often the countdown and the expiry are looked at.
const TICK: Duration = Duration::from_secs(1);
/// Pixels to a module. A whole number so the modules stay sharp, and large
/// enough that a phone held at arm's length reads them.
const MODULE_PIXELS: f32 = 5.;
/// The blank border a scanner needs to find the code's edges, in modules. The
/// QR standard asks for four.
const QUIET_ZONE_MODULES: f32 = 4.;

/// A QR code as runs of dark modules, which is far fewer quads to paint than
/// one per module.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct QrMatrix {
    pub(crate) width: usize,
    /// `(column, row, length)` of each run of dark modules along a row.
    pub(crate) dark_runs: Vec<(usize, usize, usize)>,
}

impl QrMatrix {
    /// `None` when `text` does not fit in a QR code, which the text of a
    /// pairing code never should.
    pub(crate) fn encode(text: &str) -> Option<Self> {
        // Medium error correction: the code is read from a screen, which does
        // not smudge, and a smaller code is easier to read at a distance.
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
        let width = code.width();
        let modules = code.to_colors();
        let mut dark_runs = Vec::new();
        for (row, line) in modules.chunks(width).enumerate() {
            let mut run_start = None;
            for (column, module) in line.iter().chain([&ModuleColor::Light]).enumerate() {
                match (module, run_start) {
                    (ModuleColor::Dark, None) => run_start = Some(column),
                    (ModuleColor::Light, Some(start)) => {
                        dark_runs.push((start, row, column - start));
                        run_start = None;
                    }
                    _ => {}
                }
            }
        }
        Some(Self { width, dark_runs })
    }
}

/// "1:42", rounded up so the clock never reads 0:00 while the code still works.
pub(crate) fn remaining_text(remaining: Duration) -> String {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// What a notice in place of the code offers to do about it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NoticeAction {
    None,
    SignIn,
    Unlock,
}

struct ShownCode {
    uri: String,
    matrix: QrMatrix,
}

pub struct QrPairingModal {
    host: Entity<RemoteHost>,
    shown: Option<ShownCode>,
    /// A phone paired with this code. The modal is closing; nothing is shown
    /// or made again.
    finished: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _tick: Task<()>,
}

impl EventEmitter<DismissEvent> for QrPairingModal {}
impl ModalView for QrPairingModal {}

impl Focusable for QrPairingModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Opens the modal in `workspace`, turning remote control on if it is off.
/// Signed out, the modal says so and offers to sign in.
pub(crate) fn open(
    host: Entity<RemoteHost>,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if matches!(host.read(cx).state(), HostState::Disabled) {
        crate::enable_remote_control(cx);
    }
    workspace.toggle_modal(window, cx, move |window, cx| {
        QrPairingModal::new(host, window, cx)
    });
}

impl QrPairingModal {
    pub fn new(host: Entity<RemoteHost>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&host, |this, _host, cx| this.sync(cx)),
            cx.subscribe(&host, |this, _host, event: &RemoteHostEvent, cx| {
                if matches!(event, RemoteHostEvent::DevicePaired { .. }) {
                    this.finished = true;
                    cx.emit(DismissEvent);
                }
            }),
            // Taken down with the modal, however it closes: a code nobody can
            // see must not still pair a device.
            cx.on_release({
                let host = host.clone();
                move |_, cx| host.update(cx, |host, cx| host.stop_qr_pairing(cx))
            }),
        ];
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.sync(cx)).is_err() {
                    break;
                }
            }
        });
        let mut modal = Self {
            host,
            shown: None,
            finished: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
            _tick: tick,
        };
        modal.sync(cx);
        modal
    }

    /// Brings what is drawn in line with the host: a new code when the old one
    /// ran out or was used, and a new countdown.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        let host = self.host.clone();
        host.update(cx, |host, cx| host.show_qr_pairing(cx));
        let current = host.read(cx).qr_pairing(cx);
        match current {
            Some(QrPairing { uri, .. }) => {
                if self.shown.as_ref().is_none_or(|shown| shown.uri != uri) {
                    self.shown = QrMatrix::encode(&uri).map(|matrix| ShownCode { uri, matrix });
                }
            }
            None => self.shown = None,
        }
        cx.notify();
    }

    fn render_code(
        &self,
        matrix: &QrMatrix,
        confirm_code: &str,
        remaining: Duration,
        cx: &App,
    ) -> impl IntoElement {
        let grid = px(matrix.width as f32 * MODULE_PIXELS);
        let runs = matrix.dark_runs.clone();
        v_flex()
            .gap_2()
            .items_center()
            .child(
                // Black on white whatever the theme: a scanner reads contrast,
                // not taste, and a dark-theme inversion is read by fewer.
                div()
                    .debug_selector(|| "remote-qr-code".into())
                    .bg(white())
                    .p(px(QUIET_ZONE_MODULES * MODULE_PIXELS))
                    .rounded_md()
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                for (column, row, length) in runs {
                                    let origin = bounds.origin
                                        + point(
                                            px(column as f32 * MODULE_PIXELS),
                                            px(row as f32 * MODULE_PIXELS),
                                        );
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            origin,
                                            size(
                                                px(length as f32 * MODULE_PIXELS),
                                                px(MODULE_PIXELS),
                                            ),
                                        ),
                                        black(),
                                    ));
                                }
                            },
                        )
                        .size(grid),
                    ),
            )
            .child(
                v_flex()
                    .debug_selector(|| "remote-qr-confirm-code".into())
                    .items_center()
                    .child(
                        Label::new("Confirmation code, if the phone asks for it")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(confirm_code.to_string())
                            .size(LabelSize::Large)
                            .weight(FontWeight::MEDIUM)
                            .buffer_font(cx),
                    ),
            )
            .child(
                Label::new(format!("Expires in {}", remaining_text(remaining)))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
    }

    fn render_notice(
        &self,
        notice: String,
        action: NoticeAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .w_full()
            .py_4()
            .gap_2()
            .items_center()
            .child(
                Label::new(notice)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .when(action == NoticeAction::SignIn, |column| {
                column.child(
                    Button::new("remote-qr-sign-in", "Sign In\u{2026}").on_click(cx.listener(
                        |_, _, window, cx| {
                            window.dispatch_action(Box::new(zode_actions::account::SignIn), cx);
                            cx.emit(DismissEvent);
                        },
                    )),
                )
            })
            .when(action == NoticeAction::Unlock, |column| {
                column.child(
                    // `Button` has no probe of its own, so it is wrapped for a
                    // test to find its bounds and click it.
                    div().debug_selector(|| "remote-qr-unlock".into()).child(
                        Button::new("remote-qr-unlock", "Unlock Pairing").on_click(cx.listener(
                            |this, _, _window, cx| {
                                this.host.update(cx, |host, cx| host.unlock_pairing(cx));
                                this.sync(cx);
                            },
                        )),
                    ),
                )
            })
            .into_any_element()
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let host = self.host.read(cx);
        let state = host.state().clone();
        if host.pairing_locked_out() {
            return self.render_notice(
                "Pairing is locked after repeated failed attempts, so a phone cannot pair \
                 right now. Only unlocking it here lets pairing start again."
                    .to_string(),
                NoticeAction::Unlock,
                cx,
            );
        }
        if host.pending_pairing().is_some_and(|pending| pending.via_qr) {
            return self.render_notice(
                "A phone that read this code is waiting for your answer, so no code is shown."
                    .to_string(),
                NoticeAction::None,
                cx,
            );
        }
        if let (Some(shown), Some(live)) = (&self.shown, host.qr_pairing(cx)) {
            return self
                .render_code(&shown.matrix, &live.confirm_code, live.remaining, cx)
                .into_any_element();
        }
        match state {
            HostState::SignedOut => self.render_notice(
                "Sign in to a Zode account to pair a phone: the phone and this Zode have to \
                 be on the same account."
                    .to_string(),
                NoticeAction::SignIn,
                cx,
            ),
            HostState::Halted(_) => self.render_notice(
                problem_message(&state).unwrap_or_else(|| "Remote control stopped.".to_string()),
                NoticeAction::None,
                cx,
            ),
            HostState::Failed(reason) => self.render_notice(
                format!("Remote control could not start: {reason}"),
                NoticeAction::None,
                cx,
            ),
            HostState::Disabled
            | HostState::Starting
            | HostState::Online
            | HostState::Reconnecting => self.render_notice(
                "Getting the code ready\u{2026}".to_string(),
                NoticeAction::None,
                cx,
            ),
        }
    }
}

impl Render for QrPairingModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("RemoteControlQrPairing")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(28.))
            .max_h(rems(40.))
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(
                Modal::new("remote-qr-pairing", None)
                    .header(
                        ModalHeader::new()
                            .child(Headline::new("Pair a Phone").size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new()
                            .child(
                                Label::new("Scan with the Zode app on your phone")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(div().w_full().child(self.body(cx))),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            Button::new("remote-qr-close", "Close")
                                .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_countdown_reads_minutes_and_seconds_and_rounds_up() {
        assert_eq!(remaining_text(Duration::from_secs(120)), "2:00");
        assert_eq!(remaining_text(Duration::from_secs(102)), "1:42");
        assert_eq!(remaining_text(Duration::from_secs(5)), "0:05");
        assert_eq!(remaining_text(Duration::from_millis(100)), "0:01");
        assert_eq!(remaining_text(Duration::ZERO), "0:00");
    }

    #[test]
    fn a_pairing_code_encodes_to_a_square_whose_runs_stay_inside_it() {
        let text = "zode://pair?v=1&h=dev_abc123&k=n3LqDPSVNuPGbHh_cFGG35pDeAg3U66VNtZbOtf83cQ\
                    &s=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8&e=1795000000";
        let matrix = QrMatrix::encode(text).expect("fits");
        assert!(
            matrix.width >= 21 && (matrix.width - 17).is_multiple_of(4),
            "a QR version: {}",
            matrix.width
        );
        assert!(!matrix.dark_runs.is_empty());
        for (column, row, length) in &matrix.dark_runs {
            assert!(*length >= 1);
            assert!(column + length <= matrix.width, "{column}+{length}");
            assert!(*row < matrix.width);
        }
        // The top-left finder pattern: a dark bar across the first row.
        assert!(
            matrix.dark_runs.contains(&(0, 0, 7)),
            "the finder pattern starts the first row"
        );
    }

    #[test]
    fn text_too_long_for_a_qr_code_is_none_rather_than_a_panic() {
        assert_eq!(QrMatrix::encode(&"x".repeat(10_000)), None);
    }
}
