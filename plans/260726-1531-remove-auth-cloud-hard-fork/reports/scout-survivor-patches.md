# Scout Report — Survivor Crate Patches

Scope: the 15 surviving crates that import from the delete set (excluding `zed` and `settings_ui`,
covered by other agents), plus the 2 dev-dependency-only crates.

All paths absolute-relative to `/Users/tgiap.dev/devs/zode`.

---

## TOP-LINE FINDINGS (read this first)

Three items in the current delete set are **mis-classified** and cause 80% of the pain. Fixing the
delete set is far cheaper than patching the survivors.

| Crate in delete set | Why it's mis-classified | Cost of keeping it | Cost of deleting it |
|---|---|---|---|
| `edit_prediction_types` | 380 LOC. Its ONLY auth/cloud tie is `use client::EditPredictionUsage` (3 sites). | Delete 3 lines in `crates/edit_prediction_types/src/edit_prediction_types.rs` | ~429 `edit_prediction`/`EditPrediction` references in `crates/editor/src/editor.rs` alone, +33 in `element.rs`, +5 in `display_map.rs`. Multi-thousand-line surgery on the editor core. |
| `remote_connection` | 728 LOC SSH/WSL connect UI. Its ONLY tie to the delete set is `use auto_update::AutoUpdater` (2 call sites, both for downloading the remote-server binary). | Replace 2 `AutoUpdater::*` calls with a local HTTP download or a `bail!` | Guts `recent_projects` (7,704 LOC, remote projects is its whole reason to exist), `git_ui/worktree_service.rs`, `git_ui/worktree_picker.rs`, `title_bar`, and 4 test-support deps. |
| `context_server` | MCP transport crate. Zero auth/cloud deps (`anyhow, gpui, http_client, net, serde, settings, tiny_http, url, util`). | Zero — it just stops being used by anything but `project`. | Forces `project` + `remote_server` + `project_settings` + `settings_content` surgery, and breaks the `extension_host` MCP extension path. |

**Recommendation:** move `edit_prediction_types`, `remote_connection`, and `context_server` from the
delete set into "keep, with a small patch". Doing so removes `editor`, `recent_projects`, and most of
`project` from the work breakdown entirely, and shrinks `git_ui` and `title_bar` by half.

A fourth item goes the other way:

| Crate listed as survivor | Reality |
|---|---|
| `sidebar` | 16,784 LOC (5,339 lib + 11,168 tests) whose entire subject is agent threads (`ThreadEntry`, `ThreadSwitcher`, `ThreadsArchiveView`, `ArchiveSelectedThread`, `NewThread`, import-onboarding banners). It is not patchable — **move `sidebar` into the delete set.** |

---

## 1. `sidebar` — MOVE TO DELETE SET

### Imports from deleted crates

| File:line | Import |
|---|---|
| `crates/sidebar/src/sidebar.rs:3` | `use acp_thread::ThreadStatus;` |
| `crates/sidebar/src/sidebar.rs:6` | `use agent_settings::AgentSettings;` |
| `crates/sidebar/src/sidebar.rs:7-9` | `use agent_ui::thread_metadata_store::{ThreadMetadata, ThreadMetadataStore, WorktreePaths, worktree_info_from_thread_paths};` |
| `crates/sidebar/src/sidebar.rs:10` | `use agent_ui::thread_worktree_archive;` |
| `crates/sidebar/src/sidebar.rs:11-13` | `use agent_ui::threads_archive_view::{ThreadsArchiveView, ThreadsArchiveViewEvent, format_history_entry_timestamp};` |
| `crates/sidebar/src/sidebar.rs:14-18` | `use agent_ui::{AcpThreadImportOnboarding, Agent, AgentPanel, AgentPanelEvent, ArchiveSelectedThread, CrossChannelImportOnboarding, DEFAULT_THREAD_TITLE, NewThread, ThreadId, ThreadImportModal, channels_with_threads, import_threads_from_other_channels};` |
| `crates/sidebar/src/thread_switcher.rs:3` | `use agent_ui::thread_metadata_store::ThreadMetadata;` |
| `crates/sidebar/src/sidebar.rs:451,918,2661` | `remote_connection::connect_with_modal` / `dismiss_connection_modal` |
| `crates/sidebar/src/sidebar_tests.rs:2,3,4,31,…` | `acp_thread::{AcpThread, PermissionOptions, StubAgentConnection}`, `agent::ThreadStore`, `agent::ZED_AGENT_ID`, `agent_ui::test_support`, `agent_ui::MaxIdleRetainedThreads`, `language_model::LanguageModelRegistry` (≈40 sites) |

### Structural evidence it is not salvageable

- `crates/sidebar/src/sidebar.rs:109-119` — `enum SidebarView { ThreadList, Archive(Entity<ThreadsArchiveView>) }`
- `crates/sidebar/src/sidebar.rs:121-128` — `struct ActiveEntry { thread_id: agent_ui::ThreadId, session_id: Option<acp::SessionId>, … }`
- `crates/sidebar/src/sidebar.rs:191-223` — `struct ThreadEntry`
- `crates/sidebar/src/sidebar.rs:458-492` — `struct Sidebar` fields: `thread_last_accessed`, `thread_switcher`, `pending_thread_activation`, `restoring_tasks`, `import_banners_use_verbose_labels`
- `crates/sidebar/src/sidebar.rs:4937-5040` — `Render` dispatches 12 thread actions and renders `SidebarView::ThreadList` / `Archive`

Non-agent content is limited to `dump_workspace_info` (`sidebar.rs:5101-5339`, ~240 lines) and the
`SidebarRecentProjects` popover.

### Classification
(a) whole feature to delete.

### Patch size
Delete the crate: 3 files, 16,784 lines, plus workspace member + `Cargo.toml` entry.
Downstream fixups (all in crates the other agents own):
- `crates/zed/src/zed.rs:70` `use sidebar::Sidebar;`, `:1264` `workspace.register_action(sidebar::dump_workspace_info)`
- `crates/zed/src/visual_test_runner.rs:2671`, `:3414`
- `crates/workspace/src/multi_workspace.rs` — the `WorkspaceSidebar` trait / `set_sidebar` plumbing becomes dead (trait itself can stay; nothing implements it).

If `dump_workspace_info` is worth keeping, move those ~240 lines into `crates/zed/src/zed.rs`.

### Ordering
Delete before touching `workspace` (so the `WorkspaceSidebar` decision is made once).

---

## 2. `title_bar` — big but mechanical

### Imports from deleted crates

| File:line | Import | Delete set |
|---|---|---|
| `crates/title_bar/src/title_bar.rs:23` | `use auto_update::AutoUpdateStatus;` | auto_update |
| `crates/title_bar/src/title_bar.rs:24` | `use call::ActiveCall;` | call |
| `crates/title_bar/src/update_version.rs:4` | `use auto_update::{AutoUpdateStatus, AutoUpdater, UpdateCheckType, VersionCheckType};` | auto_update |
| `crates/title_bar/src/update_version.rs:125` | `use auto_update::VersionCheckType;` (test mod) | auto_update |
| `crates/title_bar/src/collab.rs:4` | `use call::{ActiveCall, Room};` | call |
| `crates/title_bar/src/collab.rs:5` | `use channel::ChannelStore;` | channel |
| `crates/title_bar/src/collab.rs:13` | `use livekit_client::ConnectionQuality;` | livekit_client |
| `crates/title_bar/src/collab.rs:15` | `use remote_connection::RemoteConnectionModal;` | remote_connection (KEEP — see top-line) |

### Call sites

| File:line | Symbol | Action |
|---|---|---|
| `title_bar.rs:2` | `pub mod collab;` | delete module |
| `title_bar.rs:365,388` | `ActiveCall::global(cx)` + `cx.observe(&active_call, …)` in `TitleBar::new` | delete 2 stmts |
| `title_bar.rs:1002-1017` | `fn window_activation_changed` — 2× `ActiveCall::global(cx).update(… set_location …)` | delete the two branches, keep `update_active_view_for_followers` |
| `title_bar.rs:1019-1022` | `fn active_call_changed` | delete fn |
| `title_bar.rs:1024-1034` | `fn observe_diagnostics` (reads `room().diagnostics()`) | delete fn + `_diagnostics_subscription` field (`title_bar.rs:158`) |
| `title_bar.rs:1036-1043` | `fn share_project` | delete fn |
| `title_bar.rs:1045-1051` | `fn unshare_project` | delete fn |
| `title_bar.rs:249` | `children.push(self.render_collaborator_list(window, cx)…)` | delete |
| `title_bar.rs:285` | `.children(self.render_call_controls(window, cx))` | delete |
| `title_bar.rs:288` | `.child(self.update_version.clone())` | delete |
| `title_bar.rs:156,417,436,451` | `update_version: Entity<UpdateVersion>` field, construction, accessor | delete |
| `title_bar.rs:1071-1093` | auto-update label + "Please restart Zed to Collaborate" menu entry | delete block (~25 lines) |
| `title_bar.rs:1121` | `self.update_version.read(cx).show_update_in_menu_bar()` | delete |
| `title_bar.rs:1133,1148` | `user_store.plan()` / `plan_for_organization(&org.id)` (feeds `PlanChip`) | delete (depends on `client` gutting — see §5) |
| `collab.rs` (whole file, 722 lines) | `render_collaborator_list` (144), `render_collaborator` (251), `render_call_controls` (335), `render_screen_list` (631), `toggle_screen_sharing` (36), `toggle_mute` (92), `toggle_deafen` (112) | delete file |
| `plan_chip.rs` (whole file, 44 lines) | `use cloud_api_types::Plan` | delete file |
| `update_version.rs` (whole file, 145 lines) | `AutoUpdater` observer | delete file |

`onboarding_banner.rs` (177 lines) is clean (`gpui`, `ui`, `util`) — keep.
`remote_connection::RemoteConnectionModal` appears only at `collab.rs:348`, which dies with the file.
`title_bar.rs:500,519,535` use `project.remote_connection_options/state` — that's the `project` API, not the deleted crate. Keep.

### Classification
(a) whole feature to delete (collab/call/livekit/channel, auto-update, plan chip).

### Patch size
Files touched: 5 (`title_bar.rs` edit ~120 lines removed; delete `collab.rs`, `plan_chip.rs`, `update_version.rs`; `Cargo.toml` remove `auto_update`, `call`, `channel`, `livekit_client`, `cloud_api_types`, and dev-deps `call`, `notifications`).
Lines: ~1,030 deleted, ~120 edited.

### Ordering
Do **after** `client` (§5) — `title_bar.rs:1133/1148` call `UserStore::plan()`, which §5 removes.

---

## 3. `git_ui` — mixed; two real features to delete

### Imports from deleted crates

| File:line | Import |
|---|---|
| `crates/git_ui/src/conflict_view.rs:1` | `use agent_settings::AgentSettings;` |
| `crates/git_ui/src/project_diff.rs:6` | `use agent_settings::AgentSettings;` |
| `crates/git_ui/src/project_diff.rs:49` | `use zed_actions::agent::ReviewBranchDiff;` (zed_actions survives — action definition only) |
| `crates/git_ui/src/git_panel.rs:13` | `use agent_settings::AgentSettings;` |
| `crates/git_ui/src/git_panel.rs:47-50` | `use language_model::{CompletionIntent, ConfiguredModel, LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, Role};` |
| `crates/git_ui/src/worktree_service.rs:637,655` | `remote_connection::connect_with_modal`, `dismiss_connection_modal` |
| `crates/git_ui/src/worktree_picker.rs:1047,1056,1062,1077` | `remote_connection::{RemoteConnectionModal, connect}` |

### Call sites and treatment

| File:line | Symbol | Class |
|---|---|---|
| `git_panel.rs:2685-2832` | `pub fn generate_commit_message` (147 lines) — the whole LLM commit-message feature | (a) delete |
| `git_panel.rs:2670-2682` | `async fn load_commit_message_prompt` (only caller is the above) | (a) delete |
| `git_panel.rs:4030-4094` | `render_generate_commit_message_button` (65 lines) | (a) delete |
| `git_panel.rs:770-775` | `was_ai_enabled` / `is_ai_enabled` observer for AgentSettings changes | (b) small conditional to remove |
| `git_panel.rs:2686`, `:4034` | `AgentSettings::get_global(cx).enabled(cx)` guards | (b) dies with the feature |
| `git_panel.rs:2710` | `AgentSettings::temperature_for_model(&model, cx)` | (a) dies with the feature |
| `git_panel.rs:3308-3346` | `fn potential_co_authors` — `call::ActiveCall::try_global` + `room.remote_participant_for_peer_id` | (a) delete or reduce to `Vec::new()` |
| `git_panel.rs:3349-3358` | `fn local_committer(&self, room: &call::Room, …)` | (a) delete |
| `git_panel.rs:5735-5745` | `Render`: `room` lookup + `has_co_authors` | (b) set `has_co_authors = false`, drop the block |
| `git_panel.rs:4095-4137` | `render_co_authors` (co-author UI; only meaningful with a room) | (a) delete |
| `conflict_view.rs:297` | `is_ai_enabled` in `render_conflict_buttons` | (b) conditional |
| `conflict_view.rs:548-551` | `MergeConflictIndicator` event guard | (b)/(a) |
| `conflict_view.rs:576-586` | `resolve_with_agent` → `ResolveConflictedFilesWithAgent` | (a) delete |
| `conflict_view.rs:594-600` | `MergeConflictIndicator::render` guard | (a) whole indicator can go |
| `project_diff.rs:1654,1656` | `is_ai_enabled` → `show_review_button` ("Review Diff" → agent) | (a) delete the button branch (~35 lines, `:1671-1706`) |

`worktree_service.rs` / `worktree_picker.rs` `remote_connection` usage: **no change needed if
`remote_connection` is kept** (recommended). If it is deleted, both files lose their remote-worktree
path (~60 lines across 2 files) — an additional (a).

Replacement for `AgentSettings::enabled(cx)`: the underlying setting is
`project::DisableAiSettings` (`crates/project/src/project.rs:1087`), already a surviving crate.
Since every AI branch here is being deleted anyway, no replacement type is needed.

### Classification
(a) two whole features (LLM commit message, agent conflict resolution / diff review) + (a) collab co-authors; (b) a handful of guards.

### Patch size
Files: 5 (`git_panel.rs`, `conflict_view.rs`, `project_diff.rs`, `git_ui.rs` if the indicator is registered there, `Cargo.toml`).
Lines: ~400 deleted, ~30 edited. `Cargo.toml`: drop `agent_settings`, `language_model`, `call`.

### Ordering
Independent. Can be done any time. Do after the `remote_connection` keep/delete decision is final.

---

## 4. `client` — the auth gutting (highest care)

### Imports from deleted crates

| File:line | Import |
|---|---|
| `crates/client/src/client.rs:17` | `use cloud_api_client::LlmApiToken;` |
| `crates/client/src/client.rs:18` | `use cloud_api_client::websocket_protocol::MessageToClient;` |
| `crates/client/src/client.rs:19` | `use cloud_api_client::{ClientApiError, CloudApiClient};` |
| `crates/client/src/user.rs:4` | `use cloud_api_client::websocket_protocol::MessageToClient;` |
| `crates/client/src/user.rs:5-7` | `use cloud_api_client::{GetAuthenticatedUserResponse, KnownOrUnknown, Organization, OrganizationId, Plan, PlanInfo, …};` |
| `crates/client/src/user.rs:8` | `use cloud_api_types::OrganizationConfiguration;` |
| `crates/client/src/user.rs:9-11` | `use cloud_llm_client::{EDIT_PREDICTIONS_USAGE_AMOUNT_HEADER_NAME, EDIT_PREDICTIONS_USAGE_LIMIT_HEADER_NAME, UsageLimit};` |
| `crates/client/src/user.rs:746` | `use cloud_api_client::Plan;` (inside `plan()`) |
| `crates/client/src/llm_token.rs:2` | `use cloud_api_client::LlmApiToken;` |
| `crates/client/src/llm_token.rs:4` | `use cloud_llm_client::{EXPIRED_LLM_TOKEN_HEADER_NAME, OUTDATED_LLM_TOKEN_HEADER_NAME};` |
| `crates/client/src/test.rs:5-8` | `use cloud_api_client::{AuthenticatedUser, GetAuthenticatedUserResponse, KnownOrUnknown, Plan, PlanInfo}; use cloud_llm_client::{CurrentUsage, UsageData, UsageLimit};` |

### MUST GO

`crates/client/src/client.rs`
| Lines | Item |
|---|---|
| 17-19 | the three `use cloud_api_client::…` |
| 57 | `pub use llm_token::*;` |
| 189 | `pub type MessageToClientHandler = …` |
| 199 | field `cloud_client: Arc<CloudApiClient>` |
| 204 | field `message_to_client_handlers: Mutex<Vec<MessageToClientHandler>>` |
| 540 | `cloud_client: Arc::new(CloudApiClient::new(http.clone()))` in `Client::new` |
| 579-581 | `pub fn cloud_client()` |
| 907-910 | `self.cloud_client.set_credentials(…)` in `authenticate` |
| 923-939 | `async fn validate_credentials` (calls `cloud_client.validate_credentials`) |
| 941-970 | `async fn connect_to_cloud` (Cloud WebSocket) |
| 1523-1581 | `acquire_llm_token`, `refresh_llm_token`, `clear_and_refresh_llm_token` (3 fns, 59 lines) |
| 1586 | `self.cloud_client.clear_credentials()` in `sign_out` |
| 1744-1759 | `add_message_to_client_handler` + `handle_message_to_client` |

`crates/client/src/llm_token.rs` — delete the whole file (116 lines).

`crates/client/src/user.rs`
| Lines | Item |
|---|---|
| 4-11 | cloud imports |
| 115-121 | fields `edit_prediction_usage`, `plan_info`, `current_organization`, `organizations`, `plans_by_organization`, `configuration_by_organization` |
| 147 | `Event::PlanUpdated` |
| 149 | `Event::OrganizationChanged` |
| 167-173 | `pub struct EditPredictionUsage(pub RequestUsage)` + `pub struct RequestUsage` — **see the `edit_prediction_types` caveat below** |
| 197-200 | field initializers in `UserStore::new` |
| 231-282 | the `cloud_client().get_authenticated_user()` maintenance loop + `clear_plan_and_usage()` |
| 700-742 | `current_organization`, `set_current_organization`, `organizations`, `plan_for_organization`, `current_organization_configuration` |
| 743-807 | `plan()`, subscription-period/trial/too-young/overdue accessors |
| 809-830 | `edit_prediction_usage()`, `update_edit_prediction_usage()`, `clear_plan_and_usage()` |
| 832-894 | `fn update_authenticated_user(GetAuthenticatedUserResponse)` |
| 896-916 | `fn handle_message_to_client` |
| 1044-1090 | `impl RequestUsage` (header parsing, `UsageLimit`) + `impl EditPredictionUsage` |

`crates/client/src/test.rs` — lines 5-8 imports plus the `GetAuthenticatedUserResponse`/`PlanInfo`
fixture builders; the rest (`FakeServer`, proto plumbing) stays.

### MUST STAY (editor and everything else depends on these)

| Location | Item |
|---|---|
| `client.rs:58` | `pub use rpc::*;` — proto re-export |
| `client.rs:195-232` | `struct Client` (minus the two cloud fields) |
| `client.rs:268-320` | `enum Status` + `impl Status` |
| `client.rs:328-337` | `Credentials` |
| `client.rs:339-413` | `ClientCredentialsProvider` |
| `client.rs:424-454` | `Subscription` |
| `client.rs:455-515` | `PendingEntitySubscription<T>` |
| `client.rs:516-528` | `TelemetrySettings` |
| `client.rs:1766-1808` | `impl ProtoClient for Client` |
| `client.rs:1809-1822` | `pub enum ZedLink` |
| `client.rs:1824-…` | `pub fn parse_zed_link` |
| `user.rs:37-53` | `ChannelId`, `ProjectId` |
| `user.rs:55-56` | `ParticipantIndex` |
| `user.rs:58-64` | `User` |
| `user.rs:66-74`, `1031-1043` | `Collaborator` + `impl` |
| `user.rs:96-108` | `Contact`, `ContactRequestStatus` |
| `user.rs:110-133` | `UserStore` (contacts/users/participant-indices half) |
| `src/telemetry.rs`, `src/telemetry/event_coalescer.rs` | untouched (no cloud imports) |
| `src/proxy*.rs`, `src/zed_urls.rs` | untouched |

### CAVEAT: `EditPredictionUsage`

`crates/edit_prediction_types/src/edit_prediction_types.rs:3,167,215,269` is the ONLY surviving
consumer of `client::EditPredictionUsage`. Two options:

- **Preferred:** delete the `usage()` default method + trait method + handle impl in
  `edit_prediction_types` (3 sites, ~10 lines), then delete `EditPredictionUsage`/`RequestUsage`
  from `client`.
- Alternative: keep `EditPredictionUsage`/`RequestUsage` in `client` but drop the `UsageLimit`
  dependency (inline a 2-variant `UsageLimit` enum, ~12 lines). Not recommended — the type has no
  purpose once the cloud LLM path is gone.

### Classification
(a) whole feature (Cloud auth/plan/LLM-token) inside an otherwise-surviving crate.

### Patch size
Files: 4 edited (`client.rs` ~-190 lines, `user.rs` ~-330 lines, `test.rs` ~-40 lines, `Cargo.toml`) + 1 deleted (`llm_token.rs`, 116 lines). Total ~680 lines.

### Ordering
**Do this FIRST among the survivors.** `title_bar`, `edit_prediction_types`, and `cloud_api_types`
all key off what `client` still exposes. Anything done before `client` will need a second pass.

---

## 5. `cloud_api_types` — DELETE the crate instead

### Import
`crates/cloud_api_types/src/plan.rs:22` — `pub usage: cloud_llm_client::CurrentUsage,` (single site; no `use` line, fully qualified).
`crates/cloud_api_types/Cargo.toml:18` — `cloud_llm_client.workspace = true`.

`CurrentUsage` is trivially inlinable (`crates/cloud_llm_client/src/cloud_llm_client.rs:320-328`,
9 lines: `CurrentUsage { edit_predictions: UsageData }`, `UsageData { used: u32, limit: UsageLimit }`).

**But don't bother.** Every consumer of `cloud_api_types` is in the delete set or is being gutted:

| Consumer | Fate |
|---|---|
| `agent_ui`, `agent`, `ai_onboarding`, `cloud_api_client`, `collab`, `edit_prediction`, `language_models`, `web_search_providers`, `onboarding` | in delete set |
| `client` | §4 removes `OrganizationConfiguration` (`user.rs:8`) |
| `title_bar` | §2 deletes `plan_chip.rs`, the only user |
| `extension`, `extension_cli`, `extension_host`, `extensions_ui` | **verify** — these four survive; check what they pull |

Action: confirm the four `extension*` crates' usage; if it is only `Timestamp`/`KnownOrUnknown`,
inline those into `extension` and delete `cloud_api_types` entirely. Otherwise inline `CurrentUsage`
(9 lines) and keep the crate.

### Classification
(c) type needing a local replacement — or (a) delete the crate.

### Patch size
Inline path: 1 file, ~12 lines. Delete path: 1 crate + `extension*` fixups (unknown, likely small).

### Ordering
After `client` (§4).

---

## 6. `project` — `context_server` (recommend: keep `context_server`)

### Imports

| File:line | Import |
|---|---|
| `crates/project/src/project_settings.rs:3` | `use context_server::ContextServerCommand;` |
| `crates/project/src/context_server_store.rs:10` | `use context_server::oauth::{self, McpOAuthTokenProvider, OAuthDiscovery, OAuthSession};` |
| `crates/project/src/context_server_store.rs:11` | `use context_server::transport::{HttpTransport, TransportError};` |
| `crates/project/src/context_server_store.rs:12` | `use context_server::{ContextServer, ContextServerCommand, ContextServerId};` |
| `crates/project/src/context_server_store.rs:41` | `context_server,` (re-export inside a `use` group) |
| `crates/project/src/context_server_store/extension.rs`, `registry.rs` | same crate |
| `crates/project/Cargo.toml:47`, `:112` | dep + dev-dep |

### How embedded is `ContextServerStore` in `Project`? — Shallow. 11 sites total.

| File:line | Site |
|---|---|
| `project.rs:7` | `pub mod context_server_store;` |
| `project.rs:29` | `use context_server_store::ContextServerStore;` |
| `project.rs:235` | field `context_server_store: Entity<ContextServerStore>` |
| `project.rs:1151` | `context_server_store::init(cx);` inside `Project::init` |
| `project.rs:1184-1192` | `ContextServerStore::local(...)` in `Project::local` |
| `project.rs:1317` | struct literal field |
| `project.rs:1434` | `ContextServerStore::remote(...)` in `Project::remote` |
| `project.rs:1543` | struct literal field |
| `project.rs:1806-1808` | `ContextServerStore::local(...)` in the third constructor |
| `project.rs:1844` | struct literal field |
| `project.rs:2181-2183` | `pub fn context_server_store(&self) -> Entity<ContextServerStore>` |

No entry in the event loop, no `cx.subscribe`, no proto dispatch inside `project.rs` itself
(the store registers its own handlers at `context_server_store.rs:320`).

### The rest of the blast radius (if `context_server` really is deleted)

| File | Work |
|---|---|
| `crates/project/src/context_server_store.rs` (1,504 lines) + `context_server_store/extension.rs` (120) + `registry.rs` (84) | delete, 1,708 lines |
| `crates/project/src/project_settings.rs:3` | drop import |
| `crates/project/src/project_settings.rs:63,66` | drop `context_servers`, `context_server_timeout` fields |
| `crates/project/src/project_settings.rs:182-315` | delete `enum ContextServerSettings` + both `From`/`Into` impls + `impl ContextServerSettings` (~133 lines) |
| `crates/project/src/project_settings.rs:660-666` | drop the two initializers |
| `crates/project/tests/integration/context_server_store.rs` | delete test file |
| `crates/project/tests/integration/project_tests.rs` | drop the context-server test refs |
| `crates/remote_server/src/headless_project.rs:20,61,247-251,289,353` | delete the store from the headless project (**not in the survivor list — flag to the lead**) |
| `crates/settings_content/src/project.rs:365` | `ContextServerSettingsContent` becomes orphaned (settings schema; harmless if left) |
| `crates/extension_host/src/wasm_host/wit/since_v0_8_0.rs` | `ContextServerSettings` bridging — needs a look |

### Classification
(a) whole feature to delete — but only if you accept losing MCP entirely and touching `remote_server`.

### Patch size
Keep-`context_server` path: **0 lines.**
Delete path: ~1,900 lines across 8 files + 1 crate not currently in the survivor list.

### Ordering
Independent, but resolve the keep/delete decision before planning `remote_server`.

---

## 7. `editor` — `edit_prediction_types` (recommend: keep `edit_prediction_types`)

### Imports

| File:line | Import |
|---|---|
| `crates/editor/src/editor.rs:64` | `pub use edit_prediction_types::Direction;` |
| `crates/editor/src/editor.rs:106-109` | `use edit_prediction_types::{EditPredictionDelegate, EditPredictionDelegateHandle, EditPredictionDiscardReason, EditPredictionGranularity, SuggestionDisplayType};` |
| `crates/editor/src/editor.rs:8917,8923` | `edit_prediction_types::EditPrediction::{Local, Jump}` match arms |
| `crates/editor/src/editor.rs:10543,10546` | `edit_prediction_types::EditPredictionIconSet` |
| `crates/editor/src/edit_prediction_tests.rs:1` + 14 sites | test-only |
| `crates/editor/src/editor_tests.rs:10308` | test-only |
| `crates/editor/Cargo.toml:54` | dep |

### Is inline-prediction woven into the element/display layer? Yes.

| File | `edit_prediction`/`EditPrediction` reference count |
|---|---|
| `crates/editor/src/editor.rs` | **429** |
| `crates/editor/src/element.rs` | 33 |
| `crates/editor/src/display_map.rs` | 5 |

`editor.rs` carries `EditPredictionState`, `RegisteredEditPredictionDelegate`, inlay-id tracking,
the cursor popover (`render_edit_prediction_cursor_popover`, ~10530+), callout-popover colors,
`in_leading_whitespace` bookkeeping, and the `Local`/`Jump` dispatch at `:8913-8935`.
Removing the feature is a multi-thousand-line refactor of the editor core with high regression risk.

### The whole reason it's in the delete set

`crates/edit_prediction_types/src/edit_prediction_types.rs:3` — `use client::EditPredictionUsage;`
used at `:167` (default impl), `:215` (trait method), `:269` (handle impl). That is the crate's
*only* tie to the auth/cloud graph. Its other deps are `gpui`, `icons`, `language`, `text`.

### Recommended patch (in `edit_prediction_types`, not `editor`)

Delete lines 3, 165-168, 215, 267-271 (~10 lines). `editor` then needs **zero changes** — nothing in
`editor` calls `usage()`. After the providers (`edit_prediction`, `copilot`) are deleted, the
`EditPredictionDelegate` trait simply has no implementors; the editor's prediction UI stays
compiled and inert.

### Classification
(c) a type needing a local fix — in the *dependency*, not in `editor`.

### Patch size
Recommended: 1 file, ~10 lines, in `crates/edit_prediction_types/`. `editor`: 0.
If `edit_prediction_types` is deleted anyway: 3+ files in `editor`, **1,000-2,500 lines**, highest-risk item in the whole fork.

### Ordering
Must be settled with `client` (§4) since the two decisions are the same decision.

---

## 8. `notifications` — gut one file, confirmed

### Imports
`crates/notifications/src/notification_store.rs:2` — `use channel::ChannelStore;`

### Call sites

| Line | Site |
|---|---|
| 24 | field `channel_store: Entity<ChannelStore>` |
| 95 | `channel_store: ChannelStore::global(cx)` |
| 365-368 | `Notification::ChannelInvitation { channel_id, .. } => self.channel_store.update(…respond_to_channel_invite…)` |

### Is gutting `notification_store.rs` enough? YES — confirmed.

`crates/notifications/src/notifications.rs` is 4 lines:
```
mod notification_store;
pub use notification_store::*;
pub mod status_toast;
```
`crates/notifications/src/status_toast.rs` (257 lines) imports only
`std::rc::Rc`, `gpui`, `ui`, `workspace`, `zed_actions::toast` — **no `channel`, no `client`, no `rpc`.**

`NotificationStore` consumers outside the crate: `crates/collab_ui/src/collab_panel.rs` (delete set),
`crates/zed/src/zed.rs:5373` and `crates/zed/src/main.rs:729` (`notifications::init(...)`, owned by
the zed agent), `crates/collab_ui/src/collab_ui.rs:24`. Nothing surviving needs it.

`status_toast` has 13 surviving consumers (`git_ui`, `project_panel`, `workspace`, `keymap_editor`,
`onboarding`, `debugger_ui`, `component_preview`, …), so the crate must survive.

### Recommended patch
Delete `crates/notifications/src/notification_store.rs` (428 lines); reduce `notifications.rs` to
`pub mod status_toast;`. `Cargo.toml`: drop `channel`, `client`, `db`, `rpc`, `sum_tree`, `time`,
`anyhow`, `util` (verify each against `status_toast.rs`) and the whole `test-support` feature +
dev-deps.

### Classification
(a) whole feature to delete, cleanly file-scoped.

### Patch size
2 files edited/deleted + `Cargo.toml`. ~440 lines. **5-minute delete.**

### Ordering
Independent. Must precede/accompany `zed`'s `notifications::init` removal (coordinate with the zed agent).

---

## 9. `workspace` — `agent_settings`

### Imports
| File:line | Import |
|---|---|
| `crates/workspace/src/welcome.rs:7` | `use agent_settings::AgentSettings;` |
| `crates/workspace/src/multi_workspace.rs:22` | `use agent_settings::AgentSettings;` |
| `crates/workspace/Cargo.toml:30` | dep |

### Is `agent_settings` "just a settings struct"? Almost — but not entirely.

`crates/agent_settings/src/agent_settings.rs` is 1,522 lines and does carry shared types
(`LanguageModelSelection`, `ToolPermissions`, `NewThreadLocation`, `AgentProfile`), and it
depends on `language_model` and `project`. But **`workspace` touches exactly two members:**

| Site | Symbol | Class |
|---|---|---|
| `multi_workspace.rs:69` | `AgentSettings::get_global(cx).sidebar_side` (a `settings::SidebarDockPosition`, defined at `agent_settings.rs:142`) | (c) needs a home for the setting |
| `welcome.rs:427` | `AgentSettings::get_global(cx).enabled(cx)` → `ai_enabled` | (b) small conditional |

Good news: `SidebarSide` and `SidebarDockPosition` come from the `settings` crate
(`multi_workspace.rs:13` `pub use settings::SidebarSide;`, `:23` `use settings::SidebarDockPosition;`),
not from `agent_settings`. Only the *value* lives on `AgentSettings`.

Replacement for `enabled(cx)`: `project::DisableAiSettings` (`crates/project/src/project.rs:1087`) —
`AgentSettings::enabled` is literally `self.enabled && !DisableAiSettings::get_global(cx).disable_ai`.

For `sidebar_side`: if `sidebar` is deleted (§1), `sidebar_side_context_menu`
(`multi_workspace.rs:65-100`) and much of the `WorkspaceSidebar` plumbing becomes dead — delete it
rather than relocating the setting.

### Patch size
2 files + `Cargo.toml`. ~10 lines if `sidebar` is deleted; ~40 if the sidebar plumbing is kept and
the setting is relocated to `settings_content`/`workspace`.

### Ordering
After the `sidebar` decision (§1).

---

## 10. `diagnostics` — `agent_settings`

| File:line | Item | Class |
|---|---|---|
| `crates/diagnostics/src/toolbar_controls.rs:2` | `use agent_settings::AgentSettings;` | — |
| `crates/diagnostics/src/toolbar_controls.rs:51` | `let is_agent_enabled = AgentSettings::get_global(cx).enabled(cx);` | (b) small conditional |
| `crates/diagnostics/Cargo.toml:16` | dep | — |

`is_agent_enabled` gates one button in the toolbar's `render`. Delete the binding and the branch that
reads it.

### Patch size
1 file, ~5 lines + `Cargo.toml`. **5-minute delete.**

### Ordering
Independent.

---

## 11. `activity_indicator` — `auto_update`

| File:line | Item | Class |
|---|---|---|
| `crates/activity_indicator/src/activity_indicator.rs:1` | `use auto_update::DismissMessage;` | — |
| `:297` | `fn dismiss_message(&mut self, _: &DismissMessage, …)` | (d) |
| `:525`, `:554` | `this.dismiss_message(&DismissMessage, window, cx)` | (d) |

`DismissMessage` is only an action, declared at `crates/auto_update/src/auto_update.rs:97` inside an
`actions!` macro. Two choices:
- move the action into `zed_actions` (or declare it locally in `activity_indicator`), keeping the
  dismiss-a-status-message behaviour — 4 lines; or
- change `dismiss_message`'s signature to take no action and drop the `on_action` registration.

Nothing else in the file touches `auto_update`.

### Patch size
1 file, ~6 lines + `Cargo.toml` (drop `auto_update`). **5-minute delete.**

### Ordering
Independent.

---

## 12. `file_finder` — `channel`

| File:line | Item |
|---|---|
| `crates/file_finder/src/file_finder.rs:7` | `use channel::ChannelStore;` |
| `:398` | field `channel_store: Option<Entity<ChannelStore>>` |
| `:465-466` | `Match::Channel { channel_id: ChannelId, channel_name: SharedString }` variant |
| `:672` | comparison comment/branch for channel matches |
| `:834-843` | `if FileFinderSettings::get_global(cx).include_channels { ChannelStore::try_global(cx) }` + field init |
| `:976-1033` | the channel-matching block inside the search (~58 lines) |
| `:1154-1158` | rendering a channel row |
| `:1548-1551` | `Match::Channel` → `OpenChannelNotesById` dispatch |
| `crates/file_finder/Cargo.toml` | `channel` dep; dev-dep `remote_connection` (keep if `remote_connection` survives) |

Also drop `FileFinderSettings::include_channels` (locate in `crates/file_finder/src/file_finder.rs`
settings block and `settings_content`).

### Classification
(a) a self-contained feature ("open channel notes from the file finder").

### Patch size
1 file, ~110 lines removed + `Cargo.toml` + a settings field. Straightforward but not 5 minutes —
the `Match` enum is matched in several places, so expect a compile-driven cleanup pass.

### Ordering
Independent.

---

## 13. `language_tools` — `edit_prediction`

| File:line | Item | Class |
|---|---|---|
| `crates/language_tools/src/lsp_log_view.rs:2` | `use edit_prediction::EditPredictionStore;` | — |
| `crates/language_tools/src/lsp_log_view.rs:346` | `let copilot = EditPredictionStore::try_global(cx)…` | (a) delete the branch that surfaces Copilot's LSP log |
| `crates/language_tools/src/lsp_button.rs:1309` | `all_language_settings(None, cx).edit_predictions.provider` — this is `language::language_settings`, **not** the deleted crate | keep |
| `crates/language_tools/src/lsp_log_view.rs:1305` | `editor.set_show_edit_predictions(Some(false), …)` — `editor` API | keep |

Only one real site: the Copilot-LSP-log entry in the LSP log picker.

### Patch size
1 file, ~15 lines + `Cargo.toml`. **5-minute delete.**

### Ordering
Independent.

---

## 14. `recent_projects` — `remote_connection` (recommend: no change)

| File:line | Import |
|---|---|
| `crates/recent_projects/src/recent_projects.rs:21` | `pub use remote_connection::{RemoteConnectionModal, connect};` |
| `crates/recent_projects/src/remote_connections.rs:26-29` | `pub use remote_connection::{RemoteClientDelegate, RemoteConnectionModal, RemoteConnectionPrompt, SshConnectionHeader, connect};` |
| `crates/recent_projects/Cargo.toml:17,42,71` | feature, dep, dev-dep |

Both are pure re-exports. `recent_projects` is 7,704 lines across 8 files
(`remote_servers.rs` 3,025, `recent_projects.rs` 2,439, `remote_connections.rs` 897,
`disconnected_overlay.rs` 214, `ssh_config.rs` 217, `wsl_picker.rs` 292, …) and remote/SSH/WSL
project management is its primary purpose.

**If `remote_connection` is kept (recommended): 0 lines.**
If it is deleted: `recent_projects` loses roughly `remote_connections.rs` + `remote_servers.rs` +
`disconnected_overlay.rs` + `wsl_picker.rs` ≈ **4,400 lines**, and `RecentProjects` degrades to a
local-only picker. That is a much larger job than `remote_connection`'s own 728 lines.

### Recommended patch to `remote_connection` instead
`crates/remote_connection/src/remote_connection.rs:5` `use auto_update::AutoUpdater;` with two call
sites — `:484 AutoUpdater::download_remote_server_release(...)` and
`:515 AutoUpdater::get_remote_server_release_url(...)`, both inside `impl RemoteClientDelegate`.
Replace with a direct GitHub-release HTTP fetch, or `bail!("remote server auto-download disabled")`
and require a pre-installed binary. ~30 lines, 1 file.

### Classification
(c) / (b) — one dependency to sever in the dependency crate.

### Ordering
Decide before `git_ui` (§3), `title_bar` (§2), and the dev-dep crates (§16).

---

## 15. `settings_content` — `language_model_core`

### Imports (note: `pub use agent::*` at `settings_content.rs:17` and `pub use language_model::*` at `:22` are LOCAL modules — false positives)

| File:line | Item | Symbol |
|---|---|---|
| `crates/settings_content/src/language_model.rs:240` | `pub use language_model_core::ReasoningEffort as OpenAiReasoningEffort;` | 5-variant fieldless enum, `language_model_core.rs:465-476` |
| `crates/settings_content/src/language_model.rs:482` | `pub use language_model_core::ModelMode;` | 2-variant enum, `language_model_core.rs:453-459` |
| `crates/settings_content/src/merge_from.rs:59` | `language_model_core::Speed,` inside a `MergeFrom` impl list | 2-variant enum, `request.rs:340-353` |
| `crates/settings_content/src/agent.rs:428` | `pub speed: Option<language_model_core::Speed>,` | same |
| `crates/settings_content/Cargo.toml:22` | dep | — |

All three are tiny, dependency-free enums. `ReasoningEffort`'s own doc comment
(`language_model_core.rs:461-464`) says it "Mirrors the shape of `settings_content::OpenAiReasoningEffort`"
— i.e. it was hoisted *out of* settings_content in the first place. Move it back.

### Recommended patch
Inline into `crates/settings_content/src/language_model.rs`:

```rust
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, strum::EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum OpenAiReasoningEffort { Minimal, Low, Medium, High, XHigh }

#[derive(Default, ...)]
pub enum ModelMode { #[default] Default, Thinking { budget_tokens: Option<u32> } }

#[derive(Default, ...)]
pub enum Speed { #[default] Standard, Fast }
```
plus `impl Speed { pub fn toggle(self) -> Self }` if any surviving caller needs it (verify: only
`agent_ui` uses `toggle`, which is in the delete set — likely droppable).

Then swap `language_model_core::Speed` → `crate::Speed` at `merge_from.rs:59` and `agent.rs:428`.

### Classification
(c) types needing a local replacement — trivially so.

### Patch size
3 files, ~35 lines added / 4 removed + `Cargo.toml`. **5-minute delete.**

### Ordering
Independent — but it unblocks `settings_ui` (other agent's crate). Do it early.

---

## 16. Dev-dependency-only: `git_graph`, `project_panel`

| Crate | Site |
|---|---|
| `git_graph` | `crates/git_graph/Cargo.toml:19` (`test-support = [… "remote_connection/test-support" …]`), `:50` dev-dep. **No `.rs` references at all.** |
| `project_panel` | `crates/project_panel/Cargo.toml:60` dev-dep. **No `.rs` references at all.** |

If `remote_connection` is kept (recommended): 0 changes.
If deleted: remove those 3 Cargo lines. No source edits either way.

### Classification
(d) trivially removable.

### Patch size
2 `Cargo.toml` files, 3 lines. **5-minute delete.**

---

## RECOMMENDED FIX ORDER

Ordered so no file is touched twice. Phase boundaries are hard dependencies.

### Phase 0 — Revise the delete set (decision, not code)
1. Move `edit_prediction_types` OUT of the delete set → patch it (§7).
2. Move `remote_connection` OUT of the delete set → patch it (§14).
3. Move `context_server` OUT of the delete set → no patch needed (§6).
4. Move `sidebar` INTO the delete set (§1).
5. Decide `cloud_api_types`: inline `CurrentUsage` vs. delete the crate (needs an `extension*` check) (§5).

Everything downstream depends on these five calls. Getting them wrong costs ~5,000 extra lines of churn.

### Phase 1 — Independent leaves (parallelizable, no cross-dependencies)
| # | Crate | Size |
|---|---|---|
| 1a | `settings_content` (§15) | 5-min |
| 1b | `diagnostics` (§10) | 5-min |
| 1c | `activity_indicator` (§11) | 5-min |
| 1d | `language_tools` (§13) | 5-min |
| 1e | `git_graph` + `project_panel` Cargo (§16) | 5-min |
| 1f | `notifications` (§8) | 5-min, coordinate with the `zed` agent on `notifications::init` |
| 1g | `file_finder` (§12) | ~1h |

### Phase 2 — Dependency-crate patches (must precede their consumers)
| # | Crate | Depends on |
|---|---|---|
| 2a | `remote_connection` — sever `auto_update` (§14) | Phase 0 |
| 2b | `edit_prediction_types` — sever `client::EditPredictionUsage` (§7) | Phase 0 |

### Phase 3 — `client` (the auth gutting) — SINGLE-THREADED, do alone
| # | Crate | Notes |
|---|---|---|
| 3a | `client` (§4) | Must come after 2b (which removes the last surviving consumer of `EditPredictionUsage`). Everything in Phase 4 reads `client`'s post-gutting API. |

### Phase 4 — Consumers of the gutted `client`
| # | Crate | Depends on |
|---|---|---|
| 4a | `cloud_api_types` inline-or-delete (§5) | 3a |
| 4b | `title_bar` (§2) | 3a (UserStore::plan), 2a (remote_connection), Phase 0 |

### Phase 5 — Sidebar removal and its fallout
| # | Crate | Depends on |
|---|---|---|
| 5a | delete crate `sidebar` (§1) | Phase 0 |
| 5b | `workspace` (§9) | 5a — the `sidebar_side` decision follows from whether the sidebar exists |
| 5c | `git_ui` (§3) | 2a; independent of 5a but touches `git_ui.rs`, so batch with any other `git_ui` work |

### Not applicable
- `editor` (§7) — zero changes under the recommended plan.
- `project` (§6) — zero changes under the recommended plan.
- `recent_projects` (§14) — zero changes under the recommended plan.

---

## 5-MINUTE DELETES vs. NEEDS REAL THOUGHT

### 5-minute deletes
| Crate | Lines | Files |
|---|---|---|
| `settings_content` | ~40 | 4 |
| `diagnostics` | ~5 | 2 |
| `activity_indicator` | ~6 | 2 |
| `language_tools` | ~15 | 2 |
| `git_graph` + `project_panel` | 3 | 2 |
| `notifications` | ~440 (one whole file, no thinking) | 3 |
| `edit_prediction_types` (patch) | ~10 | 1 |
| `cloud_api_types` (inline path) | ~12 | 2 |

### Needs real thought
| Crate | Why |
|---|---|
| **`client`** | The riskiest survivor. `UserStore` mixes cloud plan/usage state with contacts, `ParticipantIndex`, `Collaborator`, and `User` — all of which `editor`, `project`, and `workspace` need. Requires a line-by-line split of `user.rs`, not a file delete. Sequence it alone. |
| **`sidebar`** | Not a patch — a delete decision that ripples into `workspace`'s `WorkspaceSidebar`/`MultiWorkspace` plumbing and `zed`'s registration. |
| **`title_bar`** | 1,000+ lines across 4 files, and it straddles three separate deletions (call/collab, auto-update, plan chip). Easy to leave dangling struct fields. |
| **`git_ui`** | Three unrelated features to excise from a 5,700-line `git_panel.rs`, plus the collab co-author path that threads through `Render`. |
| **`file_finder`** | The `Match::Channel` variant is matched in five places; a compile-driven cleanup, not a single-block delete. |
| **`workspace`** | Only 2 lines of `agent_settings`, but the `sidebar_side` setting needs a home (or the sidebar plumbing needs deleting) — a design call, not a mechanical edit. |
| **`remote_connection`** (if you keep it) | Replacing `AutoUpdater::download_remote_server_release` means deciding how the remote-server binary reaches the host. Small patch, real product decision. |
| **`editor`** / **`project`** / **`recent_projects`** | Only if Phase 0 is rejected. Then each becomes the single largest item in the fork (~2,500 / ~1,900 / ~4,400 lines). |

---

## Open questions for the lead

1. `crates/remote_server/src/headless_project.rs` uses `context_server_store` (lines 20, 61, 247-251, 289, 353) but is not in the survivor list. Is `remote_server` surviving? If yes and `context_server` is deleted, add it to the work breakdown.
2. `extension`, `extension_cli`, `extension_host`, `extensions_ui` all depend on `cloud_api_types` and are not in the survivor list. Confirm what they import before deciding to delete that crate.
3. If the remote-server binary can no longer be auto-downloaded (§14), what is the intended install path for remote projects?
4. `AgentSettings::sidebar_side` — is the workspace sidebar concept surviving at all, or does it go with `sidebar`?
