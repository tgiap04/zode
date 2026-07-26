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

