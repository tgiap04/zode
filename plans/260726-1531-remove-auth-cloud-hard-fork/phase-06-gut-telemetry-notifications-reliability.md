---
phase: 6
title: "Gut telemetry notifications reliability"
status: pending
effort: "1d"
---

# Phase 6: Gut telemetry notifications reliability

## Context Links

- [Survivor patch detail](./reports/scout-survivor-patches.md) §8 — `notifications`
- [`crates/zed` wiring map](./reports/scout-zed-wiring.md) §3 — `reliability.rs` split
- [Workspace surgery research](./reports/research-workspace-surgery.md) §6 — stub vs delete

## Overview

**Priority:** P1 · **Build state:** 🔴 RED · **Depends on:** Phase 4 (parallel with Phase 5)

Three independent gut jobs that together close the remaining data-egress paths:

1. **`telemetry`** — the one place in this plan where **stubbing beats deleting**. 29 crates call it with fire-and-forget one-liners; a no-op in one file is far less churn than editing 29 files.
2. **`notifications`** — delete `notification_store.rs` (collab), keep `status_toast` (13 surviving consumers).
3. **`reliability.rs`** — split a 499-line file into a keepable ~130-line hang detector and a fully deletable crash-upload half.

## Key Insights

- `crates/telemetry` is a **66-line macro shim** with no auth dependency. Deleting it would break 29 crates for nothing. Making `send_event!` a no-op achieves the privacy goal with a blast radius of one file. This is not "faking functionality" — it is honestly declining to phone home.
- The identity-bearing telemetry lives in `crates/client/src/telemetry.rs`, not the `telemetry` crate. `set_authenticated_user_info` (`:382-397`) is the only auth coupling; it is called from `user.rs:842` and `client.rs:707`, both removed in Phase 5.
- **There is no Sentry SDK.** Crash reporting is a plain HTTP multipart POST to `client::telemetry::MINIDUMP_ENDPOINT`. So `reliability.rs` splits cleanly with no dependency untangling — the only Cargo fallout is `reqwest` becoming unused in `crates/zed`.
- `notifications/src/status_toast.rs` (257 lines) imports only `std::rc::Rc`, `gpui`, `ui`, `workspace`, `zed_actions::toast`. **Verified: zero `channel`/`client`/`rpc` dependency.** Gutting `notification_store.rs` is sufficient.
- `crates/notifications/src/notifications.rs` is 4 lines. It becomes 1 line.

## Requirements

**Functional**
- No telemetry event reaches the network, by construction rather than by configuration.
- No minidump is uploaded anywhere.
- `status_toast` continues to work for its 13 consumers.
- Local hang-trace detection is preserved (it writes to disk only, never egresses).

**Non-functional**
- `telemetry`'s public macro API is unchanged so the 29 call sites need no edits.

## Architecture

```
crates/telemetry/           STUB  — send_event() becomes a no-op; API surface unchanged
crates/client/src/telemetry.rs
                            GUT   — drop set_authenticated_user_info, MINIDUMP_ENDPOINT,
                                    flush_events network path; keep the settings type
crates/notifications/
  ├── notification_store.rs DELETE (428 lines — uses channel::ChannelStore)
  ├── status_toast.rs       KEEP   (257 lines — 13 surviving consumers)
  └── notifications.rs      REDUCE to `pub mod status_toast;`
crates/zed/src/reliability.rs
  ├── monitor_hangs, save_hang_trace, cleanup_old_hang_traces   KEEP (~130 lines, disk only)
  └── upload_minidump, upload_build_timings, FormExt, GetCrashFiles harvest   DELETE (~370 lines)
```

## Related Code Files

**To delete**
- `crates/notifications/src/notification_store.rs`

**To modify**
- `crates/telemetry/src/telemetry.rs` — `send_event` `:56`, `init` `:62`
- `crates/client/src/telemetry.rs` — `set_authenticated_user_info` `:382-397`, `MINIDUMP_ENDPOINT`, event envelope `:632-636`
- `crates/notifications/src/notifications.rs` — reduce to one line
- `crates/notifications/Cargo.toml` — drop `channel`, `client`, `db`, `rpc`, `sum_tree`, `time`, and the `test-support` feature
- `crates/zed/src/reliability.rs` — `:2`, `:6`, `:9`, `:10-13`, `:17`, `:24`, `:31-51`, `:53-89`, `:222-403`, `:405-499`
- `crates/zed/Cargo.toml` — `reqwest` likely becomes unused (verify)

## Implementation Steps

### 6a. `telemetry` — no-op stub

1. Make `send_event` (`telemetry.rs:56`) a no-op that consumes its arguments without dispatching. Keep the macro signature identical so all 29 call sites compile untouched.
2. Keep `init` as a no-op too — call sites pass a handle that no longer matters.
3. Add a module-level comment explaining *why* the crate exists as a no-op (this is exactly the "why, not what" case `CLAUDE.md` asks for).
4. `cargo check -p telemetry`.

### 6b. `client/src/telemetry.rs` — remove identity and endpoints

5. Delete `set_authenticated_user_info` `:382-397` and the `metrics_id`/`is_staff` state fields `:48-58`.
6. Delete `MINIDUMP_ENDPOINT` and any event-upload HTTP path; delete `flush_events`' network body.
7. Keep `TelemetrySettings` (`client.rs:516-528`) — the settings key survives (see the settings scout's keep-list) and Phase 9 leaves `"telemetry"` in `default.json`.
8. `cargo check -p client`.

### 6c. `notifications` — keep the toast, drop the store

9. Delete `crates/notifications/src/notification_store.rs`.
10. Reduce `notifications.rs` to `pub mod status_toast;`.
11. Prune `Cargo.toml`: drop `channel`, `client`, `db`, `rpc`, `sum_tree`, `time`; verify each of `anyhow`, `util` is still needed by `status_toast` before removing.
12. Delete the `test-support` feature and its dev-deps.
13. `cargo check -p notifications`, then spot-check a consumer: `cargo check -p git_ui` (8 `status_toast` call sites).
    > The `notifications::init` call at `main.rs:729` is removed in Phase 8, not here.

### 6d. `reliability.rs` — split

14. Delete the import block: `:2` (`client::telemetry::MINIDUMP_ENDPOINT`), `:6` (`http_client`), `:9` (`proto::{CrashReport, GetCrashFilesResponse}`), `:10-13` (`reqwest` multipart), `:17` (`sysinfo`).
15. Change `pub fn init(client: Arc<Client>, cx)` `:24` to `init(cx: &mut App)`. Delete `:31-43` (`on_flags_ready` → build timings) and `:45-51` (`diagnostics_enabled` → minidump upload).
16. Delete `:53-89` — the `observe_new(Project)` remote crash-file harvest. Its server half was removed in Phase 3e.
17. Delete `upload_previous_minidumps` `:222-261`, `upload_minidump` `:263-403`, `BuildTiming` + `upload_build_timings` `:405-478`, `trait FormExt` `:480-499`.
18. **Keep** `MAX_HANG_TRACES` `:22`, `monitor_hangs` `:92-141`, `cleanup_old_hang_traces` `:143-162`, `save_hang_trace` `:164-220`. These write to disk only.
19. Update the caller at `main.rs:621` to the new signature (Phase 8 touches `main.rs` broadly; this one-line change can land here or there — do it here to keep `reliability.rs` self-contained).
20. Check whether `reqwest` is now unused in `crates/zed/Cargo.toml`; remove if so.

### 6e. Gate

21. `cargo check -p telemetry -p client -p notifications` all green.
22. Prove no egress path survives in these crates:
    ```sh
    rg -n "MINIDUMP_ENDPOINT|sentry|multipart|api\.zed\.dev" crates/telemetry crates/notifications crates/zed/src/reliability.rs crates/client/src/telemetry.rs
    ```
23. Three commits, one per sub-phase.

## Todo List

- [ ] 6a `telemetry::send_event` is a no-op; macro API unchanged; comment explains why
- [ ] 6b `set_authenticated_user_info` and `MINIDUMP_ENDPOINT` gone; `TelemetrySettings` kept
- [ ] 6c `notification_store.rs` deleted; `status_toast` intact; `git_ui` still checks
- [ ] 6c `notifications/Cargo.toml` pruned
- [ ] 6d `reliability.rs` reduced to the hang detector (~130 lines)
- [ ] 6d `init` signature changed and `main.rs:621` updated
- [ ] 6d `reqwest` removed from `crates/zed` if unused
- [ ] `cargo check -p telemetry -p client -p notifications` green
- [ ] Egress grep returns nothing
- [ ] Three standalone commits

## Success Criteria

- `cargo check -p telemetry -p client -p notifications` green.
- `rg -n "MINIDUMP_ENDPOINT|multipart" crates/` returns nothing.
- `status_toast` compiles and `git_ui` (its heaviest consumer) checks clean.
- `reliability.rs` ≤ 150 lines and imports neither `http_client` nor `reqwest`.
- Hang traces still written to disk — verified manually in Phase 11.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Deleting `telemetry` instead of stubbing | 29 crates break for zero benefit | Explicitly forbidden in this phase's requirements |
| Pruning a `notifications` dep that `status_toast` needs | `notifications` fails to compile | Step 11 verifies each dep individually before removal |
| Removing hang detection along with crash upload | Losing the one diagnostic that costs nothing and leaks nothing | Step 18 explicitly enumerates what to keep |
| `TelemetrySettings` removed | `default.json` `"telemetry"` key loses its consumer → startup panic (see Phase 9 unwrap trap) | Step 7 explicitly keeps it |
| `reqwest` removed while still used elsewhere in `crates/zed` | Build error | Step 20 says verify, not assume |

## Security Considerations

This phase completes the privacy objective at the code level:
- telemetry events no longer leave the process,
- minidumps are no longer uploaded,
- no user identity (`metrics_id`, `is_staff`) is attached to anything.

What remains and is **intentionally allowed** per the commissioner's "không gửi dữ liệu người dùng đi — vẫn cho phép tải về": extension registry downloads, LSP binary downloads. Phase 11 verifies at runtime that nothing else does.

## Next Steps

Phase 7 — the light survivors: eight crates, most under 15 lines each.
