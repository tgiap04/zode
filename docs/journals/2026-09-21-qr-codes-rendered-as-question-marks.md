# QR codes rendered as question marks — zode advertised Alacritty without delivering

**Date**: 2026-09-21 21:45
**Severity**: Medium — user-facing rendering defect; fix is one file
**Component**: terminal, terminal environment setup
**Status**: Resolved (commit `3a9194e`)

## What Happened

A user pasted a screenshot of Expo CLI's QR code inside zode's terminal. Both the floating window and bottom panel showed the same pattern: a grid of rounded boxes, each containing a single `?`. A few pale light-grey rectangles were scattered among them — those rendered; the vast majority did not.

The QR itself was invisible. Invisible enough that the user could not scan it.

## The Brutal Truth

The galling part is that zode's own environment was what broke it. The terminal advertised a capability it did not possess, and Expo CLI — reasonably — believed the claim and emitted characters the system cannot draw.

This was not a missing feature or a hard platform limit. It was zode claiming to be Alacritty and then failing to deliver Alacritty's glyphs. Nothing in the fork ever compared the identity the pty exports against what the renderer can actually draw — the claim arrived for free with a vendored crate, and no step in adopting that crate asked what the claim obligated us to.

## Technical Details

The evidence chain:

1. **The `?` is CoreText LastResort** — macOS's fallback when no font on the system has a glyph. A few solid light-grey bars **did** render correctly, which means those characters exist somewhere. The `?` means they do not.

2. **The bundled font is JetBrains Mono.** Its `cmap` table was parsed and inspected:
   - Contains `U+2588` (FULL BLOCK), `U+258C` (LEFT HALF BLOCK), `U+2590` (RIGHT HALF BLOCK) — all **rendered correctly** in the output.
   - Contains **zero** codepoints from `U+1FB00..=U+1FB3B` (Symbols for Legacy Computing — "sextants"). Those are the 60 missing glyphs.

3. **No macOS system font covers the sextant block either.** The entire Unicode block is missing from every standard macOS font.

4. **Expo CLI's QR renderer** (`@expo/cli/build/src/utils/qr.js`) has two rendering paths:
   - `supportsSextants()` returns true when `TERM_PROGRAM` is `ghostty` or `WezTerm`, or when `KITTY_WINDOW_ID` or `ALACRITTY_WINDOW_ID` is non-empty.
   - When true, it emits sextants (`U+1FB00..=U+1FB3B`) — 3 pixels per cell, 6 per row.
   - When false, it emits safe half-blocks (`▀` `▄` `█`) — 2 pixels per cell, 4 per row.

5. **`ALACRITTY_WINDOW_ID=21474836553` was live inside zode's terminal.** Verified by echoing it inside a running session. Source: `alacritty_terminal/src/tty/unix.rs:268` sets it unconditionally when spawning the pty — it is part of the `alacritty_terminal` crate's pty setup, not something zode controls at first sight.

6. **Real Alacritty renders sextants because it has `builtin_font.rs`** in the `alacritty` **binary** crate. That module draws `U+1FB00..=U+1FB3B` as bitmaps. zode vendored `alacritty_terminal` (the pty backend) but not the `alacritty` binary's rendering layer. It inherited the identity claim without the capability.

7. **The rendered geometry confirmed the diagnosis before any code was opened.** The QR measured ~20 cells wide by ~13 rows. That matches sextant packing: `ceil((width + 2) / 2)` by `ceil((height + 2) / 3)`. The same QR with half-blocks would measure ~39 by ~20. The user was definitely seeing sextants, not half-blocks.

8. **`WINDOWID` was also wrong** — it was set to the same numeric value as `ALACRITTY_WINDOW_ID`. That value is a GPUI `WindowId` (a slotmap key from `KeyData::as_ffi()`) — never a real X11 XID on any platform. On macOS and Windows it has no meaning. Decided in the same fix to blank it.

## What We Tried

1. Read the QR geometry and reasoned backward: sextants vs half-blocks.
2. Checked which glyphs JetBrains Mono carries: confirmed the sextant block is absent.
3. Searched Expo CLI's QR logic: found the `supportsSextants()` gate.
4. Traced `ALACRITTY_WINDOW_ID` to `alacritty_terminal/src/tty/unix.rs:268`, set unconditionally.
5. Checked if real Alacritty can draw sextants: yes, in `builtin_font.rs` — not present in zode.

Three options were presented to the user:
- Stop claiming the Alacritty identity (blank the env var).
- Bundle a font covering the sextant block.
- Port Alacritty's `builtin_font.rs` into zode.

The user chose the first — stop the claim.

## Root Cause Analysis

A fork that vendors a crate provides only the parts it vendored. `alacritty_terminal` sets `ALACRITTY_WINDOW_ID` to mark itself to child processes. This was an honest move for the library — it is telling the truth about what it is. But zode is not Alacritty; it is using Alacritty's pty backend. The claim should not propagate.

The deeper pattern: **a terminal identity should be audited against what it can actually render.** Not "does the crate set this variable," but "does this binary implement the rendering that consumers expect from that identity."

## Lessons Learned

1. **Capabilities and claims must be audited together.** `ALACRITTY_WINDOW_ID` being set does not mean the terminal can draw what Alacritty draws. A single grep for the variable's name in zode's codebase finds where it enters; grepping for sextant rendering finds where it should be handled. They should align.

2. **Measuring the rendered artifact narrows the search.** Geometry alone ruled out half-blocks and pointed straight at sextants before a single line of source was read. When an identity claim gets tested with real output, the evidence is unambiguous.

3. **Geometry is faster than code.** 20 cells vs 39 cells — measure, compute the packing, reason backward. Saved time hunting through Expo's QR logic.

## Trade-off Accepted

The QR is now correct but rendered larger: ~39 cells wide by ~20 rows instead of ~20 by ~13. Some consumers test mere *presence* of the `ALACRITTY_WINDOW_ID` env var rather than *non-emptiness*. No such consumer exists in this repository.

## Verification

- `cargo clippy -p terminal --tests --release` → 0 warnings (full `./script/clippy` could not complete due to pre-existing `aws-lc-sys` SDK failure on macOS, unrelated to this change).
- `cargo test -p terminal` → 72/72 tests passed before and after the commit.
- Added `test_insert_zed_terminal_env_blanks_alacritty_identity` to verify both variables are blanked.
- The insert-before-spawn ordering at `terminal.rs` (lines 487 and 585) is guaranteed by both sites sitting in one unbroken `async` block, not by a test — worth recording as a limitation.

## Next Steps

None — the fix is complete and live in commit `3a9194e`.
