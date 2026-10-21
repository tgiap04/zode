# A count of waiting agents on each project's rail square

**Date**: 2026-10-06
**Severity**: Low — a new feature, no regression found
**Component**: `agent_ui`, `sidebar`
**Status**: Shipped on `feat.release-v0.1.5`; not yet checked by hand in the app

## What Happened

Each project's square on the rail now carries a number in its top-right corner: how many of that project's agent tabs are waiting on the user. It is built on the turn signal from the transcript-based notification work, not on the pty-rate guess.

## Decisions

- **What counts:** a tab with Claude's permission dialog on screen, or a tab whose turn ended while the user was not looking at it. Focusing the tab clears the second kind. The approval counts for as long as the dialog is up, even while the tab is focused.
- **Which tabs:** only tracked Claude tabs (`reports_turns()`). Codex, OpenCode, Copilot and untracked tabs have only the pty-rate heuristic, which an hour earlier had been shown to fire on typing, resizing and silent tool calls. Counting them would have put the same false signal on the rail, so they never count.
- **No 3 s confirmation:** the notifier waits because a posted notification cannot be taken back. A badge can, so it shows on `Ended` and goes on `Started` or `Interrupted`. A whole transcript pass is folded before anything is emitted, so `Ended` followed by `Interrupted` never flashes.
- **Every project, including the active one,** because a second tab in the current project can be the one waiting.

## Technical Details

- `AgentView::needs_attention()` is true when `reports_turns()` holds and there is an unread end or a pending approval. `AgentViewEvent::Attention` is emitted only when that value changes. It is not serialized and maps to no pane event, so it causes no DB write.
- "Looked at" means keyboard focus is inside the tab while its window is active, from focus-in/out on the view's root handle plus window activation. A test with a real `TerminalView` confirmed that focus inside the terminal reaches the root watch. A second test confirmed that switching workspaces delivers focus-out, so the fallback the plan had ready was not needed.
- The sidebar counts in `rebuild_contents` only, never per frame. Rebuilds are triggered by per-workspace `ItemAdded`/`ItemRemoved` and per-tab `Attention` subscriptions, pruned on every resync.
- The badge text is black or white, whichever has the higher contrast against the theme's error colour. White on the light theme's red is under the 4.5:1 AA threshold. A 1px ring keeps the badge visible against a red avatar. The tooltip names the exact count behind "9+". The re-index dot moved to the bottom-right corner.

## What Review Caught

- The first focus tests focused the root handle, but in a running tab focus sits on the terminal's handle. A test against a real terminal was added. It passed, but until then the central claim was unproven.
- A transcript replaced mid-session left its unread flag lit. That is now cleared.
- A turn end read from the transcript after the CLI had exited could light the badge for good, because the activity tick stops at the first exit. A `cli_gone` flag now drops it.
- "Start a New Session" left the old activity tick running. While the new CLI was still starting, that tick saw no terminal and marked the CLI gone. It is harmless today, because a new session is untracked, but the tick is now dropped there too.
- The test setup was copied three times. It now lives in one test-only module.

## Gates

`./script/clippy` exit 0. `cargo test`: `agent_ui` 203, `sidebar` 61, `agent_notify` 30, `keep_awake` 30 passed. `zode`, `git_ui` and `floating_pane` compile. `rustfmt --check` and prettier exit 0. `cargo test -p workspace` has one failure, `test_hibernate_after_ms_zero_disables_hibernation`. It fails identically on `develop` at `8664e47`, checked in a temporary worktree, so it predates this work.

## Still Unverified

- Not seen in a real macOS session. The workspace-switch and window-blur behaviour was proven in a test window only.
- Linux and Windows were not checked locally.
