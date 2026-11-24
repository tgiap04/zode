//! Pairing and connecting an Android phone over Wi-Fi, from the panel.
//!
//! The pairing code never lives anywhere but the field it is typed in: it is
//! parsed, the field is replaced by an empty one in the same call, so not even
//! its undo history keeps the code, and only the parsed value goes on to adb,
//! on stdin.

use devices::{parse_pairing_code, parse_wireless_address};
use gpui::{
    AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Task, WeakEntity, Window,
};
use ui_input::InputField;
use util::ResultExt as _;

use crate::devices_panel::DevicesPanel;

const PANEL_CLOSED: &str = "The Devices panel is closed.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Pair,
    Connect,
}

pub(crate) struct WirelessModal {
    pub(crate) panel: WeakEntity<DevicesPanel>,
    pub(crate) step: Step,
    /// Whether the connect step was reached by pairing just now.
    pub(crate) paired: bool,
    pub(crate) error: Option<SharedString>,
    pub(crate) pair_address: Entity<InputField>,
    pub(crate) pair_code: Entity<InputField>,
    pub(crate) connect_address: Entity<InputField>,
    pub(crate) working: Option<Task<()>>,
    pub(crate) focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for WirelessModal {}

impl workspace::ModalView for WirelessModal {}

impl Focusable for WirelessModal {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        match self.step {
            Step::Pair => self.pair_address.focus_handle(cx),
            Step::Connect => self.connect_address.focus_handle(cx),
        }
    }
}

impl WirelessModal {
    pub(crate) fn new(
        panel: WeakEntity<DevicesPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            panel,
            step: Step::Pair,
            paired: false,
            error: None,
            pair_address: cx.new(|cx| InputField::new(window, cx, "192.168.1.5:37099")),
            pair_code: Self::new_code_field(window, cx),
            connect_address: cx.new(|cx| InputField::new(window, cx, "192.168.1.5:5555")),
            working: None,
            focus_handle: cx.focus_handle(),
        }
    }

    fn new_code_field(window: &mut Window, cx: &mut Context<Self>) -> Entity<InputField> {
        cx.new(|cx| InputField::new(window, cx, "6-digit code").masked(true))
    }

    /// Clearing alone would leave the code one undo away.
    fn forget_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let old_editor = self.pair_code.read(cx).editor().clone();
        let was_focused = old_editor.focus_handle(cx).is_focused(window);
        old_editor.clear(window, cx);
        self.pair_code = Self::new_code_field(window, cx);
        if was_focused {
            window.focus(&self.pair_code.focus_handle(cx), cx);
        }
    }

    pub(crate) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            Step::Pair => self.pair(window, cx),
            Step::Connect => self.connect(window, cx),
        }
    }

    pub(crate) fn pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let address = self.pair_address.read(cx).text(cx);
        let code = self.pair_code.read(cx).text(cx);
        let (address, code) = match (
            parse_wireless_address(address.trim()),
            parse_pairing_code(code.trim()),
        ) {
            (Ok(address), Ok(code)) => (address, code),
            (Err(error), _) | (_, Err(error)) => {
                self.error = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        // Forgotten before anything can fail or be awaited: from here the code
        // exists only inside `code`, which is moved into the adb command.
        self.forget_code(window, cx);
        let Some(panel) = self.panel.upgrade() else {
            self.error = Some(PANEL_CLOSED.into());
            cx.notify();
            return;
        };
        self.error = None;
        let task = panel.update(cx, |panel, cx| panel.pair(address, code, cx));
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.working = None;
                match result {
                    Ok(()) => this.show_step(Step::Connect, true, window, cx),
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    pub(crate) fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working.is_some() {
            return;
        }
        let address = self.connect_address.read(cx).text(cx);
        let address = match parse_wireless_address(address.trim()) {
            Ok(address) => address,
            Err(error) => {
                self.error = Some(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let Some(panel) = self.panel.upgrade() else {
            self.error = Some(PANEL_CLOSED.into());
            cx.notify();
            return;
        };
        self.error = None;
        let task = panel.update(cx, |panel, cx| panel.connect(address, cx));
        self.working = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, _window, cx| {
                this.working = None;
                match result {
                    Ok(()) => {
                        // The one read that follows a connect: the phone is
                        // there now, and nothing else would list it.
                        this.panel
                            .update(cx, |panel, cx| panel.reload_fresh(cx))
                            .log_err();
                        cx.emit(DismissEvent);
                    }
                    Err(error) => this.error = Some(format!("{error:#}").into()),
                }
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    pub(crate) fn skip_pairing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_step(Step::Connect, false, window, cx);
        cx.notify();
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_step(Step::Pair, false, window, cx);
        cx.notify();
    }

    fn show_step(&mut self, step: Step, paired: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.step = step;
        self.paired = paired;
        self.error = None;
        window.focus(&self.focus_handle(cx), cx);
    }

    pub(crate) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.forget_code(window, cx);
        cx.emit(DismissEvent);
    }
}
