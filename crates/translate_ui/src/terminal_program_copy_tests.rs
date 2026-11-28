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
use gpui::Modifiers;

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

    /// What the popover shows for a program copy before anything is sent.
    fn confirmation(&mut self) -> Option<(String, usize)> {
        let popover = self.shared.expect_popover();
        match popover.read_with(&mut self.shared.cx, |popover, _| popover.state().clone()) {
            PopoverState::ConfirmProgramCopy {
                preview,
                characters,
            } => Some((preview.to_string(), characters)),
            _ => None,
        }
    }

    /// Clicks the rendered button that sends the program's copy.
    fn click_translate(&mut self) {
        let button = self
            .shared
            .cx
            .debug_bounds("translate-confirm-program-copy")
            .expect("the confirm button is rendered");
        self.shared
            .cx
            .simulate_click(button.center(), Modifiers::none());
        self.shared.cx.run_until_parked();
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

    assert_eq!(harness.confirmation(), Some((COPIED.to_string(), 8)));
    assert_eq!(harness.shared.provider.call_count(), 0, "nothing sent yet");
    harness.click_translate();

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
            harness.click_translate();
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

/// The program chooses what it copies, possibly a remote host over ssh, so
/// the user sees what would leave before it does.
#[gpui::test]
async fn a_program_copy_is_shown_and_not_sent_until_confirmed(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    harness.program_copies(OSC_52_COPY);
    harness.take_input();

    harness.shared.press();
    harness
        .shared
        .cx
        .executor()
        .advance_clock(Duration::from_secs(5));
    harness.shared.cx.run_until_parked();

    assert_eq!(harness.confirmation(), Some((COPIED.to_string(), 8)));
    assert_eq!(harness.shared.provider.call_count(), 0);
    assert!(
        harness
            .shared
            .cx
            .debug_bounds("translate-program-copy-notice")
            .is_some(),
        "the popover says the text is the program's copy, not a selection"
    );

    harness.shared.cx.simulate_keystrokes("escape");
    harness.shared.cx.run_until_parked();
    assert!(harness.shared.popover().is_none());
    assert_eq!(harness.shared.provider.call_count(), 0, "dismissed unsent");
}

#[gpui::test]
async fn enter_confirms_a_program_copy_without_reaching_the_program(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    assert_eq!(harness.shared.provider.call_count(), 0);

    harness.shared.cx.simulate_keystrokes("enter");
    harness.shared.cx.run_until_parked();

    assert_eq!(harness.shared.provider.call_count(), 1);
    assert_eq!(harness.shared.provider.request(0).text, COPIED);
    assert!(
        harness.take_input().is_empty(),
        "Enter was not typed into the program"
    );
}

#[gpui::test]
async fn a_program_copy_is_offered_once(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    assert!(harness.shared.popover().is_some());
    harness.shared.cx.simulate_keystrokes("escape");
    harness.shared.cx.run_until_parked();
    assert!(harness.shared.popover().is_none());

    harness.shared.press();

    harness.assert_key_reached_the_program("the copy was already offered");
}

/// A selection the user made is theirs: it is sent on the key press as before.
#[gpui::test]
async fn a_terminal_selection_is_sent_without_a_confirmation(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new(TERMINAL_CONTEXT, cx).await;
    let selected = harness.select_all();
    harness.shared.press();

    assert_eq!(harness.shared.provider.request(0).text, selected);
    assert_eq!(harness.confirmation(), None);
}

impl TerminalHarness {
    /// A drag on a program that reads the mouse, as Claude Code does: the
    /// release goes to the program, which then copies what was dragged over.
    fn drag_on_the_program(&mut self) {
        self.terminal.update(&mut self.shared.cx, |terminal, cx| {
            // DECSET 1000 and 1006: report button presses, in SGR form.
            terminal.write_output(b"\x1b[?1000h\x1b[?1006h", cx);
        });
        self.sync();
        let center = self.bounds().center();
        self.terminal.update(&mut self.shared.cx, |terminal, cx| {
            terminal.mouse_up(
                &gpui::MouseUpEvent {
                    button: gpui::MouseButton::Left,
                    position: center,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                },
                cx,
            );
        });
    }
}

/// Text a program copies right after the user dragged across it is the
/// user's own selection, made on the program's screen: it is translated at
/// once, without telling the user they did not select it.
#[gpui::test]
async fn a_copy_right_after_a_drag_on_the_program_is_the_users(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();

    harness.shared.press();

    assert_eq!(harness.confirmation(), None, "no confirmation step");
    assert_eq!(harness.shared.provider.call_count(), 1, "sent at once");
    assert!(
        harness
            .shared
            .cx
            .debug_bounds("translate-program-copy-notice")
            .is_none(),
        "and no notice that the user did not select it"
    );
}

/// In the agent's input, Replace clears what is typed there and pastes the
/// translation in its place.
#[gpui::test]
async fn replace_input_clears_the_agents_input_and_pastes(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    harness.shared.provider.answer(0, Ok("Hello".into()));
    harness
        .shared
        .cx
        .executor()
        .advance_clock(Duration::from_secs(3));
    harness.shared.cx.run_until_parked();

    let button = harness
        .shared
        .cx
        .debug_bounds("translate-replace")
        .expect("Replace is offered for the agent's input");
    harness
        .shared
        .cx
        .simulate_click(button.center(), Modifiers::none());
    harness.shared.cx.run_until_parked();

    let sent: Vec<u8> = harness.take_input().concat();
    assert_eq!(sent.first(), Some(&0x03), "Ctrl+C clears the input first");
    assert!(
        String::from_utf8_lossy(&sent).contains("Hello"),
        "then the translation is pasted: {sent:?}"
    );
    assert!(harness.shared.popover().is_none(), "and the popover closes");
}

/// While the agent is still writing, Replace sends nothing: Ctrl+C to an agent
/// mid-answer interrupts it instead of clearing its input. The popover says
/// so and keeps the button for once it stops.
#[gpui::test]
async fn replace_input_waits_while_the_agent_is_writing(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    harness.shared.provider.answer(0, Ok("Hello".into()));
    harness.shared.cx.run_until_parked();
    // Fed the way a pty's output arrives, which is what counts as writing.
    for _ in 0..5 {
        harness
            .terminal
            .update(&mut harness.shared.cx, |terminal, cx| {
                terminal.feed_remote_output(b"still answering\r\n", cx)
            });
    }
    harness.sync();

    let button = harness
        .shared
        .cx
        .debug_bounds("translate-replace")
        .expect("Replace is offered");
    harness
        .shared
        .cx
        .simulate_click(button.center(), Modifiers::none());
    harness.shared.cx.run_until_parked();

    assert!(harness.take_input().is_empty(), "nothing reaches the agent");
    assert!(harness.shared.popover().is_some(), "the popover stays");
    assert!(
        harness
            .shared
            .cx
            .debug_bounds("translate-replace-blocked-reason")
            .is_some(),
        "and says why"
    );
}

/// The window is what makes a program's copy the user's: one that comes
/// later is the program's own doing again, whatever the user clicked before.
#[gpui::test]
async fn a_copy_long_after_a_drag_still_asks(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness
        .shared
        .cx
        .executor()
        .advance_clock(terminal::USER_COPY_WINDOW + Duration::from_secs(1));
    harness.program_copies(OSC_52_COPY);
    harness.take_input();

    harness.shared.press();

    assert_eq!(harness.confirmation(), Some((COPIED.to_string(), 8)));
    assert_eq!(harness.shared.provider.call_count(), 0, "not sent unasked");
}

/// A copy the program made before the user touched it was not the user's.
/// The click reaches the program as input, which forgets the copy outright.
#[gpui::test]
async fn a_copy_before_the_drag_is_not_sent_unasked(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.program_copies(OSC_52_COPY);
    harness
        .shared
        .cx
        .executor()
        .advance_clock(Duration::from_millis(100));
    harness.drag_on_the_program();
    harness.take_input();

    harness.shared.press();

    assert_eq!(harness.shared.provider.call_count(), 0, "not sent unasked");
}

/// One release vouches for one copy: a second copy the program makes on its
/// own straight after is not the user's.
#[gpui::test]
async fn one_drag_vouches_for_one_copy(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    assert_eq!(harness.shared.provider.call_count(), 1, "the user's copy");
    harness.shared.cx.simulate_keystrokes("escape");
    harness.shared.cx.run_until_parked();

    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();

    assert_eq!(harness.confirmation(), Some((COPIED.to_string(), 8)));
    assert_eq!(harness.shared.provider.call_count(), 1, "not sent again");
}

/// A translation is typed as text only: control characters in it would act
/// as keys, and a line break as Enter.
#[gpui::test]
async fn replace_input_types_no_control_characters(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    harness
        .shared
        .provider
        .answer(0, Ok("Good\x03 day\x1b[2J\nthere".into()));
    harness
        .shared
        .cx
        .executor()
        .advance_clock(Duration::from_secs(3));
    harness.shared.cx.run_until_parked();

    let button = harness
        .shared
        .cx
        .debug_bounds("translate-replace")
        .expect("Replace is offered");
    harness
        .shared
        .cx
        .simulate_click(button.center(), Modifiers::none());
    harness.shared.cx.run_until_parked();

    let sent: Vec<u8> = harness.take_input().concat();
    assert_eq!(
        sent.iter().filter(|byte| **byte == 0x03).count(),
        1,
        "only the clearing Ctrl+C: {sent:?}"
    );
    let text = String::from_utf8_lossy(&sent);
    assert!(text.contains("day") && text.contains("there"), "{text:?}");
    assert!(!text.contains('\r'), "no Enter: {text:?}");
}

#[test]
fn a_line_break_stays_one_only_inside_a_bracketed_paste() {
    use crate::terminal_source::text_for_input;
    assert_eq!(text_for_input("a\nb\tc\x07", true), "a\nb c");
    assert_eq!(text_for_input("a\r\nb", false), "a  b");
    assert_eq!(text_for_input("", false), "");
}

/// Two writes in the window still read as an idle agent; the third is one
/// that is answering.
#[gpui::test]
async fn replace_input_goes_through_below_the_busy_threshold(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new_in(TERMINAL_CONTEXT, in_agent_tab, cx).await;
    harness.drag_on_the_program();
    harness.program_copies(OSC_52_COPY);
    harness.take_input();
    harness.shared.press();
    harness.shared.provider.answer(0, Ok("Hello".into()));
    harness.shared.cx.run_until_parked();
    for _ in 0..2 {
        harness
            .terminal
            .update(&mut harness.shared.cx, |terminal, cx| {
                terminal.feed_remote_output(b"cursor blink", cx)
            });
    }
    harness.sync();

    let button = harness
        .shared
        .cx
        .debug_bounds("translate-replace")
        .expect("Replace is offered");
    harness
        .shared
        .cx
        .simulate_click(button.center(), Modifiers::none());
    harness.shared.cx.run_until_parked();

    assert_eq!(harness.take_input().concat().first(), Some(&0x03));
}
