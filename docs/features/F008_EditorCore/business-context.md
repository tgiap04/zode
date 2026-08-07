<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->

# Business Context — F008_EditorCore

## Why It Matters

This is the text-editing surface itself — every keystroke a developer makes to move around, select, and change code goes through this layer. It is the single most-used part of the whole application, and every other panel in the editor (search results, diagnostics lists, diff comparisons) displays text by reusing this same surface rather than building its own.

## Who Uses It

- **Developer** — the single local user of this desktop editor. They navigate files, select and edit text, compare two versions of a file side by side, and switch how a file's text encoding or line endings are interpreted.

## What They Do

1. Developer opens a file and moves the cursor around using structural navigation (by word, by line, by page) without needing the mouse.
2. Developer selects a piece of text and extends that selection to the next matching occurrence elsewhere in the file, so several spots can be edited at once.
3. Developer deletes a stretch of text back to the start of the line in a single action, and can undo the whole deletion in one step if needed.
4. Developer compares two versions of a file and switches between a single combined view and a side-by-side view, whichever is easier to read for that particular change.
5. If the file being edited is marked view-only, any attempted change is simply ignored — no error dialog interrupts the developer, the file just stays as it was.
6. When the developer closes a file or the whole workspace, their cursor position, selections, and collapsed sections are quietly saved so the file reopens the same way next time.

## Unresolved Questions

- **View-only source**: it is not fully confirmed which real-world situations mark a file as view-only in this fork — a local toggle and a remote-hosted file the client isn't given permission to edit are both plausible causes, but the collaboration-based case (a shared session participant with a viewer role) does not apply here since this fork has no live collaboration server.
