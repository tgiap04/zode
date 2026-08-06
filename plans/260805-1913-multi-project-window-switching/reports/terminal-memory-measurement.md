# Phase 05 Terminal Memory Measurement

**Date:** 2026-08-06
**Machine:** MacBook Pro, Apple Silicon (arm64), macOS 26.6
**Method:** `crates/terminal/src/terminal.rs::tests::test_scrollback_grid_memory_measurement` — a real
`alacritty_terminal::Term`/`Grid` (via `TerminalBuilder::new_display_only`, no PTY needed since content
is injected deterministically through `write_output`), own-process RSS read via `sysinfo` before/after,
run with `cargo test -p terminal --lib -- test_scrollback_grid_memory_measurement --nocapture`.

## Step 1 — is the per-terminal cost significant?

Filled a 200-column terminal to the default 10,000-line scrollback cap with realistic ~200-char lines:

| Stage | RSS | Delta |
|---|---|---|
| Before fill | 24,051,712 bytes | — |
| After fill (10,000 lines × 200 cols) | 82,935,808 bytes | **+58,884,096 bytes (~56.2 MB)** |

**Result: significant, not negligible.** The plan's own decision threshold was "< ~5MB/terminal →
conclude not worth it." This is over 11× that threshold. A user running even 2-3 terminals with full
scrollback (a common case — `npm run dev`, a build watcher, a REPL) is paying real, measurable memory
for content that isn't being looked at while a project is hibernated.

## Step 2 — does `set_options` actually shrink existing history, or only cap future growth?

Confirmed via source reading of the vendored alacritty fork (`alacritty_terminal` @ `9d9640d4`,
`grid/mod.rs` `update_history`, `grid/storage.rs` `shrink_lines`/`truncate`) **and** empirically:

- Shrinking `Config.scrolling_history` from 10,000 → 2,000 and calling `Term::set_options` (the same
  call pattern `Terminal::set_cursor_shape` already uses) immediately dropped `history_size()` from
  10,000 to 2,000 — logically real, synchronous, not deferred to future growth.
- `Storage::shrink_lines` calls `Vec::truncate` on the underlying row storage once the shrinkage exceeds
  its 1,000-line slack cache, which drops the excess `Row<Cell>` elements (and their own heap
  allocations) immediately at the Rust level.

**This part works exactly as the plan hoped.**

## Step 3 — the surprise: shrinking did not reduce process RSS at all

| Stage | RSS | Delta from previous stage |
|---|---|---|
| After fill (10,000 lines) | 82,935,808 bytes | — |
| After shrink to 2,000 lines | 82,935,808 bytes | **+0 bytes** |
| After growing back to 10,000 lines | 82,952,192 bytes | **+16,384 bytes** (~16 KB) |

The regrow number is the smoking gun: growing back to the *exact same* 10,000-line size that originally
cost **56.2 MB** the first time now costs only **16 KB** — a >3,000× difference. The freed row
allocations from the shrink were not lost or reused elsewhere; they went straight back into serving the
same buffer's regrowth. This is textbook allocator-level memory retention: freeing Rust objects returns
them to the allocator's own free list, not necessarily to the OS. `history_size()` (the Rust-level
truth) and process RSS (the OS-level truth) disagree here, and RSS is what actually matters for "does
hibernating this project free RAM for other programs."

**Caveat this test cannot resolve:** this measurement ran under `cargo test`'s default system allocator.
Zode optionally builds with `mimalloc` (`crates/zed/Cargo.toml` — `mimalloc = { version = "0.1", optional
= true }`, wired in `crates/zed/src/main.rs:65-67`), which the `terminal` crate's own test binary does not
enable. mimalloc has its own automatic background page-purging behavior that the default system
allocator generally does not (no code in this repo calls `mi_collect` or configures a purge delay
explicitly — it would rely on mimalloc's own defaults). Whether a real, `mimalloc`-linked Zode process
would eventually give this memory back to the OS after some idle delay is plausible but **not verified
here** — that would require a long-running measurement against the actual release binary, out of scope
for a fast unit test.

## Decision

**Proceed with FR3-FR6 (implement scrollback limiting on hibernate), despite the RSS caveat above.**
Reasoning:

- The logical cost is real and well above the plan's own significance threshold (Step 1).
- Shrinking is a **necessary precondition** for this memory to ever become reclaimable by *any*
  allocator's purge mechanism — leaving scrollback at full size while a project sleeps *guarantees* the
  memory stays committed indefinitely, regardless of allocator. Shrinking at least makes reclaiming
  *possible*.
- This does not contradict NFR2 ("if `set_options` doesn't shrink, stop — don't hack the alacritty
  fork"): `set_options` *does* shrink, exactly as designed. The allocator-retention finding is a
  different, honestly-disclosed limit on what that shrink can promise at the OS level, not a reason to
  reach into the fork's internals to force a page-level purge (which NFR2 explicitly rules out).
- FR5 (task terminals, which always get the 100,000-line cap regardless of user settings) is the
  highest-leverage case this unlocks — a single long-running `npm run dev` left in a hibernated
  project could otherwise hold up to ~560 MB of scrollback by this measurement's per-line rate,
  10× the interactive-terminal case measured above.

The default (`null` / unset) must stay disabled-by-default per the plan's own Risk Assessment: shrinking
deletes the user's actual log lines irrecoverably, and that must never happen silently.
