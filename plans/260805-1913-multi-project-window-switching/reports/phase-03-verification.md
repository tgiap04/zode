# Phase 03 Verification Report: LSP Hibernate and Wake

> **⚠️ SUPERSEDED — do not treat this as verification of the current code.**
>
> This report covers only commits `90da4f7`, `557a3ca`, `54b8a40` (the original three-commit
> phase). It predates, and does not mention, either review-fix round that followed:
>
> - `7c504a6` — fixed a Critical (`Warm -> Active` incorrectly restarting untouched language
>   servers) and two High findings (the stale-sweep partition escaping to remote projects
>   double-counting; autosave resolved at global instead of per-buffer scope) plus five Medium
>   items, all in the exact mechanism this report marked "no concerns".
> - A second fix (commit follows this report's supersession) — a second, unbounded way to reach
>   the same Critical's user-visible bug via a *deferred* hibernate (autosave-race or an active
>   debug session) that commits the `Hibernated` label without ever calling `LspStore::hibernate`,
>   then wakes unconditionally on reactivation. Closed by a guard in `LspStore::wake` itself,
>   plus wiring the remote store's staleness into the UI-facing query methods
>   (`has_stale_diagnostics`/`is_diagnostic_summary_stale`) that this report's FR3 section
>   verified for local projects only.
>
> This is left in place, unedited below, as the historical record of what the first verification
> pass actually checked — not touched or rewritten, since re-verifying the current code is the
> independent tester's call to make, not the implementer's. Look for a newer
> `phase-03-verification-*.md` (or this file's own next revision from that role) for a pass that
> covers `7c504a6` and the commit after it.

**Date:** 2026-08-06  
**Commits Verified:** 90da4f7, 557a3ca, 54b8a40  
**Status:** PASSED — all claims verified, all tests passing, no concerns
**Status as of this superseding note: STALE — see banner above.**

---

## Test Results

### All Test Suites Pass

| Suite | Command | Passed | Failed | Notes |
|-------|---------|--------|--------|-------|
| **project** | `cargo test -p project --features test-support -- --skip test_file_status --skip test_git_repository_status --skip test_rename_work_directory --skip test_rescan_and_remote_updates` | 256 | 0 | 3 ignored, 4 filtered (known hangs) |
| **workspace** | `cargo test -p workspace --all-features` | 200 | 0 | — |
| **remote_server** | `cargo test -p remote_server --features test-support` | 25 | 0 | — |
| **zode** | `cargo test -p zode --bin zode --features test-support` | 52 | 0 | 1 ignored |

**Clippy:** `./script/clippy` passed cleanly — no warnings or errors.

---

## Requirement Verification

### ✅ Protected Tests (NFR2): Byte-for-Byte Unchanged

Verified via git diff that the three protected tests remain completely unchanged:

- `test_diagnostic_summaries_cleared_on_worktree_entry_removal` (line 3791)
- `test_diagnostic_summaries_cleared_on_server_restart` (line 3869) ← **critical test**
- `test_diagnostic_summaries_cleared_on_buffer_reload` (line 3943)

**Verification Method:** Compared commit 90da4f7's version against HEAD — all three tests have zero diff, confirming the NFR2 contract is intact. The three test assertions that pin down the restart/stop behavior remain exactly as before.

---

### ✅ FR1: Diagnostic Summaries Preserved on Hibernate

**Claim:** `LspStore::hibernate()` stops servers without clearing `diagnostic_summaries`.

**Verification:**
- `stop_local_language_server_inner` (line 11196) takes a `clear_diagnostics: bool` parameter
- Public wrapper `stop_local_language_server` (line 11174) calls inner with `true` (unchanged behavior for restart)
- `LspStore::hibernate()` (line 11363) calls inner with `false` (line 11393) — skips diagnostic clearing
- Only the 4-clause conditional `if clear_diagnostics` (lines 11222–11258) is skipped; all other cleanup proceeds identically:
  - Buffer squiggles removed ✓
  - `local.diagnostics` cleaned ✓
  - Watched paths removed ✓
  - Server removed from `language_server_ids` ✓

**Test Coverage:** `test_hibernate_preserves_diagnostic_summaries` in project_tests.rs confirms the summary count survives hibernation while per-line diagnostics are cleared.

---

### ✅ FR2: Wake Re-registers Buffers Without Blocking UI

**Claim:** `LspStore::wake()` restarts servers for open buffers without blocking.

**Verification:**
- `wake()` (line 11420) sets `hibernated = false` and calls `restart_all_language_servers()`
- `restart_all_language_servers()` (line 11400) collects open buffers and calls `restart_language_servers_for_buffers`
- Server startup happens asynchronously via `cx.background_spawn()` (line 11395) — UI not blocked
- `register_buffer_with_language_servers` naturally starts fresh servers since `hibernate` already cleared `language_server_ids` — no new mechanism needed

**Design:** Reuses existing `get_or_insert_language_server` path (on-demand start per buffer), not a separate "all servers ever" enumeration.

---

### ✅ FR3 + FR4: Stale Flag Tracking and Per-File Replacement

**Claim:** Summaries marked stale; first fresh publish removes stale flag for that file.

**Verification:**
- Stale tracking: `stale_language_server_ids: HashSet<LanguageServerId>` on LocalLspStore (line 307)
- On hibernate: extends this set with all stopped server ids (line 11388-11390)
- On publish: `update_worktree_diagnostics` (line 8738-8745) calls `summaries_by_server_id.retain(|id, _| !stale_ids.contains(id))` to remove stale entries when a fresh (non-stale) server publishes
- Stale removal is exact-path granularity, not propagated to ancestors — matches diagnostic_counts structure

**Test Coverage:** `test_wake_replaces_stale_summaries` confirms stale entries don't duplicate; a fresh publish from the new server replaces the old stale entry.

---

### ✅ FR4b: Project-Wide Stale Sweep on Indexing Complete

**Claim:** When the first post-wake server finishes indexing (`disk_based_diagnostics_finished`), ALL remaining stale entries are cleared project-wide.

**Verification:**
- `disk_based_diagnostics_finished` (line 9982) calls `clear_stale_diagnostics_after_reindex()` (line 10016)
- `clear_stale_diagnostics_after_reindex` (line 10027) sweeps `diagnostic_summaries` across all worktrees/paths, removing entries keyed by any stale id
- Comment (lines 10004-10015) explains the safety net: catches files that never changed themselves but broke due to dependency changes, preventing silent errors

**Trade-off (documented):** Fires on the FIRST post-wake server to report indexing complete, not the last — a slower second server's stale entries clear slightly early. Same order of risk as hibernation itself (showing possibly-outdated data slightly longer), not a new class of problem.

**Test Coverage:** `test_wake_clears_stale_summary_after_reindex_complete` confirms the project-wide sweep happens and files that never got a fresh publish still have their stale summaries cleared.

---

### ✅ FR5: Prettier Hibernation

**Claim:** `PrettierStore` hibernates (drops instance) and wakes (lazy-restarts).

**Verification:**
- `PrettierStore::hibernate()` exists at line 118 of prettier_store.rs
- Called from `Project::try_hibernate_resources()` (line 4796-4798)
- Prettier lazy-restarts on demand (already the pattern) — no wake-specific code needed

---

### ✅ FR6: Hibernation is Idempotent

**Claim:** Calling `hibernate()` twice doesn't error or double-stop.

**Verification:**
- `hibernated: bool` flag (line 3985) gates the stale-id logic
- `hibernate()` sets this flag (line 11364) before stopping servers
- Stopping a server that's already stopped via `stop_local_language_server_inner` is safe — `language_server_ids` won't have it (already removed on first stop)
- Extended stale ids are idempotent (HashSet deduplicates)

**Test Coverage:** `test_hibernate_is_idempotent` confirms calling `set_activity(Hibernated)` twice works without error or side effects.

---

### ✅ FR7: Remote/SSH Guard Against Stale Wipe

**Claim:** Host's zero-count `UpdateDiagnosticSummary` is dropped while client is hibernated.

**Verification:**

**Architecture:**
- When a remote project hibernates, `hibernate()` branches on `upstream_client()` (line 11365)
- Sends `proto::StopLanguageServers { all: true }` to the host — reuses existing shutdown path
- Host's handle clears diagnostics and pushes zeroed `UpdateDiagnosticSummary` down
- Client's `handle_update_diagnostic_summary` (line 9628) checks: if summary is empty AND project is `hibernated`, `continue` (drop the message) (line 9652-9670)

**Field:** `hibernated: bool` on LspStore (lines 3975-3984) — set by `hibernate()` (line 11364), cleared by `wake()` (line 11421)

**Test Coverage:** `test_hibernate_ignores_stale_zero_diagnostic_summary_from_host` in remote_server/remote_editing_tests.rs tests this with a REAL client/host pair connected via `RemoteClient::connect_mock()`:
- Creates HeadlessProject on server, RemoteProject on client
- Opens buffer, publishes diagnostic from fake rust-analyzer on host
- Confirms it propagates to client (summary shows error_count=1)
- Hibernates the client project → sends `StopLanguageServers` to host
- Host's normal shutdown path clears and pushes zeroed `UpdateDiagnosticSummary`
- Client assertion: summary still shows error_count=1 (stale but preserved) ✓

---

### ✅ FR8: Autosave Barrier (Deferred Hibernation)

**Claim:** Autosave + dirty buffer defers hibernation; autosave off never blocks.

**Verification:**

**Implementation:**
- `Project::try_hibernate_resources()` (line 4781) checks barriers before stopping servers
- `has_active_debug_session()` (line 4808): checks `DapStore::sessions()`, filtering on `!is_terminated()`
- `autosave_would_race_hibernate()` (line 4820): returns true only if a dirty buffer exists AND autosave is not `Off`
- Both barriers call `schedule_hibernate_retry()` to defer, not cancel (line 4784, 4789)

**Default:** `"autosave": "off"` in assets/settings/default.json — dirty buffer alone never blocks

**Tests:**
1. `test_hibernate_deferred_by_autosave_after_delay_with_dirty_buffer` — setting `"after_delay": { "milliseconds": 1000 }`, dirty buffer, hibernation deferred ✓
   - Activity label commits to `Hibernated` (Phase 2 contract)
   - But language server still running (per-line diagnostics survive) — the REAL contract
2. `test_hibernate_deferred_by_autosave_on_focus_change_with_dirty_buffer` — same with `on_focus_change` ✓
3. `test_hibernate_not_blocked_by_dirty_buffer_when_autosave_off` — autosave off, dirty buffer, hibernation completes ✓
   - Activity label commits to `Hibernated`
   - No retry scheduled

---

### ✅ FR8 Subtlety: Activity Label ≠ Resource State

**Claim:** `activity() == Hibernated` doesn't guarantee language server stopped when FR8 barriers defer it.

**Verification:**
- `Project::set_activity()` (Phase 2, deliberately untouched) always commits the label on valid transitions (line 4681 in commit message)
- `try_hibernate_resources()` is called AFTER the label commits
- If a barrier fires, `schedule_hibernate_retry()` defers the actual stop
- Tests explicitly check the REAL contract: buffer's per-line diagnostics still exist (language server running), not the label

**Confirmed in test comments (line 13421-13426):** "The activity label still commits per Phase 2's pure state machine (this phase was told not to change that) -- what FR8 actually guarantees is that the *resource* hibernation itself is deferred, which is observable as the language server still being alive: its per-line diagnostics haven't been cleared the way a real hibernate would clear them."

**Design rationale:** The implementer's first draft asserted `activity()` reverts on barrier fire — wrong. The label commits, but the resources stay. Tests were corrected to check the real contract.

---

### ✅ DAP Barrier: Full Stop When Debug Session Active

**Claim:** Active debug session skips hibernation entirely (not even LSP stops).

**Verification:**
- `Project::try_hibernate_resources()` checks `has_active_debug_session()` first (line 4782)
- If true, defers and returns — never reaches LSP stop
- `has_active_debug_session()` checks `DapStore::sessions()` filtered on `!Session::is_terminated()` — catches background sessions, not just focused ones

**Comment (4801-4807):** "FR5/step 8: a running debugger is the user's active work. If one is attached to this project, hibernation is skipped entirely for it (not even the LSP stops) rather than trying to hibernate around it. Deliberately checks every session the project knows about, not just whichever one currently has UI focus."

**Test:** `test_hibernate_skipped_when_debug_session_active` creates a session via `DapStore::new_session()` (no live adapter needed), hibernation request is issued, confirms:
- Activity label commits to `Hibernated` (Phase 2)
- But language server still running (per-line diagnostics survive) — hibernation was skipped

---

### ✅ Architecture: Dependency Direction Verified

**Claim:** `crates/project` has no dependency on `crates/workspace` (one-way edge: workspace → project).

**Verification:**
```
crates/project/Cargo.toml:   [no "workspace" crate dependency]
crates/workspace/Cargo.toml: project = { workspace = true, features = ["test-support"] }
```

**Why it matters:** `ProjectSettings` needs `autosave` setting, but can't import from `WorkspaceSettings`. Instead, `ProjectSettings::from_settings()` reads `content.workspace.autosave` directly from the flattened `SettingsContent` (line 743 of project_settings.rs) — no new crate dependency required.

**Field added:** `autosave: AutosaveSetting` on ProjectSettings (line 91 of project_settings.rs), initialized from `content.workspace.autosave.unwrap_or(AutosaveSetting::Off)`.

---

## Commit-Message/Content Mismatch (Confirmed)

**Claim:** Commit 90da4f7's message describes flipping `retain_background_projects`, but the actual change is in 54b8a40.

**Verification:**
- Commit 90da4f7: message says "Also flips workspace.multi_project.retain_background_projects's default from false to true in assets/settings/default.json"
- **But:** `assets/settings/default.json` is NOT in 90da4f7's file diff
- Commit 54b8a40: **DOES** include `assets/settings/default.json | 16 ++++++++++------` in file changes
- Commit 54b8a40's message explicitly notes the mismatch: "This is the actual retain_background_projects default flip the previous commit's message described -- it belongs with the tests it breaks, which live in two files outside that commit's scope (crates/project), so it's split out here rather than folded in inaccurately. Noting the mismatch plainly rather than leaving it implicit."

**Reason:** Flipping the default exposed test failures in `crates/workspace` and `crates/zede`, which are outside the scope of the commit touching `crates/project`. Split into two commits for clarity and to keep test-fixing changes with the file they test.

**Tests fixed in 54b8a40:**
- `crates/workspace/src/multi_workspace_tests.rs`: 3 tests (added missing `disable_background_project_retention()` call)
- `crates/zed/src/zed.rs`: test_open_paths_action (updated assertion from 1→2 live workspaces, rewrote comment)

---

## Clippy and Code Quality

- **Release clippy (--deny warnings):** ✓ Clean
- **Touched crates:** project, project_panel, diagnostics, remote_server, workspace, zed
- **Code patterns:** No unwrap/panic on fallible paths; errors propagate via `?` or `.log_err()` ✓
- **DRY principle:** `stop_local_language_server` refactor is minimal — thin wrapper + inner with parameter, no copy-paste ✓

---

## Summary of Verification

| Item | Status | Notes |
|------|--------|-------|
| Test suites (256 + 200 + 25 + 52) | ✅ All pass | No failures, known hangs skipped |
| Protected tests unchanged | ✅ Verified | Byte-for-byte diff confirms NFR2 |
| FR1: Summary preserved | ✅ Verified | `clear_diagnostics: false` path confirmed |
| FR2: Wake non-blocking | ✅ Verified | Uses async `background_spawn` |
| FR3+FR4: Stale tracking | ✅ Verified | HashSet stale ids, per-file removal on publish |
| FR4b: Project-wide sweep | ✅ Verified | Called on indexing complete, documented trade-off |
| FR5: Prettier hibernation | ✅ Verified | `hibernate()` exists, lazy-restart on wake |
| FR6: Idempotent | ✅ Verified | Flag gates, HashSet deduplicates |
| FR7: Remote guard | ✅ Verified | Real client/host test proves guard works |
| FR8: Autosave barrier | ✅ Verified | Defers on autosave+dirty, never blocks on autosave-off |
| FR8: Activity ≠ resource state | ✅ Verified | Tests check real contract (language server running), not label |
| DAP barrier | ✅ Verified | Full skip on active session, no LSP stop attempted |
| Dependency direction | ✅ Verified | No project→workspace edge, settings flow via SettingsContent |
| Commit mismatch explained | ✅ Verified | Split justified, tests fixed for default flip |
| Clippy | ✅ Clean | No warnings |

---

## Conclusion

**Phase 3 is complete and correct.** All three commits pass comprehensive verification:

1. **90da4f7:** Core hibernation logic, stale tracking, FR8/DAP barriers, Prettier hibernation
2. **557a3ca:** Real client/host integration test for FR7 remote guard
3. **54b8a40:** Default retention flip + test fixes for reverse dependencies

The implementation honors the phase requirements exactly:
- LSP hibernation preserves summaries, wakes non-blocking
- Stale flags cleared on first publish (FR4) and project-wide after reindex (FR4b)
- Remote projects guarded against diagnostic wipe
- Autosave and DAP barriers defer/skip hibernation correctly
- Protected restart tests remain unchanged
- Code is clean, DRY, and covered by specific test cases

All FRs and NFRs are met. Ready for review.
