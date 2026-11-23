use gpui::{App, Entity, Focusable as _, Window};
use terminal::ProgramCopy;
use terminal_view::TerminalView;
use workspace::PopoverLayer;

use crate::{SelectionSource, TranslateSelection, TranslationPopover, mouse_position_within};

/// Shows the popover for the terminal's selection or, without one, for what
/// the program in the terminal has just copied. Returns `false` when there is
/// neither, so the caller can hand the keystroke back to the program.
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
    let selected = content
        .selection_text
        .clone()
        .filter(|text| !text.trim().is_empty());
    let (text, too_long) = match selected {
        Some(text) => (text, None),
        None => match terminal.read(cx).recent_program_copy() {
            Some(ProgramCopy::Text(text)) if !text.trim().is_empty() => (text.clone(), None),
            Some(ProgramCopy::TooLong(length)) => (String::new(), Some(*length)),
            Some(ProgramCopy::Text(_)) | None => return false,
        },
    };
    let source = SelectionSource {
        text,
        position: mouse_position_within(content.terminal_bounds.bounds, window),
        replace_target: None,
        too_long,
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
