<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Behavior Logic

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). Five of the
original 13 items described subsystems this fork deleted entirely (auto-update, the AI agent
thread loop, per-vendor LLM provider clients, LiveKit calling) or reduced to something no longer
fitting a behavior-logic pattern (the notification center, now just a UI toast component) — those
are removed rather than rewritten. The remaining 8 items are renumbered sequentially.

**Project**: Zode
**Analysis Scope**: Full monorepo, `generic-source` profile (no screen-list/screen-flow upstream — `screen_source:none`)

**Code Format**: All codes follow `BL###_NameSlug` format.

**Behavior Logic Types** (canonical 10 — language-neutral): `scheduled-job`, `queue-worker`, `event-listener`, `observer`, `mail`, `notification`, `middleware`, `custom-command`, `integration`, `webhook`.

## ⚠️ Representative-Not-Exhaustive Notice

Zode is a native GPUI desktop application, not an HTTP-framework backend, so none of the canonical
BL types have a direct framework row (no Actix/Axum routes, no `tokio-cron`, no Kafka/Bull queue,
no ORM lifecycle hooks). Every item below is tagged `[SIGNAL_INFERRED]`: GPUI's own primitives
(`cx.spawn`, `cx.background_spawn`, `EventEmitter`/`cx.subscribe`, `actions!`/`.on_action()`,
`impl Settings for`) are treated as the closest analogues of the 10 canonical types.

This inventory is a representative sample of pattern categories, not an exhaustive
per-call-site enumeration.

`webhook`, `mail`, and `notification` are genuinely absent now: there is no server to receive
inbound webhooks, a desktop editor has no email-sending logic, and the in-app notification center
this fork removed (it stored collab invites/mentions) left behind only a plain UI toast component
(`status_toast`) that doesn't fit any of the 10 canonical background-logic types.

---

## Behavior Logic Index

| Code | Name | Type | Trigger |
|------|------|------|---------|
| BL001_SchedulerTrait | Scheduler trait | scheduled-job | Deterministic timer/timeout abstraction used across GPUI |
| BL002_ExtensionHostWasmDispatch | Extension host WASM dispatch | queue-worker | Background execution of sandboxed WASM extension calls |
| BL003_WorkspaceEventEmitterSubscribe | Workspace event emitter/subscribe | event-listener | Entity pub/sub via `cx.emit`/`cx.subscribe` |
| BL004_LspRequestDispatch | LSP request dispatch | middleware | LSP request/response handler table on `Project`/`LspStore` |
| BL005_RpcProtoFraming | RPC proto framing | middleware | Wire-protocol framing for remote development (local ↔ `remote_server`) |
| BL006_GitHostingProviderDetection | Git hosting provider detection | integration | GitHub/GitLab/Bitbucket permalink & PR metadata detection |
| BL007_StandaloneCliBinaries | Standalone CLI binaries | custom-command | Binary invoked directly from the command line |
| BL008_SettingsStoreObserver | Settings-store observer | observer | Settings JSON content changes and `SettingsStore` re-parses it |

---

## BL001_SchedulerTrait

**Type**: scheduled-job
**Trigger**: Any code path requesting a deterministic timer/timeout
**Source File**: crates/scheduler/src/scheduler.rs
**Source Symbol**: Scheduler [SIGNAL_INFERRED]

### Description

Custom in-house scheduling abstraction (`pub trait Scheduler: Send + Sync`), not a named crate
like `tokio-cron`/`gocron`. Backs timers/timeouts across GPUI; a fake implementation drives
deterministic tests by controlling virtual time.

### Related Modules

- crates/scheduler

---

## BL002_ExtensionHostWasmDispatch

**Type**: queue-worker
**Trigger**: Extension invokes a host-provided capability
**Source File**: crates/extension_host/src
**Source Symbol**: extension_host [SIGNAL_INFERRED]

### Description

Custom WASM host runtime, not a standard queue library. Extension calls are dispatched via
`cx.background_spawn` into the wasm runtime and results routed back to the main thread —
background execution of sandboxed extension calls off the main thread.

### Related Modules

- crates/extension_host

---

## BL003_WorkspaceEventEmitterSubscribe

**Type**: event-listener
**Trigger**: Any entity calling `cx.emit(event)`
**Payload**: Entity-defined event enum (varies per `impl EventEmitter<Event>`)
**Source File**: crates/workspace/src/workspace.rs
**Source Symbol**: Workspace [SIGNAL_INFERRED]

### Description

GPUI's pub/sub is a first-party entity-event system. Entities `cx.emit(event)`; other entities
register `cx.subscribe(other_entity, |this, other, event, cx| ...)` at construction time, storing
the result as a `Subscription` field.

### Related Modules

- crates/workspace

---

## BL004_LspRequestDispatch

**Type**: middleware
**Trigger**: Editor issues or receives an LSP protocol message (completions, code actions, queries)
**Source File**: crates/project/src/lsp_store.rs
**Source Symbol**: LspStore::handle_lsp_get_completions [SIGNAL_INFERRED]

### Description

This is an LSP client dispatch table, not an HTTP middleware stack. `async fn handle_lsp_*`
methods on `Project`/`LspStore` intercept, transform, and route LSP protocol messages.

### Related Modules

- crates/project (lsp_store.rs)

---

## BL005_RpcProtoFraming

**Type**: middleware
**Trigger**: Local instance or `remote_server` sends/receives a proto message over the connection
**Source File**: crates/rpc/src
**Source Symbol**: rpc [SIGNAL_INFERRED]

### Description

Custom binary wire protocol, not HTTP. Typed proto message enums are matched and routed to
per-message handler functions. This fork removed the collaboration server this framing used to
route messages to/from — it now carries local-to-`remote_server` traffic for SSH remote
development instead (`crates/remote`, `crates/remote_connection`).

### Related Modules

- crates/rpc
- crates/proto

---

## BL006_GitHostingProviderDetection

**Type**: integration
**Trigger**: Editor needs to resolve a permalink or PR metadata for the current git remote
**Source File**: crates/git_hosting_providers/src
**Source Symbol**: GitHostingProvider [SIGNAL_INFERRED]

### Description

Provider-detection module set, not an SDK client library. One submodule per hosting provider
implementing a shared `GitHostingProvider` trait, used for permalinks and PR metadata across
GitHub/GitLab/Bitbucket.

### Related Modules

- crates/git_hosting_providers

---

## BL007_StandaloneCliBinaries

**Type**: custom-command
**Trigger**: Binary invoked directly from the command line
**Source File**: crates/cli/src
**Source Symbol**: main [SIGNAL_INFERRED]

### Description

Each is its own binary crate with a `main.rs`/binary target parsing CLI args and driving a
focused task. `crates/cli/src` (launching the app) is the representative example; the same
standalone-binary pattern also covers `crates/extension_cli` (packaging extensions) and
`crates/schema_generator` (generating JSON schemas). This fork removed `eval_cli` and
`edit_prediction_cli` along with the AI subsystems they supported.

### Related Modules

- crates/cli, crates/extension_cli, crates/schema_generator

---

## BL008_SettingsStoreObserver

**Type**: observer
**Trigger**: Settings JSON content changes and `SettingsStore` re-parses it
**Source File**: crates/settings/src/settings_store.rs
**Source Symbol**: SettingsStore [SIGNAL_INFERRED]

### Description

GPUI's global settings-store observer pattern. Crates implement `impl Settings for FooSettings`
against `SettingsStore`, which re-parses settings JSON and notifies registrants when it changes —
the closest analogue to a model lifecycle hook (created/updated/deleted).

### Related Modules

- crates/settings (settings_store.rs)

---

## Summary

- **Total Behavior Logic Items**: 8 (representative pattern categories, not per-call-site count)
- **By Type**: custom-command: 1, event-listener: 1, integration: 1, mail: 0 (none found), middleware: 2, notification: 0 (none found), observer: 1, queue-worker: 1, scheduled-job: 1, webhook: 0 (none found)

---

## Cross-Reference Validation

- [x] All BL### codes are unique
- [ ] All BL### codes are referenced in UserStories.md (type=system) — N/A this pass, `screen_source:none` / no upstream US artifact generated
- [ ] All BL### codes are referenced in FeatureList.md — N/A this pass, no upstream FeatureList generated
- [x] No related route/data-model references claimed (none available upstream; fields correctly omitted rather than fabricated)
- [x] All BL items have Source File + Source Symbol fields
- [x] All items explicitly tagged `[SIGNAL_INFERRED]`

---

## Client-Side Logic

Zode is a native GPUI desktop application with no browser DOM/JS runtime; the debounce/throttle,
optimistic-UI, polling, upload-progress, and realtime-websocket extraction signatures in this
section target web-frontend code patterns that do not apply to this codebase's stack.

### Debounce / Throttle

N/A — no debounce or throttle patterns detected (no web-frontend JS/TS runtime in this codebase).

### Optimistic UI

N/A — no optimistic UI patterns detected (no web-frontend JS/TS runtime in this codebase).

### Polling

N/A — no polling patterns detected in the web-frontend sense. This fork removed the one polling
loop that existed (the auto-update poller) along with the auto-updater itself.

### Upload Progress

N/A — no upload progress patterns detected (no web-frontend JS/TS runtime in this codebase).

### Realtime (WebSocket / SSE / EventSource)

N/A — no browser WebSocket/EventSource patterns detected. Closest analogue is
BL005_RpcProtoFraming (native binary RPC protocol), already documented above as middleware, not
duplicated here.

<!-- docs/system/business-rules.md draft deferred: no business-rule (BR/DEC/SM/ALG) source
     material exists in this generic-source, no-screen-list pass — this artifact covers
     background/system logic only, not user-facing business rules. -->
