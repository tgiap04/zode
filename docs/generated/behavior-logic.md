<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Behavior Logic

> [!CAUTION]
> **STALE — do not treat this document as a description of the current code.**
> It was generated on 2026-07-26 against the pre-fork tree of 240 packages /
> 232 crates. The hard fork has since removed 54 crates and gutted several
> more; the workspace is now 186 packages / 178 crates.
>
> Anything here describing accounts, sign-in, collaboration, calls, channels,
> AI agents, LLM providers, edit prediction, auto-update or crash reporting is
> **fiction** — that code no longer exists. Feature codes F007, F008, F013,
> F019, F020, F021 and F022 in particular no longer have an implementation.
>
> Regeneration is deliberately deferred until the fork is green and verified
> (`/tkm:rebuild-spec` after phase 11). Running it against a half-cut tree
> would just produce a second stale document.


**Project**: Zed (zode)
**Generated**: 2026-07-26
**Analysis Scope**: Full monorepo, 1.3M LOC, `generic-source` profile (no screen-list/screen-flow upstream — `screen_source:none`)

**Code Format**: All codes follow `BL###_NameSlug` format.

**Behavior Logic Types** (canonical 10 — language-neutral): `scheduled-job`, `queue-worker`, `event-listener`, `observer`, `mail`, `notification`, `middleware`, `custom-command`, `integration`, `webhook`.

## ⚠️ Representative-Not-Exhaustive Notice

Zed is a native GPUI desktop application, not an HTTP-framework backend, so none of the canonical
BL types have a direct framework row (no Actix/Axum routes, no `tokio-cron`, no Kafka/Bull queue,
no ORM lifecycle hooks). Every item below is tagged `[SIGNAL_INFERRED]` per the scout inventory's
adaptation protocol: GPUI's own primitives (`cx.spawn`, `cx.background_spawn`, `EventEmitter`/
`cx.subscribe`, `actions!`/`.on_action()`, `impl Settings for`) are treated as the closest
analogues of the 10 canonical types.

**This inventory is a best-effort, representative sample of PATTERN CATEGORIES — it is NOT an
exhaustive per-call-site enumeration.** The scout inventory itself reports raw hit counts far
exceeding the number of BL items below (e.g. 360 files contain `cx.spawn`, 153 contain
`cx.background_spawn`, 133 use `.on_action()`, 187 implement `EventEmitter`). Per Wave 0
breadth-over-depth guidance for a 1.3M LOC codebase, each BL### item here represents ONE
recurring pattern/category, evidenced by 1-5 representative `file:line` citations pulled directly
from the scout `## Background Logic Source Inventory` section — not a claim that only that file
uses the pattern. Do not read the BL### count below as "the total count of background-logic call
sites in Zed"; read it as "the total count of distinct background-logic pattern categories
identified in this pass."

`webhook` and `mail` are genuinely absent (not under-sampled): Zed's `collab` server uses a custom
binary RPC/proto protocol, not inbound HTTP webhooks; a desktop editor has no email-sending logic.

---

## Behavior Logic Index

| Code | Name | Type | Trigger |
|------|------|------|---------|
| BL001_AutoUpdatePoller | Auto-update poller | scheduled-job | App-start background loop that periodically polls the update server |
| BL002_SchedulerTrait | Scheduler trait | scheduled-job | Deterministic timer/timeout abstraction used across GPUI |
| BL003_AgentThreadToolCallLoop | Agent thread tool-call loop | queue-worker | Agent thread loop awaiting streamed model responses and dispatching queued tool calls |
| BL004_ExtensionHostWasmDispatch | Extension host WASM dispatch | queue-worker | Background execution of sandboxed WASM extension calls |
| BL005_WorkspaceEventEmitterSubscribe | Workspace event emitter/subscribe | event-listener | Entity pub/sub via `cx.emit`/`cx.subscribe` |
| BL006_LspRequestDispatch | LSP request dispatch | middleware | LSP request/response handler table on `Project`/`LspStore` |
| BL007_RpcProtoMessageRouting | RPC proto message routing | middleware | Client↔collab-server binary RPC message routing |
| BL008_InAppNotificationCenter | In-app notification center | notification | In-app notification model (invites, mentions) |
| BL009_LlmProviderClients | LLM provider clients | integration | Per-vendor LLM API client crates |
| BL010_LiveKitCallingIntegration | LiveKit calling integration | integration | WebRTC calling via LiveKit SDK |
| BL011_GitHostingProviderDetection | Git hosting provider detection | integration | GitHub/GitLab/Bitbucket permalink & PR metadata detection |
| BL012_StandaloneCliBinaries | Standalone CLI binaries | custom-command | Binary invoked directly from the command line |
| BL013_SettingsStoreObserver | Settings-store observer | observer | Settings JSON content changes and `SettingsStore` re-parses it |

---

## BL001_AutoUpdatePoller

**Type**: scheduled-job
**Trigger**: App start; re-arms itself on a sleep/timer loop
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/auto_update/src/auto_update.rs
**Source Symbol**: auto_update [SIGNAL_INFERRED]

### Description

No HTTP-framework cron scheduler exists in Zed; this is the app's own async task loop, not
`tokio-cron`/Actix. A background task is spawned at app start via `cx.background_spawn` that
periodically polls the update server for a new release and re-arms itself after a sleep/timer,
functioning as the closest analogue to a scheduled job.

### Related Modules

- crates/auto_update

---

## BL002_SchedulerTrait

**Type**: scheduled-job
**Trigger**: Any code path requesting a deterministic timer/timeout
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/scheduler/src/scheduler.rs
**Source Symbol**: Scheduler [SIGNAL_INFERRED]

### Description

Custom in-house scheduling abstraction (`pub trait Scheduler: Send + Sync`), not a named crate
like `tokio-cron`/`gocron`. Backs timers/timeouts across GPUI; a `test_scheduler.rs` fake
implementation drives deterministic tests by controlling virtual time.

### Related Modules

- crates/scheduler

---

## BL003_AgentThreadToolCallLoop

**Type**: queue-worker
**Trigger**: Agent conversation turn begins; loop runs while pending tool calls/messages exist
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/agent/src/thread.rs
**Source Symbol**: Thread [SIGNAL_INFERRED]

### Description

Not a message-queue library (no Kafka/Bull/asynq) — an in-process `cx.spawn` loop over agent
turns. `Thread` drives a loop that awaits streamed model responses and dispatches queued tool
invocations, the closest analogue to an async queue consumer.

### Related Modules

- crates/agent (thread.rs:936)

---

## BL004_ExtensionHostWasmDispatch

**Type**: queue-worker
**Trigger**: Extension invokes a host-provided capability
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/extension_host/src
**Source Symbol**: extension_host [SIGNAL_INFERRED]

### Description

Custom WASM host runtime, not a standard queue library. Extension calls are dispatched via
`cx.background_spawn` into the wasmtime runtime and results routed back to the main thread —
background execution of sandboxed extension calls off the main thread.

### Related Modules

- crates/extension_host

---

## BL005_WorkspaceEventEmitterSubscribe

**Type**: event-listener
**Trigger**: Any entity calling `cx.emit(event)`
**Payload**: Entity-defined event enum (varies per `impl EventEmitter<Event>` — no fixed channel/topic; payload is the Rust event type itself)
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/workspace.rs
**Source Symbol**: Workspace [SIGNAL_INFERRED]

### Description

GPUI's pub/sub is a first-party entity-event system, not a library like `@OnEvent`/
`ActiveSupport::Notifications`. Entities `cx.emit(event)`; other entities register
`cx.subscribe(other_entity, |this, other, event, cx| ...)` at construction time, storing the
result as a `Subscription` field. 187 files across the codebase implement `impl EventEmitter<...>`
— `workspace.rs` is the representative example, not the only one.

### Related Modules

- crates/workspace

---

## BL006_LspRequestDispatch

**Type**: middleware
**Trigger**: Editor issues or receives an LSP protocol message (completions, code actions, queries)
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/lsp_store.rs
**Source Symbol**: LspStore::handle_lsp_get_completions [SIGNAL_INFERRED]

### Description

This is an LSP client dispatch table, not an HTTP middleware stack (auth middleware excluded per
protocol — see Permissions.md). `async fn handle_lsp_*` methods on `Project`/`LspStore`
intercept, transform, and route LSP protocol messages. Representative handlers span
lsp_store.rs:8983-9401, including `handle_lsp_command`, `handle_lsp_query`, and
`handle_apply_code_action`.

### Related Modules

- crates/project (lsp_store.rs)

---

## BL007_RpcProtoMessageRouting

**Type**: middleware
**Trigger**: Client or `collab` server receives a proto message over the RPC connection
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/rpc/src
**Source Symbol**: rpc [SIGNAL_INFERRED]

### Description

Custom binary RPC protocol, not HTTP. Typed proto message enums are matched and routed to
per-message handler functions registered on client/server peer objects — the RPC dispatch layer
routing proto messages between client and `collab` server.

### Related Modules

- crates/rpc
- crates/proto

---

## BL008_InAppNotificationCenter

**Type**: notification
**Trigger**: A collab invite, mention, or other in-app event occurs
**Payload**: Notification entity model fields (invite/mention metadata) surfaced to the UI — exact schema in crates/notifications/src (doc-confirmed via `pub struct` model crate)
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/notifications/src
**Source Symbol**: notifications [SIGNAL_INFERRED]

### Description

Standard in-app notification center, not a named notification library. An entity model stores and
surfaces collab invites, mentions, and other app notifications to the UI.

### Related Modules

- crates/notifications

---

## BL009_LlmProviderClients

**Type**: integration
**Trigger**: Agent/assistant feature issues a request to a configured LLM provider
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/anthropic/src
**Source Symbol**: anthropic [SIGNAL_INFERRED]

### Description

Not `@FeignClient`/SDK-injection style — each vendor integration is a bespoke reqwest-based HTTP
client crate defining request/response structs mirroring the vendor's REST API and a client
struct implementing `language_model`'s provider trait. `crates/anthropic/src` is the representative
example; the same pattern repeats per-vendor across crates/open_ai, crates/google_ai,
crates/bedrock, crates/mistral, crates/ollama, crates/lmstudio, crates/open_router,
crates/vercel, crates/x_ai, crates/copilot, and crates/copilot_chat.

### Related Modules

- crates/anthropic, crates/open_ai, crates/google_ai, crates/bedrock, crates/mistral,
  crates/ollama, crates/lmstudio, crates/open_router, crates/vercel, crates/x_ai,
  crates/copilot, crates/copilot_chat

---

## BL010_LiveKitCallingIntegration

**Type**: integration
**Trigger**: User starts or joins a voice/video call
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/livekit_api/src
**Source Symbol**: livekit_api [SIGNAL_INFERRED]

### Description

No framework-specific injection pattern — a direct SDK binding crate. `livekit_api` wraps
LiveKit's server API; `livekit_client` adapts it into GPUI's async/entity model for the `call`
feature (WebRTC calling).

### Related Modules

- crates/livekit_api, crates/livekit_client

---

## BL011_GitHostingProviderDetection

**Type**: integration
**Trigger**: Editor needs to resolve a permalink or PR metadata for the current git remote
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_hosting_providers/src
**Source Symbol**: GitHostingProvider [SIGNAL_INFERRED]

### Description

Provider-detection module set, not an SDK client library. One submodule per hosting provider
implementing a shared `GitHostingProvider` trait, used for permalinks and PR metadata across
GitHub/GitLab/Bitbucket.

### Related Modules

- crates/git_hosting_providers

---

## BL012_StandaloneCliBinaries

**Type**: custom-command
**Trigger**: Binary invoked directly from the command line
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/cli/src
**Source Symbol**: main [SIGNAL_INFERRED]

### Description

Not a `cobra`/`clap`-subcommand-of-server pattern — each is its own binary crate with a `main.rs`/
binary target parsing CLI args and driving a focused task. `crates/cli/src` (launching zed) is the
representative example; the same standalone-binary pattern also covers crates/extension_cli
(packaging extensions), crates/eval_cli (running the eval harness), crates/edit_prediction_cli,
and crates/schema_generator (generating JSON schemas).

### Related Modules

- crates/cli, crates/extension_cli, crates/eval_cli, crates/edit_prediction_cli,
  crates/schema_generator

---

## BL013_SettingsStoreObserver

**Type**: observer
**Trigger**: Settings JSON content changes and `SettingsStore` re-parses it
**Payload**: N/A — not an event/notification type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/settings/src/settings_store.rs
**Source Symbol**: SettingsStore [SIGNAL_INFERRED]

### Description

Not an ORM lifecycle hook — GPUI's global settings-store observer pattern. Crates implement
`impl Settings for FooSettings` against `SettingsStore` (~40 hits across the codebase), which
re-parses settings JSON and notifies registrants when it changes — the closest analogue to a
model lifecycle hook (created/updated/deleted).

### Related Modules

- crates/settings (settings_store.rs:145)

---

## Summary

- **Total Behavior Logic Items**: 13 (representative pattern categories, not per-call-site count — see Representative-Not-Exhaustive Notice above)
- **By Type**: custom-command: 1, event-listener: 1, integration: 3, mail: 0 (none found), middleware: 2, notification: 1, observer: 1, queue-worker: 2, scheduled-job: 2, webhook: 0 (none found)

---

## Cross-Reference Validation

- [x] All BL### codes are unique
- [ ] All BL### codes are referenced in UserStories.md (type=system) — N/A this pass, `screen_source:none` / no upstream US artifact generated
- [ ] All BL### codes are referenced in FeatureList.md — N/A this pass, no upstream FeatureList generated
- [x] No related route/data-model references claimed (none available upstream; fields correctly omitted rather than fabricated)
- [x] All BL items have Source File + Source Symbol fields (Rule C2)
- [x] All Source File paths match scout Background Logic Source Inventory entries (Rule C2/C3)
- [x] All items explicitly tagged `[SIGNAL_INFERRED]` per scout protocol; justification given in each Description (Rule C3)

---

## Client-Side Logic

Zed is a native GPUI desktop application with no browser DOM/JS runtime; the debounce/throttle,
optimistic-UI, polling, upload-progress, and realtime-websocket extraction signatures in this
section target web-frontend code patterns (`setTimeout`, `XHR.upload.onprogress`,
`new WebSocket(...)`, React hooks, etc.) that do not apply to this codebase's stack.

### Debounce / Throttle

N/A — no debounce or throttle patterns detected (no web-frontend JS/TS runtime in this codebase).

### Optimistic UI

N/A — no optimistic UI patterns detected (no web-frontend JS/TS runtime in this codebase).

### Polling

N/A — no polling patterns detected in the web-frontend sense. Closest analogue is
BL001_AutoUpdatePoller (native background task loop), already documented above as a
scheduled-job, not duplicated here.

### Upload Progress

N/A — no upload progress patterns detected (no web-frontend JS/TS runtime in this codebase).

### Realtime (WebSocket / SSE / EventSource)

N/A — no browser WebSocket/EventSource patterns detected. Closest analogue is
BL007_RpcProtoMessageRouting (native binary RPC protocol), already documented above as
middleware, not duplicated here.

<!-- docs/system/business-rules.md draft deferred: no business-rule (BR/DEC/SM/ALG) source
     material exists in this generic-source, no-screen-list pass — this artifact covers
     background/system logic only, not user-facing business rules. -->
