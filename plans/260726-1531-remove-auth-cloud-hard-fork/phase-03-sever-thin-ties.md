---
phase: 3
title: Sever thin ties
status: completed
effort: 1d
---

# Phase 3: Sever thin ties

## Context Links

- [Survivor patch detail](./reports/scout-survivor-patches.md) §5, §7, §14, §15
- [plan.md](./plan.md) — "The three findings that shaped this plan"

## Overview

**Priority:** P1 · **Build state:** GREEN (must stay green) · **Depends on:** Phase 2

Cut the thin symbol-level dependencies that are the *only* reason three valuable crates looked deletable. The keep-decision they protect (`editor`, `recent_projects` at zero changes) is what makes this the highest-leverage work in the plan.

## ⚠ Structural correction, found during execution

**Only two of the five severances can run while the build is green.** The other three share a shape the plan did not account for:

> A severance that **replaces a shared type with a local copy** cannot run while the crate that
> originally owned the type is still present. The two definitions are distinct types, so every
> cross-crate consumer breaks — and those consumers only disappear at Phase 4.

| Severance | Shape | Can run pre-cut? |
|---|---|---|
| 3d `remote_connection` ← `auto_update` | replaces **call sites** | ✅ yes |
| 3e `remote_server` ← `crashes` | deletes **call sites** | ✅ **done** |
| 3a `edit_prediction_types` ← `client` | removes a **trait method** 3 delete-set crates override | ❌ E0407 |
| 3b `settings_content` ← `language_model_core` | **re-export → local copy** of 3 enums | ❌ type mismatch |
| 3c `cloud_api_types` ← `cloud_llm_client` | **shared struct → local copy** | ❌ type mismatch |

Evidence for each:
- **3a** — `EditPredictionDelegate::usage()` has a default impl (`edit_prediction_types.rs:167`) overridden by `codestral:195`, `copilot:40`, `edit_prediction:48`.
- **3b** — `copilot_chat/responses.rs:9` (`pub use settings::OpenAiReasoningEffort`), `open_router:7` (`pub use settings::ModelMode`), `language_models/provider/anthropic.rs:198-199` all consume the type *through* the `settings` re-export.
- **3c** — `client/src/test.rs:278` constructs a `cloud_llm_client::CurrentUsage` and assigns it into a `cloud_api_types` field.

**Where they went.** 3a → Phase 5 step 0. 3b and 3c → the head of the red period, taken in the derived topological order (`cloud_api_types` is leaf #1, `settings_content` leaf #2 — see `research/survivor-fix-order.txt`).

**The plan's value is unharmed.** Phase 3 was never about keeping the build green for its own sake — it was about making the keep-decision real so `editor` and `recent_projects` never need surgery. That still holds; the severances just land a few hours later in the sequence.

## Key Insights

- `edit_prediction_types` (380 LOC) touches the cloud graph through exactly **one import used at 3 sites**: `use client::EditPredictionUsage`. Deleting the crate instead would mean ~429 `edit_prediction` references in `editor.rs`, 33 in `element.rs`, 5 in `display_map.rs`.
- **⚠ 3a MOVED to Phase 5 step 0 — discovered during execution.** The plan assumed `edit_prediction_types` was the last consumer of `client::EditPredictionUsage`. It is not. `EditPredictionDelegate::usage()` has a **default impl** (`edit_prediction_types.rs:167`) that three delete-set crates override — `codestral:195`, `copilot:40`, `edit_prediction:48` (`zed_edit_prediction_delegate.rs:140`). Removing the trait method while those crates still exist is **E0407**, turning the build red *before* Phase 4.

  Correct window: **after** Phase 4 removes the implementors, **before** Phase 5 deletes the type. Verified safe on the far side — the only surviving implementors are `editor`'s two test fakes (`edit_prediction_tests.rs:1580`, `:1658`), which rely on the default and never override it; `editor` references neither `.usage()` nor `EditPredictionUsage`.
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

### 3a. `edit_prediction_types` — **MOVED to Phase 5 step 0**

See the Key Insights note above. Doing it here breaks the build; the procedure now lives in
`phase-05-gut-client-auth-core.md` step 0, which runs after Phase 4 has removed the three
overriding implementors.

### 3b. `settings_content` — **DEFERRED to the head of the red period**

Cannot run pre-cut (see the structural correction above). Runs as the **second** survivor fix after
Phase 4, per the derived topological order. Procedure unchanged:

5. Move `ReasoningEffort` (`language_model_core.rs:465-476`), `ModelMode` (`:453-459`), and `Speed` (`request.rs:340-353`) into `crates/settings_content/`. All three are small fieldless/2-variant enums with no dependencies.
6. Rewrite `language_model.rs:240` and `:482` from `pub use language_model_core::X` to local definitions; change `agent.rs:428` and `merge_from.rs:59` to `crate::Speed`.
7. Drop the dep; `cargo check -p settings_content`.
   > Note: `agent.rs` and `language_model.rs` are deleted wholesale in Phase 8, but this phase must keep the build green *now*, so patch them rather than pre-deleting.

### 3c. `cloud_api_types` — **DEFERRED to the head of the red period**

Cannot run pre-cut (see the structural correction above). Runs as the **first** survivor fix after
Phase 4 — it is leaf #1 in the topological order. Procedure unchanged:

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

17. `cargo check --workspace` green. (3a is no longer part of this phase.)
18. Re-run `research/final-delete-set.py`; confirm the five crates report `must-patch: (none)` and the survivor count is 15 minus the ones just fixed.
19. Commit as four separate commits, one per severance.

## Todo List

- [x] 3a **moved to Phase 5 step 0** — cannot run before Phase 4 (E0407 on three delete-set implementors)
- [x] 3b **deferred** to post-Phase-4 (type-identity conflict while `language_model_core` lives)
- [x] 3c **deferred** to post-Phase-4 (type-identity conflict while `cloud_llm_client` lives)
- [x] 3d `remote_connection` severed — all four call sites across both impls (commit `867dd5c`)
- [x] 3d Transport: release manifest + artifact fetch via `http_client` (`remote_server_release.rs`)
- [x] 3d **Integrity verification implemented** — streaming SHA-256 checked before rename; 5 unit tests prove malformed digests and tampering are both rejected
- [x] 3e `remote_server` severed — `crashes` dep dropped, both `crashes::init` sites and the `--crash-handler` entry point removed, orphaned bindings cleaned, warning-free (commit `47113fa`)
- [x] `cargo check --workspace` green
- [ ] `final-delete-set.py` re-run and confirms the keep-list
- [ ] Deferred severances (3a/3b/3c) recorded in the red-period sequence
- [ ] Five standalone commits

## Success Criteria

- `cargo check --workspace` green.
- `cargo tree -i auto_update -e normal` no longer lists `remote_connection`.
- ✅ `cargo tree -i crashes -e normal` no longer lists `remote_server` — **verified**.
- `editor` and `recent_projects` untouched — verified by `git diff --stat`. (`project` needs 2 small edits, handled in Phases 7 and 9 — see plan.md finding 5.)
- **The remote-server binary fetch verifies a signature or checksum.** Demonstrate it: point the fetch at a deliberately corrupted artifact and confirm it is rejected.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Remote-binary download replacement is under-designed | SSH remote dev silently broken until manual testing | Step 10 forces an explicit decision; Phase 11 includes a live SSH remote-dev smoke test |
| `edit_prediction_types` patch attempted in this phase | **Build goes red before Phase 4** (E0407) | Moved to Phase 5 step 0; this phase no longer touches it |
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
