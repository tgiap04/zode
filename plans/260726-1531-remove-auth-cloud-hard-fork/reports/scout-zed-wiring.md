# Scout report — `crates/zed` wiring into the delete set

Work context: `/Users/tgiap.dev/devs/zode`
Scope: `crates/zed/**` only. Adjacent crates noted where they leak into this crate's compile surface.

Files in scope (LOC):

| File | LOC |
|---|---|
| `crates/zed/src/main.rs` | 1940 |
| `crates/zed/src/zed.rs` | 6587 |
| `crates/zed/src/reliability.rs` | 499 |
| `crates/zed/src/visual_test_runner.rs` | 3589 |
| `crates/zed/src/zed/app_menus.rs` | 333 |
| `crates/zed/src/zed/edit_prediction_registry.rs` | 416 |
| `crates/zed/src/zed/open_listener.rs` | 2261 |
| `crates/zed/src/zed/quick_action_bar.rs` | 766 |
| `crates/zed/src/zed/telemetry_log.rs` | 621 |
| `crates/zed/src/zed/visual_tests.rs` | 551 |
| `crates/zed/src/zed/{migrate,open_url_modal,remote_debug,mac_only_instance,windows_only_instance}.rs` | clean |
| `crates/zed/src/zed/quick_action_bar/{preview,repl_menu}.rs` | clean |

Legend for removal notes: **DELETE** = remove the line/block outright · **GUT** = keep the symbol but strip the auth/cloud half · **KEEP** = untouched, listed only because it sits in a churned region.

---

## 1. `crates/zed/src/main.rs`

### 1a. `use` statements

| file:line | what | removal note |
|---|---|---|
| `main.rs:7` | `use agent::{SharedThread, ThreadStore};` | DELETE |
| `main.rs:8` | `use agent_client_protocol::schema as acp;` | DELETE (crates.io dep `agent-client-protocol`, only used by `SharedAgentThread`) |
| `main.rs:9` | `use agent_ui::AgentPanel;` | DELETE |
| `main.rs:13` | `use client::{Client, ProxySettings, RefreshLlmTokenListener, UserStore, parse_zed_link};` | GUT → keep `Client`, `ProxySettings`, `UserStore`; drop `RefreshLlmTokenListener`; `parse_zed_link` only feeds `zed://channel*` (see 1e) so drop it too |
| `main.rs:14` | `use collab_ui::channel_view::ChannelView;` | DELETE |
| `main.rs:16` | `use crashes::InitCrashHandler;` | DELETE |
| `main.rs:41` | `use proto;` | KEEP (proto stays in `client`) — but its only use here is `proto::GetSharedAgentThread` at :985, so the import becomes dead → DELETE |
| `main.rs:64-68` | `use zed::{... edit_prediction_registry ...}` | GUT → drop `edit_prediction_registry` from the list |

### 1b. Pre-`app.run` / process-mode wiring

| file:line | what | removal note |
|---|---|---|
| `main.rs:196-200` | `--crash-handler` arm → `crashes::crash_server(socket)` | DELETE (with the `Args::crash_handler` field, see 1f) |
| `main.rs:341-361` | `crashes::init(InitCrashHandler{ session_id, zed_version, binary, release_channel, commit_sha }, |task| bg.spawn(task).detach())` | DELETE. `session_id` (`main.rs:335`) is still needed by `Session::new` at :336, so keep the variable. |

### 1c. Init calls inside `app.run` (ordered as they appear)

| file:line | init call | removal note |
|---|---|---|
| `main.rs:495` | `let client = Client::production(cx);` | GUT — must survive (proto/rpc + http_client), auth path stripped |
| `main.rs:531` | `UserStore::new(client.clone(), cx)` | GUT (UserStore is the auth identity holder) |
| `main.rs:553` | `Client::set_global(client.clone(), cx)` | KEEP |
| `main.rs:556` | `project::Project::init(&client, cx)` | KEEP |
| `main.rs:559` | `client::init(&client, cx)` | GUT (registers sign-in/out actions in `client`) |
| `main.rs:566-572` | `let telemetry = client.telemetry(); telemetry.start(system_id, installation_id, session_id, cx)` | GUT → no-op start |
| `main.rs:573-585` | `cx.subscribe(&user_store, …)` on `client::user::Event::PrivateUserInfoUpdated` → `crashes::set_user_info(crashes::UserInfo{ metrics_id, is_staff })` | DELETE whole subscription |
| `main.rs:590-603` | `telemetry::event!("App First Opened" / "…For Release Channel" / "App Opened")` | GUT → macro becomes no-op; the `IdType` match can stay or be dropped. `is_new_install` (:587) is consumed by `agent_ui::init` only → becomes dead. |
| `main.rs:618` | `auto_update::init(client.clone(), cx)` | DELETE |
| `main.rs:620` | `auto_update_ui::init(cx)` | DELETE |
| `main.rs:621` | `reliability::init(client.clone(), cx)` | GUT (see §3) |
| `main.rs:622-628` | `extension_host::init(proxy, fs, app_state.client.clone(), node_runtime, cx)` | KEEP — still needs `Arc<Client>` |
| `main.rs:638-644` | `let copilot_chat_configuration = copilot_chat::CopilotChatConfiguration{ enterprise_uri: …edit_predictions.copilot.enterprise_uri }` | DELETE (also depends on `language::language_settings.edit_predictions.copilot`, a field that lives in `language` — verify it survives) |
| `main.rs:645-650` | `copilot_chat::init(fs, client.http_client(), config, cx)` | DELETE |
| `main.rs:652` | `copilot_ui::init(&app_state, cx)` | DELETE |
| `main.rs:653` | `language_model::init(cx)` | DELETE |
| `main.rs:654-658` | `RefreshLlmTokenListener::register(client, user_store, cx)` | DELETE |
| `main.rs:659` | `language_models::init(user_store, client, cx)` | DELETE |
| `main.rs:660` | `acp_tools::init(cx)` | DELETE |
| `main.rs:661` | `zed::telemetry_log::init(cx)` | GUT or DELETE — see §4, depends on `client::telemetry::Telemetry` |
| `main.rs:662` | `zed::remote_debug::init(cx)` | KEEP (uses `remote`, not `remote_connection`) |
| `main.rs:663` | `edit_prediction_ui::init(cx)` | DELETE |
| `main.rs:664` | `web_search::init(cx)` | DELETE |
| `main.rs:665` | `web_search_providers::init(client, user_store, cx)` | DELETE |
| `main.rs:667` | `edit_prediction_registry::init(client, user_store, cx)` | DELETE (whole module, see §4) |
| `main.rs:668` | `let prompt_builder = PromptBuilder::load(fs, stdout_is_a_pty(), cx)` | `prompt_store` is NOT in the delete set, but its only consumer here is `agent_ui::init` → becomes dead. DELETE the local. |
| `main.rs:669-673` | `project::AgentRegistryStore::init_global(cx, fs, client.http_client())` | KEEP-VERIFY — lives in `project` (kept), but confirm `project` still compiles with `agent_servers`/`acp` gone |
| `main.rs:674-681` | `agent_ui::init(fs, prompt_builder, languages, is_new_install, false, cx)` | DELETE |
| `main.rs:707` | `channel::init(&client, user_store, cx)` | DELETE |
| `main.rs:728` | `call::init(client, user_store, cx)` | DELETE |
| `main.rs:729` | `notifications::init(client, user_store, cx)` | DELETE the call (this is `notification_store` init; `status_toast` needs no init) |
| `main.rs:730` | `collab_ui::init(&app_state, cx)` | DELETE |
| `main.rs:741` | `edit_prediction::init(cx)` | DELETE |
| `main.rs:694` | `audio::init(cx)` | KEEP-VERIFY — `audio` survives, but it was pulled in for `call`/`livekit_client`; check nothing else breaks |
| `main.rs:899` | `component_preview::init(app_state.clone(), cx)` | KEEP — `component_preview/Cargo.toml` has no delete-set deps |

### 1d. Post-init closures / spawns

| file:line | what | removal note |
|---|---|---|
| `main.rs:749-783` | `cx.observe_global::<SettingsStore>` closure capturing `http` + `client`; body reads `client::ClientSettings::get_global(cx).server_url` (:774) and calls `client.reconnect(&cx.to_async())` (:778) | GUT — keep the theme/text-rendering half (:753-772), drop the server-url/reconnect half (:774-780) and the two captures at :750-751 |
| `main.rs:792-801` | two `telemetry::event!("Settings Changed", …)` | GUT (no-op macro) |
| `main.rs:802` | `telemetry.flush_events().detach()` | GUT |
| `main.rs:810-811` | `let menus = app_menus(cx); cx.set_menus(menus)` | KEEP — but `app_menus.rs` must be fixed first (§4) |
| `main.rs:816-820` | `cx.spawn({ let client = app_state.client.clone(); async move |cx| authenticate(client, cx).await }).detach_and_log_err(cx)` | DELETE entire spawn |

### 1e. `handle_open_request` (main.rs:914-1309)

| file:line | what | removal note |
|---|---|---|
| `main.rs:937-959` | `OpenRequestKind::AgentPanel { external_source_prompt }` arm — focuses `AgentPanel`, calls `new_agent_thread_with_external_source_prompt` | DELETE |
| `main.rs:960-1040` | `OpenRequestKind::SharedAgentThread { session_id }` arm — `workspace.panel::<AgentPanel>()`, `proto::GetSharedAgentThread`, `SharedThread::from_bytes`, `thread_store.save_thread`, `panel.open_thread`, toast | DELETE (largest single block) |
| `main.rs:1206-1220` | `if let Some(connection_options) = request.remote_connection { open_remote_project(...) }` | KEEP — `RemoteConnectionOptions` comes from `remote` (kept), not `remote_connection` |
| `main.rs:1250-1300` | `if !request.open_channel_notes.is_empty() || request.join_channel.is_some()` block: `authenticate(client, cx)` (:1259), `workspace::join_channel(client::ChannelId(...))` (:1263-1264), `ChannelView::open(client::ChannelId(...))` (:1282-1283) | DELETE the whole `if` arm; collapse to the `else if let Some(task) = task` branch at :1301 |
| `main.rs:1311-1323` | `async fn authenticate(client: Arc<Client>, cx: &AsyncApp)` — `client::IMPERSONATE_LOGIN`, `client.has_credentials`, `client.sign_in_with_optional_connect` | DELETE the whole function (only two callers: :818 and :1259, both deleted) |

Also: `main.rs:1741` `parse_zed_link(arg, cx).is_some()` inside `parse_url_arg` — this is the `zed.dev/channel/...` URL sniffer. GUT: drop the `parse_zed_link` arm, keep the `file://` / `zed://` / `zed-cli://` / `ssh://` prefixes.

### 1f. Env vars and CLI args

| file:line | what | removal note |
|---|---|---|
| `main.rs:1595-1599` | `FORCE_CLI_MODE` from `cli::FORCE_CLI_MODE_ENV_VAR_NAME` | KEEP (`cli` crate survives) |
| `main.rs:1313` | `client::IMPERSONATE_LOGIN` (env `ZED_IMPERSONATE`) | DELETE with `authenticate()` |
| `main.rs:365-366` | `*zed_env_vars::ZED_STATELESS` | KEEP |
| `main.rs:1667-1670` | `Args::crash_handler: Option<PathBuf>` (`--crash-handler`) | DELETE — hidden flag, no external callers besides `crashes` re-exec |
| `main.rs:1650-1652` | `Args::dev_server_token` (`--dev-server-token`) | Already a no-op ("not implemented"). Cheap to DELETE in the same pass — it is collab-era vocabulary. |
| `main.rs:1684-1689` | `Args::askpass` | KEEP (`askpass` + `remote` SSH flow) |

No other env-var handling in `main.rs` touches the delete set.

---

## 2. `crates/zed/src/zed.rs`

| file:line | what | removal note |
|---|---|---|
| `zed.rs:2` | `pub mod edit_prediction_registry;` | DELETE (module file deleted) |
| `zed.rs:11-12` | `#[cfg(all(target_os="macos", feature="visual-tests"))] pub mod visual_tests;` | DELETE (feature goes away, §5/§6) |
| `zed.rs:16` | `use agent_ui::AgentDiffToolbar;` | DELETE |
| `zed.rs:22` | `use client::zed_urls;` | DELETE (only use is `account_url`) |
| `zed.rs:99` | `use zed_actions::{About, OpenAccountSettings, …}` | GUT → drop `OpenAccountSettings` from the import list |
| `zed.rs:219-223` | `.on_action(|_: &OpenAccountSettings, cx| { cx.open_url(&zed_urls::account_url(cx)) })` inside `zed::init` | DELETE. Also remove `OpenAccountSettings` from `crates/zed_actions/src/*.rs:59` (out of this crate, but the action becomes unhandled otherwise). |
| `zed.rs:438-451` | inside the `MultiWorkspaceEvent::ActiveWorkspaceChanged` subscription: `workspace.panel::<agent_ui::AgentPanel>()` → `initialize_from_source_workspace_if_needed` | DELETE |
| `zed.rs:452-453` | `ensure_agent_panel_for_workspace(workspace, source_workspace, window, cx).detach_and_log_err(cx)` | DELETE |
| `zed.rs:499-503` | `if let Some(specs) = window.gpu_specs() { … crashes::set_gpu_info(specs) }` | GUT → keep `log::info!` + `show_software_emulation_warning_if_needed`, drop `crashes::set_gpu_info` |
| `zed.rs:505-520` | `edit_prediction_menu_handle` + `edit_prediction_ui::EditPredictionButton::new(fs, user_store, handle, project, cx)` + `register_action(edit_prediction_ui::ToggleMenu)` | DELETE |
| `zed.rs:562` | `status_bar.add_right_item(edit_prediction_ui, window, cx)` | DELETE |
| `zed.rs:701-702` | `let channels_panel = collab_ui::collab_panel::CollabPanel::load(...)` | DELETE |
| `zed.rs:724` | `add_panel_when_ready(channels_panel, …)` inside `futures::join!` | DELETE |
| `zed.rs:727` | `initialize_agent_panel(workspace_handle, cx.clone()).map(|r| r.log_err())` inside the same `join!` | DELETE |
| `zed.rs:733-766` | `fn setup_or_teardown_ai_panel<P: Panel>(…)` | DELETE — generic, but `agent_ui::AgentPanel` is its only instantiation |
| `zed.rs:768-791` | `fn ensure_agent_panel_for_workspace(…)` → `agent_ui::AgentPanel::load`, `workspace.panel::<agent_ui::AgentPanel>()` | DELETE |
| `zed.rs:793-824` | `async fn initialize_agent_panel(…)` → registers `AgentPanel::toggle_focus` / `focus` / `toggle` / `InlineAssistant::inline_assist` (:818-821) | DELETE |
| `zed.rs:1112-1118` | `.register_action(|workspace, _: &collab_ui::collab_panel::ToggleFocus, …| workspace.toggle_panel_focus::<CollabPanel>())` | DELETE |
| `zed.rs:1299-1300` | `let acp_tools_item = cx.new(|_| acp_tools::AcpToolsToolbarItemView::new()); toolbar.add_item(...)` | DELETE |
| `zed.rs:1301-1303` | `telemetry_log::TelemetryLogToolbarItemView::new(window, cx)` + `add_item` | GUT/DELETE with §4 telemetry_log decision |
| `zed.rs:1318-1319` | `let agent_diff_toolbar = cx.new(AgentDiffToolbar::new); toolbar.add_item(...)` | DELETE |
| `zed.rs:891`, `zed.rs:931` | `telemetry::event!("Project Opened")` | GUT (no-op macro) |
| `zed.rs:749`, `zed.rs:756`, `zed.rs:773` | `DisableAiSettings` reads inside `setup_or_teardown_ai_panel` | DELETE with that fn. `DisableAiSettings` itself lives in `project` — decide separately whether the setting stays. |

### Test-only wiring in `zed.rs`

| file:line | what | removal note |
|---|---|---|
| `zed.rs:5110-5150` | `expected_namespaces` vec in the action-namespace assertion test — contains `"agent"`, `"assistant"`, `"assistant2"`, `"auto_update"`, `"channel_modal"`, `"collab"`, `"collab_panel"`, `"context_server"`, `"copilot"`, `"edit_prediction"`, `"inline_assistant"`, `"agents_sidebar"` | GUT — prune these entries or the test fails loudly. Good canary: run it to catch leftover registered actions. |
| `zed.rs:4842` | `client::init(&app_state.client, cx)` in `init_keymap_test` | GUT |
| `zed.rs:5371-5412` | `init_test_with_state` mirrors the whole `main.rs` init block: `channel::init` (:5371), `call::init` (:5372), `notifications::init` (:5373), `collab_ui::init` (:5378), `copilot_chat::init` (:5383-5388), `language_model::init` (:5390), `client::RefreshLlmTokenListener::register` (:5391-5395), `language_models::init` (:5396), `web_search::init` (:5397), `web_search_providers::init` (:5399), `PromptBuilder::load` (:5400), `agent_ui::init` (:5406-5413) | DELETE all — apply exactly the same edits as `main.rs`, this is the twin |

---

## 3. `crates/zed/src/reliability.rs`

The file is **two unrelated subsystems glued together**. Split it.

### Worth keeping (generic, no cloud dependency)

| file:line | what | note |
|---|---|---|
| `reliability.rs:22`, `92-141` | `MAX_HANG_TRACES`, `monitor_hangs(cx)` — foreground-thread ping/pong hang detector | KEEP verbatim |
| `reliability.rs:143-162` | `cleanup_old_hang_traces()` | KEEP |
| `reliability.rs:164-220` | `save_hang_trace(...)` — writes `hang-*.miniprof.json` to `paths::hang_traces_dir()` | KEEP |

These depend only on `gpui`, `paths`, `chrono`, `serde_json`, `util`, `STARTUP_TIME`.

### Must be removed

| file:line | what | removal note |
|---|---|---|
| `reliability.rs:2` | `use client::{Client, telemetry::MINIDUMP_ENDPOINT};` | DELETE |
| `reliability.rs:6` | `use http_client::{AsyncBody, HttpClient, Request};` | DELETE (only for minidump upload) |
| `reliability.rs:9` | `use proto::{CrashReport, GetCrashFilesResponse};` | DELETE |
| `reliability.rs:10-13` | `use reqwest::{Method, multipart::{Form, Part}};` | DELETE |
| `reliability.rs:17` | `use sysinfo::{MemoryRefreshKind, RefreshKind, System};` | DELETE (only for build-timings telemetry) |
| `reliability.rs:24` | `pub fn init(client: Arc<Client>, cx: &mut App)` | GUT → `pub fn init(cx: &mut App)`; caller `main.rs:621` updates accordingly |
| `reliability.rs:31-43` | `cx.on_flags_ready(... if flags_ready.is_staff { upload_build_timings(client) })` | DELETE |
| `reliability.rs:45-51` | `if client.telemetry().diagnostics_enabled() { upload_previous_minidumps(client) }` | DELETE |
| `reliability.rs:53-89` | `cx.observe_new(|project: &mut Project|…)` — pulls crash files off the remote server via `proto::GetCrashFiles` and uploads them | DELETE. Note the paired server side lives in `remote_server` (deleted), so this is doubly dead. |
| `reliability.rs:222-261` | `pub async fn upload_previous_minidumps(client)` | DELETE |
| `reliability.rs:263-403` | `async fn upload_minidump(...)` — the entire Sentry multipart envelope (`sentry[tags][*]`, `sentry[user][id]`, `sentry[contexts][gpu*]`), `crashes::CrashInfo`, `::telemetry::event!("Minidump Uploaded")` at :316-321 | DELETE |
| `reliability.rs:405-478` | `struct BuildTiming` + `async fn upload_build_timings(_client)` — `telemetry::event!("Build Timing: Cargo Build", …)` | DELETE |
| `reliability.rs:480-499` | `trait FormExt` / `impl FormExt for Form` | DELETE (reqwest multipart helper, orphaned) |

Net result: `reliability.rs` shrinks from 499 → roughly 130 lines, imports reduce to `anyhow`, `gpui`, `chrono`, `serde_json`, `paths`, `util`, `std`, `futures`, `log`.

There is **no `sentry` crate dependency** — Sentry is reached purely as an HTTP minidump endpoint through `client::telemetry::MINIDUMP_ENDPOINT`. Nothing to remove from `Cargo.toml` for Sentry beyond `reqwest` (still used elsewhere? `reqwest.workspace = true` at `Cargo.toml:177` — after this deletion the only remaining use is `reqwest_client`, so `reqwest` becomes a removable direct dep; verify with `cargo machete`).

---

## 4. `crates/zed/src/zed/` subdirectory

### `zed/edit_prediction_registry.rs` (416 lines) — **DELETE THE WHOLE FILE**

Every meaningful line is in the delete set: `client::{Client, UserStore}` (:1), `codestral::{CodestralEditPredictionDelegate, load_codestral_api_key}` (:2), `copilot::CopilotEditPredictionDelegate` (:4), `edit_prediction::{EditPredictionModel, ZedEditPredictionDelegate}` (:5). Plus `client::user::Event::{PrivateUserInfoUpdated, OrganizationChanged}` (:66-67), `telemetry::event!` (:88), `copilot::Suggest` back-compat action alias (:225-232), and a `#[cfg(test)]` block (:329-402) that itself calls `client::init` / `language_model::init` / `RefreshLlmTokenListener::register`.

Removing the file forces two edits: `zed.rs:2` (`pub mod`) and `main.rs:66` + `main.rs:667`.

### `zed/app_menus.rs`

| file:line | what | removal note |
|---|---|---|
| `app_menus.rs:1` | `use collab_ui::collab_panel;` | DELETE |
| `app_menus.rs:45` | `MenuItem::action("Collab Panel", collab_panel::ToggleFocus)` | DELETE |
| `app_menus.rs:67` | `MenuItem::action("Check for Updates", auto_update::Check)` | DELETE (plus the following `MenuItem::separator()` at :68 becomes a leading separator — tidy it) |
| `app_menus.rs:299-302` | `MenuItem::action("View Release Notes Locally", auto_update_ui::ViewReleaseNotesLocally)` | DELETE |
| `app_menus.rs:303` | `MenuItem::action("View Telemetry", zed_actions::OpenTelemetryLog)` | Keep or drop with the §4 `telemetry_log` decision |

Everything else in the file is `zed_actions::*` / kept crates.

### `zed/quick_action_bar.rs`

| file:line | what | removal note |
|---|---|---|
| `quick_action_bar.rs:4` | `use agent_settings::AgentSettings;` | DELETE |
| `quick_action_bar.rs:30` | `use zed_actions::{agent::AddSelectionToThread, assistant::InlineAssist, outline::ToggleOutline};` | GUT → keep only `outline::ToggleOutline` |
| `quick_action_bar.rs:51-62` | `was_agent_enabled` / `was_agent_button` settings-observer that re-renders the bar when `AgentSettings` change | DELETE |
| `quick_action_bar.rs:159-166` | the `IconName::ZedAssistant` "Inline Assist" button dispatching `InlineAssist::default()` | DELETE |
| `quick_action_bar.rs:290` | `Box::new(AddSelectionToThread)` menu entry | DELETE |
| `quick_action_bar/preview.rs`, `quick_action_bar/repl_menu.rs` | — | clean, KEEP |

`zed_actions::agent` (`crates/zed_actions/src/*.rs:486`), `zed_actions::assistant` (:557), `zed_actions::agents_sidebar` (:816) become orphaned modules — out of this crate but should be pruned in the same PR or the namespace test at `zed.rs:5110` will still list them.

### `zed/open_listener.rs`

| file:line | what | removal note |
|---|---|---|
| `open_listener.rs:3` | `use agent_ui::ExternalSourcePrompt;` | DELETE |
| `open_listener.rs:7` | `use client::{ZedLink, parse_zed_link};` | DELETE (both only feed the channel URL branch) |
| `open_listener.rs:42-43` | `pub open_channel_notes: Vec<(u64, Option<String>)>`, `pub join_channel: Option<u64>` fields on `OpenRequest` | DELETE (paired with `main.rs:1250-1300`) |
| `open_listener.rs:44` | `pub remote_connection: Option<RemoteConnectionOptions>` | KEEP — type comes from `remote`, not `remote_connection` |
| `open_listener.rs:57-59` | `OpenRequestKind::AgentPanel { external_source_prompt }` variant | DELETE |
| `open_listener.rs:60-62` | `OpenRequestKind::SharedAgentThread { session_id }` variant | DELETE |
| `open_listener.rs:89-97` | the manual `Debug` impl arms for `AgentPanel` / `SharedAgentThread` | DELETE |
| `open_listener.rs:160-170` | URL parsing: `zed://agent/shared/{id}` → `SharedAgentThread`, `zed://agent` → `parse_agent_url` | DELETE both `else if` arms |
| `open_listener.rs:186-197` | `parse_zed_link` arm → `ZedLink::Channel` / `ZedLink::ChannelNotes` | DELETE |
| `open_listener.rs:215-222` | `fn parse_agent_url` body building `ExternalSourcePrompt` + `OpenRequestKind::AgentPanel` | DELETE the whole fn |
| `open_listener.rs:1000-1130` | tests `test_parse_agent_url` family + `SharedAgentThread` tests | DELETE |
| `open_listener.rs:266-292`, `:752`, `:943`, `:969-991` | SSH URL parsing + `remote::SshConnectionOptions` tests | KEEP |

`handle_cli_connection` (the CLI IPC loop, `open_listener.rs:358+`) has no delete-set references beyond the fields above.

### `zed/telemetry_log.rs` (621 lines) — decision needed

Depends on `client::telemetry::Telemetry` (:6) and `client::Client::global(cx).telemetry().subscribe_with_history(fs)` (:91-97). Registers the `zed_actions::OpenTelemetryLog` action (:34) and a toolbar item consumed at `zed.rs:1301`.

Two options:
- **DELETE the module** (simplest): drop `main.rs:661`, `zed.rs:10` (`pub mod telemetry_log`), `zed.rs:1301-1303`, `app_menus.rs:303`, and the `OpenTelemetryLog` action.
- **GUT**: keep the viewer but have it read the local telemetry log file directly, once `telemetry::send_event` is a no-op the subscription stream is empty and the pane shows "No telemetry events recorded yet" (:509) forever.

Recommendation: DELETE. A viewer for a no-op subsystem is dead UI.

### `zed/visual_tests.rs` (551 lines)

Only reference is `app_state.client.clone()` at :88 feeding `project::Project::local`. It is `#[cfg(feature = "visual-tests")]`-gated and the feature carries `acp_thread`/`agent`/`agent_ui`/`language_model`/`agent_servers`. DELETE together with the feature (§5/§6).

### Clean files (no action)

`zed/migrate.rs`, `zed/open_url_modal.rs`, `zed/remote_debug.rs`, `zed/mac_only_instance.rs`, `zed/windows_only_instance.rs`.
`remote_debug.rs` uses `remote::remote_client` (:47) and `zed_actions::remote_debug` (:2) — both survive.

---

## 5. `crates/zed/Cargo.toml`

### `[dependencies]` lines to DELETE (22 delete-set edges + 1 crates.io)

| line | dep |
|---|---|
| 66 | `acp_tools.workspace = true` |
| 68 | `agent.workspace = true` |
| 69 | `agent-client-protocol.workspace = true` (crates.io; only `main.rs:8`) |
| 70 | `agent_settings.workspace = true` |
| 71 | `agent_ui = { workspace = true, features = ["audio"] }` |
| 76 | `auto_update.workspace = true` |
| 77 | `auto_update_ui.workspace = true` |
| 79 | `call.workspace = true` |
| 81 | `channel.workspace = true` |
| 85 | `codestral.workspace = true` |
| 86 | `collab_ui.workspace = true` |
| 91 | `copilot.workspace = true` |
| 92 | `copilot_chat.workspace = true` |
| 93 | `copilot_ui.workspace = true` |
| 94 | `crashes.workspace = true` |
| 126 | `acp_thread.workspace = true` (non-optional but used only by `visual_test_runner.rs`) |
| 127 | `action_log = { workspace = true, optional = true }` (kept crate, but only the visual-tests path uses it → drop) |
| 128 | `agent_servers = { workspace = true, optional = true }` |
| 132 | `edit_prediction.workspace = true` |
| 133 | `edit_prediction_ui.workspace = true` |
| 144 | `language_model.workspace = true` |
| 145 | `language_models.workspace = true` |
| 219 | `web_search.workspace = true` |
| 220 | `web_search_providers.workspace = true` |

### Deps that become unused as a side effect (verify, then delete)

| line | dep | why |
|---|---|---|
| 123 / 264 | `semver` | only `crashes::init` version formatting at `main.rs:345` — check `zed.rs` too |
| 124 | `tempfile = { optional = true }` | visual-tests only |
| 125 | `clock = { optional = true }` | visual-tests only |
| 122 | `image = { optional = true }` | test-support + visual-tests only (note the unconditional `image.workspace = true` at line 246 for linux/freebsd stays) |
| 177 | `reqwest` | only `reliability.rs` multipart |
| 171 | `prompt_store` | only `PromptBuilder::load` at `main.rs:668` feeding `agent_ui::init` |
| 197 | `telemetry` | keep (macro becomes no-op but call sites remain) |
| 198 | `telemetry_events` | grep for use — likely orphaned once `client` telemetry is gutted |
| 160 | `notifications` | keep only if `status_toast` is used from this crate; the only call was `notifications::init` at `main.rs:729` → likely deletable here |

### `[features]` to edit

| lines | what | removal note |
|---|---|---|
| 18-30 | `test-support` | KEEP; no delete-set entries |
| 31-54 | `visual-tests` | **DELETE the whole feature.** It names `dep:action_log`, `dep:agent_servers`, `acp_thread/test-support`, `action_log/test-support`, `agent_ui/test-support`, `agent/test-support`, `language_model/test-support` |
| 14-17 | `tracy`, `track-project-leak` | KEEP |

### `[dev-dependencies]` to edit

| line | what |
|---|---|
| 256 | `call = { workspace = true, features = ["test-support"] }` → DELETE |
| 269 | `agent_ui = { workspace = true, features = ["test-support"] }` → DELETE |

---

## 6. Binary targets

`crates/zed/Cargo.toml` defines two:

```
[[bin]] name = "zed"                    path = "src/main.rs"                     # lines 56-58
[[bin]] name = "zed_visual_test_runner" path = "src/visual_test_runner.rs"       # lines 60-63
        required-features = ["visual-tests"]
```

`zed_visual_test_runner` (`src/visual_test_runner.rs`, 3589 lines) is saturated with the delete set — 46 `agent`/`acp` hits alone. Representative: `acp_thread::{AgentConnection, StubAgentConnection}` (:97), `agent_servers::{AgentServer, AgentServerDelegate}` (:99), `call::init` (:186), `language_model::init` (:204), `RefreshLlmTokenListener::register` (:205-209), `language_models::init` (:210), `agent_ui::init` (:217), `agent_settings::AgentSettings::override_global` (:236-241), `agent::{AgentTool, ToolInput, ReadFileTool, ToolCallEventStream, ThreadStore, DbThread}` (:2008-2062, :2686-2720), `client::Client::new` (:985).

**Recommendation: delete `[[bin]] zed_visual_test_runner`, `src/visual_test_runner.rs`, `src/zed/visual_tests.rs`, and the `visual-tests` feature in one move.** Salvaging it would mean rewriting ~40% of a 3.6k-line file for a screenshot-diff harness. Check `.github/workflows/*` and `script/*` for `zed_visual_test_runner` invocations before cutting.

`build.rs` is clean (icons / winresource only).

---

## 7. Init ORDER in `main.rs` — dependency graph

Ordering constraints that matter when crates disappear. `→` means "must run before".

**Pre-`app.run` (process mode):**
```
Args::parse (:187)
  → --askpass (:191)  [askpass, KEEP]
  → --crash-handler (:197)  [crashes, DELETE]
  → --nc (:227)  [nc, KEEP]
init_paths (:273) → zlog::init (:279) → ztracing::init (:290)
AppVersion::load (:295)
app_db (:330) → system_id (:331) / installation_id (:332) / session (:336)
   session_id (:335) → crashes::init (:341)  [DELETE — session_id still needed by Session::new]
OpenListener::new (:363) → single-instance check (:365-389)
```

**Inside `app.run`, the load-bearing chain:**
```
 1. trusted_worktrees::init (456) → menu::init (457) → zed_actions::init (458)
 2. release_channel::init (460) → gpui_tokio::init (461)
 3. settings::init (465)                     ── REQUIRED BY everything reading settings
 4.   ProxySettings::get_global (476) → ReqwestClient (477-482) → cx.set_http_client (483)
 5. extension::init (492) → ExtensionHostProxy::global (493)
 6. Client::production (495)                 ── ROOT of the Arc<Client> graph
 7.   languages / LanguageRegistry (497-499)
 8.   NodeRuntime::new(client.http_client(), …) (527)
 9.   UserStore::new(client) (531)           ── Arc<Client>
10.   WorkspaceStore::new(client) (532)      ── Arc<Client>
11. Client::set_global (553)
12. zed::init (555)                          ── registers OpenAccountSettings (DELETE)
13. project::Project::init(&client) (556)    ── Arc<Client>, KEEP
14. debugger_ui::init (557) / debugger_tools::init (558)
15. client::init(&client) (559)              ── Arc<Client>, GUT
16. feature_flags::FeatureFlagStore::init (560)   ── REQUIRED BEFORE reliability::init (621) `on_flags_ready`
17. block_on(system_id/installation_id/session) (562-564)
18. client.telemetry() + telemetry.start(...) (566-572)   ── needs 15 + 17
19. cx.subscribe(user_store → crashes::set_user_info) (573-585)  ── needs 9 + 18 + crashes::init
20. AppState { client, user_store, workspace_store, node_runtime, … } (606-615)
21. AppState::set_global (616)               ── REQUIRED BY every `app_state.*` init below
22. auto_update::init(client) (618)          ── Arc<Client>
23. auto_update_ui::init (620)               ── needs 22 (action registration)
24. reliability::init(client) (621)          ── Arc<Client>; needs 16 + 18
25. extension_host::init(proxy, fs, client, node_runtime) (622-628)  ── Arc<Client>, KEEP
26. theme_settings::init (630)               ── REQUIRED BEFORE cx.theme() at 754 / 784 / 792
27. command_palette::init (637)
28. copilot_chat config (638-644) ← reads language::language_settings (needs 530 languages::init)
29. copilot_chat::init(fs, client.http_client(), …) (645)
30. copilot_ui::init(&app_state) (652)       ── needs 29
31. language_model::init (653)               ── REQUIRED BEFORE 33
32. RefreshLlmTokenListener::register(client, user_store) (654)  ── Arc<Client>
33. language_models::init(user_store, client) (659)              ── Arc<Client>; needs 31 + 32
34. acp_tools::init (660)
35. telemetry_log::init (661) ← Client::global(cx).telemetry()   ── needs 11 + 18
36. remote_debug::init (662)                 KEEP
37. edit_prediction_ui::init (663)
38. web_search::init (664) → web_search_providers::init(client, user_store) (665)  ── Arc<Client>
39. edit_prediction_registry::init(client, user_store) (667)     ── Arc<Client>
      ⚠ ORDERING ODDITY: calls edit_prediction::EditPredictionStore::global()
        yet edit_prediction::init(cx) does not run until line 741.
40. PromptBuilder::load (668) → project::AgentRegistryStore::init_global(fs, client.http_client()) (669)
41. agent_ui::init(fs, prompt_builder, languages, is_new_install, …) (674)  ── needs 40, 21, 26
42. editor::init (689) … audio::init (694)
43. workspace::init(app_state) (695)         ── REQUIRED BEFORE 812 initialize_workspace
44. channel::init(&client, user_store) (707)     ── Arc<Client>
45. call::init(client, user_store) (728)         ── Arc<Client>; needs 42 audio + 44
46. notifications::init(client, user_store) (729) ── Arc<Client>
47. collab_ui::init(&app_state) (730)             ── needs 44 + 45 + 46
48. edit_prediction::init (741)
49. observe_global::<SettingsStore> theme+server_url closure (749-783)  ── needs 26
50. telemetry events + telemetry.flush_events (792-802)
51. app_menus(cx) (810) → cx.set_menus (811)     ── app_menus.rs pulls auto_update + collab_panel
52. initialize_workspace(app_state) (812)        ── needs 43; wires panels/toolbars in zed.rs
53. authenticate(client) spawn (816-820)         ── Arc<Client>
54. component_preview::init(app_state) (899)
```

**Inits that take `Arc<Client>` (or `&Arc<Client>`):**
`project::Project::init` (556, KEEP) · `client::init` (559, GUT) · `auto_update::init` (618) · `reliability::init` (621, GUT to drop it) · `extension_host::init` (622, KEEP) · `RefreshLlmTokenListener::register` (654) · `language_models::init` (659) · `web_search_providers::init` (665) · `edit_prediction_registry::init` (667) · `channel::init` (707) · `call::init` (728) · `notifications::init` (729) · `authenticate` (818).
Plus `client.http_client()` consumers: `NodeRuntime::new` (527, KEEP), `copilot_chat::init` (647), `AgentRegistryStore::init_global` (672, KEEP), `reliability::upload_minidump` (393).

**Breakage cascade if a crate vanishes without editing:** 21 → 22/24 → 51 (menus reference `auto_update::Check`) and 21 → 41 → 52 (`initialize_workspace` → `initialize_panels` → `initialize_agent_panel`). The `app_menus` → `initialize_workspace` tail is what turns one missing crate into a wall of errors, which is why `Cargo.toml` + `main.rs` should be cut before the leaf modules.

---

## 8. `remote_server` / `remote_connection` in `crates/zed`

**No direct edges.** `crates/zed/Cargo.toml` depends on `remote` (line 175) and `recent_projects` (line 173), neither of which is `remote_connection` or `remote_server`.

- `crates/remote/Cargo.toml` has **no** `remote_connection` / `remote_server` dependency. Safe.
- `crates/recent_projects/Cargo.toml` **does**: `remote_connection.workspace = true` (line 42), `test-support = ["remote_connection/test-support", …]` (line 17), and `remote_server.workspace = true` in `[dev-dependencies]` (line 72). This is a transitive problem for the `zed` binary — `recent_projects` is the crate that must be fixed, not `zed`.
- `remote` also gates a `build-remote-server-binary` feature (not enabled by `zed`).

**Would removal leave dangling CLI args or actions in `crates/zed`?** No.

| surface | file:line | verdict |
|---|---|---|
| `--askpass` | `main.rs:1684-1689` | KEEP — used by SSH/Git password auth via `askpass` |
| `ssh://` and `zed://ssh` URL parsing | `open_listener.rs:153-155`, `:184-185`, `:266-292` | KEEP — `remote::SshConnectionOptions` |
| `OpenRequest::remote_connection` field | `open_listener.rs:44`, `main.rs:1206` | KEEP — type is `remote::RemoteConnectionOptions` |
| `open_remote_project` / `RemoteSettings` | `main.rs:42`, `:1209`, `:1376-1401` | KEEP — from `recent_projects` |
| `zed_actions::OpenRemote` menu entry | `app_menus.rs:132` | KEEP |
| `zed::remote_debug` actions | `zed/remote_debug.rs:2-47` | KEEP — `remote::remote_client::MAX_RECONNECT_ATTEMPTS` |
| `--dev-server-token` | `main.rs:1650-1652` | Already a documented no-op; delete opportunistically |
| `proto::GetCrashFiles` remote crash pull | `reliability.rs:53-89` | DELETE — its server half is in `remote_server` |

So the SSH remote-dev flow survives intact inside `crates/zed`; the only remote-related deletion here is the crash-file harvest in `reliability.rs`.

---

## Recommended EDIT ORDER for `crates/zed`

Each step is chosen to collapse the largest number of downstream errors first, and each ends at a point where `cargo check -p zed` gives strictly fewer, more localized errors than before.

1. **Delete `src/visual_test_runner.rs` + `src/zed/visual_tests.rs`, drop `[[bin]] zed_visual_test_runner` and the `visual-tests` feature** (`Cargo.toml:31-54`, `:60-63`; `zed.rs:11-12`). Zero-risk, removes ~4,100 lines and every `acp_thread` / `agent_servers` / `action_log` / `clock` / `tempfile` edge in one cut. Grep CI for the binary name first.
2. **Delete `src/zed/edit_prediction_registry.rs`** and its two references (`zed.rs:2`, `main.rs:66`). Kills all `copilot`, `codestral`, `edit_prediction` edges outside `main.rs`/`zed.rs`.
3. **`Cargo.toml`** — remove the 24 dependency lines from §5, the two `[dev-dependencies]` entries, and the `visual-tests` feature. This is what makes the compiler report *unresolved crate* rather than *unresolved item*, giving one clean error per real call site.
4. **`main.rs`** — top-down: imports (7-16, 41, 64-68), `crashes` (196-200, 341-361), the init block (618-741 with the deletions in §1c), the settings closure (749-783), `authenticate` spawn (816-820), then `handle_open_request` (937-1040, 1250-1300), then `fn authenticate` (1311-1323), then the `Args` fields (1650-1652, 1667-1670) and `parse_url_arg` (1741). Biggest single unblocker — main.rs holds 22 of the ~40 non-test call sites.
5. **`src/reliability.rs`** — apply §3: keep `monitor_hangs` / `cleanup_old_hang_traces` / `save_hang_trace`, delete everything else, change the signature to `init(cx: &mut App)` and update `main.rs:621`. Self-contained, no ripple.
6. **`src/zed/app_menus.rs`** — 4 line deletions. Must land before `main.rs:810` type-checks.
7. **`src/zed/quick_action_bar.rs`** — imports + 3 blocks.
8. **`src/zed/open_listener.rs`** — enum variants, `OpenRequest` fields, URL arms, `parse_agent_url`, tests. Do this *after* `main.rs:914-1309`, since the field/variant deletions are driven by what `handle_open_request` no longer consumes.
9. **`src/zed/telemetry_log.rs`** — delete the module (recommended) plus `main.rs:661`, `zed.rs:10`, `zed.rs:1301-1303`, `app_menus.rs:303`.
10. **`src/zed.rs` non-test half** — imports (2, 11-12, 16, 22, 99), `OpenAccountSettings` handler (219-223), workspace observer (438-453), GPU/crashes (499-503), edit-prediction status bar (505-520, 562), panel loading (701-702, 724-727), the three AI-panel functions (733-824), collab toggle action (1112-1118), toolbar items (1299-1303, 1318-1319). Left near the end because it is the widest file and every earlier step shrinks the diff here.
11. **`src/zed.rs` test half** — `init_test_with_state` (5371-5412), `init_keymap_test` (4842), and finally prune `expected_namespaces` (5110-5150). Run this test last: it is the best canary for any action from a deleted crate that is still being registered somewhere.

**Parallelizable:** steps 5-9 touch disjoint files and can be split across workers once step 3 has landed. Steps 1-4 are strictly sequential.
