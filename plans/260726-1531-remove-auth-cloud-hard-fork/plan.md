---
title: 'Hard fork Zed: remove auth, cloud and AI subsystems'
description: >-
  Delete 53 crates (auth, collab, AI, edit-prediction, auto-update, crash
  reporting) from the Zed workspace, gut client/telemetry, and rebrand as an
  independent privacy-first IDE
status: pending
priority: P1
effort: 4-6w
tags:
  - refactor
  - deletion
  - privacy
  - fork
  - rust
  - workspace
blockedBy: []
blocks: []
work_type: deliverable
spec_waived: >-
  One-off subtractive refactor of an existing codebase — no new user-facing
  feature to specify. Requirements are fully captured by the delete set and the
  verified dependency graph.
created: 2026-07-26T00:00:00.000Z
---

# Hard fork Zed: remove auth, cloud and AI subsystems

## Overview

Turn this Zed fork into an independent, privacy-first IDE: **no Zed account, no telemetry egress, no cloud AI, no collaboration**. Editor + LSP + git + terminal + extensions + SSH remote dev survive.

**Method: big-bang (Hướng B)** — chosen by the commissioner after being shown that the incremental path reaches the same endpoint with checkpoints. The build stays red from Phase 4 until Phase 11. Every design decision below is aimed at making that red period short and navigable.

| | |
|---|---|
| Delete | **53 crates** (46 named + 7 auto-orphaned) — workspace goes 240 → 187 packages, −22% |
| Gut, don't delete | `client` (keep proto/rpc), `telemetry` (no-op), `notifications` (keep `status_toast`), `cloud_api_types` (keep extension DTOs) |
| Survivors needing code changes | **17 crates** — 15 from the dependency graph, plus `onboarding` and `project` found by red team |
| Near-zero-change survivors | `editor`, `recent_projects` — preserved by keeping 3 mis-classified crates. **`project` needs 2 small edits** (see below) |
| Distribution | Public, GPL-3.0, package-manager delivery (no `auto_update`) |

> **Package count, precisely:** `cargo metadata` reports 240 packages — 232 directories under `crates/`, plus 4 under `extensions/` and 3 under `tooling/`. The 53 deletions are all under `crates/`, so the tree goes 232 → 179 crate directories and 240 → 187 packages. Phase 1 records the exact numbers as the baseline.

## Cross-Plan Dependencies

| Relationship | Plan | Status |
|-------------|------|--------|
| Invalidates output of | [rebuild-spec docs](../260726-1400-rebuild-spec/) | artifacts-only, no plan.md |

`docs/system/*` and `docs/generated/*` describe a 240-crate workspace including all deleted subsystems. They become factually wrong at Phase 4 and must be regenerated (`/tkm:rebuild-spec`) after Phase 11. Tracked in Phase 10.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Preparation and baseline](./phase-01-preparation-and-baseline.md) | Completed |
| 2 | [Free wins - visual test harness](./phase-02-free-wins-visual-test-harness.md) | Completed |
| 3 | [Sever thin ties](./phase-03-sever-thin-ties.md) | Completed |
| 4 | [Atomic structural cut](./phase-04-atomic-structural-cut.md) | Completed |
| 5 | [Gut client auth core](./phase-05-gut-client-auth-core.md) | Completed |
| 6 | [Gut telemetry notifications reliability](./phase-06-gut-telemetry-notifications-reliability.md) | Completed |
| 7 | [Fix light survivors](./phase-07-fix-light-survivors.md) | Completed (9/9 — `file_finder` landed in Phase 8) |
| 8 | [Fix heavy survivors](./phase-08-fix-heavy-survivors.md) | Completed (8f closed as a no-op — MCP kept) |
| 9 | [Data files keymaps and settings](./phase-09-data-files-keymaps-and-settings.md) | Completed — **the app launches** |
| 10 | [Tests and docs](./phase-10-tests-and-docs.md) | Completed — **1 open regression handed to Phase 11** |
| 11 | [Green gates and privacy verification](./phase-11-green-gates-and-privacy-verification.md) | Gate A (mechanical) complete — Gate B/C (runtime, GUI) need the user |
| 12 | [Rebrand and packaging](./phase-12-rebrand-and-packaging.md) | Legal/naming complete — packaging + icon/desktop-file rebrand need the user |

**Build-state map:** Phases 1–3 green · **Phase 4 goes RED** · Phases 5–7 red · **Phase 8 returns green
(earlier than planned — `cargo check --workspace` is green at the end of 8e, not Phase 11)** · Phase 12 green.

## The four findings that shaped this plan

1. **Bundled keymaps panic the app.** `keymap_file.rs:196` / `:199` `bail!` → `zed.rs:2066` `.unwrap()`. **403 action strings** across 8 keymap files must be purged or Zed will not boot. Not cleanup — a hard requirement of the cut (Phase 9).
2. **Settings are permissive.** No `deny_unknown_fields` anywhere (`fallible_options.rs:11-44`, plus regression test `test_edits_for_update_preserves_unknown_keys`). Users' existing `settings.json` still loads ⇒ migration is optional polish, not a blocker. **But** `default.json` keys and their `.unwrap()`ing consumers must be removed in the same commit or startup panics.
3. **Three crates were mis-classified.** Keeping `edit_prediction_types`, `remote_connection`, `context_server` costs ~40 lines of patch; deleting them costs ~8,800 lines of surgery in `editor.rs`, `project.rs`, and `recent_projects`. Verified via `cargo metadata` reverse-dependency analysis, not assumption.
4. **Crate-level dependency analysis has a blind spot, and it cost us a crate.** `onboarding` depends on `client` and `cloud_api_types` — **both kept** — so the graph showed no edge into the delete set. But it calls `sign_in_with_optional_connect` (`onboarding.rs:251`, `basics_page.rs:649`) and matches on `Plan::ZedPro` / `Status::AuthenticationError`: a **symbol-level** dependency on exactly the parts of `client` that Phase 5 removes. Found by red team, not by the graph.

   The class is now fully enumerated. Every survivor touching a removed auth symbol:
   ```
   rg -l "sign_in_with_optional_connect|has_credentials|RefreshLlmTokenListener|
          EditPredictionUsage|\.plan\(\)|plan_for_organization|Plan::Zed" crates/*/src
   → client · cloud_api_types · edit_prediction_types · onboarding · title_bar · zed
   ```
   All six are now scoped: Phase 5, 3c, 3a, **8e (new)**, 8b, 8d.

## Known contradiction to resolve before release

**The "zero outbound to `*.zed.dev`" goal and a working extension marketplace are currently incompatible.** Phase 9 keeps `server_url` (`assets/settings/default.json:2472`); `HttpClientWithUrl::build_zed_api_url` (`http_client.rs:214-224`) maps it to `api.zed.dev`; `ExtensionStore::install_extension` (`extension_host.rs:693`, `:792`, `:837`) calls it. So a rebranded fork would silently keep hitting Zed Industries' production API.

The commissioner allowed downloads ("vẫn cho phép tải về"), so this is permitted egress — but it must be a **disclosed, deliberate** dependency, not an unnoticed one. Phase 11 carves it out of the blackhole test; **Phase 12 must decide**: stand up an independent registry, or disclose the dependency in `legal/third-party-terms.md` and check Zed's API terms.

## Dependencies

- `cargo metadata`, `cargo machete` (already a repo convention — `script/clippy:14-16`), `cargo tree -i`
- `script/crate-dep-graph` (existing, wraps `cargo depgraph`)
- `script/clippy` — final lint gate, not the iteration loop
- Little Snitch or LuLu — Phase 11 privacy verification
- Homebrew cask tooling — Phase 12

## Open questions (do not block Phases 1–11)

1. New brand name — blocks Phase 12 only.
2. Which package manager ships first (Homebrew cask recommended for macOS).

## Red Team Review

### Session — 2026-07-26
**Findings:** 22 (18 accepted, 4 rejected) · **Severity:** 4 Critical, 6 High, 8 Medium
**Reviewers:** Security Adversary · Failure Mode Analyst · Assumption Destroyer · Scope & Complexity Critic

| # | Finding | Sev | Disposition | Applied To |
|---|---------|-----|-------------|------------|
| 1 | `onboarding` is an unscoped 16th survivor calling deleted auth APIs | Critical | Accept | Completed |
| 2 | Extension registry still hits `api.zed.dev`; contradicts "zero outbound" | Critical | Accept | Completed |
| 3 | Phase 3d binary-download integrity is prose only, not a gate | Critical | Accept | Completed |
| 4 | No abort/rollback protocol for the 6-phase red period | Critical | Accept | Completed |
| 5 | `project` is not zero-change — `agent_servers` unwrap at `agent_server_store.rs:1896` | High | Accept | Completed |
| 6 | `git_panel.rs` is 8,142 lines, not 5,700 — Phase 8 under-estimated | High | Accept | Phase 8 (est. 8-10d) |
| 7 | `tail -40` test baseline cannot support "every delta explained" | High | Accept | Phase 1, Phase 10 |
| 8 | Phase 5 `reconnect`/`disconnect` rationale factually wrong | High | Accept | Phase 5 |
| 9 | Phase 9 (P0) 2d estimate has no margin | High | Accept | Phase 9 (est. 3-4d) |
| 10 | `context_server_store` is a live wired field, not inert UI | High | Accept | Phase 8 |
| 11 | 4 `AutoUpdater` call sites across 2 impls, not 2 | Medium | Accept | Phase 3 |
| 12 | `ClientCredentialsProvider` kept on a false pretext | Medium | Accept | Phase 5 |
| 13 | Deleting `crashes` drops the signal handler from the main app too | Medium | Accept | Phase 8 |
| 14 | `title_bar:1148` is `plan_for_organization`; `organizations()` unscoped | Medium | Accept | Phase 5, Phase 8 |
| 15 | Phase 3c extension-DTO list omits `ExtensionApiManifest`, `GetExtensionsResponse` | Medium | Accept | Phase 3 |
| 16 | Phase 9 step 9 says "recommend" inside a phase whose rule is "must" | Medium | Accept | Phase 9 |
| 17 | Crate count 240→187 conflates packages with `crates/` directories | Medium | Accept | plan.md, Phase 1 |
| 18 | `keymap_file.rs` `bail!` is at `:196`/`:199`, not `:195` | Medium | Accept | plan.md |
| 19 | 12-phase split is bureaucracy; merge 5-7 | Medium | **Reject** | — |
| 20 | Phase 11 five-layer network verification is gold-plated | Medium | **Reject** | — |
| 21 | `remote_connection` kept on cost, not value | High | **Reject** | — |
| 22 | `notifications` missing `component` dep | Medium | **Reject** | — |

**Rejection rationale**
- **19** — phases here are documentation structure for a multi-week effort, not build gates. The commissioner rejected incremental *builds*, not incremental *documentation*. Merging 5-7 would lengthen the already-longest phase.
- **20** — privacy is the stated product goal and this ships publicly. Proving it is a core requirement, not polish. (Finding 2's blackhole/extension conflict *was* accepted.)
- **21** — the commissioner was asked directly and confirmed keeping SSH remote dev after seeing the cost comparison. The reviewer lacked that context.
- **22** — the reviewer self-retracted mid-finding.

### Cross-phase consistency sweep

Re-read `plan.md` + all 12 phase files after applying. Reconciled:
- Survivor count 15 → **17** everywhere (`onboarding`, `project` added).
- "zero-change survivors" narrowed to `editor`, `recent_projects`; `project` reclassified.
- Phase 8 estimate 5-8d → **8-10d**; Phase 9 2d → **3-4d**; plan total 3-5w → **4-6w**.
- `keymap_file.rs:195` → `:196`/`:199` in plan.md and Phase 9.
- Extension-registry contradiction now stated in plan.md, carved out in Phase 11, and assigned a decision in Phase 12.

**Unresolved:** none. The extension-registry question is open but explicitly *assigned* to Phase 12 rather than left implicit.

## Execution log

### Phase 1 — 2026-07-26

Branch `fork/remove-auth-and-cloud` off `7ac6829`.

**Baseline is GREEN** — `cargo check --workspace` exit 0 in **1m23s**. That number matters: the red-period
fix loop is far more tolerable than the plan assumed when it recommended `-p` over `--workspace`. Both
remain worth using, but the cost of a full check is ~90s, not the many minutes feared.

**Three corrections the baseline capture forced:**

1. **Phase 7 was ordered wrong.** The first draft fixed `activity_indicator`/`diagnostics`/`language_tools`
   before `project` and `workspace` — but all four of the former depend on the latter two, so their errors
   would have been masked by unfixed upstream crates. Corrected order: `project` → `workspace` →
   `activity_indicator` → `diagnostics` → `file_finder` → `language_tools` → `notifications`.
2. **Phase 8 was ordered wrong.** `title_bar` depends on `git_ui`; the draft put `git_ui` last. Corrected:
   `settings_ui` → `git_ui` → `onboarding` → `title_bar` → `zed`.
3. **The action dump found two namespaces the plan never listed** and two partial removals a
   namespace-prefix sweep would miss entirely:
   - `bedrock` (2 actions) and `zed_predict_onboarding` (1) — crates in the delete set, absent from the plan's list.
   - `client::{SignIn,SignOut,Reconnect}` — the **crate survives** but these 3 actions are declared at
     `client.rs:90-98` and must die in Phase 5. Added as step 1b.
   - `onboarding::{SignIn,OpenAccount}` go while `Finish`/`ResetHints` stay.

   Baseline: **1,292 actions / 88 namespaces**; **142** in namespaces that vanish, plus the partials.

**Resolved the scope critic's open question.** "403 keymap action strings" and "142 doomed actions" measure
different things and are both right: 403 counts *bindings* across 8 keymap files (one action is bound many
times, per platform); 142 counts *distinct actions* that cease to exist.

Artifacts: `research/baseline-metrics.md` · `baseline-actions.txt` (JSON) · `baseline-action-names.txt` ·
`baseline-action-inventory.md` · `survivor-fix-order.py` + `.txt`

### Phase 2 — 2026-07-26 · commit `3a78fc9`

Visual-test harness removed: **4,274 deletions**, build green, five dependency edges dropped from
`crates/zed` (`acp_thread`, `action_log`, `agent_servers`, `clock`, `tempfile`).

**Correction the plan missed:** `.github/workflows/*.yml` are **generated** from `tooling/xtask`, and
`run_tests.yml:730-733` fails CI if `.github` has uncommitted changes after `cargo xtask workflows`.
Hand-editing the YAML would have been a guaranteed CI failure. Editing the generator removed the job
plus its four gate-wiring points in one change. Also dropped four optional deps the `visual-tests`
feature was the sole enabler of.

### Phase 3 — 2026-07-26 · commit `47113fa` (partial)

**Structural finding: only 2 of 5 severances can run while green.** A severance that replaces a
*shared type* with a local copy breaks every cross-crate consumer while the original crate still
exists — and those consumers only vanish at Phase 4.

- ✅ **3e** `remote_server` ← `crashes` — done, warning-free.
- ⏸ **3d** `remote_connection` ← `auto_update` — safe to run, **blocked on a security decision**
  (integrity mechanism for the remote-server binary fetch).
- ⤴ **3a** → Phase 5 step 0 · **3b**, **3c** → head of the red period, in topological order.

The plan's value is intact: Phase 3 exists to make the keep-decision real so `editor` and
`recent_projects` never need surgery. That still holds — the severances just land later in the
sequence than drafted.

### Phase 4 — 2026-07-26 · commit `c3e2ac3`

**54 crates** removed (not 53 — `action_log` became an orphan once Phase 2 took the visual-test
harness, its last non-delete-set consumer). `crates/` 232 → 178, packages 240 → 186, **372,180 lines**.
Tags `pre-structural-cut` / `post-structural-cut` bracket the cut.

**The atomic cut is FOUR edits, not three.** The plan listed directories + `[workspace] members` +
`[workspace.dependencies]`. It missed the fourth: every surviving crate's own `[dependencies]`.
Inherited `name.workspace = true` fails at **manifest-parse time**, so `cargo metadata` will not even
load the workspace until all 14 survivor manifests are stripped — in the same commit.

**The `Cargo.lock` gate was stated wrong.** The plan said "zero version bumps". Reality: zero packages
added, 168 removed, and **five version _downgrades_** (`bindgen`, `hashlink`, `nom`,
`tokio-tungstenite`, `tungstenite`) — the crates that forced unification to the higher versions are
gone, so resolution correctly falls back. The gate should read *"zero version increases; downgrades
are expected and must be enumerated."*

Post-cut census: **1 error, in `cloud_api_types`** — leaf #1 of the derived order, with everything
downstream blocked behind it. The census is a map, exactly as designed.

### Phase 7 (partial) — 2026-07-26 · commit `f753b91`

The two topological leaves, which are the deferred 3c and 3b.

- `cloud_api_types`: `PlanInfo` loses its `usage` field.
- `settings_content`: `ReasoningEffort`, `ModelMode`, `Speed` come home from the deleted `language_model_core`.

**Caught a silent data-corruption bug.** `Speed` was nearly relocated with `rename_all = "lowercase"`
instead of the original `snake_case`. For `Standard`/`Fast` the two are **identical**, so it would
have compiled, passed every test, and only diverged the first time a multi-word variant appeared —
by which point users' `settings.json` would silently stop parsing. Found by diffing against
`git show pre-structural-cut`. Three round-trip tests now pin all three wire formats.

Also: `.config/nextest.toml` filtered on the deleted `collab` and `language_model` packages. A stale
filter there is a **hard config parse error**, not a warning — the entire test suite refuses to run.
That file was not in Phase 9's data-file inventory.

Also: `crates/audio` pulls `libwebrtc`/`webrtc-sys`. The scout cleared `audio` as safe but never noted
it drags in WebRTC native libs — questionable for a fork that deleted all voice/video. Phase 11 candidate.

### Phase 5 — 2026-07-26 · commit `52b9d6e`

`client.rs` 2303 → ~1570, `user.rs` 1079 → ~715. **23 of 23 keepers verified present, 9 of 9 auth
symbols verified gone.** The relocated 3a landed too: `edit_prediction_types` no longer imports
`client::EditPredictionUsage`, and `crates/editor` needed **zero** changes — the entire justification
for keeping that crate, now proven rather than argued.

Deliberately left, with the compiler already naming every item (~500 lines):
`ClientCredentialsProvider` (**red team finding 12 confirmed** — no caller once `sign_in` went), the
dead collab transport (`connect_with_credentials`, `set_connection`, `establish_connection`,
`rpc_url`, `establish_websocket_connection`), all of `proxy.rs` + `proxy/http_proxy.rs`, `ZED_RPC_URL`.
Phase 11's `--deny warnings` will not let them ship.

### Phase 6 — 2026-07-26 · commits `f66d5c2` `bd7b79a` `affdb88` `8884e39` `2677953`

All telemetry egress is gone **by construction**, not by configuration. `client/src/telemetry.rs`
977 → 439 lines; `reliability.rs` 499 → 151.

**The plan contradicted itself, and the contradiction was load-bearing.** 6a said make
`send_event` a no-op; 6b and 6d said keep the disk-write path and the hang detector because
"disk-only is fine". Both cannot hold: with the macro no-op'd nothing feeds the event queue, so
the **Help → View Telemetry** pane would have shipped permanently empty on top of a dead
collection subsystem — and because those items are `pub`, Phase 11's `--deny warnings` would
never have flagged them. Commissioner chose deletion. `zed/telemetry_log.rs` (562 lines), its
toolbar item, its menu item and `zed_actions::OpenTelemetryLog` went with it.

**A removed Cargo *feature* is a manifest-parse error, not a warning** — the same class as Phase
4's "fourth edit". Dropping `notifications`' `test-support` feature broke `title_bar`'s
dev-dependency on it, and `cargo check -p notifications` refused to resolve the workspace at all
until both landed in one commit. Feature removals need the same reverse-dependency sweep the
plan already prescribes for dependency removals.

Also: `metrics_enabled`/`diagnostics_enabled` and `os_name`/`os_version` were kept on evidence,
not habit — `system_specs` calls the latter for Copy System Specs, and the `telemetry` settings
key survives with `settings_ui` and `onboarding` still writing it.

### Phase 7 (8 of 9) — 2026-07-26 · commits `de097a6` `64a5708` `a74d128` `993414f` `3c8c30f`

Six crates green: `project`, `workspace`, `activity_indicator`, `diagnostics`, `language_tools`,
and `notifications` (which needed nothing beyond 6c once `project` landed).

**Ordering correction #3 — the derived order has the same blind spot twice.**
`file_finder → project_panel → git_ui`, so `file_finder` sits downstream of a **Phase 8** crate.
`cargo check -p file_finder` returns 16 errors and **not one is in `file_finder`** — cargo never
reaches it. Its own errors are masked, which is precisely what the reverse-topological ordering
exists to prevent. `file_finder` is re-sequenced to Phase 8, after `git_ui`. Doing its 14-site
`Match::Channel` unwind blind would violate cutting rules 3 and 4 — no compiler means no
assert-after-cut and no consumer check, the exact conditions that produced Phase 5's three
over-cuts.

**`project` was wrong in the plan for a third reason.** Phase 7a predicted two problems
(`agent_server_store.rs:1896` unwrap, `disable_ai`). The crate's *only actual compile error* was
neither: `project.rs:1647` called the `Client::connect` that Phase 5 removed, inside
`Project::in_room` — the collab room-join constructor, which had **no callers left**. It and
`from_join_project_response` (whose only caller was `in_room`) came out together, 310 lines.

**The plan's 9a recommendation is not executable as written.** "Delete `agent_server_store.rs`
and its wiring" overlooked that `AgentServerStore` is used by
`remote_server/headless_project.rs` — a **survivor**. 2,241 lines across a survivor boundary is
Phase 8 work. Deferred safely: the `:1896` `.unwrap()` cannot panic while `default.json` keeps the
key, so **Phase 9 must not remove that key before the decision is made.**

**One dead `pub` item was caught only because it was noticed by hand.** Removing
`language_tools`' Copilot branch orphaned `LogStore::copilot_state_for_project` in `project`.
Being `pub`, neither `dead_code` nor `--deny warnings` would ever have reported it. Removed with
its `copilot_log_subscription` plumbing. **This is a gap in the Phase 11 gate**: `--deny warnings`
cannot see orphaned `pub` API, so `cargo machete` and reverse-dependency greps are not optional
there.

### Phase 8 (6 of 7) — 2026-07-27 · commits `a01115b` `5d4e998` `2e3082b` `b20604d` `1cbc2ac` `82bc7eb`

**`cargo check --workspace` is GREEN — 0 errors, 0 warnings in `crates/zed`.** The build came back
at the end of 8e, three phases earlier than the plan's build-state map predicted.

`settings_ui` 9,501 → 8,657 · `git_panel.rs` 8,142 → 7,619 · `title_bar.rs` 1,309 → 904 ·
`conflict_view.rs` 675 → 426 · plus 8 whole files deleted.

**The census in 12a paid for itself, twice.** `git_ui`'s "three features" were nine: the plan missed
`generate_commit_message_action`, `load_commit_message_prompt`, `load_project_rules`,
`show_commit_message_error`, `load_local_committer`, `toggle_fill_co_authors`, `fill_co_authors`, the
whole `MergeConflictIndicator` status item, and **a second review-button block in `project_diff.rs`**
that step 16 never mentioned.

**Five settings keys and 15 keymap bindings had to move with their code, not wait for Phase 9.**
Every one had a live `.unwrap()` or a `bail!` behind it, so splitting them across commits would have
shipped a startup panic between the two:
`git::GenerateCommitMessage` (6 bindings) · `git::ReviewDiff` (3) · `onboarding::{SignIn,OpenAccount}`
(6) · `file_finder.include_channels` · `title_bar.{show_user_picture,show_sign_in,show_user_menu}`.
**The lesson generalises: Phase 9 is not a container for deferred coupling. A key and its consumer
are one commit, wherever the consumer happens to live.**

**Two array arities recounted by hand and confirmed by the compiler**, as step 4 demands:
`toolbar_section` 6 → 5, `title_bar_section` 10 → 7.

**Three of this phase's own instructions were wrong and were not followed:**

1. **Step 3 said delete the Server URL settings item. It is KEPT.** Red-team finding 2 established
   that `server_url` must survive for the extension marketplace, and `client.rs:86` unwraps it.
   Deleting the UI would have hidden the exact egress `plan.md` requires be *disclosed*.
2. **Step 29's "orphaned `agent`/`assistant`/`agents_sidebar` modules" in `zed_actions` are not
   orphaned.** `workspace`, `editor` and `terminal_view` still declare against them — and `editor` is
   the zero-change crate. Only `OpenAccountSettings` was removable.
3. **Step 20 was already done.** Phase 4's fourth-edit lesson had stripped `crates/zed`'s delete-set
   deps, which is why all 84 errors arrived as clean *unresolved-crate* errors rather than hundreds
   of unresolved-item ones. The ordering advice was right; the work was already banked.

**A near-miss worth recording.** `project_diff.rs` has **two identical `fn dispatch_action` signatures**
— one dead on `BranchDiffToolbar`, one live on `ProjectDiffToolbar` with 5 callers. A
first-match-wins anchor removed the live one. Caught immediately because the cut was followed by a
compile, and recovered by restoring it and deleting the intended copy. **Brace-matched extraction is
not enough when a signature is not unique; the anchor must be scoped to its `impl` block.**

**8f is NOT done — it is blocked on a decision, not on work.** See below.

## Phase 8f: why `context_server_store` was not removed

Red-team finding 10's premise — "after Phase 4 every external consumer is gone" — is **false**:

- **`remote_server/src/headless_project.rs` is a live consumer** and `remote_server` is a *survivor*
  (the commissioner kept SSH remote dev; rejection rationale 21). It constructs the store, calls
  `shared(REMOTE_SERVER_PROJECT_ID, …)`, subscribes to it, and runs `ContextServerStore::init_headless`.
- **`extension_host` still registers MCP servers** from extension manifests
  (`extension_host.rs:1202`, `:1432`).
- The plan's counts were also stale in our favour: 2 constructor sites remain, not 3 — Phase 7a's
  removal of `Project::in_room` already took the third.

What *is* true, and is a stronger argument than the red team had: **nothing consumes the output.**
`project.context_server_store()` has **no production caller** — only `project`'s own integration
tests. So the subsystem spawns external processes from user config for no purpose.

**Decision (commissioner, 2026-07-27): keep MCP, remove only the dead accessor. On inspection the
accessor is not dead either, so 8f closes as a deliberate no-op.**
`Project::context_server_store()` has no *production* caller but is used at five sites in
`crates/project/tests/integration/context_server_store.rs`. Removing it would mean deleting the
integration tests of the very subsystem we just chose to keep working. The accessor stays.

But removing the subsystem is not the contained edit 29a-29b describe. It reaches `project`, `remote_server`,
`extension_host`, and potentially the **extension WIT API** (`since_v0_8_0.rs`,
`extension_api.rs`) — which would break the extension compatibility the commissioner explicitly
wanted kept. That is a scope and compatibility trade the plan did not price, so it is the
commissioner's call.

### Phase 9 — 2026-07-27 · commits `0400e3d` `5848812` `f0ad624`

**The editor starts.** That is this phase's whole gate and it passes: launched with
`ZED_STATELESS`, alive 20s, no panic. All **8** `base_keymap` values (7 named + `None`) launch clean,
each in an isolated `--user-data-dir`. A legacy `settings.json` carrying 13 of the removed keys
loads with no error, confirming finding 2's permissiveness claim empirically rather than by reading.

`default.json` 2603 → 2321 · 320 keymap bindings and 41 dead blocks gone · 35 icons · 8 sounds ·
3 prompts.

**The derived diff beat the estimate, in both directions.** 1,292 → 1,165 actions: **128** removed,
not the ~142 predicted, and exactly **1** added — `activity_indicator::DismissMessage`, which
independently confirms the Phase 7c hand-off. Hand-curating by namespace would have been wrong for
the reason the phase warned about *and* one it did not anticipate.

**A naive resolution check condemns 17 working bindings.** `editor::CopyPath`,
`branches::OpenRecent`, `project_panel::CopyPath` and friends are absent from
`--dump-all-actions` **at baseline too**, because the dump lists only *preferred* names while these
are declared `#[action(deprecated_aliases = [...])]`. Any "does every binding resolve?" test must
consult the alias set. The new regression test goes through the real action registry instead of the
dump, so it gets this right for free — a test written against the dump would have failed on day one.

**Three rows of the 9b settings table are wrong and were not followed:**

1. **`edit_predictions` + its `copilot`/`codestral`/`ollama` sub-keys must be KEPT.**
   `language/src/language_settings.rs:779,787,794` **unwraps all three**, and `language` is a
   survivor. Removing them is precisely the startup panic this phase exists to prevent. Same for
   `show_edit_predictions` and `edit_predictions_disabled_in`.
2. **`context_servers` / `context_server_timeout` / `agent_servers` stay** — Phase 8f kept MCP, and
   `AgentServerStore` survives because `remote_server` shares it over SSH.
3. **`disable_ai` stays** — `settings_store` merges it across project files and `DisableAiSettings`
   still gates the surviving context-server and agent-registry stores.

**Two estimates in 9c were wrong in both directions.** The "~48 orphaned icons" are 35: `MicMute`
and `Screen` *are* used while `Mic`/`Bell*`/`Chat`/`User*` are not, and 12 of the supposedly-orphaned
set are live (`ai_zed` alone has 27 references). Step 12's "check `IconName` consumers first" is the
instruction that saved this.

**A test that looked like a casualty was actually load-bearing.** `settings_store`'s
`AutoUpdateSetting` was not testing auto-update — it was the store's generic example of a
*global-only bool*, carrying ~15 assertions about layering, project-vs-user schema scoping and
update formatting. Repointed at `vim_mode` with every value inverted (`vim_mode` defaults false where
`auto_update` defaulted true), so each step still proves something. 27 settings tests pass.

**`crates/audio` is now fully orphaned.** Removing `play_sound` took the last consumer of the output
mixer, and the crate has **zero** external API consumers left — only `audio::init`, which just
registers settings. With the libwebrtc/webrtc-sys edge flagged in Phase 7, deleting the crate is a
Phase 11 candidate; its three settings keys are kept only because `audio_settings.rs` still reads them.

**Also cleaned:** `assets/keymaps/storybook.json` and `script/{storybook,seed-db,squawk,zed-local}`
(their crates died in Phase 4), and the macOS camera/microphone/speech-recognition usage descriptions
plus the `device.audio-input`/`device.camera` entitlements. Note those descriptions cover programs run
*inside* Zed, so a terminal child process wanting the microphone is now denied — correct for this fork,
but a real behaviour change.

**Deferred to Phase 11:** `.github/workflows/{deploy_collab,run_tests}.yml` still name the `collab`
crate, but they are **generated** from `tooling/xtask` and `run_tests` fails CI on any hand edit to
`.github` — that is a generator change, not a YAML edit.

**Unresolved, carried to Phase 10:** the `telemetry` settings key. Phase 8d deferred the decision
here, and this phase's step 8 says "keep" — but that instruction predates the commissioner's Phase 6
choice to delete the collection subsystem outright, so the toggle now controls nothing. Removing it
spans `default.json`, the two live unwraps at `client.rs:379-380`, `TelemetrySettings`, `settings_ui`
and `onboarding`'s telemetry section. It is a coherent single commit, but it is a **user-visible
settings removal** and wants an explicit decision rather than being smuggled in behind a stale
instruction.

### Phase 10 — 2026-07-28 · commits `94ed295` `d6e5881` `83da263` `5235bb4`

**4,000 of 4,001 tests pass.** `cargo check --workspace --all-targets` is clean — every test target
compiles for the first time since Phase 4. The docs validate. One test fails and is **not hidden**.

**The blocker was one file.** `crates/client/src/test.rs` (FakeServer) mocked the auth endpoint and
called the `Client::connect` that Phase 5 removed. Anything enabling `client`'s `test-support`
feature pulled it in, so it blocked *every* test target in the workspace. Deleted with the 8 tests
that needed it — 5 for reconnection/auth, 3 for `Client::subscribe_to_entity`, which is only
reachable from `Project::shared()`, and that has no callers left. `client`'s tests now run: 17 pass,
including the four telemetry tests written in Phase 6 that had never once been executed.

**The canary earned its keep.** `expected_namespaces`, regenerated from the dump rather than
hand-edited, caught two actions still registered for deleted features:
`zed_predict_onboarding::OpenZedPredictOnboarding` and `collab::ShowCallStats` — neither with any
reference or keymap binding.

**The alias trap bites twice.** Phase 9 found that a dump-derived keymap check condemns 17 working
bindings because `--dump-all-actions` reports only preferred names. The same trap sits under the
canary from the other side: `cx.all_action_names()` *includes* deprecated aliases, so a list derived
from preferred names alone **fails** — three namespaces (`zed_actions`, `assistant2`, `branches`)
exist purely as aliases. The expected list must be built from `name` **and** `deprecated_aliases`.

**Recorded rather than silently shipped:** 27 actions are still declared with no handler left —
`agent` (16), `agents_sidebar` (2), `assistant` (1) and 8 of `collab`. They would appear in the
command palette and do nothing. Removing them means editing `workspace`'s ActiveCall trait surface
and the zero-change `editor` crate, so they are noted in the test itself and assigned to Phase 11.
`collab::{SwitchBranch,ToggleProjectMenu}` are **live** title-bar features merely sitting in a
misleading namespace.

**A test that looked like a casualty was load-bearing** (the second time this plan has hit that).
`settings_store`'s `AutoUpdateSetting` in Phase 9, and here `test_disable_ai_crash` — which this
phase's step 5 says to *delete*. It stays: Phase 9 kept `disable_ai` because it still gates the
surviving context-server and agent-registry stores. Steps 3 and 4 were likewise already done in
Phase 9. Three of this phase's six test steps were moot on arrival.

**The docs gate was run without its tool.** `mdbook` is not installed here, so rather than skip step
12 the preprocessor — the component `check_docs` actually enforces — was driven directly with a
synthetic book of all 144 chapters. **It caught three real errors**: settings snippets still carrying
`title_bar.show_user_picture`/`show_user_menu`/`show_sign_in` and `agent_buffer_font_size`. Now exits
0. A true `mdbook build` remains unverified and is a Phase 11 item.

**Docs corrected, not just pruned.** 26 pages deleted and `remote-development.md` kept. 27 dangling
links fixed; where a link carried a sentence or a section, the prose went with it rather than
leaving a stump. `docs/src/telemetry.md` was false in nearly every sentence — it described Sentry,
Snowflake, Amplitude, minidump upload and a telemetry log viewer, none of which exist — and is
rewritten to state what is true and how to check it.

**`legal/*.md` were marked, not rewritten.** They are Zed Industries' corporate instruments for a
hosted service this fork does not run. Leaving them unmarked would be exactly the false statement
this phase warns about, but authoring a replacement privacy policy or terms of service is the
maintainer's decision and possibly a lawyer's — not something to invent. Each now carries a warning
naming what is inapplicable; Phase 12 owns the replacement.

**Deferred to Phase 11:** `tooling/xtask/src/tasks/workflows/` still generates CI steps that upload
debug symbols to Sentry and deploy the deleted `collab` crate. These are **generated** files and
`run_tests` fails CI on any hand edit to `.github`, so they are a generator change.

### Phase 11 — 2026-08-04 · commits `c575bc3` `84917ef` `f8a667b` `f459957` `315d4e0`

**Gate A (mechanical) is green: `cargo check --workspace` (+`--all-features`), `cargo build
--release` (15m12s), `cargo nextest run --workspace --no-fail-fast`, and `./script/clippy`
(`cargo clippy --release --all-targets --all-features -- --deny warnings` + `cargo machete`) all
pass. `cargo machete` finds zero unused dependencies workspace-wide.**

**`--deny warnings` under `--all-targets --all-features` is what finally forced the debt Phases
5–10 deliberately deferred into the open.** A plain `cargo check` never compiles `#[cfg(test)]`
code, so none of it had ever actually been checked for dead code until this phase ran the real
gate. rustc's own dead-code lint found:

- `crates/audio`'s `EchoCanceller` — the last thing in the workspace still pulling in
  `libwebrtc`/`webrtc-sys`/`livekit`, and its owning `Audio` struct has had zero external callers
  since collab voice calls left. Removed, along with the now-dead `[patch.crates-io]` entries.
- `client`'s `connect_with_credentials`/`set_connection`/`establish_connection`/`rpc_url`/
  `establish_websocket_connection` and the `proxy.rs`/`proxy/{http,socks}_proxy.rs` module they
  were the only caller of — exactly the ~500 lines Phase 5 flagged as "kept on a false pretext."
  26 warnings in one crate, all resolved. **Deliberately left**: the `authenticate`/
  `establish_connection`/`rpc_url` fields and their `override_*` setters — genuine `pub` API at
  the crate root (rustc correctly treats it as reachable), zero real callers, which is precisely
  this phase's own bar for "may exist as a type but must have no caller."
- 30 unused dependency entries across `client`, `zed`, `title_bar`, `settings_ui`, `file_finder`,
  `onboarding`, and `audio` (cargo-machete, each verified individually with `rg` before removal).
- Two dead builder-pattern methods on `settings_ui` components, and three unrelated pre-existing
  clippy violations (`int_plus_one` in `remote_connection`, `redundant_clone` ×2 in `git_ui`/
  `zed`, one unused import) — none tied to this fork's own deletions, all first-time-visible
  because this is the first time the real gate command has ever completed against this branch.

**Two false alarms, checked rather than assumed.** A plain `cargo build --release` (no
`--all-targets`) flags `ProjectClientState::Collab` as never-constructed — but it *is* constructed,
via Phase 8's deliberately-kept `mark_as_collab_for_testing` test helper, the moment `--all-targets`
compiles test code. Confirmed with `cargo check -p project --all-targets --all-features`: zero
warnings. Left untouched. Separately, a `--all-features`-only (dev-profile, `--all-targets`) build
of `settings` hits a real `rust-embed`/`debug-embed` feature-unification quirk from
`remote_server`'s optional debug-embed feature — reproduced on a clean baseline stash, so
pre-existing and unrelated to this session. It does not reproduce under `--release`, which is what
`script/clippy` actually runs, so it does not block this gate.

**Tests: 3993 run (was 4001), 3992 passed, 1 failed, 11 skipped — same 193 binaries, same single
failure.** The 8-test delta is exactly `client::proxy::{http_proxy,socks_proxy}` — the module just
deleted. Verified by diffing against Phase 10's actual run log (not the separate all-features
inventory list, which was never the right comparison). The one failure is the same
`test_refresh_requested_multi_server` regression Phase 10 bisected to Phase 4 and left open.

**Gate B and Gate C are not done — they need the user.** Static endpoint sweep is complete and
recorded in `research/network-verification.md`: the only real background-egress path is the
disclosed `build_zed_api_url` (extension registry / LSP downloads), plus a new, non-egress finding
- `context_server/oauth.rs`'s `CIMD_URL` presents this fork as `zed.dev` to third-party MCP OAuth
servers, a rebrand question handed to Phase 12. What's left - `lsof`/`nettop` snapshots, the
two-tier `/etc/hosts` blackhole test, a Little Snitch/LuLu session, and the full functional pass
(extension install, LSP auto-download, SSH remote dev, hang traces, legacy config, all 7 base
keymaps) - all require launching the actual release GUI binary, editing `/etc/hosts` under `sudo`
twice, and driving the app interactively. The recipe is written down in
`research/network-verification.md` for whoever runs it.

### Phase 12 — 2026-08-04 · commits `7c2d97d` `e2bd2f0` `fc25ae9` `916f5d3` `bb88bb6`

**The two blocking decisions got made.** Brand name: "Zode" (the user already had it in
mind — it also matches this repository's own directory name). Extension registry:
disclose and keep, the lowest-cost option, per the user's explicit choice between the
four the phase file laid out.

**Renamed the package and binary — `zed` → `zode`, bundle id `dev.zed.Zed` family →
`io.github.tgiap04.zode` family (derived from the actual GitHub repo, no dedicated
domain owned).** Version reset to 0.1.0 — a fork version must never be mistaken for an
upstream Zed one. `release_channel::display_name()`/`app_id()` turned out to be the
single source of truth the window title, About dialog, and every platform's app-id
(Wayland/X11 WM_CLASS, macOS bundle id) all read from — one change, wide effect.

**Renaming the `[[bin]]` broke things silently, and caught them one at a time.**
Every packaging script (`bundle-mac`, `bundle-linux`, `bundle-windows.ps1`) hardcoded
`target/.../release/zed` as the compiled artifact's path — all now pointed at a file
that no longer existed. Worse: `crates/cli`'s own app-detection logic (`Detect::detect`)
hardcoded the same path for **dev-build discovery and the macOS bundle's
`Contents/MacOS/zed`** — left unfixed, the `zode` command literally could not have
found the binary to launch on any platform. Fixed the dev-build and macOS paths (both
verified: `cargo build -p zode -p cli` produces `target/debug/zode`, and the `cli`
crate's own `Contents/MacOS/zode` path was checked against the exact string the
`bundle-mac` script now copies to). Windows installer branding (`Zed.exe`, the `.iss`
script, registry keys) was **not** carried through — deferred to the packaging pass,
since fixing it fully requires a Windows toolchain to verify, and a dead
`auto_update_helper` package reference (that crate no longer exists) made clear nobody
has run this script since Phase 3 removed auto-update.

**A second, unrelated real bug found while sweeping user-visible strings**: "File Bug
Report...", "Request Feature...", and "Zed Repository" in the Help menu pointed at
`zed-industries/zed`'s own GitHub issues, discussions, and repo — left as-is, a user
hitting a bug in this fork's own changes would file it against a codebase Zed
Industries has never seen. Repointed at this fork's repository; removed "Email Us..."
(`mailto:hi@zed.dev`), "Zed Twitter", and "Join the Team" (`zed.dev/jobs`) outright —
there is no support address, social account, or team for this fork to point them at.
Verified with the full `zed::tests::` suite (33 passed) that removing the now-dead
`EmailZed` action didn't orphan a keymap binding or break the namespace canary.

**Two more findings had nothing to do with rebranding and predate this whole plan.**
`README.md` and `AGENTS.md` have described an entirely unrelated AI development kit
("FayeDark Agent Kit") since this repository's very first commit — leftover
scaffolding cruft, not something any earlier phase touched. Replaced `README.md`
(explicitly in this phase's scope); `AGENTS.md` has the same problem but wasn't in
scope here, so it's flagged rather than fixed. Separately, `ztracing`/`ztracing_macro`
carried stray `LICENSE-AGPL`/`LICENSE-APACHE` symlinks inconsistent with their own
`Cargo.toml` (`GPL-3.0-or-later`) — found only because deleting the now-verified-unused
root `LICENSE-AGPL` (0 of 169 crates declare AGPL) turned them into dangling symlinks.

**`legal/*.md` rewritten, not just re-marked.** Phase 10 added warning banners and
deliberately stopped there — "a privacy policy and terms of service are legal
instruments... authoring them is the maintainer's decision." With the brand name and
registry decision now made, that decision had a floor under it. All four files
(`privacy-policy.md`, `subprocessors.md`, `terms.md`, `third-party-terms.md`) rewritten
short and accurate rather than reproduced at upstream's original ~40-page length: no
company, no service, no subprocessors, one disclosed dependency (the extension
registry), and the GPL/Apache license as the only thing actually governing use of
locally-run software.

**What this phase did not do — not a shortfall, a boundary.** The actual Zed logo
assets (PNG/ICNS/ICO icon files) are unchanged — replacing them needs a designer or an
image tool this session doesn't have. Signing, notarization, and a Homebrew tap need an
Apple Developer ID certificate and a separate repository. Linux `.desktop`/
`snapcraft.yaml.in`/flatpak-manifest branding and the Windows `.iss` installer were left
alone deliberately — editing them meaningfully requires building and testing on each
target platform, which isn't available here, and `snapcraft.yaml.in` in particular
still downloads **upstream Zed's own release tarball**, confirming nobody has touched
Linux packaging in this fork's history yet either. Onboarding/welcome-screen copy and
tooltips were not swept exhaustively — the highest-visibility strings (menu bar, About
dialog, Help menu, permission prompts) were, everything else is still open.

## Execution rule: how to cut (learned the hard way in Phase 5)

**Three over-cuts happened in Phase 5.** All three had the same cause, and all three were caught only
by checking `git show`:

| # | Destroyed | Caught by |
|---|---|---|
| 1 | **The entire RPC request API** — `send`, `request`, `request_stream`, `request_envelope`, `request_dynamic`, `handle_message`, `connection_id` | `no method named handle_message` |
| 2 | `ClientSettings`, `ProxySettings` | `zed_urls.rs` failed to resolve |
| 3 | `Subscription`, `PendingEntitySubscription`, `TelemetrySettings`, `impl Default for ClientState` | syntax error |

The cause every time: cutting **"from marker A to marker B"** where A and B are two functions that
both need deleting. Keepers sit *between* them. In a 2,300-line file the doomed functions are not
contiguous, and the eye that picked the markers cannot see what it swept up.

**The rule, for Phases 7, 8 and 9 — which still have to cut a 9,501-line `page_data.rs`, an
8,142-line `git_panel.rs`, and `crates/zed`:**

1. **Cut one function at a time.** The end marker must be the *next* item you also intend to delete,
   never "the next thing that happens to appear".
2. **Before cutting, dump the function map** of the region:
   `grep -n "^    pub fn \|^    pub async fn \|^    async fn \|^    fn \|^impl \|^pub struct \|^pub enum " <file>`
   Then decide delete/keep for **every** entry in the range. If any entry inside a proposed A→B range
   is a keeper, the range is wrong — split it.
3. **Assert the keep-list after every cut**, in the same script. Phase 5's 23 assertions are the
   template. A cut that compiles is not a cut that is correct.
4. **Never trust `cargo check` alone.** Over-cut #2 compiled the file it damaged; only a *different*
   crate revealed it.
5. **Commit per crate, always.** This is what made all three recoveries cheap. It is the rollback
   unit, not a tidiness preference.

## Reference material

- [Consultation & decision record](./brainstorm-report.md)
- [Data-file blast radius](./reports/scout-data-files.md) — 403 keymap actions, 21 settings keys
- [`crates/zed` wiring map](./reports/scout-zed-wiring.md) — init order, edit order
- [Settings schema & migration](./reports/scout-settings-migration.md) — the permissiveness verdict
- [Survivor patch detail](./reports/scout-survivor-patches.md) — per-crate symbol inventory
- [Workspace surgery research](./reports/research-workspace-surgery.md) — cargo mechanics, verification tooling
- [Final delete set](./research/final-delete-set.py) — re-runnable after every phase
