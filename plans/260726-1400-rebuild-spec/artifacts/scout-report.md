# Scout Report

## Detected Language

Generic source
<!-- stack-profile "generic-source" resolved for this project. Underlying implementation language is Rust
     (root Cargo.toml workspace, rust-toolchain.toml channel 1.94.1). This is a native GPUI desktop
     application, not a web app — there are no HTTP routes/screens in the web sense, so
     route-list/screen-list/screen-flow/api-map/crud-matrix/db-objects artifacts are SKIPPED per profile. -->

## Scanned Directories

- crates/ (232 member crates)
- extensions/ (glsl, html, proto, test-extension — in-repo sample/bundled extensions)
- tooling/ (compliance, perf, xtask — build/dev tooling crates)
- script/ (non-Rust build/release/CI shell & Python scripts, not source-of-truth for app logic)
- docs/ (mdBook source: docs/src/*.md; docs/system, docs/features, docs/generated, docs/flows exist as directories but are currently EMPTY — no prior rebuild-spec output present)
- assets/ (icons, themes, fonts, keymaps — static resources, not code)
- .github/ (CI workflows — not scanned as source)
- nix/, legal/, ci/ — packaging/legal/CI config, not scanned as source

<!-- Excluded as test/vendor: any crates/*/src/**/*test*.rs, crates/*_benchmarks/**, target/, node_modules
     (extensions may bundle their own via extension_host), and Cargo.lock/vendored deps. -->

## File Inventory

<!-- 234 crates (232 under crates/ + 2 nested: crates/refineable/derive_refineable is a workspace member
     under refineable/) plus 4 extensions/ crates and 3 tooling/ crates = ~241 Cargo packages, ~1760 .rs
     files under crates/ (excluding extensions/tooling), ~1.30M lines. Given the scale (per Wave 0
     instructions: prioritize breadth over exhaustive per-file listing), this section is a
     directory-level (per-crate) inventory: one row per crate noting purpose, subsystem grouping, and
     approximate .rs file count under its src/. Every crate is a workspace member compiled into the
     `zed` binary or a supporting tool; nearly all crate content is `other` (library/application code) —
     `config` is used for settings-schema crates, `model` for entity/data-model-bearing crates,
     `background` for crates hosting scheduled/async task infra. Per-file granularity is available on
     request for any specific crate via targeted follow-up. -->

### Subsystem: Application Shell & Entry Point
crates/zed/src/**	other (17 files) — main Zed application: window/app bootstrap, CLI arg handling, app menus, OS integration glue. Binary crate (`default-members`).
crates/zed_actions/src/**	other (1 file) — shared `actions!()` definitions dispatched app-wide.
crates/cli/src/**	other (2 files) — the `zed` CLI launcher binary (opens files, IPC to running instance).
crates/install_cli/src/**	other (3 files) — installs/uninstalls the `zed` CLI shim.
crates/release_channel/src/**	config (1 file) — app version / release channel (stable/preview/nightly/dev) constants.
crates/auto_update/src, auto_update_ui/src, auto_update_helper/src	background (1+1+3 files) — update-check background task, UI, and helper installer process.
crates/system_specs/src	other (1 file) — collects OS/hardware specs for bug reports/telemetry.
crates/env_var/src, zed_env_vars/src	config (1+1) — environment variable helpers.
crates/paths/src	config (1 file) — canonical filesystem paths used across the app.
crates/crashes/src, etw_tracing/src	other (1+0 files) — crash reporting (Sentry-like) and Windows ETW tracing hooks.

### Subsystem: GPUI UI Framework (core rendering/windowing engine)
crates/gpui/src/**	other (77 files) — the GPU-accelerated UI framework: entities, elements, layout (Taffy/flex), input, windows, actions, async executors. Core dependency of the entire app.
crates/gpui_platform/src	other (1 file) — re-exports platform trait + `current_platform()` selector.
crates/gpui_macos/src	other (15 files) — macOS platform backend (Cocoa/Metal via objc2).
crates/gpui_linux/src	other (24 files) — Linux platform backend (X11/Wayland).
crates/gpui_windows/src	other (18 files) — Windows platform backend (Win32/Direct3D).
crates/gpui_web/src	other (9 files) — experimental web/WASM backend (wasm32-unknown-unknown target).
crates/gpui_wgpu/src	other (5 files) — wgpu-based renderer used across platform backends.
crates/gpui_tokio/src	other (1 file) — bridges a Tokio runtime into GPUI's executor for crates needing tokio (e.g. AWS SDK).
crates/gpui_macros/src	other (11 files) — proc-macros for GPUI (`#[derive(IntoElement)]`, test macros).
crates/gpui_util/src, gpui_shared_string/src	other (2+0 files) — small GPUI utility/shared-string helpers.
crates/refineable/src + refineable/derive_refineable	other (1+? files) — "refinement" types macro for partial struct init/merge (used heavily by settings/theme).

### Subsystem: Editor Core (text/buffer engine)
crates/text/src	model (10 files) — the `Buffer`/`Rope`-backed CRDT-like text storage, edits, anchors, undo history (`crates/text/src/text.rs:59 pub struct Buffer`).
crates/rope/src	model (6 files) — the `Rope` data structure text is stored in.
crates/sum_tree/src	other (4 files) — generic B-tree-like `SumTree` used by rope/buffer/multi_buffer for efficient indexing.
crates/language/src	model (17 files) — language-aware `Buffer` wrapper (syntax highlighting, diagnostics, outline) — `crates/language/src/buffer.rs:98 pub struct Buffer`.
crates/language_core/src	model (11 files) — core language types shared without pulling in tree-sitter/lsp deps.
crates/multi_buffer/src	model (5 files) — combines multiple buffer excerpts into one multi-buffer view (used by search results, diagnostics, diffs).
crates/buffer_diff/src	model (1 file) — diffing buffers against git HEAD/base for inline diff gutters.
crates/streaming_diff/src	other (1 file) — incremental diffing for streamed content (e.g. AI edit streaming).
crates/editor/src	other (57 files) — `crates/editor/src/editor.rs:1131 pub struct Editor` — the visual text editor component (cursor/selection, scroll, code actions, completions rendering) used everywhere in the app.
crates/markdown/src, markdown_preview/src	other (8+2) — markdown rendering element + preview panel.
crates/outline/src, outline_panel/src	other (1+2) — symbol outline extraction and outline panel UI.
crates/snippet/src, snippet_provider/src, snippets_ui/src	other (1+4+1) — snippet expansion engine, provider, and settings UI.
crates/go_to_line/src, tab_switcher/src, file_finder/src, project_symbols/src	other — navigation pickers (go-to-line, tab switcher, fuzzy file finder, symbol search).
crates/search/src	other (6 files) — project/buffer search (find/replace, regex).
crates/diagnostics/src	other (6 files) — diagnostics panel (LSP errors/warnings aggregation UI).
crates/image_viewer/src, svg_preview/src, csv_preview/src	other — file-type preview panes.
crates/encoding_selector/src, line_ending_selector/src	other — status-bar pickers for text encoding / EOL style.

### Subsystem: Project / Filesystem / Workspace
crates/project/src	model (62 files) — `crates/project/src/project.rs:213 pub struct Project` — central coordinator: worktrees, LSP store, DAP/debugger sessions, task runner, git, settings resolution. Largest single non-UI crate.
crates/worktree/src	model (3 files) — filesystem tree watcher/index for a project root (`Worktree` entity).
crates/worktree_benchmarks/src	other (1 file, bench-only).
crates/workspace/src	model (25 files) — `crates/workspace/src/workspace.rs:1343 pub struct Workspace` — top-level window content: panes, panels, docks, tabs; hosts `register_action` (workspace.rs:7460).
crates/fs/src	other (3 files) — filesystem abstraction (real + in-memory test fs).
crates/fs_benchmarks/src	other (1 file, bench-only).
crates/project_panel/src	other (7 files) — the file-tree sidebar panel.
crates/recent_projects/src	other (8 files) — recent-projects picker/modal.
crates/session/src	other (1 file) — restores previously open workspaces on relaunch.
crates/db/src	model (3 files) — sqlite-backed local app database (workspace state, kv store) built on sqlez.
crates/sqlez/src, sqlez_macros/src	other (10+1) — thin async SQLite wrapper + query macros.
crates/watch/src	other (2 files) — filesystem change notification primitive.
crates/toolchain_selector/src	other (2 files) — picker for language toolchains (e.g. Python venv).
crates/dev_container/src	other (8 files) — devcontainer.json support for containerized dev environments.
crates/remote/src, remote_connection/src, remote_server/src	other (11+1+5) — remote development: `remote` is the client-side subsystem, `remote_server` the daemon run on the remote host, `remote_connection` shared connection plumbing.

### Subsystem: Language Intelligence / LSP / Extensions
crates/lsp/src	other (2 files) — Language Server Protocol client transport/types.
crates/language_extension/src	other (2 files) — bridges WASM extensions' language server definitions into `project`/`language`.
crates/languages/src	other (16 files) — built-in language definitions/configs (bundled grammars/queries wiring).
crates/language_tools/src	other (7 files) — dev tools (syntax tree view, LSP log viewer).
crates/language_selector/src	other (2 files) — language picker in status bar.
crates/language_onboarding/src	other (1 file) — first-run language setup nudges.
crates/grammars/src	other (1 file) — tree-sitter grammar registration/build glue.
crates/prettier/src	other (1 file) — Prettier formatter integration.
crates/json_schema_store/src	config (1 file) — JSON schema registry for settings/tasks/etc. JSON files.
crates/extension/src	other (14 files) — extension manifest/model types.
crates/extension_api/src	other (4 files) — public Rust API surface for authoring Zed extensions (doc: "APIs for creating Zed extensions in Rust").
crates/extension_host/src	other (17 files) — WASM extension runtime host (loads/sandboxes/executes extension `.wasm`).
crates/extension_cli/src	other (1 file) — CLI for packaging/publishing extensions.
crates/extensions_ui/src	other (5 files) — in-app extensions marketplace/manager panel.
crates/dap/src, dap_adapters/src, debug_adapter_extension/src	other (8+6+3) — Debug Adapter Protocol client, bundled adapters, extension-provided adapters.
crates/debugger_ui/src, debugger_tools/src	other (26+2) — debugger panel UI and dev tooling.
extensions/glsl, extensions/html, extensions/proto, extensions/test-extension	other — sample/bundled WASM extensions (language support + a test fixture), each a separate Cargo package built to wasm32-wasip2.

### Subsystem: Edit Prediction / AI Code Completion
crates/edit_prediction/src	other (17 files) — edit-prediction (AI inline completion) core trait/provider abstraction.
crates/edit_prediction_context/src	other (4 files) — context gathering (surrounding code) fed to prediction models.
crates/edit_prediction_types/src	model (1 file) — shared request/response types.
crates/edit_prediction_ui/src	other (4 files) — inline UI for showing/accepting predictions.
crates/edit_prediction_metrics/src	other (7 files) — telemetry/accuracy metrics for predictions.
crates/edit_prediction_cli/src	other (29 files) — CLI harness for evaluating prediction models offline.
crates/zeta_prompt/src	other (4 files) — prompt construction for Zed's "Zeta" edit-prediction model.
crates/codestral/src, deepseek/src	other (1 each) — specific model provider integrations feeding edit prediction.

### Subsystem: AI / Agent (chat, tools, model providers)
crates/agent/src	model (61 files) — `crates/agent/src/thread.rs:936 pub struct Thread` — core agent conversation engine: threads, tool calls, context assembly.
crates/agent_ui/src	other (47 files) — chat/agent panel UI (message list, tool-call rendering, editor).
crates/agent_settings/src	config (2 files) — agent-specific settings schema.
crates/agent_servers/src	other (4 files) — manages external agent server processes (e.g. ACP-compatible agents).
crates/acp_thread/src, acp_tools/src	model/other (5+1) — Agent Client Protocol thread model + dev tools for inspecting ACP traffic.
crates/ai_onboarding/src	other (7 files) — first-run AI feature onboarding UI.
crates/prompt_store/src	model (2 files) — stores/manages reusable prompt templates ("rules"/prompt library).
crates/rules_library/src	other (1 file) — UI for the prompt/rules library.
crates/context_server/src	other (10 files) — Model Context Protocol (MCP) server client integration.
crates/language_model/src, language_model_core/src	model (7+7) — abstraction over LLM providers (messages, tool schemas, streaming).
crates/language_models/src, language_models_cloud/src	other (21+1) — concrete provider registrations (wires anthropic/open_ai/etc. into `language_model`), cloud-hosted model variants.
crates/anthropic/src, open_ai/src, google_ai/src, bedrock/src, mistral/src, ollama/src, lmstudio/src, deepseek/src(dup above), open_router/src, vercel/src, x_ai/src, copilot/src, copilot_chat/src	other (~3,4,2,2,1,1,1,–,1,1,1,3,2) — per-provider LLM API client crates (HTTP request/response mapping to each vendor's API).
crates/copilot_ui/src	other (2 files) — GitHub Copilot status/sign-in UI.
crates/cloud_api_client/src, cloud_api_types/src, cloud_llm_client/src	other (3+6+2) — Zed's own cloud proxy API client (auth, billing, model routing) and its wire types.
crates/eval_cli/src, eval_utils/src	other (2+1) — offline evaluation harness for agent/model quality.
crates/web_search/src, web_search_providers/src	other (1+2) — web search tool integration for agents (e.g. Bing/Brave provider glue).
crates/repl/src	other (26 files) — Jupyter-kernel REPL integration (notebook-style code execution panel).

### Subsystem: Collaboration & Networking
crates/collab/src	other (60 files) — the `collab` server binary (Zed's collaboration backend: rooms, calls, channels; separately deployable, not shipped in the desktop app).
crates/collab_ui/src	other (10 files) — desktop client UI for collaboration (shared projects, chat).
crates/call/src	other (6 files) — voice/screen-share call state management (client-side).
crates/channel/src	model (5 files) — channels (persistent chat/collab spaces) client model.
crates/client/src	other (10 files) — connects the desktop app to the collab/cloud backend (auth, websocket).
crates/rpc/src	other (8 files) — shared RPC message framing between client and collab server (doc: "Shared logic for communication between the Zed app and the zed.dev server").
crates/proto/src	other (4 files) — protobuf-esque shared wire protocol definitions (doc: "Shared protocol for communication between the Zed app and the zed.dev server").
crates/net/src	other (6 files) — low-level networking helpers.
crates/http_client/src, http_client_tls/src, reqwest_client/src, aws_http_client/src	other (4+1+1+1) — HTTP client abstraction + TLS + reqwest/AWS-SigV4-specific implementations.
crates/livekit_api/src, livekit_client/src	other (3+11) — LiveKit (WebRTC) SDK bindings and GPUI integration for audio/video calls.
crates/media/src, audio/src, denoise/src	other (2+5+2) — macOS media APIs, audio capture/playback, noise suppression.
crates/notifications/src	model (3 files) — in-app notification center (collab invites, mentions).
crates/git_hosting_providers/src	other (12 files) — GitHub/GitLab/Bitbucket/etc. hosting-provider detection (for permalinks, PR links).
crates/telemetry/src, telemetry_events/src	other (1+1) — anonymized usage telemetry client + event schema.
crates/feedback/src	other (1 file) — in-app bug-report/feedback submission.

### Subsystem: Version Control (Git)
crates/git/src	other (8 files) — git plumbing (status, blame, diff, commit via `git` CLI or libgit2-like ops).
crates/git_ui/src	other (24 files) — git panel UI (staging, commit, diff view, blame gutter).
crates/git_graph/src	other (1 file) — commit graph visualization.
crates/buffer_diff/src (also listed above) — buffer-level diff-vs-git integration.

### Subsystem: Settings / Theme / Keymap
crates/settings/src	model (9 files) — `crates/settings/src/settings_store.rs:145 pub struct SettingsStore` — central settings registry; ~40 crates implement `impl Settings for` to register their schema.
crates/settings_content/src	config (15 files) — the settings JSON schema/content model (deserialize targets).
crates/settings_json/src	config (1 file) — raw JSON settings file parsing helpers.
crates/settings_macros/src	other (1 file) — `#[derive]`/macro support for settings structs.
crates/settings_ui/src	other (17 files) — in-app graphical settings editor.
crates/settings_profile_selector/src	other (1 file) — picker for settings profiles (e.g. per-OS overrides).
crates/theme/src	model (18 files) — theme data model (colors, syntax highlighting themes).
crates/theme_settings/src	config (3 files) — theme-related settings schema.
crates/theme_extension/src	other (1 file) — loads themes contributed by WASM extensions.
crates/theme_importer/src	other (6 files) — imports themes from external formats (e.g. TextMate/VS Code).
crates/theme_selector/src	other (2 files) — theme picker UI.
crates/syntax_theme/src	model (1 file) — syntax-highlight-specific theme subset.
crates/icons/src, file_icons/src	other (1+1) — icon asset registry and file-type-to-icon mapping.
crates/keymap_editor/src	other (4 files) — in-app keybinding editor.
crates/component_preview/src, component/src	other (2+2) — internal component gallery/preview tool for `ui` crate components.
crates/feature_flags/src, feature_flags_macros/src	config (4+1) — remote-configurable feature flag client + macros.
crates/migrator/src	other (46 files) — settings/keymap migration engine (doc: rewrites old settings/keymap formats when actions/settings are renamed).

### Subsystem: UI Component Library / Widgets
crates/ui/src	other (106 files) — `crates/ui/src` — Zed's design-system component library (buttons, lists, tooltips, etc.) built on GPUI.
crates/ui_input/src	other (2 files) — form-style input/number-field components.
crates/ui_macros/src, ui_prompt/src	other (3+1) — UI proc-macros and a prompt/dialog helper.
crates/picker/src	other (4 files) — generic fuzzy-picker widget reused by many pickers above.
crates/panel/src, sidebar/src, breadcrumbs/src, platform_title_bar/src, title_bar/src	other (1+3+1+5+8) — dock/panel trait, sidebar container, breadcrumb bar, and window title bar chrome (platform-specific + shared).
crates/menu/src, command_palette/src, command_palette_hooks/src	other (1+2+1) — menu action dispatch, command palette, and its extension hooks.
crates/which_key/src	other (3 files) — "which-key"-style keybinding hint popup (doc: "Which-key support for Zed").
crates/onboarding/src	other (5 files) — first-run app onboarding flow.
crates/miniprofiler_ui/src, inspector_ui/src, input_latency_ui/src	other (1+3+1) — internal dev/perf-inspection UI overlays.
crates/tasks_ui/src, task/src	other (2+8) — task runner (build/run tasks defined in tasks.json) engine + panel UI.
crates/shell_command_parser/src	other (1 file) — parses shell command strings for tasks.
crates/open_path_prompt/src	other (3 files) — "open path" quick dialog.
crates/journal/src	other (1 file) — daily-journal note-taking feature.
crates/time_format/src	other (1 file) — relative/absolute time formatting helper.

### Subsystem: Vim / Modal Editing
crates/vim/src	other (39 files) — Vim emulation (doc: "Vim support for Zed"): modes, motions, operators, registers.
crates/vim_mode_setting/src	config (1 file) — `VimModeSetting`/`HelixModeSetting` toggles (doc confirmed).
crates/which_key/src — also used by vim's leader-key UX (cross-listed above).

### Subsystem: Utilities, Macros, Testing, Data Structures
crates/util/src	other (23 files) — general-purpose utility functions/structs shared by Zed and GPUI.
crates/util_macros/src	other (1 file) — utility proc-macros.
crates/collections/src	model (3 files) — standard collection type aliases (HashMap/HashSet wrappers) used across the codebase.
crates/fuzzy/src, fuzzy_nucleo/src	other (5+4) — fuzzy-matching algorithm + Nucleo-backed implementation.
crates/clock/src	other (2 files) — logical clock / vector-clock primitives for CRDT ordering.
crates/html_to_markdown/src	other (6 files) — HTML→Markdown conversion (used e.g. for pasting rich content, doc previews).
crates/zlog/src, zlog_settings/src	other (4+1) — Zed's structured logging facade + its settings.
crates/ztracing/src, ztracing_macro/src	other (1+1) — tracing/instrumentation macros.
crates/schema_generator/src	other (1 file, tool) — generates JSON Schema files from settings/task Rust types for docs/editor completion.
crates/docs_preprocessor/src	other (1 file, tool) — mdBook preprocessor for docs/ build.
crates/credentials_provider/src, zed_credentials_provider/src, askpass/src	other (1+1+2) — OS keychain credential storage and SSH askpass helper.
crates/explorer_command_injector/src	other (1 file) — Windows Explorer "Open with Zed" context-menu shell extension.
crates/nc/src	other (1 file) — small netcat-like helper (likely used by remote/dev tooling).
crates/scheduler/src	other (5 files) — `crates/scheduler/src/scheduler.rs:72 pub trait Scheduler` — deterministic task scheduling abstraction (production + `test_scheduler.rs` fake for tests).

### Subsystem: Tooling (outside crates/)
tooling/xtask	other — repo-wide dev task runner (cargo xtask-style automation).
tooling/perf	other — performance benchmarking harness.
tooling/compliance	other — license/dependency compliance checks.
script/*	other (non-Rust: bash/PowerShell/Python/JS) — build, bundling, release, CI helper scripts (bootstrap, bundle-mac/linux/windows/freebsd, bump-version, check-licenses, clippy wrappers, etc.) — not part of the compiled application, listed for completeness only.

## Background Logic Source Inventory

### Rust (Actix/Axum + tokio-cron pattern row) — with [SIGNAL_INFERRED] adaptation

<!-- Zed is not a web backend (no Actix/Axum routes); the closest table row (Rust) assumes an HTTP
     server shape that doesn't fit. Applying [SIGNAL_INFERRED] protocol throughout: GPUI's own
     concurrency primitives (`cx.spawn`, `cx.background_spawn`), the `Scheduler` trait, `actions!`
     dispatch, and `impl Settings for` registrations are the actual analogues of the 10 canonical BL
     types in this desktop-app architecture. Representative examples only — 360 files contain
     `cx.spawn` and 153 contain `cx.background_spawn`; exhaustive per-call enumeration is out of scope
     per Wave 0 breadth-over-depth guidance. -->

- scheduled-job: crates/auto_update/src/auto_update.rs [SIGNAL_INFERRED]
  - Intent matched: scheduled-job — periodic background check pattern
  - No-row reason: no HTTP-framework cron scheduler exists; Zed uses its own async task loop, not `tokio-cron`/Actix
  - Observed pattern: background task spawned at app start that periodically polls the update server and re-arms itself via `cx.background_spawn` + a sleep/timer loop
- scheduled-job: crates/scheduler/src/scheduler.rs:72 [SIGNAL_INFERRED]
  - Intent matched: scheduled-job — the `Scheduler` trait is the app's deterministic-time task scheduling abstraction, backing timers/timeouts across GPUI
  - No-row reason: custom in-house scheduler, not a named crate like `tokio-cron`/`gocron`
  - Observed pattern: `pub trait Scheduler: Send + Sync` with a `test_scheduler.rs` fake implementation used to drive deterministic tests
- queue-worker: crates/agent/src/thread.rs:936 (Thread tool-call execution loop) [SIGNAL_INFERRED]
  - Intent matched: queue-worker — async consumer that processes a queue of pending tool-calls/messages
  - No-row reason: not a message-queue library (no Kafka/Bull/asynq); it's an in-process `cx.spawn` loop over agent turns
  - Observed pattern: `Thread` drives a loop that awaits streamed model responses and dispatches queued tool invocations via `cx.spawn`
- queue-worker: crates/extension_host/src (WASM extension task execution) [SIGNAL_INFERRED]
  - Intent matched: queue-worker — background execution of sandboxed extension calls off the main thread
  - No-row reason: custom WASM host runtime, not a standard queue library
  - Observed pattern: extension calls are dispatched via `cx.background_spawn` into the wasmtime runtime and results routed back to the main thread
- event-listener: crates/workspace/src/workspace.rs (EventEmitter subscriptions) [SIGNAL_INFERRED]
  - Intent matched: event-listener — 187 files across the workspace implement `impl EventEmitter<...>`, consumed via `cx.subscribe`
  - No-row reason: GPUI's pub/sub is a first-party entity-event system, not a library like `@OnEvent`/`ActiveSupport::Notifications`
  - Observed pattern: entities `cx.emit(event)`; other entities register `cx.subscribe(other_entity, |this, other, event, cx| ...)` at construction time, stored as `Subscription` fields
- middleware: crates/project/src/lsp_store.rs:8983-9401 (`handle_lsp_get_completions`, `handle_lsp_command`, `handle_lsp_query`, `handle_apply_code_action`, etc.) [SIGNAL_INFERRED]
  - Intent matched: middleware — request/response processing chain, here for LSP requests flowing between editor and language servers (closest non-HTTP analogue; excludes auth per protocol)
  - No-row reason: this is an LSP client dispatch table, not an HTTP middleware stack
  - Observed pattern: `async fn handle_lsp_*` methods on `Project`/`LspStore` intercept, transform, and route LSP protocol messages
- middleware: crates/rpc/src, crates/proto/src (client↔collab-server message routing) [SIGNAL_INFERRED]
  - Intent matched: middleware — the RPC dispatch layer routing proto messages to handlers on both client and `collab` server
  - No-row reason: custom binary RPC protocol, not HTTP
  - Observed pattern: typed proto message enums matched and routed to per-message handler functions registered on client/server peer objects
- notification: crates/notifications/src (in-app notification center) [SIGNAL_INFERRED]
  - Intent matched: notification — in-app notification logic, doc-confirmed via `pub struct` model crate
  - No-row reason: standard in-app notification center, not a named notification library
  - Observed pattern: entity model storing/surfacing collab invites, mentions, and other app notifications to the UI
- integration: crates/anthropic, open_ai, google_ai, bedrock, mistral, ollama, lmstudio, open_router, vercel, x_ai, copilot, copilot_chat/src (per-provider LLM API clients) [SIGNAL_INFERRED]
  - Intent matched: integration — third-party API client crates, one per external LLM vendor
  - No-row reason: not `@FeignClient`/SDK-injection style; each is a bespoke reqwest-based HTTP client crate
  - Observed pattern: each crate defines request/response structs mirroring the vendor's REST API and a client struct implementing `language_model`'s provider trait
- integration: crates/livekit_api/src, livekit_client/src (WebRTC calling) [SIGNAL_INFERRED]
  - Intent matched: integration — third-party service integration (LiveKit)
  - No-row reason: no framework-specific injection pattern; direct SDK binding crate
  - Observed pattern: `livekit_api` wraps LiveKit's server API; `livekit_client` adapts it into GPUI's async/entity model for `call`
- integration: crates/git_hosting_providers/src (GitHub/GitLab/Bitbucket detection) [SIGNAL_INFERRED]
  - Intent matched: integration — external hosting-provider API/URL integration for permalinks and PR metadata
  - No-row reason: provider-detection module set, not an SDK client library
  - Observed pattern: one submodule per hosting provider implementing a shared `GitHostingProvider` trait
- custom-command: crates/cli/src, extension_cli/src, eval_cli/src, edit_prediction_cli/src, schema_generator/src (standalone CLI binaries) [SIGNAL_INFERRED]
  - Intent matched: custom-command — standalone CLI entry points (`fn main()`) distinct from the GUI app
  - No-row reason: not a `cobra`/`clap`-subcommand-of-server pattern; each is its own binary crate
  - Observed pattern: each crate has a `main.rs`/binary target parsing CLI args and driving a focused task (launching zed, packaging extensions, running eval harness, generating JSON schemas)
- webhook: _(none found)_ — Zed's desktop app and `collab` server expose no inbound webhook endpoints found in this scan; `collab` (crates/collab/src) is itself a server but its client protocol is the custom RPC/proto system above, not HTTP webhooks.
- mail: _(none found)_ — no email-sending logic found; not applicable to a desktop editor.
- observer: crates/settings/src/settings_store.rs:145 (`impl Settings for` registrations, ~40 hits) [SIGNAL_INFERRED]
  - Intent matched: observer — lifecycle-hook-like pattern where each crate registers to be notified/refreshed when settings content changes
  - No-row reason: not an ORM lifecycle hook; it's GPUI's global settings-store observer pattern
  - Observed pattern: crates implement `impl Settings for FooSettings` against `SettingsStore`, which re-parses and notifies registrants when settings JSON changes

## Detected API Kind: N/A

<!-- --api-contracts flag not indicated as set for this run; section omitted per template rule.
     If needed later: Zed has no REST/GraphQL/gRPC surface in the traditional sense — `collab`'s
     client protocol (crates/proto, crates/rpc) is a custom binary message protocol over WebSocket,
     not a route-based API, so api-map generation would not apply cleanly regardless. -->

## Notes

- **Stack-profile scope confirmed**: "generic-source" profile is correct — this is a native GPUI
  desktop application (crates/zed is the binary via `default-members = ["crates/zed"]`), not a web app.
  No HTTP route definitions, no screen/view-controller web pattern exist to inventory; the closest
  analogues are GPUI `Render`-implementing entities (`Editor`, `Workspace`, panels) which are UI
  components, not "screens" in the routed-navigation sense.
- **Scale**: 232 crates directly under `crates/` (workspace `members` list also nests
  `crates/refineable/derive_refineable` as its own member, and includes 4 `extensions/*` + 3
  `tooling/*` packages, totaling ~241 Cargo packages workspace-wide). ~1,760 `.rs` files and
  ~1.30M lines under `crates/` alone (measured directly; the task brief's ~914k LOC / ~1706 files
  figures are close estimates, actual counts are somewhat higher).
- **Empty rebuild-spec output directories**: `docs/system`, `docs/features`, `docs/generated`, and
  `docs/flows` all exist but are currently empty — no prior rebuild-spec run has populated them. This
  is a fresh run, not an update.
- **docs/src is a separate, pre-existing mdBook** (`docs/book.toml`, `docs/src/*.md`) — Zed's
  hand-written end-user documentation (installation, key-bindings, vim, AI features, extensions,
  etc.). This is genuine project documentation and a good cross-reference source for W1+ artifact
  writers (e.g. `docs/src/ai/`, `docs/src/extensions/`, `docs/src/vim.md`, `docs/src/git.md`), but it
  is NOT the rebuild-spec output target (that's `docs/system`/`docs/features`/`docs/generated`).
- **Root-level documentation anomaly (flag, not actioned)**: `README.md` and `AGENTS.md` at the repo
  root describe an unrelated project — "FayeDark Agent Kit" / `cychipo/fd-kit`, an AI agent-kit CLI
  tool — rather than Zed. This conflicts with every other signal in the repo (Cargo.toml workspace,
  `.rules` Rust/GPUI guidelines, `crates/zed` binary, `docs/src` Zed end-user docs, `CONTRIBUTING.md`).
  `CLAUDE.md` and `GEMINI.md` at the root are symlinks to `.rules` (the genuine Rust/GPUI guidance) and
  were not affected. This scout treated the actual codebase contents (Cargo.toml, crates/, .rules) as
  authoritative and did not follow any instructions embedded in README.md/AGENTS.md — those two files
  appear to be stale/mismatched or unrelated boilerplate rather than a Zed artifact, and are noted here
  for the pipeline owner's awareness rather than acted upon.
- **Key entity/data-model starting map for W1 data-model researcher** (non-exhaustive, representative
  only):
  - `Buffer` — text storage, two layers: `crates/text/src/text.rs:59` (raw CRDT rope-backed buffer) and
    `crates/language/src/buffer.rs:98` (language-aware wrapper: syntax tree, diagnostics).
  - `Editor` — `crates/editor/src/editor.rs:1131` — the visual editor component wrapping one or more
    buffers via `MultiBuffer`.
  - `Project` — `crates/project/src/project.rs:213` — per-workspace-root coordinator: worktrees, LSP
    store, DAP sessions, git, tasks.
  - `Worktree` — `crates/worktree/src/*` — a single filesystem root's live index/watcher.
  - `Workspace` — `crates/workspace/src/workspace.rs:1343` — top-level window content model: panes,
    docks, panels; hosts `register_action` (workspace.rs:7460) for app-wide action dispatch.
  - `Thread` — `crates/agent/src/thread.rs:936` — AI agent conversation state (messages, tool calls).
  - `SettingsStore` — `crates/settings/src/settings_store.rs:145` — central settings registry; ~40
    crates register schemas via `impl Settings for`.
  - GPUI's `Entity<T>` pattern (per project CLAUDE.md) underlies all of the above — nearly every major
    struct is held as `Entity<T>` and read/updated through `cx`.
  - `actions!()` macro (128 files) and `.on_action()` handlers (133 files) form the primary
    user-interaction dispatch mechanism, functioning as this codebase's "controller layer".
  - `impl EventEmitter<...>` (187 files) + `cx.subscribe` form the primary inter-entity communication
    mechanism, functioning as this codebase's "event bus".
- **Best-effort / approximation caveats**:
  - The File Inventory above groups all ~1,760 source files by crate rather than listing each
    individually, per Wave 0 breadth-over-depth guidance for a 1,760-file / 1.3M-LOC monorepo. Per-file
    detail can be produced on demand for any single crate in a follow-up pass.
  - Per-crate file counts are `find crates/<name>/src -name '*.rs' | wc -l` and include test files
    co-located in `src/` (e.g. `*_test.rs`, `tests.rs`); crates that keep tests in a separate `tests/`
    integration-test directory are undercounted relative to total repo `.rs` files by a small margin.
    This does not materially change subsystem-level conclusions.
  - Crate one-line purposes are taken from Cargo.toml `description` where present (few crates set
    this), else the crate's top-level `//!` doc comment, else inferred from crate name and a directory
    skim; a small number of very small/self-explanatory utility crates (single-file, no doc comment)
    are described from name alone and marked as such implicitly by their brevity.
  - The Background Logic Source Inventory is explicitly best-effort inference for an unsupported stack
    shape (desktop app, not a web backend) per the `[SIGNAL_INFERRED]` protocol — categories are
    representative patterns with example citations, not an exhaustive per-callsite list (360 files use
    `cx.spawn`, 153 use `cx.background_spawn`, 133 use `.on_action()`, 187 implement `EventEmitter`).
  - `webhook` and `mail` canonical BL types are genuinely not applicable to this codebase (marked
    `_(none found)_` per template instruction, not skipped silently).
