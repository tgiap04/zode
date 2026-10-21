# Remote terminal showed a few lines, and Fork never forked

**Date**: 2026-10-16 11:47
**Severity**: High — a blank remote terminal, and a Fork that ran the agent without forking
**Component**: `terminal` snapshot, `remote_control`, `agent_ui` resume, web `/remote`, `project_server` test fixture
**Status**: ongoing — fixes committed; no live check with a real Claude Code session in a browser has been run

## What Happened

A browser remote terminal running `claude --dangerously-skip-permissions` showed about three lines at the top and black below. The same round added session resume and a responsive remote page. Commits: zode `110d784`, `7b445ad`, `a3f0544`; web `ba042b8`, `56b7f81`; fork pin `e13b2ea`.

## The Brutal Truth

We have no proven cause for the blank screen. The study said: "I couldn't prove a single cause for the '3 lines then black' from code alone." Its first theory, that the host sends only live bytes, was wrong; the host sends a full snapshot on attach. Every candidate was fixed together. Nobody has watched it work in a browser against Claude Code.

The galling part is Fork. It worked when the history panel landed in `bbe4e5d` (2026-08-23), which launched `resume_command(session, fork)` directly. `e364e69` (2026-08-27) moved launching onto session intents and mapped a fork to `Untracked`, and from then on it started the CLI with no arguments, for seven weeks. The `project_server` flake had been filed as known, to rerun and leave alone. It was a race in our own fixture. Honestly it stings.

## Technical Details

- Snapshot: `terminal_remote_snapshot.rs` now writes the saved cursor, scroll region and charsets. `terminal_mirror.rs:49-62` settles after 400 ms quiet, capped at 2 s.
- Fork: `session_history/actions.rs:90` mapped `Fork::New` to `SessionIntent::Untracked`, and `resolve_session` returned `Fresh`. Now `--resume <id> --fork-session` (Claude) or `-s <id> --fork` (opencode).
- Flake: the fixture ran `sh -c 'sleep 1000; :' daemon ... & echo $! > server.pid`. `ide_bridge.rs:372-374` signals only a process whose command line contains the pid-file path. An instrumented failure showed `cmd=[]`.
- Flake rate: about 39% of runs, also on clean HEAD (per session notes, not in the repo). Post-fix, `temper-results.json` records 10/10, then 6/6 under 8 CPU burners.
- Gates: zode 414, 237, 273 (twice); web 457 (twice); clippy, fmt, lint, build all green.

## What We Tried

- "Only live bytes": disproved by reading `terminal_mirror.rs:738`, where an attachment that needs a snapshot takes one.
- Candidates, all addressed, none isolated: a pane fitted to width only (about 540 px), xterm not following output after `ESC c`, scroll region and saved cursor missing, snapshots missing synchronized updates, Unicode width, xterm's query replies sent as keystrokes.

## Root Cause Analysis

Blank screen: the snapshot omitted state a program can set before attach, and the browser never scrolled to the cursor. Which one caused the three lines is unknown.

Fork: `Untracked` was chosen so a fork would not resume the original conversation, which is sound for tab identity. The same mapping dropped the arguments. No pre-fix test drove a fork through to its launch arguments, so the refactor that broke it passed.

Flake: the fixture wrote `$!` before exec. The host's pid-reuse guard read an empty command line and declined to signal.

## Lessons Learned

- Disprove the easy theory first, and say plainly when a fix has no proven root cause.
- A refactor can kill a feature that no test drives end to end. Drive the real entry point to the spawned arguments.
- A test fixture must reproduce the ordering production depends on. "Known flake, rerun it" hid a bug with a clear mechanism.

## Next Steps

- **User:** live browser check with a real Claude Code session, both themes and a phone, resume and fork end to end. The web is already deployed, so this covers production. Before the 0.1.5 release.
- **Main agent:** deferred items: `hides()` compares paths textually (`session_history.rs:147`); the site header and footer stay focusable behind the phone sheet; no DOM test layer in the web repo.
- History stays hidden until a Zode build advertises `sessions`.
