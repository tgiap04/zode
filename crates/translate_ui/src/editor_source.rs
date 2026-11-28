use std::ops::Range;

use editor::{Anchor, Editor, EditorEvent, MBTextSummary, MultiBufferOffsetUtf16};
use gpui::{
    App, Context, Entity, EntityInputHandler as _, Focusable as _, Pixels, Point, WeakEntity,
    Window,
};
use settings::Settings as _;
use translate::TranslateSettings;
use workspace::Workspace;

use crate::{SelectionSource, TranslateSelection, TranslationPopover, mouse_position_within};

/// Why the translation cannot be written back over the original text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceBlocked {
    EditorClosed,
    ReadOnly,
    TextChanged,
    SpansExcerpts,
    TerminalClosed,
    /// The program in the terminal is still writing: clearing its input now
    /// could interrupt an agent mid-answer instead.
    TerminalBusy,
}

impl std::fmt::Display for ReplaceBlocked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EditorClosed => "The editor was closed.",
            Self::ReadOnly => "The editor is read-only.",
            Self::TextChanged => "The text changed since it was translated.",
            Self::SpansExcerpts => "The selection spans more than one excerpt.",
            Self::TerminalClosed => "The terminal was closed.",
            Self::TerminalBusy => "The agent is still working. Replace once it stops.",
        })
    }
}

/// The span of an editor a translation can be written back to. Anchors keep
/// pointing at the same text while the buffer is edited around it.
pub struct EditorReplaceTarget {
    editor: WeakEntity<Editor>,
    range: Range<Anchor>,
}

impl EditorReplaceTarget {
    pub fn new(editor: WeakEntity<Editor>, range: Range<Anchor>) -> Self {
        Self { editor, range }
    }

    /// Compares chunk by chunk, so the check that runs on every render of the
    /// popover copies nothing, however long the selection is.
    fn holds_text(&self, editor: &Editor, expected: &str, cx: &App) -> bool {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let mut remaining = expected;
        for chunk in snapshot.text_for_range(self.range.clone()) {
            match remaining.strip_prefix(chunk) {
                Some(rest) => remaining = rest,
                None => return false,
            }
        }
        remaining.is_empty()
    }

    /// Whether `original` is still what lies between the anchors of an
    /// editor that can be changed.
    pub fn check(&self, original: &str, cx: &App) -> Result<(), ReplaceBlocked> {
        let editor = self.editor.upgrade().ok_or(ReplaceBlocked::EditorClosed)?;
        let editor = editor.read(cx);
        if editor.read_only(cx) {
            return Err(ReplaceBlocked::ReadOnly);
        }
        // One edit across excerpts would write the translation into the first
        // buffer and delete the selected text from the others.
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        if snapshot.excerpt_containing(self.range.clone()).is_none() {
            return Err(ReplaceBlocked::SpansExcerpts);
        }
        if !self.holds_text(editor, original, cx) {
            return Err(ReplaceBlocked::TextChanged);
        }
        Ok(())
    }

    /// Replaces `original` with `replacement` in one transaction, so a single
    /// undo brings the original back.
    pub fn replace(
        &self,
        original: &str,
        replacement: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<(), ReplaceBlocked> {
        self.check(original, cx)?;
        let editor = self.editor.upgrade().ok_or(ReplaceBlocked::EditorClosed)?;
        let range = self.range.clone();
        editor.update(cx, |editor, cx| {
            let replacement =
                fit_replacement(original, replacement, editor.mode().is_single_line());
            editor.transact(window, cx, |editor, _window, cx| {
                editor.edit([(range, replacement)], cx);
            });
        });
        Ok(())
    }
}

/// What to write over `original` for a provider's `translation`.
///
/// Providers trim their answer, so the selection's own leading and trailing
/// whitespace (indentation, the line break at the end of a selected line) is
/// put back; without it, replacing `"    text\n"` would pull the next line up
/// and drop the indent. A single-line input (a commit subject, a rename
/// field) takes no line break, so runs of them become one space.
fn fit_replacement(original: &str, translation: &str, single_line: bool) -> String {
    let leading_end = original.len() - original.trim_start().len();
    let trailing_start = original.trim_end().len().max(leading_end);
    let fitted = format!(
        "{}{}{}",
        &original[..leading_end],
        translation.trim(),
        &original[trailing_start..]
    );
    if !single_line {
        return fitted;
    }
    let mut collapsed = String::with_capacity(fitted.len());
    let mut in_line_break = false;
    for character in fitted.chars() {
        if matches!(character, '\n' | '\r') {
            if !in_line_break {
                collapsed.push(' ');
            }
            in_line_break = true;
        } else {
            in_line_break = false;
            collapsed.push(character);
        }
    }
    collapsed
}

/// Returns `false`, leaving the key to other handlers, when the editor holds
/// no selection, masks its text (a password or key field), or the focus is in
/// something drawn inside it.
///
/// The popover goes on the workspace of the window, which also covers inputs
/// that are not workspace items (modals, the search bar). A window without a
/// workspace, such as the Settings window, gets it in the editor's own menu.
pub(crate) fn translate_selection(
    editor: &WeakEntity<Editor>,
    action: &TranslateSelection,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(editor) = editor.upgrade() else {
        return false;
    };
    // The editor-hosted popover holds focus in the editor's menu; a second
    // keypress there still belongs to this editor.
    let owns_focus = editor.focus_handle(cx).is_focused(window)
        || editor.read(cx).mouse_menu_is_focused(window, cx);
    if !owns_focus || editor.read(cx).is_masked(cx) {
        return false;
    }
    let Some(source) = selection_source(&editor, window, cx) else {
        return false;
    };
    let (from, to) = (action.from.clone(), action.to.clone());
    let editor_focus = editor.focus_handle(cx);
    // The window's own workspace: the editor's may belong to another window.
    let popover = match Workspace::for_window(window, cx) {
        Some(workspace) => {
            let popover = TranslationPopover::deploy(source, from, to, &workspace, window, cx);
            popover.update(cx, |popover, cx| {
                popover.follow_focus_of(&editor_focus, window, cx)
            });
            popover
        }
        // The menu takes the focus and closes itself when it loses it.
        None => {
            let popover =
                TranslationPopover::deploy_in_editor(source, from, to, &editor, window, cx);
            popover.update(cx, |popover, _| popover.set_escape_scope(editor_focus));
            popover
        }
    };
    popover.update(cx, |popover, cx| {
        popover.dismiss_on(&editor, dismisses_popover, cx);
        popover.dismiss_on_release(&editor, cx);
    });
    true
}

/// The newest selection, as text and anchors, and the window point just
/// below its head. Gives `None` when there is nothing to send: a blank
/// selection, or one over a fold, whose text is not on screen.
///
/// A selection over the length limit is measured from the buffer's summary
/// and never copied, so selecting a huge file costs nothing.
fn selection_source(
    editor: &Entity<Editor>,
    window: &mut Window,
    cx: &mut App,
) -> Option<SelectionSource> {
    let limit = TranslateSettings::get_global(cx).max_characters;
    editor.update(cx, |editor, cx| {
        let display_snapshot = editor.display_snapshot(cx);
        let selection = editor.selections.newest_anchor().clone();
        let range = selection.start..selection.end;
        if display_snapshot
            .folds_in_range(range.clone())
            .next()
            .is_some()
        {
            return None;
        }
        let buffer_snapshot = display_snapshot.buffer_snapshot();
        let length = buffer_snapshot
            .text_summary_for_range::<MBTextSummary, _>(range.clone())
            .chars;
        let (text, too_long) = if length > limit {
            (String::new(), Some(length))
        } else {
            let text: String = buffer_snapshot.text_for_range(range.clone()).collect();
            if text.trim().is_empty() {
                return None;
            }
            (text, None)
        };
        let head = editor
            .selections
            .newest::<MultiBufferOffsetUtf16>(&display_snapshot)
            .head();
        let position =
            head_position(editor, head, window, cx).unwrap_or_else(|| match editor.last_bounds() {
                Some(bounds) => mouse_position_within(*bounds, window),
                None => window.mouse_position(),
            });
        Some(SelectionSource {
            text,
            position,
            replace_target: Some(crate::ReplaceTarget::Editor(EditorReplaceTarget::new(
                cx.weak_entity(),
                range,
            ))),
            too_long,
            program_copy: false,
        })
    })
}

fn dismisses_popover(event: &EditorEvent) -> bool {
    matches!(
        event,
        EditorEvent::SelectionsChanged { local: true }
            | EditorEvent::ScrollPositionChanged { local: true, .. }
    )
}

/// The bottom-left corner of the character at `head`, in window coordinates.
///
/// Goes through the same `bounds_for_range` the IME uses to place its
/// candidate window, which adds the full gutter width (gutter plus its
/// margin) and subtracts the horizontal scroll. `display_to_pixel_point`
/// alone is relative to the text area and ignores horizontal scroll.
fn head_position(
    editor: &mut Editor,
    head: MultiBufferOffsetUtf16,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<Point<Pixels>> {
    let editor_bounds = *editor.last_bounds()?;
    let offset = head.0.0;
    let character_bounds = editor.bounds_for_range(offset..offset, editor_bounds, window, cx)?;
    let position = character_bounds.bottom_left();
    editor_bounds.contains(&position).then_some(position)
}

#[cfg(test)]
mod tests {
    use super::fit_replacement;

    #[test]
    fn the_selections_edges_are_put_back_around_the_translation() {
        for (original, translation, expected) in [
            ("text", "Hello", "Hello"),
            ("  text\n", "Hello", "  Hello\n"),
            ("\ttext  ", "  Hello \n", "\tHello  "),
            ("text\n\n", "Hello", "Hello\n\n"),
            ("   ", "Hello", "   Hello"),
        ] {
            assert_eq!(
                fit_replacement(original, translation, false),
                expected,
                "{original:?}"
            );
        }
    }

    #[test]
    fn multi_line_translations_keep_their_breaks_unless_the_editor_is_single_line() {
        assert_eq!(fit_replacement("a", "x\ny\r\nz", false), "x\ny\r\nz");
        assert_eq!(fit_replacement("a", "x\ny\r\n\r\nz", true), "x y z");
        assert_eq!(fit_replacement(" a\n", "x", true), " x ");
    }
}
