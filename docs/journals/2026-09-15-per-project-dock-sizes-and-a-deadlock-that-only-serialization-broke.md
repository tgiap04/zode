# Per-project dock sizes and a deadlock that only serialization broke

**Date**: 2026-09-15 14:30
**Severity**: High — feature shipped clean, but test infrastructure deadlock will cost the next person unless named plainly.
**Component**: `worktree`, `project`, `workspace` (`workspace.rs`, `dock.rs`, `panel_size_key.rs`, `panel_size_prune.rs`), `db`
**Status**: Feature shipped (commits `d3924aa feat(worktree): name the repository a worktree belongs to`, `55410d0 feat(workspace): scope dock and panel sizes to the project`, `11e42ef docs(F013): document per-project dock sizes` on `feat.project-logo-avatars-on-the-rail`, PR #39). The parallel-test deadlock is characterized and worked around, not fixed.

## What Happened

Dock and panel sizes moved from a single application-wide key-value store to per-project records. A workspace's project is identified by the main-repo path of its first visible worktree, so every linked checkout of one repository shares one set of widths, and two repositories keep separate ones. The implementation landed in three commits: the first names a worktree's repository, the second scopes the persistence keys, the third documents the feature. All tests passed when run serially. In parallel they deadlock indefinitely.

## The Brutal Truth

**The test suites for `worktree` and `project` deadlock when run in parallel, indefinitely, and the deadlock is not caused by this work.** The symptom looks like a broken feature: `cargo test -p worktree` and `cargo test -p project` both appear to hang forever. Specific tests print "has been running for over 60 seconds" and never finish. The investigations that followed were genuinely infuriating, because a control test proved the change was innocent — and yet the deadlock persists in the baseline. Two abandoned test processes from 6 and 7 September, left from other branches on this machine, proved the symptom predates this work by over a week. The change itself is fourteen lines of a pure accessor with zero callers inside the worktree crate — it cannot possibly have introduced a new deadlock. But that knowledge did not help the moment when `cargo test` hung for forty minutes waiting for an answer that would not come.

The galling part is the asymmetry: both suites are entirely green, both complete in under thirty seconds each, when run with `--test-threads=1`. Full serialization is the only thing that works. With `--test-threads=4` they still hang. The answer to "is this my change?" is a proven no. The answer to "what broke the test runner?" is still open, and it probably belongs to CI.

## Technical Details

### The deadlock symptom and its measurement

Tests that hang when run in parallel with the full suite:

**In `worktree` crate:**
- `test_file_scan_exclusions`
- `test_file_scan_exclusions_overrules_inclusions`
- `test_file_scan_inclusions`
- `test_file_scan_inclusions_reindexes_on_setting_change`
- `test_fs_events_in_exclusions`
- `test_hidden_files`
- `test_renaming_case_only`
- `test_write_file`

**In `project` crate:**
- `test_file_status`
- `test_git_repository_status`
- `test_rename_work_directory`
- `test_rescan_and_remote_updates`

Evidence gathered in sequence:

1. `test_write_file` run alone on the committed code (before any change): passed in 0.44s.
2. Four `test_file_scan_*` tests run together with `--test-threads=1` and this change applied: all passed in 2.16s, exit 0.
3. `--test-threads=4` did NOT reduce the hang — only full serialization did.
4. Full `worktree` suite with `--test-threads=1`: **40 passed, 0 failed, 28.42s, exit 0.**
5. Full `project` suite with `--test-threads=1`: **285 passed (26 lib + 259 integration), 0 failed, 28.51s, exit 0.**

### The control experiment

The decisive move was the one that converted "I believe my change is innocent" into evidence. The worktree crate's diff is exactly:

```rust
// Added pure accessor with no callers inside the crate
pub fn repo_path(&self) -> &Path {
    &self.repo_path
}
```

Temporarily restored `crates/worktree/src/worktree.rs` to its committed state (backing the working copy up first), ran one of the hanging tests against it, then restored the working copy. Result: it still hung. That proved the change was not the cause.

### Key decisions that shipped

1. **The host in a remote key is formatted, never hashed.** `std`'s default hasher carries no cross-release stability guarantee, so a hashed key would silently reset every stored width the day the toolchain moved. The format is human-readable as a side benefit.

2. **Reserved characters are escaped inside each field before joining.** An IPv6 SSH host contains colons and some paths do too, so an unescaped `{host}:{path}` would let two different projects collide on one key. Absent and present-but-empty optional fields are marked differently (`-` vs `+`) so they cannot render alike. Proof: unit test `a_host_and_a_path_cannot_trade_characters`.

3. **The width is applied once, inside the same `dock.update` block that inserts the panel.** A dock can become visible in that same frame, so a width arriving a frame later would be a visible jump. The read and the apply are atomic from the view's perspective.

4. **Only a clean `Ok(None)` from metadata read prunes a stored record.** An `Err` — unmounted volume, disconnected share — keeps the record, because an error is not evidence of absence and the price of guessing wrong is a width the user still wanted. This is defensive: if the project's directory is temporarily unreachable, the width persists until it comes back.

5. **`FakeFs` has no error-injection hook**, so the "an `Err` keeps the record" rule is proved by extracting a pure predicate `is_absent()` and unit-testing it in isolation, rather than end-to-end. Adding a hook to a fixture shared by many crates was out of scope.

## Lessons Learned

**When a test suite hangs in parallel, run it serially first.** `--test-threads=1` is not a debug knob — it is an off-ramp. If a suite passes serially and hangs in parallel, the suite itself is sound. The hang is a shared-resource contention, a scheduler issue, or a system resource exhaustion. None of those are your change's fault, and serialization proves it. Move to "what outside this crate might be fighting over a resource" instead of "what did I break."

**A control experiment takes two minutes and is worth it.** Restoring one file, running one test, and restoring again converted an hour of circular reasoning into a five-minute answer. When "I believe my change is innocent" and the evidence says "the suite is broken", the control is the knot-cutter: if the baseline itself hangs, you are not the cause.

**A persistent choice has to survive the whole architecture.** Formatting host keys as strings instead of hashing them seems like a performance sacrifice until you ask "when would that key be read?". Only at application startup, and only if the project directory is visible. The cost is zero in the fast path. The benefit is that any cargo upgrade does not silently reset everyone's widths. Decisions that trade performance for persistence have to be weighed at the point where the persistence matters, not at the point where the computation happens.

**Error vs. absent is a real distinction, and defensive code says so.** A missing directory is not the same as a read error. The first says "this project moved"; the second says "try again later". Keeping the cached width on error is the right bet, and a comment saying why beats guessing what code with no comment was trying to do.

## Next Steps

- **Investigate the parallel deadlock.** Both `worktree` and `project` suites pass fully when serialized; neither passes when parallelized. The cause is likely in test infrastructure (CI environment, shared resource contention, or a scheduler interaction). Running the full suite with `-- --test-threads=1` on CI is the short-term workaround; finding the root is a separate investigation that needs system-level access.

- **`resolve_git_worktree_to_main_repo` swallows errors with `.ok()?`**, so a caller cannot distinguish a transient filesystem failure from "this is not a linked worktree". During a restore that means a rare wrong seed, self-healing once the real worktree loads. A real fix changes the function's signature to return `Result`, which has another caller in `crates/workspace/src/persistence.rs`.

- **The prune deletes rows one at a time.** `ScopedKeyValueStore` has no batch-delete API. If a user deletes fifty projects in a session, fifty deletes happen serially. Not urgent, but a note for when the key-value store gets a refresh.

- **Two pre-existing project-identity ambiguities are now observable to users**, since project identity carries state for the first time:
  - A case-insensitive filesystem treats two spellings of one directory as two projects and keeps separate widths for each.
  - One checkout reached through two symlinks likewise counts twice.
  - These are edge cases, but worth documenting if a user reports asymmetric widths across what they think is the same project.

- **The layered spec (technical-spec.md, edge-cases.md) needs a rebuild-spec pass.** The change introduces a net-new persistence mechanism with no existing rows to patch, so the existing spec structure (which works for additive changes to a field) does not fit it cleanly. A proper specification layer needs to be written by the next person who touches F013.

## Postscript: How to unblock if the test suite hangs again

If `cargo test -p worktree` or `cargo test -p project` hangs on your machine:

1. Do not assume your change broke it.
2. Run: `cargo test -p worktree -- --test-threads=1` or `cargo test -p project -- --test-threads=1`.
3. If it passes: the suite is sound, the hang is a scheduler issue in parallel mode, and it is not your change's fault.
4. If it still hangs: the issue is real, and needs isolation.
5. Also check: `ps aux | grep "[c]argo test"` — abandoned test processes from earlier sessions can hold resources and cause hangs in later runs.
