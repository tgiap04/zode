---
phase: 8
title: "Fix heavy survivors"
status: completed
effort: "8-10d"
---

# Phase 8: Fix heavy survivors

## Context Links

- [`crates/zed` wiring map](./reports/scout-zed-wiring.md) — the authoritative edit order for `crates/zed`
- [Survivor patch detail](./reports/scout-survivor-patches.md) §2 `title_bar`, §3 `git_ui`
- [Settings scout](./reports/scout-settings-migration.md) §3 — `settings_ui` page surgery

## Overview

**Priority:** P1 · **Build state:** 🔴 RED · **Depends on:** Phase 7 (`workspace` must be green)

The five crates carrying real weight: `zed` (23 dependency edges), `settings_ui` (7), `title_bar` (4), `git_ui` (3), and **`onboarding`** — added by the red team after it was found calling deleted auth APIs while invisible to the crate-level dependency graph. This is the longest phase in the plan and where most of the red period is spent.

> **Estimate raised 5-8d → 8-10d.** The red team measured the files: `git_panel.rs` is **8,142** lines (the plan had said 5,700 — 43% under) and `page_data.rs` is **9,501**. Phase 4's abort protocol sets a 12-working-day tripwire on this phase.

`crates/zed` alone is ~40 non-test call sites plus a **duplicate of the whole init sequence inside its test module** that must receive identical edits.

## ⚠ Cutting discipline (mandatory — see plan.md "Execution rule")

Phase 5 over-cut **three times**, each time destroying keepers that sat between two doomed functions.
This phase cuts the two largest files in the fork, so the rule is not optional here:

1. **Dump the function map first**, for every region you intend to touch:
   ```sh
   grep -n "^    pub fn \|^    pub async fn \|^    async fn \|^    fn \|^impl \|^pub struct \|^pub enum " <file>
   ```
   Classify **every** entry in the range as delete or keep before writing a single edit.
2. **One function per cut.** The end marker must be the next item you also intend to delete — never
   "the next thing that appears". If a keeper sits inside a proposed A→B range, the range is wrong.
3. **Assert the keep-list after every cut**, in the same script. Phase 5 used 23 assertions; that is
   the template, not the ceiling.
4. **A cut that compiles is not a cut that is correct.** Phase 5's second over-cut compiled the file
   it damaged — only a different crate revealed it. Check the *consumers*.
5. **Commit per crate.** This is the rollback unit. All three Phase 5 recoveries were cheap because
   of it.

**Where this bites hardest in this phase:**

| File | Lines | Why the rule matters |
|---|---|---|
| `settings_ui/src/page_data.rs` | **9,501** | Seven separate section deletions plus two hand-counted array arities. Prime over-cut territory. |
| `git_ui/src/git_panel.rs` | **8,142** | Three unrelated features to excise from one file; the original size estimate was 43% low, so the census in 12a is not optional. |
| `zed/src/main.rs` + `zed.rs` | 1,940 + 6,587 | `zed.rs` holds a duplicate of the whole init sequence inside its test module. |

## Key Insights

- `zed.rs:5371-5412` (`init_test_with_state`) is a **twin of the `main.rs` init block**: `channel::init`, `call::init`, `notifications::init`, `collab_ui::init`, `copilot_chat::init`, `language_model::init`, `RefreshLlmTokenListener::register`, `language_models::init`, `web_search*::init`, `PromptBuilder::load`, `agent_ui::init`. Every edit to `main.rs` must be mirrored here or the tests will not compile.
- `zed.rs:5110-5198` `expected_namespaces` in `test_action_namespaces` is a **hardcoded list** including `"agent"`, `"assistant"`, `"copilot"`, `"collab"`, `"zeta"`, `"channel_modal"`, `"context_server"`. It is the best canary in the codebase for a leftover registered action — treat a failure here as a real finding, not a test to silence.
- Edit `crates/zed/Cargo.toml` **before** the `.rs` files. That converts hundreds of *unresolved item* errors into a smaller set of *unresolved crate* errors — one clean error per real call site.
- Biggest single unblocker inside `main.rs`: the init block `:618-741`. Twenty-two of ~40 non-test call sites live there.
- `title_bar` is three separate deletions (collab/call/livekit, auto-update, plan chip) straddling one file — easy to leave dangling struct fields. It must come **after** Phase 5, because `title_bar.rs:1133`/`:1148` call `UserStore::plan()`.
- `git_ui` has three unrelated features to excise from an **8,142-line** `git_panel.rs` (measured, not estimated): LLM commit-message generation (~210 lines), collab co-authors (~90 lines), agent conflict resolution (~50 lines). Because the original size estimate was 43% low, **run a full census before trusting that three features is exhaustive** (step 12a).
- `settings_ui` needs three whole files deleted plus surgical `page_data.rs` work — including **two array-arity fixes** (`:2412` `[SettingsPageItem; 6]` → `; 5`, `:7695` `; 3` → `; 2`) that the compiler will flag but whose correct new value must be counted by hand.

## Requirements

**Functional**
- All six crates check clean; `cargo check --workspace` green by end of phase (that is the Phase 11 gate, but this phase should get there).
- No dangling struct fields, unused imports, or orphaned actions.
- Test modules updated in lockstep with production code.

**Non-functional**
- No `#[allow(dead_code)]` used to paper over an incomplete removal.

## Architecture

Order matters. `zed` last because every earlier fix shrinks its diff:

**Corrected against the derived topology** (`research/survivor-fix-order.txt`). `title_bar` depends on
`git_ui`, so the first draft's order (settings_ui → onboarding → title_bar → git_ui) would have made
`title_bar` show errors sourced from an unfixed `git_ui`.

```
1. settings_ui   ← 3 files deleted + page_data.rs (9,501 ln) surgery  ~2,900 lines
2. git_ui        ← 3 features excised from git_panel.rs (8,142 ln)     ~400 lines
3. onboarding    ← sign-in + plan UI removed                           ~200 lines   (needs Phase 5)
4. title_bar     ← 3 files deleted + title_bar.rs edits              ~1,030 lines   (needs Phase 5 + git_ui)
5. zed           ← Cargo.toml → main.rs → reliability → menus
                   → quick_action_bar → open_listener → telemetry_log
                   → zed.rs → zed.rs tests                          ~40 call sites
```

`onboarding` and `title_bar` both strip sign-in/plan UI and both depend on Phase 5 — adjacent on
purpose, so that context is loaded once.

## Related Code Files

**To delete**
- `crates/settings_ui/src/pages/edit_prediction_provider_setup.rs` (748)
- `crates/settings_ui/src/pages/tool_permissions_setup.rs` (1,459)
- `crates/settings_ui/src/components/ollama_model_picker.rs` (213)
- `crates/settings_ui/src/pages/audio_test_window.rs` (304) + `audio_input_output_setup.rs` (153) — with the Collaboration page
- `crates/title_bar/src/collab.rs` (722), `src/plan_chip.rs` (44), `src/update_version.rs` (145)
- `crates/zed/src/zed/edit_prediction_registry.rs` (416)
- `crates/zed/src/zed/telemetry_log.rs` (621)

**To modify**
- `crates/settings_ui/src/page_data.rs`, `src/pages.rs`, `src/components.rs`, `src/settings_ui.rs`, `Cargo.toml`
- `crates/title_bar/src/title_bar.rs`, `Cargo.toml`
- `crates/git_ui/src/git_panel.rs`, `src/conflict_view.rs`, `src/project_diff.rs`, `Cargo.toml`
- `crates/zed/Cargo.toml`, `src/main.rs`, `src/zed.rs`, `src/zed/app_menus.rs`, `src/zed/quick_action_bar.rs`, `src/zed/open_listener.rs`
- `crates/zed_actions/src/lib.rs` — prune `OpenAccountSettings` `:58-59` and orphaned `agent`/`assistant`/`agents_sidebar` modules

## Implementation Steps

### 8a. `settings_ui`

1. Delete the three (or five, with audio) whole files listed above.
2. `page_data.rs` — delete `collaboration_page()` `:7211-7310` and `ai_page(cx)` `:7312-7692`; remove `:76`/`:77` from the page vec.
3. `page_data.rs` — delete sections: `auto_update_section()` `:372-389` (+ call site `:400`), `agent_panel_font_section()` `:998-1040` (+ `:1256`), `Agent Review` item `:2487-2510`, `collaboration_panel_section()` `:5688-5754` (+ `:5895`), `agent_panel_section()` `:5756-5885` (+ `:5896`), `edit_prediction_language_settings_section()` `:9364-9417` (+ `:3087`, `:7688`), `Server URL` item `:7715`.
4. **Fix the two array arities by counting the remaining items**: `:2412` and `:7695`.
5. Prune imports `:14-17`, `:25-28`, `:3-6`; module wiring in `pages.rs`, `components.rs`, `settings_ui.rs` (`:51`, `:53`, `:505`, `:528`, `:529`, `:550`, `:555`).
6. `cargo check -p settings_ui`.

### 8b. `title_bar` (after Phase 5)

7. Delete `collab.rs`, `plan_chip.rs`, `update_version.rs`; remove `pub mod collab;` `:2`.
8. `title_bar.rs` — remove imports `:23`, `:24`; the `ActiveCall` observer `:365`/`:388`; `window_activation_changed` call branches `:1002-1017`; `active_call_changed` `:1019-1022`; `observe_diagnostics` `:1024-1034` + field `:158`; `share_project` `:1036-1043`; `unshare_project` `:1045-1051`.
9. Remove render sites `:249`, `:285`, `:288`; the `update_version` field/ctor/accessor `:156`, `:417`, `:436`, `:451`; the auto-update menu block `:1071-1093`; `:1121`.
9b. **Plan reads — two distinct sites, not one** (red team finding 14): `:1133` is `user_store_read.plan()`; `:1145-1151` is a whole `.organizations().iter().map(|org| … plan_for_organization(&org.id) …)` block. Phase 5 step 6b deletes both `organizations()` and `plan_for_organization`, so **delete the entire `:1145-1151` block**, not just a line.
10. Keep `onboarding_banner.rs` (clean) and `project.remote_connection_options/state` reads at `:500`, `:519`, `:535` — those are the `project` API.
11. Drop `auto_update`, `call`, `channel`, `livekit_client`, `cloud_api_types` and the dev-deps. `cargo check -p title_bar`.

### 8c. `git_ui`

12a. **Census first.** The original 5,700-line estimate for `git_panel.rs` was 43% low (it is 8,142), which is itself evidence the file was not read end to end. Before trusting that three features is the whole story:
```sh
rg -n "language_model|call::|channel::|agent_settings|AgentSettings|ActiveCall|telemetry::" crates/git_ui/src/
```
Anything outside the three known features is a new finding — scope it before starting.

12. Delete the LLM commit-message feature: `generate_commit_message` `:2685-2832`, `load_commit_message_prompt` `:2670-2682`, `render_generate_commit_message_button` `:4030-4094`.
13. Delete the collab co-author path: `potential_co_authors` `:3308-3346`, `local_committer` `:3349-3358`, `render_co_authors` `:4095-4137`; in `Render` `:5735-5745` set `has_co_authors = false` and drop the room lookup.
14. Delete the AgentSettings observer `:770-775` and the `is_ai_enabled` guards `:2686`, `:4034`.
15. `conflict_view.rs` — remove `:297`, `:548-551`, `resolve_with_agent` `:576-586`, the `MergeConflictIndicator` guard `:594-600`.
16. `project_diff.rs` — remove `:1654`, `:1656` and the review-button branch `:1671-1706`; drop `use zed_actions::agent::ReviewBranchDiff` `:49`.
17. Leave `worktree_service.rs` and `worktree_picker.rs` **unchanged** — `remote_connection` survives.
18. Drop `agent_settings`, `language_model`, `call`. `cargo check -p git_ui`.

### 8d. `onboarding` — the 16th survivor (red team finding 1)

**Why the graph missed it.** `crates/onboarding` depends on `client` and `cloud_api_types` — both kept — so `cargo metadata` showed no edge into the delete set. But it consumes exactly the symbols Phase 5 removes. Note it is a *different crate* from `ai_onboarding` (`Cargo.toml:12`), which **is** deleted; `onboarding` is at `Cargo.toml:142`.

| File:line | What |
|---|---|
| `onboarding.rs:2` | `use client::{Client, UserStore, zed_urls};` |
| `onboarding.rs:251` | `.sign_in_with_optional_connect(true, &cx)` |
| `onboarding.rs:244`, `:281` | account URL / sign-in wiring |
| `basics_page.rs:4` | `use client::{Client, TelemetrySettings, UserStore, zed_urls};` |
| `basics_page.rs:585-654` | `render_zed_agent_button` — the whole Sign In / Start Free Trial UI |
| `basics_page.rs:590-592` | `Plan::ZedFree` / `ZedPro` / `ZedProTrial` matches |
| `basics_page.rs:597` | `client::Status::AuthenticationError \| ConnectionError` |
| `basics_page.rs:649` | `client.sign_in_with_optional_connect(true, cx)` |
| `basics_page.rs:656` | `render_ai_section` |

18a. Delete `render_zed_agent_button` (`basics_page.rs:585-654`) and `render_ai_section` (`:656`), plus their call sites in `render_basics_page` (`:698`).
18b. Delete the sign-in path in `onboarding.rs` (`:244`, `:251`, `:281`) and the `zed_urls` import at `:2`.
18c. Keep `TelemetrySettings` usage — the telemetry **setting** survives (Phase 6b) even though events are no-ops. The onboarding telemetry opt-in checkbox is now cosmetic; decide whether to keep it as an honest "we collect nothing" statement or remove it. **Recommend remove** — a toggle that controls nothing is worse than no toggle.
18d. Drop `cloud_api_types` from `Cargo.toml` if `Plan` was its only use.
18e. `cargo check -p onboarding`.

### 8e. `zed` — follow the scout's edit order exactly

19. Delete `src/zed/edit_prediction_registry.rs` + refs (`zed.rs:2`, `main.rs:66`).
20. **`Cargo.toml` first**: remove the 22 delete-set dep lines, `agent-client-protocol`, the two dev-deps (`:256` `call`, `:269` `agent_ui`). Then verify-and-remove the collateral: `semver`, `tempfile`, `clock`, `reqwest`, `prompt_store`, `telemetry_events`, `notifications`, optional `image`/`action_log`/`agent_servers`. **Keep `telemetry`.**
21. `main.rs` top-down: imports `:7-16`, `:41`, `:64-68` → `crashes` `:196-200`, `:341-361` → init block `:618-741` → settings closure `:749-783` (keep the theme half `:753-772`) → telemetry events `:792-802` → authenticate spawn `:816-820` → `handle_open_request` `:937-1040`, `:1250-1300` → `fn authenticate` `:1311-1323` → `Args` `:1650-1652`, `:1667-1670` → `parse_url_arg` `:1741`.
22. `reliability.rs` caller — already handled in Phase 6d; verify `main.rs:621` matches the new signature.
23. `app_menus.rs` — `:1`, `:45`, `:67`, `:299-302`, and `:303` with the telemetry_log decision.
24. `quick_action_bar.rs` — `:4`, `:30`, `:51-62`, `:159-166`, `:290`.
25. `open_listener.rs` — `:3`, `:7`, `:42-43`, `:57-62`, `:89-97`, `:160-170`, `:186-197`, `:215-222`, tests `:1000-1130`. **Keep** `:44` (`remote_connection` field — type is `remote::RemoteConnectionOptions`) and all SSH parsing.
26. Delete `src/zed/telemetry_log.rs` + `main.rs:661`, `zed.rs:10`, `zed.rs:1301-1303`, the `OpenTelemetryLog` action.
27. `zed.rs` non-test half: imports `:2`, `:11-12`, `:16`, `:22`, `:99` → `OpenAccountSettings` handler `:219-223` → workspace observer `:438-453` → `crashes::set_gpu_info` `:499-503` (keep the log) → edit-prediction status bar `:505-520`, `:562` → panel loading `:701-702`, `:724-727` → `setup_or_teardown_ai_panel` `:733-766`, `ensure_agent_panel_for_workspace` `:768-791`, `initialize_agent_panel` `:793-824` → collab toggle `:1112-1118` → toolbars `:1299-1303`, `:1318-1319` → telemetry events `:891`, `:931`.
28. `zed.rs` test half: `init_test_with_state` `:5371-5412` (mirror of step 21), `init_keymap_test` `:4842`. **Leave `expected_namespaces` `:5110-5198` for Phase 10** — it is the canary and should be updated only after the action list is regenerated.
29. `zed_actions/src/lib.rs` — prune `OpenAccountSettings` `:58-59` and orphaned modules.
30. `cargo check -p zed`.

### 8f. `project` — remove the orphaned `context_server_store` (red team finding 10)

The plan calls `context_server` "inert, clean up later". The red team showed that is understated: `ContextServerStore` is a **live `Entity<>` field on `Project`**, initialized at three constructor sites, with `context_server_store::init(cx)` running at startup — and after Phase 4 every external consumer (`agent`, `agent_ui`, `agent_servers`, `remote_server/headless_project.rs`) is gone.

Left alone it ships as a working-but-orphaned MCP subsystem that **can still spawn external processes** from config — a maintenance and security surface nobody asked to keep, in a fork whose selling point is minimalism.

29a. Remove the field (`project.rs:235`), the `init` call (`:1151`), the three constructor sites (`:1184-1192`, `:1434`, `:1806-1808`), the three struct-literal fields (`:1317`, `:1543`, `:1844`), and the accessor (`:2181-2183`).
29b. Delete `crates/project/src/context_server_store.rs` and `context_server_store/extension.rs`.
29c. Keep the `context_server` **crate** if MCP-via-extensions has independent value; it simply becomes unreferenced. Otherwise add it to a follow-up deletion.
29d. `cargo check -p project`.

### 8g. Gate

31. `cargo check --workspace` — this should now be green or very close. Remaining errors belong to Phase 9/10 territory (test fixtures, data files).
32. `rg -n "TODO|unimplemented!|allow\(dead_code\)"` on the diff — nothing new introduced.
33. Commit per crate; `zed` may warrant several sub-commits following the step order.

## Todo List

- [x] 8a `settings_ui` — 3-5 files deleted, `page_data.rs` sections removed, **both array arities recounted**
- [x] 8b `title_bar` — 3 files deleted, all struct fields cleaned, `remote_connection` reads preserved
- [x] 8b `:1145-1151` `organizations()`/`plan_for_organization` block deleted whole
- [x] 8c `git_ui` — LLM commit message, co-authors, agent conflict resolution all excised
- [x] 8c `worktree_service.rs`/`worktree_picker.rs` left untouched
- [x] 8c **`git_ui` census run** before assuming three features is the whole story
- [x] 8d `onboarding` — sign-in button, plan UI, AI section removed
- [x] 8d `onboarding` telemetry-toggle decision recorded (deferred to Phase 9 with reasons)
- [x] 8e `edit_prediction_registry.rs` deleted
- [x] 8e `crates/zed/Cargo.toml` — already clean from Phase 4; nothing to edit **before** the `.rs` files
- [x] 8e `main.rs` fully swept
- [x] 8e `telemetry_log.rs` deleted (in Phase 6)
- [x] 8e `zed.rs` non-test half swept
- [x] 8e **`init_test_with_state` mirrored** — same edits as `main.rs`
- [x] 8e `expected_namespaces` deliberately deferred to Phase 10
- [x] 8e `zed_actions` pruned (only `OpenAccountSettings` was actually orphaned)
- [x] 8f **NOT removed — commissioner kept MCP.** Finding 10's "zero consumers" premise was false: `remote_server` is a live consumer. Closed as a deliberate no-op
- [x] `cargo check --workspace` green — 0 errors, 0 warnings in `crates/zed` (or only Phase 9/10 errors remain)
- [x] No new `TODO`/`unimplemented!`/`allow(dead_code)`

## Success Criteria

- `cargo check -p settings_ui -p onboarding -p title_bar -p git_ui -p project -p zed` all green.
- `cargo check --workspace` green.
- `rg -n "agent_ui::|collab_ui::|copilot::|auto_update::|language_models::" crates/` returns nothing outside deleted directories.
- `rg -n "sign_in_with_optional_connect|Plan::Zed" crates/` returns nothing.
- `ContextServerStore` no longer appears in `crates/project/src/project.rs`.
- No struct field left unread, no import left unused (clippy in Phase 11 will confirm).

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| `init_test_with_state` not mirrored | Tests fail to compile; discovered late and blamed on the wrong change | Step 28 is explicit; treat it as part of the `main.rs` edit, not a separate task |
| Array arity guessed instead of counted | Compile error, or worse a silently wrong page list | Step 4 says count the remaining items by hand |
| Editing `.rs` before `Cargo.toml` in `crates/zed` | Hundreds of unresolved-item errors instead of a handful of unresolved-crate errors | Step 20 ordered first, called out explicitly |
| Dangling struct fields in `title_bar` | Compiles, but dead state and confusing code | Clippy `--deny warnings` in Phase 11 catches unread fields |
| `expected_namespaces` edited now to "make it pass" | Loses the one canary for leftover registered actions | Step 28 defers it to Phase 10 by design |
| Phase spans days; context lost between sessions | Inconsistent edits | The scout report is the authoritative checklist; work through it linearly |

## Security Considerations

`main.rs:816-820` (the `authenticate` spawn), `main.rs:1311-1323` (`fn authenticate`), and `onboarding`'s two sign-in call sites are the last live callers of the auth machinery removed in Phase 5. Once 8d and 8e land, no code path can initiate a Zed-account sign-in. Verify workspace-wide, not just in `crates/zed`:
```sh
rg -n "sign_in|authenticate|has_credentials" crates/
```

**Crash signal handler — an accepted loss worth stating plainly (red team finding 13).** Step 21 deletes `crashes::init` from `main.rs:196-200`, `:341-361`. `crates/crashes/src/crashes.rs:60-101` installs a native signal handler (SIGSEGV/SIGABRT class) plus a panic hook that strips user strings before recording context (`:356`). Phases 3e and 6 framed crash removal as "no more minidump upload" — but this also removes **local** structured crash capture from the main editor process. A memory-safety fault in the extension host or an FFI boundary will now hit the OS default handler with no sanitized diagnostics.

This follows from the commissioner's "bỏ crash hoàn toàn" decision and is accepted — but it is a diagnostics regression that no gate in Phase 11 can detect, so it is recorded here rather than discovered later.

**`context_server_store` removal (8f) is partly a security action** — it eliminates a subsystem that can spawn external processes from user config while having zero remaining consumers.

## Next Steps

Phase 9 — data files. **The panic-critical phase**: 403 keymap action strings and 21 settings keys that the compiler cannot see.
