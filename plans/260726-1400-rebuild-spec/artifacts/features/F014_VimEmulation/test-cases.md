# Test Cases — F014_VimEmulation

**SIDECAR (v26.1.0):** this is a 5th, optional file alongside the 4 mandatory feature-spec files.
Its absence never blocks feature-spec promotion.

**Code Format**: `TC###` — 3-digit zero-padded, **resets per feature** (this file's own scope is
the reset boundary; unlike `JOB###`, which is file-global).

**Citation-source split**: `UT`/`IT` rows cite a `BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, a
`file:line`, or an `edge-cases.md` row. `UAT` rows cite a `screens.md`/`business-context.md`
section — NEVER a bare code (UAT is less code-traceable by design; see
`references/test-cases-researcher-contract.md`).

**CSV export**: out of scope v1 — Markdown is the sole output. See plan Decision.

---

## Test Cases

| Test-ID | Type (UT\|IT\|UAT) | Given | When | Then | Traces-to |
|---------|---------------------|-------|------|------|-----------|
| TC001 | UT | `Vim.mode` is `Normal` and the developer types a letter that is also a Vim command key (e.g. `w`) | The keystroke is dispatched | The character is NOT inserted into the buffer; it resolves to a `vim::Action` (motion/operator) instead | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC002 | UT | `Vim.mode` is `Insert` | The developer types a character | The character is inserted directly into the buffer at the cursor, and the mode indicator shows "INSERT" | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC003 | UT | `Vim.mode` is `Replace` and the cursor sits over an existing character | The developer types a character, then presses `UndoReplace` | The typed character overwrites the existing one (no insertion); `UndoReplace` restores the single original character from `Vim.replacements` | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC004 | UT | `Vim.mode` is `Visual` with a selection anchored at some point | The developer moves the cursor with a motion, then leaves the mode | The selection extends character-wise with the cursor; on leaving, the selection endpoints are written to the `` `< ``/`` `> `` visual marks | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC005 | UT | `Vim.mode` is `VisualLine` across 3 lines | The developer extends the selection down one more line | The selection snaps to whole lines (line-granular), not individual characters | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC006 | IT | `Vim.mode` is `VisualBlock` spanning 3 rows and 3 columns | The developer applies an insert/change edit | The edit is applied identically to every one of the 3 selected rows (implemented as multiple single-line selections under the hood) | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC007 | UT | `HelixModeSetting` is on and `Vim.mode` transitions to `HelixNormal` | The developer presses a motion key | The motion selects text first, before any action is applied (selection-first semantics), and the mode indicator shows "NORMAL" (Helix skin) | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC008 | UT | `Vim.mode` is `HelixSelect` under Helix mode | The developer performs a selection-extending action | Behavior matches `Visual` mode's selection model, and the mode indicator shows "SELECT" | `DISC-F014-01`; `crates/vim/src/state.rs:42-81` |
| TC009 | UT | `Vim.mode` is `Normal` (not `Insert`/`Replace`/`temp_mode`) | The developer types a printable character | No text is inserted into the buffer; the keystroke is resolved as a motion/operator/text-object instead | `BR-001` |
| TC010 | IT | The developer sets a mark named `A` (uppercase) on a file, then a mark named `a` (lowercase) on the same file | Both mark writes are serialized | The uppercase mark's `(workspace_id, name, path)` is persisted to `vim_global_marks_paths` in addition to `vim_marks`; the lowercase mark is persisted only to `vim_marks` | `BR-002` |
| TC011 | UT | The cursor sits at the start of a line with several words, no operator pending | The developer types the count-prefixed motion `3w` | The cursor advances by exactly 3 words as a single logical action (one undo step) | `BR-003` |
| TC012 | IT | An ex command's range endpoint references a mark name that was never set | The command is submitted | Range resolution fails with "mark {name} not set"; the error is surfaced via a notification and the buffer is unchanged | `BR-004`, edge-cases.md § "range refers to a saved position (mark) that was never set" |
| TC013 | IT | An ex command's range endpoint references a mark that exists but holds no stored anchors | The command is submitted | Range resolution fails with "mark {name} contains empty anchors"; the error is surfaced via a notification and the buffer is unchanged | `BR-004`, edge-cases.md § "mark that exists but has no stored position" |
| TC014 | IT | `Vim.mode` is `Normal`, cursor mid-line | The developer presses `A` (`InsertAfter`-family action) | The cursor repositions to end of line before the mode flips, then `Vim.mode` becomes `Insert` | `SM-001` |
| TC015 | IT | `Vim.mode` is `Insert` with buffer edits made during the session | The developer presses `Escape` (`NormalBefore`) | `Vim.mode` returns to `Normal`; all edits made during the insert session are grouped into the single undo transaction started at `current_tx` | `SM-001` |
| TC016 | UT | `Vim.mode` is `Normal` | The developer presses `R` | `Vim.mode` transitions to `Replace` and the mode indicator shows "REPLACE" | `SM-001` |
| TC017 | IT | `Vim.mode` is `Visual` with an active selection | The developer applies an operator (or presses `Escape`) to leave visual mode | `Vim.mode` returns to `Normal`; `create_visual_marks` writes the `` `< ``/`` `> `` marks from the outgoing selection | `SM-001` |
| TC018 | IT | `Vim.mode` is `Visual` (character-wise) with an active selection | The developer immediately presses `V` (switch to `VisualLine`) without leaving visual mode first | The selection is re-derived cleanly from the prior visual anchor as line-wise, not stacked/compounded with the character-wise selection | `SM-001`, edge-cases.md § "switches from one visual selection type to another very quickly" |
| TC019 | UT | `HelixModeSetting` is on and `Vim.mode` is `Normal` | `switch_mode(Normal)` is invoked, then a selection-first motion is pressed, then `Escape` | Mode goes `Normal` → `HelixNormal` → `HelixSelect` → `HelixNormal`, following the Helix-specific edges of the lifecycle | `SM-001` |
| TC020 | IT | The `vim` namespace has an `Action` registered for a given command (e.g. any action in `motion.rs`/`normal.rs`/`visual.rs`) | The developer looks up that command via its keybinding AND via the command palette, independently | Both paths reach the same action; no command requires another command to fire first | `crates/vim/src/vim.rs:551-553` |
| TC021 | UT | The developer opens the ex command line and types a string matching no known command or pattern | The developer submits it | Nothing executes against the buffer; the command line stays open showing no matching suggestion | edge-cases.md § "matches no known command or pattern" |
| TC022 | UT | `Editor.use_modal_editing` is true and the developer is mid-selection or mid-insert | `use_modal_editing` is toggled to false mid-session | Vim's mode-tracking stops applying to that editor; keystrokes fall through to standard non-modal editor behavior | edge-cases.md § "Vim mode is turned off ... mid-selection or mid-insert" |
| TC023 | IT | A global (uppercase) mark points at a file path that has since been moved, renamed, or deleted | The workspace reloads and `loaded()` attempts to resolve the mark's stored path | The `vim_global_marks_paths` row is kept in storage as-is, but the mark is not reattached to any open file/worktree until a matching path reappears | edge-cases.md § "global mark points at a file that has since been moved, renamed, or deleted" |
| TC024 | UAT | Developer opens a file in the editor pane with Vim mode enabled | The editor loads | The developer starts in Normal mode, shown by the mode indicator reading "NORMAL" | screens.md § User Journey step 1 |
| TC025 | UAT | Developer is in the editor pane, mode indicator reads "NORMAL" | The developer navigates and selects text using keyboard commands, entering Visual then Insert mode | The mode indicator updates live to reflect each mode as it is entered | screens.md § User Journey step 2 |
| TC026 | UAT | Developer is in Normal mode in the editor pane | The developer presses `:`, types a command, and submits it | The command runs against the editor pane's buffer/range, and the ex command line closes | screens.md § User Journey step 3 |
| TC027 | UAT | Developer just submitted an ex command or left Insert/Visual mode | The developer's editor settles back to Normal mode | The mode indicator reads "NORMAL" again and the developer continues editing | screens.md § User Journey step 4 |
| TC028 | UAT | Developer has just made an edit, or has recorded a short sequence of edits into a register | The developer repeats the edit with a single keystroke (`.`), or replays the recorded macro | The last edit is reproduced at the new cursor location, or the exact recorded keystroke sequence is replayed | business-context.md § What They Do (step 6) |
| TC029 | UAT | Developer's cursor is inside a meaningful chunk of text (a word, a quoted phrase, bracket contents) | The developer issues one short command targeting that chunk, instead of manually selecting it | The whole chunk is acted on (deleted/changed/yanked) in one step | business-context.md § What They Do (step 7) |
| TC030 | UAT | Developer is actively editing (navigating, typing, or selecting) | The developer glances at the bottom of the screen at any point | The current mode is always visible there, so the developer never loses track of what their next keystroke will do | business-context.md § What They Do (step 8) |

---

## Coverage Notes

- `SM-001` — additional lifecycle edges (`Replace → Normal`, `VisualLine → Normal`,
  `VisualBlock → Normal`) apply the identical mark-write / mode-flip side effects already verified
  by TC016/TC017 for their sibling edges (`Normal → Replace`, `Visual → Normal`); not
  independently re-tested to avoid redundant rows for a mechanism already covered once per
  distinct side-effect (DRY).
- Decision Logic — technical-spec.md states "N/A — no user-facing decision logic beyond
  DISC-F014-01"; no `DEC-###` codes exist for this feature, so none are listed here.
