# Architecture

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). This fork
removed authentication, cloud/collaboration, AI/agent, edit-prediction, auto-update, and
telemetry/crash-reporting — see `plans/260726-1531-remove-auth-cloud-hard-fork/plan.md` for the
full 12-phase history of what changed and why.

<!-- Stack: Rust workspace (generic-source profile), native GPUI desktop app.
     Verified via root Cargo.toml (187 workspace members), rust-toolchain.toml (channel 1.94.1,
     targets: wasm32-wasip2 [extensions], wasm32-unknown-unknown [gpui-web], x86_64-unknown-linux-musl [remote server]). -->

## System Architecture

The binary `crates/zed` (package name `zode`) statically links the in-workspace library crates.
There is no client/server split of any kind anymore — no cloud backend, no collaboration server.
The only outbound network paths are the disclosed extension registry (`api.zed.dev`) and
per-language-server downloads from each server's own distributor.

```mermaid
graph TB
    subgraph "Application Shell"
        ZED[crates/zed — binary entry point, package name zode]
        WS[workspace — panes/docks/panels]
        CLI[cli — zode launcher binary]
    end

    subgraph "GPUI Framework (rendering/windowing engine)"
        GPUI[gpui — entities, elements, layout, executors]
        PLAT[gpui_macos / gpui_linux / gpui_windows / gpui_web — platform backends]
        WGPU[gpui_wgpu — renderer]
    end

    subgraph "Editor Core"
        EDITOR[editor — visual text editor]
        LANG[language / language_core — syntax, diagnostics]
        TEXT[text / rope / sum_tree — buffer storage]
        MB[multi_buffer]
    end

    subgraph "Project / Filesystem"
        PROJECT[project — worktrees, LSP store, DAP, tasks, git]
        WORKTREE[worktree]
        FS[fs]
        DB[db / sqlez — local sqlite state]
    end

    subgraph "Language Intelligence / Extensions"
        LSP[lsp]
        EXT_HOST[extension_host — WASM runtime]
        EXTENSIONS[extensions/* — glsl, html, proto, test-extension]
        DAP[dap / dap_adapters]
    end

    subgraph "Remote Development"
        REMOTE[remote / remote_connection — direct SSH connection]
        REMOTE_SERVER[remote_server — runs on the remote host]
    end

    subgraph "Version Control"
        GIT[git]
        GIT_UI[git_ui]
        BUFFER_DIFF[buffer_diff]
    end

    subgraph "Settings / Theme / Keymap"
        SETTINGS[settings — SettingsStore]
        THEME[theme]
        KEYMAP[keymap_editor]
    end

    subgraph "UI Component Library"
        UI[ui — design system]
        PICKER[picker]
    end

    ZED --> WS
    WS --> EDITOR
    ZED --> EXT_HOST
    ZED --> GIT_UI
    ZED --> REMOTE

    EDITOR --> MB --> TEXT
    EDITOR --> LANG --> TEXT
    EDITOR --> PROJECT
    EDITOR --> UI
    EDITOR --> GPUI

    PROJECT --> WORKTREE
    PROJECT --> FS
    PROJECT --> LSP
    PROJECT --> DAP
    PROJECT --> GIT
    PROJECT --> GPUI

    LANG --> LSP
    EXT_HOST --> EXTENSIONS
    EXT_HOST -.-> PROJECT

    REMOTE --> REMOTE_SERVER
    GIT_UI --> GIT --> BUFFER_DIFF

    WS --> SETTINGS
    WS --> THEME
    UI --> GPUI

    GPUI --> PLAT
    GPUI --> WGPU
    PLAT --> WGPU
```

## Tech Stack

| Layer | Technology | Version / Evidence |
|-------|------------|---------|
| Language | Rust | toolchain `1.94.1`, `rust-toolchain.toml` |
| UI framework | GPUI (in-house, `crates/gpui`) | GPU-accelerated retained-mode UI; own entity/element/layout system (Taffy flex layout) |
| Rendering backend | `gpui_wgpu` (wgpu) + platform backends | `gpui_macos` (Cocoa/Metal via objc2), `gpui_linux` (X11/Wayland), `gpui_windows` (Win32/Direct3D), `gpui_web` (wasm32-unknown-unknown, experimental) |
| Text/buffer engine | `text`, `rope`, `sum_tree` (in-house rope + B-tree index) | `crates/text/src/text.rs` `Buffer` |
| Language intelligence | `lsp` (custom LSP client), tree-sitter grammars via `grammars`/`languages` | LSP client transport, not tower-lsp |
| Extension runtime | WASM via `extension_host`, targets `wasm32-wasip2` | confirmed in `rust-toolchain.toml` targets |
| Local persistence | SQLite via `sqlez` (thin async wrapper) + `db` | app state (workspace layout, kv store) |
| Remote development | direct SSH connection via `remote`/`remote_connection`, running `remote_server` on the host | rebuilt in this fork — no relay server |
| Async runtime | GPUI's own executor (`cx.spawn`, `cx.background_spawn`); `gpui_tokio` bridges Tokio only where a dep requires it | not tokio-first |
| Build/workspace | Cargo workspace, resolver "2", 187 members | root `Cargo.toml` |
| Packaging targets | macOS/Linux/Windows native, `x86_64-unknown-linux-musl` (remote server), WASM (extensions + experimental web) | `rust-toolchain.toml` |

## Concurrency & Event Model

GPUI supplies its own concurrency and pub/sub primitives in place of an OS/web-framework thread
pool or message broker:

```mermaid
graph LR
    A[UI thread: Entity update via cx] -->|cx.spawn| B[Foreground async task]
    A -->|cx.background_spawn| C[Background thread pool task]
    C -->|result| A
    A -->|cx.emit event| D[EventEmitter impls]
    D -->|cx.subscribe| E[Other entities react]
    F[SettingsStore] -->|notifies impl Settings for registrants| G[Per-crate settings structs]
```

- **Entity model**: nearly every major struct (`Editor`, `Project`, `Workspace`) is held as
  `Entity<T>`, mutated only through `cx.update`/`cx.update_in` — enforced single-writer discipline
  (per project `CLAUDE.md`).
- **Scheduler abstraction**: `crates/scheduler` — deterministic time abstraction backing timers,
  with a fake implementation for tests.
- **Action dispatch**: `actions!()` macro + `.on_action()` handlers function as the app's
  "controller layer" for user-triggered commands.

## Remote Development (replaces the old client/server collaboration split)

```mermaid
sequenceDiagram
    participant Local as Zode (local machine)
    participant SSH as SSH connection
    participant Remote as remote_server (remote host)

    Local->>SSH: establish direct SSH connection
    SSH->>Remote: spawn remote_server binary
    Remote-->>SSH: project/LSP/file operations over the same rpc/proto framing
    SSH-->>Local: results routed back to the local UI
```

This is the only host-boundary-crossing path in the system, and it is entirely opt-in (the user
explicitly connects to a remote host). The core editing experience (Editor Core,
Project/Filesystem, Language Intelligence) runs fully local with no network dependency at all.

## Notes / Limits

- No REST/GraphQL/gRPC API surface exists; this app has no API-gateway layer.
- SSH remote development is the single least-tested change in this fork relative to upstream Zed
  — it was rebuilt rather than salvaged, since the original path assumed collaboration
  infrastructure (`collab`) that no longer exists. See Phase 11's `network-verification.md` for
  what's been verified so far.
