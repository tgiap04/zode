# Business Context — F014_VimEmulation

## Why It Matters

Many professional developers have years of muscle memory built around Vim's keyboard-driven editing style and will not adopt an editor that forces them to give it up. Vim emulation lets Zode offer a familiar, fully keyboard-driven editing experience — navigation, text selection, and bulk text changes without touching the mouse — layered on top of the same editor everyone else uses, so Vim users and non-Vim users share one codebase and one set of files without friction.

## Who Uses It

- **Developers who already know Vim** — turn the mode on and keep using the motions, mode switching, and command shortcuts they already rely on, rather than relearning a new editor's shortcuts.
- **Developers curious about Vim** — can try Vim-style editing inside their normal environment without installing or switching to a separate editor.

## What They Do

1. A developer turns on Vim mode for the editor.
2. The developer navigates through a file using short keyboard commands (move by word, by line, to the end of a paragraph) instead of arrow keys or the mouse.
3. The developer switches into a typing mode at a precise spot in the text — before or after the cursor, at the start or end of a line — then types normally.
4. The developer selects a stretch of text (a few characters, whole lines, or a rectangular block) using keyboard commands, then deletes, copies, or changes it in one step.
5. For bulk or precise changes, the developer types a short command line (for example, "replace every 'foo' with 'bar' in this file") and the change is applied in one action; if the command isn't recognized or refers to something that doesn't exist (like a saved position that was never set), the developer sees an error message instead of the change silently failing to happen.
6. The developer repeats their last edit at a new location with a single keystroke, or records a short sequence of edits once and replays it as many times as needed.
7. The developer targets a whole meaningful chunk of text — a word, a quoted phrase, the contents of a bracket — with one short command, instead of manually selecting it.
8. The current mode (navigating, typing, selecting, etc.) is always visible at the bottom of the screen, so the developer never loses track of what their next keystroke will do.

## Unresolved Questions

- **Recording-into-a-used-register behavior**: it isn't documented from the code whether starting a new recording into a register that already has something recorded overwrites or appends to it — this affects how confidently users can reuse register letters for macros.
- **Silent persistence failures**: if a saved position (mark) fails to save in the background, the developer is not told. Product should confirm whether this "fail silently" behavior is intentional or should surface a warning.
