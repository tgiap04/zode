use std::time::Duration;

use gpui::{App, Entity, Focusable as _, WeakEntity, Window};
use terminal::{ProgramCopy, Terminal};
use terminal_view::TerminalView;
use workspace::PopoverLayer;

use crate::{
    ReplaceBlocked, ReplaceTarget, SelectionSource, TranslateSelection, TranslationPopover,
    mouse_position_within,
};

/// Shows the popover for the terminal's selection or, without one, for what
/// the program in the terminal has just copied. Returns `false` when there is
/// neither, so the caller can hand the keystroke back to the program.
///
/// A selection is the user's own and is sent at once. A program's copy is
/// text the program picked (a remote host over ssh can set it too), so the
/// popover shows it and waits for the user to confirm.
///
/// The program's copy matters because TUIs that read the mouse (Claude Code's
/// full-screen mode, vim with `mouse=a`) select on their own screen: the drag
/// never reaches the terminal, which is left with no selection. The general
/// clipboard is never read: it may hold anything from any app.
pub(crate) fn translate_selection(
    terminal_view: &Entity<TerminalView>,
    action: &TranslateSelection,
    popover_layer: &Entity<PopoverLayer>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let terminal = terminal_view.read(cx).terminal().clone();
    let content = &terminal.read(cx).last_content;
    let bounds = content.terminal_bounds.bounds;
    let selected = content
        .selection_text
        .clone()
        .filter(|text| !text.trim().is_empty());
    // A copy that followed the user's own drag on the program's screen is
    // the user's selection: sent at once, and its input can be replaced.
    let users_copy = selected.is_none() && terminal.read(cx).recent_program_copy_followed_mouse();
    let (text, too_long, program_copy) = match selected {
        Some(text) => (text, None, false),
        // Offered once: whether or not the user translates it, a second key
        // press does not bring the same text back.
        None => match terminal.update(cx, |terminal, _| terminal.take_recent_program_copy()) {
            Some(ProgramCopy::Text(text)) if !text.trim().is_empty() => (text, None, !users_copy),
            Some(ProgramCopy::TooLong(length)) => (String::new(), Some(length), false),
            Some(ProgramCopy::Text(_)) | None => return false,
        },
    };
    let source = SelectionSource {
        text,
        position: mouse_position_within(bounds, window),
        replace_target: users_copy
            .then(|| ReplaceTarget::TerminalInput(TerminalInputTarget::new(terminal.downgrade()))),
        too_long,
        program_copy,
    };
    let popover = TranslationPopover::deploy_on(
        source,
        action.from.clone(),
        action.to.clone(),
        popover_layer,
        window,
        cx,
    );
    let terminal_focus = terminal_view.focus_handle(cx);
    popover.update(cx, |popover, cx| {
        popover.follow_focus_of(&terminal_focus, window, cx);
        popover.dismiss_on_release(terminal_view, cx);
        // Typing clears the selection, which raises this event, so a popover
        // for a program copy closes on typing too.
        popover.dismiss_on(
            &terminal,
            |event: &terminal::Event| matches!(event, terminal::Event::SelectionsChanged),
            cx,
        );
    });
    true
}

/// The input of the program in a terminal -- the agent's prompt -- as a place a
/// translation can be written back to.
///
/// The terminal cannot see where the program's input cursor is, so the
/// selection cannot be replaced in place. The whole input is: Ctrl+C clears
/// what is typed there (Claude Code's prompt, and a shell's line alike), and
/// the translation is pasted after it. That fits the case this is for: a
/// prompt typed in one language, selected whole, and sent in another.
pub struct TerminalInputTarget {
    terminal: WeakEntity<Terminal>,
}

/// How many pty writes in [`BUSY_WINDOW`] mean the program is still working.
/// A program at rest writes nothing; an agent answering writes many times a
/// second.
const BUSY_WRITES: usize = 3;
const BUSY_WINDOW: Duration = Duration::from_secs(1);

impl TerminalInputTarget {
    pub fn new(terminal: WeakEntity<Terminal>) -> Self {
        Self { terminal }
    }

    pub fn check(&self) -> Result<(), ReplaceBlocked> {
        self.terminal
            .upgrade()
            .map(|_| ())
            .ok_or(ReplaceBlocked::TerminalClosed)
    }

    /// Refused while the program is writing, checked at the press rather than
    /// on every render: Ctrl+C to an agent mid-answer interrupts it instead of
    /// clearing its input.
    pub fn replace(&self, replacement: &str, cx: &mut App) -> Result<(), ReplaceBlocked> {
        let terminal = self
            .terminal
            .upgrade()
            .ok_or(ReplaceBlocked::TerminalClosed)?;
        if terminal.read(cx).pty_writes_within(BUSY_WINDOW) >= BUSY_WRITES {
            return Err(ReplaceBlocked::TerminalBusy);
        }
        terminal.update(cx, |terminal, _| {
            let bracketed = terminal
                .last_content()
                .mode
                .contains(terminal::alacritty_terminal::term::TermMode::BRACKETED_PASTE);
            terminal.input(b"\x03".to_vec());
            terminal.paste(&text_for_input(replacement, bracketed));
        });
        Ok(())
    }
}

/// The translation as it may be typed into a program's input.
///
/// It came back from a translation service, and the pty takes every byte as
/// a key: a control character would act (another Ctrl+C, an escape sequence),
/// and without bracketed paste each line break is Enter, which submits half
/// the text. So controls are dropped, and line breaks become spaces unless the
/// program takes the paste as one block.
pub(crate) fn text_for_input(replacement: &str, bracketed: bool) -> String {
    replacement
        .chars()
        .filter_map(|character| match character {
            '\n' if bracketed => Some('\n'),
            '\n' | '\r' | '\t' => Some(' '),
            character if character.is_control() => None,
            character => Some(character),
        })
        .collect()
}
