use gpui::{App, Entity, Focusable as _, Window};
use terminal::ProgramCopy;
use terminal_view::TerminalView;
use workspace::PopoverLayer;

use crate::{SelectionSource, TranslateSelection, TranslationPopover, mouse_position_within};

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
    let (text, too_long, program_copy) = match selected {
        Some(text) => (text, None, false),
        // Offered once: whether or not the user translates it, a second key
        // press does not bring the same text back.
        None => match terminal.update(cx, |terminal, _| terminal.take_recent_program_copy()) {
            Some(ProgramCopy::Text(text)) if !text.trim().is_empty() => (text, None, true),
            Some(ProgramCopy::TooLong(length)) => (String::new(), Some(length), false),
            Some(ProgramCopy::Text(_)) | None => return false,
        },
    };
    let source = SelectionSource {
        text,
        position: mouse_position_within(bounds, window),
        replace_target: None,
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
