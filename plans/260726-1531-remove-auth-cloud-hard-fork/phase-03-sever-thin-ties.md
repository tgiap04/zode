---
phase: 3
title: "Sever thin ties"
status: pending
effort: "1d"
---

# Phase 3: Sever thin ties

## Context Links

- [Survivor patch detail](./reports/scout-survivor-patches.md) §5, §7, §14, §15
- [plan.md](./plan.md) — "The three findings that shaped this plan"

## Overview

**Priority:** P1 · **Build state:** GREEN (must stay green) · **Depends on:** Phase 2

Cut the handful of thin symbol-level dependencies that are the *only* reason three valuable crates looked deletable. Each patch is small; together they keep `editor`, `project`, and `recent_projects` at **zero changes** for the entire operation.

**This is the highest-leverage phase in the plan.** ~40 lines of patch here avoids ~8,800 lines of surgery in the three most dangerous files in the codebase.

## Key Insights

- `edit_prediction_types` (380 LOC) touches the cloud graph through exactly **one import used at 3 sites**: `use client::EditPredictionUsage`. Deleting the crate instead would mean ~429 `edit_prediction` references in `editor.rs`, 33 in `element.rs`, 5 in `display_map.rs`.
- `remote_connection` (728 LOC) touches it through **four call sites across two separate trait impls** — corrected by red team, the scout report undercounted by half:
  - `impl RemoteClientDelegate for RemoteClientDelegate` `:450` → `download_remote_server_release` `:484`, `get_remote_server_release_url` `:515`
  - `impl RemoteClientDelegate for BackgroundRemoteClientDelegate` `:626` → `download_remote_server_release` `:649`, `get_remote_server_release_url` `:680`
- `remote_server` touches it through `crashes::init()` (×2) + `crashes::crash_server()`, plus a `proto::GetCrashFiles` handler.
- `settings_content` needs three tiny dependency-free enums from `language_model_core`: `ReasoningEffort`, `ModelMode`, `Speed`. `ReasoningEffort`'s own doc comment (`language_model_core.rs:461-464`) says it mirrors `settings_content` — it was hoisted *out* originally, so moving it back is a restoration.
- `cloud_api_types` needs one 9-line struct from `cloud_llm_client`: `CurrentUsage`.
- **Ordering trap:** `edit_prediction_types` must be patched *before* Phase 5 guts `client`, because it is the last surviving consumer of `client::EditPredictionUsage`.

## Requirements

**Functional**
- After this phase, none of `edit_prediction_types`, `remote_connection`, `remote_server`, `settings_content`, `cloud_api_types` references any crate in the Phase 4 delete set.
- SSH remote development must still function (commissioner's decision — reversed from the earlier "drop it" answer once the true cost was measured).

**Non-functional**
- Build stays green throughout; each patch is independently committable.

## Architecture

Five independent severances, no ordering constraints among themselves:

| Crate | Severs | Replacement strategy |
|---|---|---|
| `edit_prediction_types` | `client::EditPredictionUsage` | Delete the `usage()` trait method + handle impl (~10 lines). Nothing calls it. |
| `remote_connection` | `auto_update::AutoUpdater` | Replace with a direct GitHub Releases fetch, or `bail!` requiring a pre-installed binary |
| `remote_server` | `crashes::*` | Delete crash-handler init + the `GetCrashFiles` RPC handler |
| `settings_content` | `language_model_core` | Move 3 enums into `settings_content` (restoration, not new code) |
| `cloud_api_types` | `cloud_llm_client` | Inline the 9-line `CurrentUsage` struct |

## Related Code Files

**To modify**
- `crates/edit_prediction_types/src/edit_prediction_types.rs` — `:3`, `:165-168`, `:215`, `:267-271`; `Cargo.toml` drop `client`
- `crates/remote_connection/src/remote_connection.rs` — `:5`, `:484`, `:515`; `Cargo.toml` drop `auto_update`
- `crates/remote_server/src/main.rs` `:15`, `:33`; `crates/remote_server/src/server.rs` `:230`, `:261`, `:273`, `:464-465`, `:719-720`; `Cargo.toml` drop `crashes`
- `crates/settings_content/src/language_model.rs` `:240`, `:482`; `src/agent.rs` `:428`; `src/merge_from.rs` `:59`; `Cargo.toml` drop `language_model_core`
- `crates/cloud_api_types/src/plan.rs` `:22`; `Cargo.toml` drop `cloud_llm_client`

**To read for reference**
- `crates/language_model_core/src/language_model_core.rs` `:453-476` (the enums to move)
- `crates/cloud_llm_client/src/cloud_llm_client.rs` `:320-328` (`CurrentUsage`)

## Implementation Steps

### 3a. `edit_prediction_types` — sever `client` (do this first; Phase 5 depends on it)

1. Delete `use client::EditPredictionUsage;` at `:3`.
2. Delete the `usage()` default method (`:165-168`), the trait method reference at `:215`, and the handle impl (`:267-271`).
3. Drop `client` from `crates/edit_prediction_types/Cargo.toml`.
4. `cargo check -p edit_prediction_types && cargo check -p editor` — **editor must need zero changes.** If it does not, stop and re-scope.

### 3b. `settings_content` — sever `language_model_core`

5. Move `ReasoningEffort` (`language_model_core.rs:465-476`), `ModelMode` (`:453-459`), and `Speed` (`request.rs:340-353`) into `crates/settings_content/`. All three are small fieldless/2-variant enums with no dependencies.
6. Rewrite `language_model.rs:240` and `:482` from `pub use language_model_core::X` to local definitions; change `agent.rs:428` and `merge_from.rs:59` to `crate::Speed`.
7. Drop the dep; `cargo check -p settings_content`.
   > Note: `agent.rs` and `language_model.rs` are deleted wholesale in Phase 8, but this phase must keep the build green *now*, so patch them rather than pre-deleting.

### 3c. `cloud_api_types` — sever `cloud_llm_client`

8. Inline `CurrentUsage` (9 lines) into `crates/cloud_api_types/src/plan.rs`, replacing the qualified reference at `:22`. Drop the dep.
9. **Verify the four extension crates.** The expected set is `ExtensionMetadata`, `ExtensionProvides`, `Timestamp`, `KnownOrUnknown` — **plus two the scout report omitted** (red team): `ExtensionApiManifest` (`extension_cli/src/main.rs:111`) and `GetExtensionsResponse` (`extension_host/src/extension_host.rs:13`). Seeing those two is **expected**, not a stop-and-rescope trigger.
   ```sh
   rg -n "cloud_api_types::" crates/extension*/src/
   ```

### 3d. `remote_connection` — sever `auto_update`

10. **Decide the remote-server binary delivery mechanism, and its integrity check.** Two decisions, not one:
    - *Transport:* fetch from GitHub Releases via the existing `http_client`, mirroring the URL shape `AutoUpdater::get_remote_server_release_url` produced. Fallback: `bail!("remote server binary not found; install it manually at <path>")`.
    - *Integrity:* **mandatory.** See the security note below — this is a hard gate, not advice.
11. Replace **all four** call sites — `:484`, `:515` (in `RemoteClientDelegate`) **and** `:649`, `:680` (in `BackgroundRemoteClientDelegate`). Delete `use auto_update::AutoUpdater;` at `:5`. Drop the dep.
12. Implement the integrity check before the binary is written into place (the equivalent of `auto_update.rs:885`'s `smol::fs::rename`). Verify a signed release manifest or a published SHA256. **Do not ship a bare HTTPS GET.**
13. `cargo check -p remote_connection -p recent_projects` — **`recent_projects` must need zero changes.**

### 3e. `remote_server` — sever `crashes`

13. Delete `crashes::crash_server(socket)` (`main.rs:33`) and the `--crash-handler` arg plumbing at `main.rs:15`.
14. Delete both `crashes::init(InitCrashHandler{…})` blocks (`server.rs:464-465`, `:719-720`).
15. Delete the `GetCrashFiles` RPC handler (`server.rs:230`, `:261`, `:273`). **Leave the proto messages in `proto/app.proto` and `zed.proto` alone** — unused proto messages are harmless, and renumbering the envelope is a needless risk.
16. Drop the dep; `cargo check -p remote_server`.

### 3f. Gate

17. `cargo check --workspace` green.
18. Re-run `research/final-delete-set.py`; confirm the five crates report `must-patch: (none)` and the survivor count is 15 minus the ones just fixed.
19. Commit as five separate commits, one per severance.

## Todo List

- [ ] 3a `edit_prediction_types` severed; `cargo check -p editor` needs **zero** changes
- [ ] 3b `settings_content` severed; three enums relocated
- [ ] 3c `cloud_api_types` severed; extension crates verified
- [ ] 3d `remote_connection` severed — **all four call sites** (`:484`, `:515`, `:649`, `:680`) across **both** impls
- [ ] 3d Remote-binary delivery transport decided and implemented
- [ ] 3d **Integrity verification implemented** — signature or published SHA256 checked before the binary is moved into place. A bare HTTPS GET does not satisfy this item.
- [ ] 3e `remote_server` severed; proto messages left intact
- [ ] `cargo check --workspace` green
- [ ] `final-delete-set.py` re-run and confirms the keep-list
- [ ] Five standalone commits

## Success Criteria

- `cargo check --workspace` green.
- `cargo tree -i client -e normal` no longer lists `edit_prediction_types`.
- `cargo tree -i auto_update -e normal` no longer lists `remote_connection`.
- `cargo tree -i crashes -e normal` no longer lists `remote_server`.
- `editor` and `recent_projects` untouched — verified by `git diff --stat`. (`project` needs 2 small edits, handled in Phases 7 and 9 — see plan.md finding 5.)
- **The remote-server binary fetch verifies a signature or checksum.** Demonstrate it: point the fetch at a deliberately corrupted artifact and confirm it is rejected.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Remote-binary download replacement is under-designed | SSH remote dev silently broken until manual testing | Step 10 forces an explicit decision; Phase 11 includes a live SSH remote-dev smoke test |
| `edit_prediction_types` patch leaves `editor` needing changes | The whole keep-it rationale collapses | Step 4 is a hard gate — stop and re-scope rather than proceeding |
| Moving enums changes serde representation | Users' `settings.json` values silently stop parsing | Copy the enums **verbatim** including all `#[serde(...)]` attributes; add a round-trip test |
| Extension crates need more from `cloud_api_types` than expected | Phase 4 breaks the extension system | Step 9 verifies before committing |

## Security Considerations

**The remote-server binary fetch is the single most security-sensitive change in this plan.** It downloads an executable and runs it on a remote host.

The red team established that this is a **trust downgrade, not a lateral move**:

| | Today | Naive replacement |
|---|---|---|
| Transport | HTTPS | HTTPS |
| URL source | brokered by Zed's **authenticated** cloud API | raw GitHub Releases URL |
| Checksum / signature | **none** (`auto_update.rs:570-609`, `:870-888` — verified, no `sha256`/`verify` anywhere) | none, unless we add it |

So the existing code was already weak, and the obvious replacement removes even the authenticated broker. **Adding integrity verification is therefore mandatory, not an improvement** — it is what keeps the replacement from being strictly worse than what it replaces.

Concrete requirement: a signed release manifest (minisign or sigstore) or a published SHA256, verified **before** the binary is moved into place. Key custody must be decided in Phase 12 alongside release signing. This is gated in the Todo List and Success Criteria — it cannot be quietly skipped.

Also: removing the crash handler from `remote_server` means remote-side panics leave no artifact. Accepted per the commissioner's "bỏ crash hoàn toàn" decision.

## Next Steps

Phase 4 — the atomic structural cut. **The build goes red and stays red until Phase 11.**
