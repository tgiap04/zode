**Project**: Zode
**Generated**: 2026-07-26; rewritten 2026-08-04 against the post-fork tree (187 packages / 178 crates)
**Architecture Type**: Native multi-platform desktop application (GPUI-based). No server component — no cloud, no collaboration backend, no AI backend.

## Executive Summary

Zode is a fork of Zed — a native, GPU-accelerated code editor written in Rust, built on the
project's own in-house UI framework, GPUI (`crates/gpui`) — with every account, cloud,
AI/agent, real-time-collaboration, edit-prediction, auto-update, and telemetry/crash-reporting
code path removed. The workspace is a single Cargo monorepo of 187 packages (178 under
`crates/`, plus bundled/sample WASM extensions under `extensions/` and dev-tooling crates under
`tooling/`), compiling to one primary binary (`crates/zed`, package name `zode`) that runs on
macOS, Linux, and Windows (with an experimental WASM/web backend in `crates/gpui_web`).

Architecturally the system is organized as concentric layers for a purely local app — there is
no client/server split anymore: GPUI (rendering, windowing, entity/event model, async executors)
underpins an editor-core layer (`text`/`rope`/`sum_tree` for buffer storage, `language` for
syntax/diagnostics-aware buffers, `multi_buffer`, `editor`), which is coordinated by a project
layer (`project`, `worktree`, `workspace`, `fs`, `db`) that wires in Language Server Protocol and
Debug Adapter Protocol support, a WASM extension host (`extension_host`), settings/theme/keymap
infrastructure (`settings`, `theme`, migration engine), and a large first-party UI component
library (`ui`).

This is a desktop application, not a web app: there are no HTTP routes or "screens" in the
routed-navigation sense (route-list/screen-list/screen-flow/api-map artifacts are out of scope
per the generic-source stack profile). The nearest analogues to "screens" are GPUI
`Render`-implementing entities (`Editor`, `Workspace`, dock panels), and the nearest analogue to
a request-routing layer is Zed's `actions!()` dispatch mechanism combined with the
`impl EventEmitter<...>` / `cx.subscribe` pub-sub pattern used for inter-entity communication.

## What this fork removed, and why

See `plans/260726-1531-remove-auth-cloud-hard-fork/plan.md` for the full 12-phase history. In
summary:

- **Authentication and accounts** (`ClientCredentialsProvider`, sign-in flow, OS-keychain
  credential storage for a Zed account) — deleted. No account exists to authenticate.
- **Cloud and real-time collaboration** — the `collab` server, `call`, `channel` crates, LiveKit
  WebRTC voice/video, the custom binary RPC protocol's connection-establishment path — deleted.
  Editing is single-player; only the RPC framing (`crates/rpc`, `crates/proto`) that other
  surviving features (LSP, DAP, remote development) also use was kept.
- **AI and agent features** (`agent`, `agent_ui`, `language_model` and its ~13 per-vendor LLM
  provider crates, `edit_prediction*` inline completions) — deleted.
- **Auto-update** — no in-app updater; the crate was removed entirely.
- **Telemetry and crash reporting** — `telemetry::send_event` is a no-op by construction; no
  event queue, no local telemetry log, no minidump generation or upload. See
  `docs/src/telemetry.md` for the verification recipe.

## What's kept

The editor core, LSP integration, the terminal, git integration, the debugger (DAP), extensions
(WASM-sandboxed, via `crates/extension_host`), and SSH remote development — rebuilt on a direct
connection path (`crates/remote`, `crates/remote_connection`) rather than through a relay server.
The one disclosed, retained network dependency is the extension registry
(`api.zed.dev`) — see `legal/third-party-terms.md`.

## Key Design Decisions

### Decision 1: A first-party UI framework (GPUI) instead of an existing GUI toolkit

**Context**: A code editor needs low-latency, GPU-accelerated text rendering and fine-grained
control over layout, input, and cross-platform windowing that general-purpose Rust GUI crates
did not offer at the performance/control level the editor core requires.

**Decision**: Build and own GPUI (`crates/gpui`) as the foundation: an entity/context model
(`Entity<T>`, `Context<T>`, `App`), Taffy-based flexbox layout, per-platform rendering backends
(`gpui_macos` via Metal/objc2, `gpui_linux` via X11/Wayland, `gpui_windows` via Direct3D,
`gpui_wgpu` shared wgpu renderer, experimental `gpui_web`), and its own async executor
abstractions (`cx.spawn`, `cx.background_spawn`) rather than depending on Tokio directly
(a `gpui_tokio` bridge crate exists for the few crates that still need Tokio directly).

**Rationale**: Owning the framework lets the editor co-design the rendering pipeline with the
text/rope data structures for performance, and gives one consistent concurrency/entity model
used uniformly across every remaining subsystem.

### Decision 2: WASM sandboxing for third-party extensions

**Context**: Extensions (language grammars, themes, LSP/DAP adapters) are third-party code that
must not be able to crash or compromise the host editor process, while still needing controlled
access to project files and network in specific cases.

**Decision**: Compile extensions to `wasm32-wasip2` and run them inside a sandboxed WASM runtime
hosted by `crates/extension_host`, with a stable Rust API surface (`crates/extension_api`) that
extension authors code against, and calls dispatched off the main thread via
`cx.background_spawn`.

**Rationale**: WASM sandboxing isolates untrusted extension code from the host process (memory
safety, no arbitrary syscalls) while still allowing extensions to be distributed as portable
binaries across all supported platforms; bundled/sample extensions (`extensions/glsl`,
`extensions/html`, `extensions/proto`) validate the same API path used by third-party publishers.

### Decision 3: SSH remote development over a direct connection, not a relay server

**Context**: Upstream Zed's remote-development path went through the same `collab`-adjacent
relay infrastructure as real-time collaboration. Removing `collab` meant remote development
needed its own path.

**Decision**: `crates/remote` and `crates/remote_connection` establish a direct SSH connection to
the remote host and run a `remote_server` binary there, with no intermediary relay.

**Rationale**: This is the single least-tested change in this fork relative to upstream — it was
rebuilt rather than salvaged, since the original path assumed collaboration infrastructure that
no longer exists.

## Security Overview

- **Authentication**: None. There is no account to authenticate. SSH remote development uses
  `crates/askpass` for interactive/non-interactive SSH credential prompts — that's the extent of
  credential handling in this fork.
- **Authorization**: No RBAC/route-guard system (not a multi-tenant web app, and no collaboration
  server to enforce room/channel membership anymore); extension sandboxing (WASM) limits what
  loaded third-party code can access.
- **Data Encryption**: Network transport relies on `crates/http_client_tls` / `reqwest_client`
  for TLS on the two remaining outbound paths (extension registry, LSP server downloads); no
  application-level at-rest encryption layer (local SQLite state via `crates/db`/`sqlez` is
  unencrypted on disk, consistent with a local developer tool).
- **API Security**: Not applicable — there is no API surface exposed by this application, and no
  third-party AI provider credentials are ever stored or transmitted (no AI features exist).

## Scalability

Not applicable in the traditional sense — this is a single-user desktop process per running
instance; scaling is per-editor-instance (large files, many open buffers/worktrees) rather than
concurrent-request throughput. There is no server component left to scale.

- **Scaling Strategy**: Async task offloading via GPUI's `cx.background_spawn`/`Scheduler`
  abstraction keeps LSP requests, extension calls, and indexing off the UI thread; large-file/
  large-project performance relies on the `Rope`/`SumTree` data structures for efficient text
  indexing and `Worktree` for incremental filesystem watching rather than full re-scans.
- **Performance Targets**: No documented numeric SLOs; `tooling/perf` and `crates/*_benchmarks`
  crates provide micro-benchmarks.
