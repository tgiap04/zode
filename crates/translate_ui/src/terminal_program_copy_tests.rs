//! The terminal as a translation source when the program in it does its own
//! selecting, as Claude Code's full-screen mode does in an agent tab: the drag
//! goes to the program, which copies with OSC 52, and the terminal itself has
//! no selection.

use std::time::Duration;

use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    TestAppContext, Window, div,
};
use terminal::PROGRAM_COPY_LIFETIME;
use terminal_view::TerminalView;
use workspace::{Item, Workspace, item::ItemHandle};

use crate::PopoverState;
use crate::read_only_sources_tests::{KEYSTROKE_BYTES, TerminalHarness};

/// The binding the bundled keymaps use for terminals.
const TERMINAL_CONTEXT: &str = "Terminal";
const COPIED: &str = "Xin chao";
/// `COPIED` sent the way a TUI sends a copy: OSC 52, base64, BEL-terminated.
const OSC_52_COPY: &[u8] = b"\x1b]52;c;WGluIGNoYW8=\x07";

/// Built the way `agent_ui::AgentView` hosts the agent's CLI: a tab that is
/// not the terminal view, draws it as a child, keeps its own focus handle,
/// and hands the terminal's out as the item's.
struct AgentTab {
    focus_handle: FocusHandle,
    terminal_view: Entity<TerminalView>,
}

impl EventEmitter<()> for AgentTab {}

impl Focusable for AgentTab {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.terminal_view.focus_handle(cx)
    }
}

impl Render for AgentTab {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .child(div().flex_1().min_h_0().child(self.terminal_view.clone()))
    }
}

impl Item for AgentTab {
    type Event = ();

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Claude Code".into()
    }
}

fn in_agent_tab(
    terminal_view: Entity<TerminalView>,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Box<dyn ItemHandle> {
    Box::new(cx.new(|cx| AgentTab {
        focus_handle: cx.focus_handle(),
        terminal_view,
    }))
}

impl TerminalHarness {
    fn program_copies(&mut self, output: &[u8]) {
        self.terminal.update(&mut self.shared.cx, |terminal, cx| {
            terminal.write_output(output, cx)
        });
        self.sync();
    }

    fn assert_key_reached_the_program(&mut self, case: &str) {
        assert!(self.shared.popover().is_none(), "{case}: no popover");
        assert_eq!(self.shared.provider.call_count(), 0, "{case}");
        assert_eq!(
            self.take_input(),
            vec![KEYSTROKE_BYTES.to_vec()],
            "{case}: the program gets the key"
        );
    }
}

#[gpui::test]
async fn a_selection_in_an_agent_tab_terminal_opens_the_popover(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    let selected = harness.select_all();
    let bounds = harness.bounds();
    harness.shared.move_mouse(bounds.center());
    harness.take_input();

    harness.shared.press();

    harness
        .shared
        .assert_read_only_popover_for(&selected, bounds);
    assert!(harness.take_input().is_empty());
}

#[gpui::test]
async fn a_program_copy_in_an_agent_tab_opens_the_popover(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.program_copies(OSC_52_COPY);
    let bounds = harness.bounds();
    let inside = bounds.center();
    harness.shared.move_mouse(inside);
    harness.take_input();

    harness.shared.press();

    harness.shared.assert_read_only_popover_for(COPIED, bounds);
    assert_eq!(
        harness
            .shared
            .expect_popover()
            .read_with(&mut harness.shared.cx, |popover, _| popover.position()),
        inside,
        "the popover opens at the mouse"
    );
    assert!(
        harness.take_input().is_empty(),
        "the keystroke went to the translation, not the program"
    );
    assert_eq!(
        harness
            .shared
            .cx
            .update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text()),
        Some(COPIED.to_string()),
        "the copy still lands on the clipboard"
    );
}

#[gpui::test]
async fn the_terminal_selection_wins_over_a_program_copy(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    harness.program_copies(OSC_52_COPY);
    let selected = harness.select_all();
    assert_ne!(selected, COPIED);
    let bounds = harness.bounds();
    harness.shared.move_mouse(bounds.center());

    harness.shared.press();

    harness
        .shared
        .assert_read_only_popover_for(&selected, bounds);
}

#[gpui::test]
async fn user_input_forgets_the_program_copy(cx: &mut TestAppContext) {
    for input in ["typing", "pasting"] {
        let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
        harness.program_copies(OSC_52_COPY);
        match input {
            "typing" => harness.shared.cx.simulate_keystrokes("a"),
            _ => harness
                .terminal
                .update(&mut harness.shared.cx, |terminal, _| terminal.paste("b")),
        }
        harness.sync();
        assert!(!harness.take_input().is_empty(), "{input} reached the pty");

        harness.shared.press();

        harness.assert_key_reached_the_program(input);
    }
}

#[gpui::test]
async fn a_program_copy_expires(cx: &mut TestAppContext) {
    for (elapsed, offered) in [
        (PROGRAM_COPY_LIFETIME - Duration::from_secs(1), true),
        (PROGRAM_COPY_LIFETIME, false),
    ] {
        let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
        harness.program_copies(OSC_52_COPY);
        harness.take_input();
        harness.shared.cx.executor().advance_clock(elapsed);

        harness.shared.press();

        let case = format!("after {elapsed:?}");
        if offered {
            assert!(harness.shared.popover().is_some(), "{case}");
            assert_eq!(harness.shared.provider.request(0).text, COPIED, "{case}");
        } else {
            harness.assert_key_reached_the_program(&case);
        }
    }
}

#[gpui::test]
async fn an_oversized_program_copy_reports_its_length(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    // 30,003 characters: one batch of three past the cap.
    let mut copy = b"\x1b]52;c;".to_vec();
    copy.extend("YWFh".repeat(10_001).into_bytes());
    copy.push(0x07);
    harness.program_copies(&copy);

    harness.shared.press();

    let popover = harness.shared.expect_popover();
    let state = popover.read_with(&mut harness.shared.cx, |popover, _| popover.state().clone());
    assert!(
        matches!(state, PopoverState::TooLong { length: 30_003, .. }),
        "{state:?}"
    );
    assert_eq!(harness.shared.provider.call_count(), 0);
}

#[gpui::test]
async fn the_general_clipboard_is_never_translated(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    harness
        .shared
        .cx
        .update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(COPIED.to_string())));
    harness.take_input();

    harness.shared.press();

    harness.assert_key_reached_the_program("clipboard written by another app");
}
