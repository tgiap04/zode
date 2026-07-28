
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

**Project**: Zed
**Generated**: 2026-07-26
**Architecture Type**: Native multi-platform desktop application (GPUI-based), with an optional collaboration backend service

## Executive Summary

Zed is a native, GPU-accelerated code editor written in Rust, built on the project's own in-house UI framework, GPUI (`crates/gpui`). The workspace is a single Cargo monorepo of ~241 packages (232 under `crates/`, plus bundled/sample WASM extensions under `extensions/` and dev-tooling crates under `tooling/`), compiling to one primary binary (`crates/zed`, `default-members`) that runs on macOS, Linux, and Windows (with an experimental WASM/web backend in `crates/gpui_web`).

Architecturally the system is organized as concentric layers rather than a client/server split for the local app: GPUI (rendering, windowing, entity/event model, async executors) underpins an editor-core layer (`text`/`rope`/`sum_tree` for CRDT-like buffer storage, `language` for syntax/diagnostics-aware buffers, `multi_buffer`, `editor`), which is coordinated by a project layer (`project`, `worktree`, `workspace`, `fs`, `db`) that wires in Language Server Protocol and Debug Adapter Protocol support, a WASM extension host (`extension_host`), settings/theme/keymap infrastructure (`settings`, `theme`, migration engine), and a large first-party UI component library (`ui`, 106 files). Layered on top of the editor core is an AI/agent subsystem (`agent`, `agent_ui`, `language_model` + ~13 per-vendor LLM provider crates, `edit_prediction*` for inline AI completions) and an optional real-time collaboration subsystem: a separately-deployable `collab` server plus client-side `call`/`channel`/`client`/`rpc`/`proto` crates communicating over a custom binary RPC protocol (not HTTP routes), with LiveKit providing WebRTC voice/video.

This is a desktop application, not a web app: there are no HTTP routes or "screens" in the routed-navigation sense (route-list/screen-list/screen-flow/api-map artifacts are out of scope for this project per the generic-source stack profile). The nearest analogues to "screens" are GPUI `Render`-implementing entities (`Editor`, `Workspace`, dock panels), and the nearest analogue to a request-routing layer is Zed's `actions!()` dispatch mechanism (128 files define actions; 133 files register `.on_action()` handlers) combined with the `impl EventEmitter<...>` / `cx.subscribe` pub-sub pattern (187 files) used for inter-entity communication.

For per-subsystem crate detail and the background-logic inventory, see [scout-report.md](scout-report.md).

## Key Design Decisions

### Decision 1: A first-party UI framework (GPUI) instead of an existing GUI toolkit

**Context**: A code editor needs low-latency, GPU-accelerated text rendering and fine-grained control over layout, input, and cross-platform windowing that general-purpose Rust GUI crates did not offer at the performance/control level Zed's editor core requires.

**Decision**: Build and own GPUI (`crates/gpui`, 77 files) as the foundation: an entity/context model (`Entity<T>`, `Context<T>`, `App`), Taffy-based flexbox layout, per-platform rendering backends (`gpui_macos` via Metal/objc2, `gpui_linux` via X11/Wayland, `gpui_windows` via Direct3D, `gpui_wgpu` shared wgpu renderer, experimental `gpui_web`), and its own async executor abstractions (`cx.spawn`, `cx.background_spawn`) rather than depending on Tokio directly (a `gpui_tokio` bridge crate exists only for crates that need Tokio, e.g. the AWS SDK).

**Rationale**: Owning the framework lets Zed co-design the rendering pipeline with the editor's text/rope data structures for performance, and gives one consistent concurrency/entity model (`Entity<T>`, events, actions) used uniformly across editor, project, AI, and collaboration subsystems — avoiding a patchwork of different UI-toolkit and async idioms.

### Decision 2: Custom binary RPC protocol for collaboration instead of a REST/GraphQL API

**Context**: Real-time collaboration (shared projects, cursors, chat, calls) needs low-latency bidirectional messaging between many desktop clients and a central `collab` server — a shape that a request/response HTTP API is not naturally suited to.

**Decision**: Define a custom wire protocol (`crates/proto`) and shared RPC framing (`crates/rpc`) used symmetrically by the desktop client (`client`, `call`, `channel`) and the separately-deployed `collab` server binary (`crates/collab`), rather than exposing REST/GraphQL routes.

**Rationale**: A typed, protobuf-esque message protocol over a persistent connection supports low-latency bidirectional push (presence, edits, calls) more naturally than polling or per-request HTTP, and lets client and server share the same generated message types. This is also why no `api-map`/route-list artifact applies to this codebase — `collab`'s "API surface" is protocol messages, not HTTP routes.

### Decision 3: WASM sandboxing for third-party extensions

**Context**: Extensions (language grammars, themes, LSP/DAP adapters, slash-commands) are third-party code that must not be able to crash or compromise the host editor process, while still needing controlled access to project files and network in specific cases.

**Decision**: Compile extensions to `wasm32-wasip2` and run them inside a sandboxed WASM runtime hosted by `crates/extension_host` (17 files), with a stable Rust API surface (`crates/extension_api`) that extension authors code against, and calls dispatched off the main thread via `cx.background_spawn`.

**Rationale**: WASM sandboxing isolates untrusted extension code from the host process (memory safety, no arbitrary syscalls) while still allowing extensions to be distributed as portable binaries across all supported platforms; bundled/sample extensions (`extensions/glsl`, `extensions/html`, `extensions/proto`) validate the same API path used by third-party publishers.

## Security Overview

- **Authentication**: Desktop client authenticates to Zed's cloud/collab backend via `crates/client` (auth + websocket connection) using OS-keychain-backed credential storage (`crates/credentials_provider`, `zed_credentials_provider`); SSH remote development uses `crates/askpass` for interactive/non-interactive SSH credential prompts.
- **Authorization**: No traditional RBAC/route-guard system was found (not a multi-tenant web app); the closest analogue is collaboration-channel/room membership enforced server-side in `crates/collab`, and extension sandboxing (WASM) which limits what loaded third-party code can access rather than gating user roles.
- **Data Encryption**: Network transport relies on `crates/http_client_tls` / `reqwest_client` for TLS on outbound HTTP (LLM provider APIs, update checks, telemetry) and LiveKit's own WebRTC transport security for call media; no application-level at-rest encryption layer was identified in scope for this pass (local SQLite state via `crates/db`/`sqlez` is unencrypted on disk, consistent with a local developer tool).
- **API Security**: Third-party LLM provider credentials (Anthropic, OpenAI, Google, Bedrock, etc.) are stored via the credentials provider and sent per-request over TLS by each vendor-specific client crate (`crates/anthropic`, `open_ai`, etc.); Zed's own cloud proxy (`cloud_api_client`) mediates auth/billing/model routing for cloud-hosted model usage rather than exposing raw vendor keys to all code paths.

## Scalability

- **Current Capacity**: Single-user desktop process per running instance; scaling is per-editor-instance (large files, many open buffers/worktrees) rather than concurrent-request throughput. The `collab` server is the one component with traditional multi-client server scaling concerns (rooms, calls, channels across many simultaneously connected desktop clients).
- **Scaling Strategy**: Editor-side: async task offloading via GPUI's `cx.background_spawn`/`Scheduler` abstraction keeps LSP requests, extension calls, indexing, and AI streaming off the UI thread (360 files use `cx.spawn`, 153 use `cx.background_spawn`); large-file/large-project performance relies on the `Rope`/`SumTree` data structures for efficient text indexing and `Worktree` for incremental filesystem watching rather than full re-scans. Server-side (`collab`): a separately deployable service, scaled independently of the desktop client release cadence.
- **Performance Targets**: No explicit numeric SLOs were found in scanned sources; `tooling/perf` exists as an in-repo benchmarking harness and `crates/*_benchmarks` crates (e.g. `worktree_benchmarks`, `fs_benchmarks`) provide micro-benchmarks, but this pass found no documented target latencies/throughput figures to cite — flagged as a gap for a deeper performance-focused research pass if needed.
