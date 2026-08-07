<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->

# Edge Cases — F008_EditorCore

| Scenario | What Happens | User-Facing Message |
|----------|--------------|---------------------|
| Developer tries to edit a file that is marked view-only (read or read-only capability) | The edit is silently dropped — buffer content does not change and no operation is recorded in the undo history | "None — the edit is silently ignored; no dialog appears" |
| Developer toggles split-diff view while the editor pane is too narrow to actually show two panes side by side | The internal preference flips to "split," but the editor visually stays as a single pane until it's widened | "None — no message; the view simply doesn't visually split until there's room" |
| Developer triggers "extend selection to next match" but no further occurrences exist in the file | The existing selection(s) stay exactly as they were; nothing new is added | "None — silent no-op" |
| Developer deletes text to the beginning of a line, then immediately presses Undo | The entire deletion (reversing the selection, extending it, and removing the text) reverts in one single Undo step | "None — Undo silently restores the removed text" |
| Developer closes a file or the whole workspace while it has unsaved changes | The dirty buffer's content, cursor position, and collapsed sections are saved locally so they can be restored next time the file is reopened | "None — restore happens automatically on next open, no prompt shown" |
