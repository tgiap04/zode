# Deleted agent sessions stayed listed until Refresh

**Date**: 2026-10-05
**Severity**: Medium — a delete looked like it had done nothing, though the transcript was already in the trash
**Component**: `fs`, `worktree`, `agent_ui`, `git_ui`
**Status**: Resolved on `feat.release-v0.1.5`; not yet checked by hand in the app

## What Happened

After deleting agent sessions — one at a time or with Delete All in Agent History, or with "Delete Session…" in the branch panel — the rows stayed until Refresh was pressed. Reading the delete code first suggested nothing was wrong: every path calls `SessionStore::forget` or `forget_many`, and both panels observe the store. The answer came from `Zode.log`, not from the code.

## The Brutal Truth

There were three separate causes, and only the first was visible in a log. The other two were timing windows that only a failing test could show. Each was written as a test first, and each one went red before its fix.

## Technical Details

### Delete All: `RealFs::trash` ignored `ignore_if_not_exists`

The log showed `Could not canonicalize the path of the file … No such file or directory` from `execute_session_deletion`. A Claude session's deletion lists both `<id>.jsonl` and the sidecar directory `<id>/`, and the sidecar directory does not exist for a session that never spawned a subagent. `RealFs::trash` canonicalized first and never read its options, so the missing directory was an error. `every_path_gone` became false, and `forget_many` skipped a session whose transcript had already gone to the trash. A second click a few seconds later logged the `.jsonl` as missing too, which is the proof.

`Fs::trash` now returns `Result<Option<TrashedEntry>>`: `Ok(None)` means the path was absent and the caller allowed that. `RealFs` checks `symlink_metadata` first, so a dangling symlink still counts as present, and checks again after a trash error in case the path vanished in between. `FakeFs` has the same semantics. Worktree passes `ignore_if_not_exists: false`, so its behaviour is unchanged. Single deletes never hit this, because `delete_via_trash` already skips paths that do not exist.

### A sweep in flight overwrote the delete

Opening a panel starts a `SessionStore` sweep. A delete that lands while the sweep is running is applied to the index, and then the sweep finishes and installs a listing it read before the trash, which brings the row back. The store now keeps the ids forgotten while a sweep is in flight and filters them out of that sweep's result. The set is taken when the sweep lands, so it never grows beyond one sweep's deletes.

### The branch panel held the deleted row

While the store is scanning, `hold_known_agents` hands back a checkout's last known agents, so the panel does not blink empty. That hold kept the deleted row. The first fix filtered forgotten rows out of what the hold returns. Review showed that was not enough: after a refresh coalesced into a second sweep, the forgotten set is empty again but the hold still has the row. The hold now writes the pruned list back, so a forgotten row cannot outlive the rebuild that filtered it.

## Lessons Learned

- When the code looks right, read the app's log before reading more code. `log_err()` had been recording this failure the whole time.
- A function that takes an options struct and ignores it is worse than one that takes none, because callers reasonably rely on what they pass. `_options` in a signature is worth a second look.
- Delete and sweep race on one index. Any cache that holds index results across a sweep, like the branch panel's hold, needs the same treatment as the index itself.

## Gates

`./script/clippy` exit 0. `cargo test`: `agent_ui` 184, `fs` 19 (1 ignored), `git_ui` 251, `project_panel` 101 passed. `worktree` 40 passed with `--test-threads=1`. Run in parallel, seven of its real-filesystem integration tests stall on this machine, and none of them touch `trash`. The new store and branch-panel tests ran 5 times each with no failure. `rustfmt --check` on touched files exit 0. Linux and Windows were not checked locally.

## Still Open

- A branch panel that stays hidden through the first sweep and is shown during the second can still show the deleted row until the second sweep lands. It clears itself.
- The `Fs::trash` signature now differs from upstream Zed. Each future upstream merge needs its new callers adapted.
- Not yet checked by hand: open Agent History, delete a session at once, and confirm the row stays gone; the same for Delete All and the branch panel.
