---
phase: 5
title: "Gut client auth core"
status: pending
effort: "2d"
---

# Phase 5: Gut client auth core

## Context Links

- [Survivor patch detail](./reports/scout-survivor-patches.md) §4 — the full keep/remove line inventory
- [Consultation record](./brainstorm-report.md) — why `client` is gutted, not deleted

## Overview

**Priority:** P1 · **Build state:** 🔴 RED · **Depends on:** Phase 4 · **Run alone, single-threaded**

Remove authentication from `crates/client` while preserving the proto/rpc plumbing that 37 crates depend on. This is the riskiest survivor patch in the plan and the reason `client` was never a deletion candidate: `editor`, `project`, and `workspace` all import types from it that have nothing to do with auth.

`UserStore` is the hard part — it interleaves cloud plan/usage state with contacts, `ParticipantIndex`, `Collaborator`, and `User`. This is a line-by-line split of `user.rs`, not a file deletion.

## Key Insights

- `editor` imports exactly one line: `use client::{Collaborator, ParticipantIndex, parse_zed_link};` (`editor.rs:95`). All three must survive.
- `workspace` needs only `proto`, `init`, `Subscription`, `ConnectionIdentifier`, `CONNECTION_TIMEOUT`, `FakeHttpClient`.
- `project` uses `client::proto` throughout `lsp_command.rs`, `lsp_store.rs`, `search.rs` — **`proto` is the internal wire format for LSP command serialization**, entirely unrelated to auth. This is the single fact that makes deleting `client` impossible.
- Phase 3a already removed the last surviving consumer of `client::EditPredictionUsage`. If Phase 3a was skipped, **stop** — `edit_prediction_types` will break.
- `crates/client/src/telemetry.rs` has no cloud imports and is handled in Phase 6, not here.
- `Status::SignedOut` is already the default startup state (`client.rs:418`), and `main.rs:1311-1322` only auto-authenticates when credentials exist. The signed-out path is already well-trodden — gutting makes it the only path.

## Requirements

**Functional**
- No code path in `client` can reach the network for authentication.
- `proto`/`rpc` re-exports, `Collaborator`, `ParticipantIndex`, `User`, `parse_zed_link`, `ProtoClient` all preserved with unchanged signatures.
- `cargo check -p client` green by end of phase (dependents may still be red).

**Non-functional**
- Public API narrowed by deletion, not by stubbing. Per `CLAUDE.md`: "Write the real implementation — never fake it or stub it out as a stand-in." Fabricating a fake authenticated user would violate that; honestly removing the capability does not.

## Architecture

`crates/client` splits cleanly along a line that already exists in the source:

```
KEEP                                    REMOVE
────────────────────────────────────    ──────────────────────────────────────
pub use rpc::*            (:58)         cloud_api_client imports  (:17-19)
struct Client (minus 2 fields)          field cloud_client        (:199)
enum Status + impl     (:268-320)       field message_to_client_handlers (:204)
Credentials            (:328-337)       validate_credentials      (:923-939)
ClientCredentialsProvider (:339-411)    connect_to_cloud          (:941-970)
Subscription           (:424-454)       acquire/refresh/clear_llm_token (:1523-1581)
PendingEntitySubscription (:455-515)    add/handle_message_to_client (:1744-1759)
TelemetrySettings      (:516-528)       src/llm_token.rs          (whole file, 116 lines)
impl ProtoClient       (:1766-1808)
ZedLink, parse_zed_link (:1809+)        user.rs: plan/usage/organization state
user.rs: ChannelId, ProjectId (:37-53)    fields                  (:115-121)
user.rs: ParticipantIndex     (:55-56)    Event::PlanUpdated      (:147)
user.rs: User                 (:58-64)    EditPredictionUsage     (:167-173)
user.rs: Collaborator         (:66-74)    cloud maintenance loop  (:231-282)
user.rs: Contact              (:96-108)   plan()/trial accessors  (:743-807)
user.rs: UserStore contacts half          update_authenticated_user (:832-894)
                                          impl RequestUsage       (:1044-1090)
```

## Related Code Files

**To delete**
- `crates/client/src/llm_token.rs` (116 lines)

**To modify**
- `crates/client/src/client.rs` — ~190 lines removed
- `crates/client/src/user.rs` — ~330 lines removed
- `crates/client/src/test.rs` — `:5-8` imports + `GetAuthenticatedUserResponse`/`PlanInfo` fixture builders (~40 lines)
- `crates/client/Cargo.toml` — drop `cloud_api_client`, `cloud_llm_client`

**Must remain byte-compatible for dependents**
- `crates/client/src/telemetry.rs` (Phase 6 handles it)
- `crates/client/src/proxy*.rs`, `crates/client/src/zed_urls.rs` (Phase 12 handles URLs)

## Implementation Steps

1. **Precondition check** — Phase 3a must have landed:
   ```sh
   rg -n "EditPredictionUsage" crates/   # expect: only crates/client
   ```
   If `edit_prediction_types` still references it, go back and do Phase 3a first.
2. Delete `crates/client/src/llm_token.rs` and its `pub use llm_token::*;` at `client.rs:57`.
3. `client.rs` — remove imports `:17-19`, then the two struct fields (`:199` `cloud_client`, `:204` `message_to_client_handlers`) and their initializer at `:540`.
4. `client.rs` — remove the accessor `cloud_client()` `:579-581`, the credential push at `:907-910`, `validate_credentials` `:923-939`, `connect_to_cloud` `:941-970`.
5. `client.rs` — remove the three LLM-token fns `:1523-1581`, the `clear_credentials()` call in `sign_out` `:1586`, and `add_message_to_client_handler`/`handle_message_to_client` `:1744-1759`.
6. `client.rs` — delete `sign_in`, `authenticate`, `authenticate_with_browser`, `authenticate_as_admin`, `sign_in_with_optional_connect`, `connect`.
   > **`onboarding` calls `sign_in_with_optional_connect`** (`onboarding.rs:251`, `basics_page.rs:649`). Phase 8e handles it — but be aware this deletion is what breaks that crate.

   **Keep `Status`** — 14 bespoke signed-out branches match on it across the codebase; narrowing it is gratuitous churn.

   **`reconnect` / `disconnect`: verify, don't assume.** An earlier draft claimed these were "used by the RPC layer that `project` still consults" — the red team disproved it: `rg -n "reconnect|disconnect|Status::" crates/project/src/` returns **nothing**. The only surviving-crate caller is `crates/zed/src/main.rs:778`, inside the `server_url`-change handler — and Phase 8 step 21 deletes that block. So after Phase 8 they likely have **zero** callers.
   ```sh
   rg -n "\.reconnect\(|\.disconnect\(" crates/     # run this, then decide
   ```
   If the only hits are in `client` itself and `main.rs:773-782`, **delete them** along with the rest. Keeping them would leave dead code that Phase 11's `--deny warnings` will flag anyway.

6b. `user.rs` — **`plan_for_organization` and `organizations()`** also go (they sit inside the `:700-830` block below). `title_bar.rs:1145-1151` iterates `.organizations()` and calls `.plan_for_organization(&org.id)` — Phase 8b deletes that block. Record the decision here so the two phases agree.
7. `user.rs` — remove imports `:4-11`, the six cloud fields `:115-121` and their initializers `:197-200`.
8. `user.rs` — remove `Event::PlanUpdated` `:147`, `Event::OrganizationChanged` `:149`, `EditPredictionUsage`/`RequestUsage` `:167-173`.
9. `user.rs` — remove the cloud maintenance loop `:231-282` from `_maintain_current_user`. **Keep the `Status::SignedOut` / `ConnectionLost` arms** — they drive contact-list clearing that survives.
10. `user.rs` — remove the organization/plan accessor block `:700-830` and `update_authenticated_user` `:832-894`, `handle_message_to_client` `:896-916`, `impl RequestUsage`/`impl EditPredictionUsage` `:1044-1090`.
11. `test.rs` — remove `:5-8` and the authenticated-user fixture builders. Keep `FakeServer` and all proto plumbing.
12. `Cargo.toml` — drop `cloud_api_client`, `cloud_llm_client`.
13. Iterate to green **on this crate only**:
    ```sh
    cargo check -p client
    ```
14. Prove the critical exports survived:
    ```sh
    rg -n "pub use rpc" crates/client/src/client.rs
    rg -n "pub struct Collaborator|pub struct ParticipantIndex|pub fn parse_zed_link" crates/client/src/
    ```
15. Commit alone: `refactor(client): remove authentication and cloud API surface`.

## Todo List

- [ ] Precondition verified: `EditPredictionUsage` only in `client`
- [ ] `llm_token.rs` deleted
- [ ] `client.rs` cloud fields, accessors, cloud connect removed
- [ ] LLM-token fns removed
- [ ] Auth entry points (`sign_in`, `authenticate*`, `connect`) removed
- [ ] `reconnect`/`disconnect` decision made **from a fresh `rg`**, not from the old (disproved) rationale
- [ ] `plan_for_organization` / `organizations()` fate recorded; Phase 8b informed
- [ ] `ClientCredentialsProvider` deleted after confirming `client.rs:867` was its only caller
- [ ] `Status` enum left **intact** (all variants)
- [ ] `user.rs` cloud state, plan accessors, `update_authenticated_user` removed
- [ ] `user.rs` contacts/`Collaborator`/`ParticipantIndex`/`User` **preserved**
- [ ] `test.rs` fixtures pruned, `FakeServer` intact
- [ ] `Cargo.toml` deps dropped
- [ ] `cargo check -p client` green
- [ ] Critical exports verified present
- [ ] Standalone commit

## Success Criteria

- `cargo check -p client` green.
- `rg -n "cloud_api_client|cloud_llm_client" crates/client/` returns nothing.
- `Collaborator`, `ParticipantIndex`, `User`, `parse_zed_link`, `pub use rpc::*`, `impl ProtoClient` all still exported.
- `rg -n "authenticate_with_browser|IMPERSONATE_LOGIN|native_app_signin" crates/client/` returns nothing.
- No `TODO`/`unimplemented!()`/fabricated user left behind.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Removing a `UserStore` method that contacts/collab-free code still needs | Cascading breakage across `project`/`workspace` in Phase 8 | Work method-by-method; after each removal run `rg -n "<method_name>" crates/` before deleting |
| `reconnect`/`disconnect` removed but the RPC state machine needs them | `project` connection handling breaks subtly | Step 6 requires an explicit call-site check |
| Stubbing instead of deleting | Violates `CLAUDE.md`; leaves fake data paths | Requirements section makes delete-not-stub explicit |
| Phase 3a skipped | `edit_prediction_types` breaks and the keep-list rationale collapses | Step 1 is a hard precondition gate |
| `Status` enum narrowed | 14 bespoke signed-out branches across the codebase break | **Do not change `Status`.** Leave all variants; they are cheap and widely matched |

## Security Considerations

This is the phase that actually delivers "no account". Verify by inspection that after it:
- no code constructs an `Authorization` header from `Credentials`;
- `zed_urls.rs` still exists but nothing calls `account_url` (Phase 12 rewrites the URLs).

**`ClientCredentialsProvider` — delete it, do not keep it.** An earlier draft said to keep it "for the generic keychain path". The red team disproved that: every other credential consumer calls `zed_credentials_provider::global(cx)` **directly**, bypassing `client::ClientCredentialsProvider` entirely — see `language_model/src/api_key.rs:115`, `:119`, `:258`; `project/src/context_server_store.rs:1185`, `:1197`, `:1213`; `codestral/src/codestral.rs:51`. The **only** caller of `ClientCredentialsProvider::{read,write,delete}_credentials` is `client.rs:867`, inside the sign-in flow that step 6 deletes.

So after step 6 the field at `client.rs:201` (constructed at `:542`) has no live reader. Keeping it would be dead code — which Phase 8's own success criteria forbid and Phase 11's `--deny warnings` will flag.

```sh
rg -n "credentials_provider\." crates/client/src/client.rs   # run AFTER step 6
```
If `:867` was the only hit, delete `ClientCredentialsProvider`, the field, and `Credentials`.

## Next Steps

Phase 6 — gut telemetry, `notifications`, and `reliability.rs`. Independent of this phase's dependents, so it can proceed immediately.
