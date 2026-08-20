# F002_LanguageIntelligence: Technical Spec

**Priority**: P0
**Type**: mixed
**Generated**: 2026-08-07

## Overview

Language Intelligence is Zode's LSP client layer: it spawns and supervises per-worktree language-server processes, lets a developer override toolchain/language assignment from the status bar, bridges bespoke non-standard protocol extensions for specific servers (JSON schema, rust-analyzer health, Vue↔TS proxying), runs Prettier as an equivalent server-lifecycle process, and exposes dev-tool log/tree views over the whole pipeline. It is consumed by Editor Core (F008, syntax/inlay/completions) and Diagnostics (F009), and is gated by workspace/worktree trust before any process is spawned.

## Polymorphic Behavior

N/A — no discriminator fields in Key Entities. `MODEL017_LanguageServer` has no DISC-### entries (`data-model.md:610`); `LanguageServerSelector::{Id, Name}` is documented there as a lookup-key enum, not a behavioral discriminator on stored state.

## Cross-Cutting Logic

### Requirements

| Code   | Description                                                                                                  | Endpoint/Handler                                                    | Verifiable |
| ------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- | ---------- |
| FR-001 | A worktree only gets a language server once trusted AND a buffer in that language is opened/reassigned       | `LspStore::start_language_server`                                   | yes        |
| FR-002 | Language-server processes communicate over `Content-Length:`-framed JSON-RPC on stdio and are killed on drop | `LanguageServer::new`                                               | yes        |
| FR-003 | Server shutdown is graceful with a fixed timeout before a hard kill                                          | `LanguageServer` (constant)                                         | yes        |
| FR-004 | Toolchain selection is persisted to the per-workspace SQLite database and re-applied to the active project   | `ToolchainSelectorDelegate::confirm` → `WorkspaceDb::set_toolchain` | yes        |

**Source:** `crates/project/src/lsp_store.rs:423-500`, `crates/lsp/src/lsp.rs:61,1098-1105`, `crates/toolchain_selector/src/toolchain_selector.rs:909-951`

### Business Rules

_(See itemized entries below.)_

### BR-001_WorktreeTrustGatesServerSpawn

**Linked FR:** FR-001
**Source:** `crates/project/src/lsp_store.rs:448-486`
**Applies to:** `LspStore::start_language_server`
**Rule:** If the worktree is not yet trusted (`TrustedWorktrees::can_trust` returns false), the binary lookup/spawn is deferred behind a `watch` channel keyed by `worktree_id`; a subscription resolves the deferral once a `TrustedWorktreesEvent::Trusted` event includes that worktree's path. Only once trust resolves does `get_language_server_binary` run and the process actually spawn.

**Pseudocode:**

```text
fn start_language_server(worktree, adapter, ...):
    if not TrustedWorktrees.can_trust(worktree_id):
        register_watch_channel(worktree_id)  # deferred, no spawn yet
        wait_for(TrustedWorktreesEvent::Trusted(worktree_id))
    binary = get_language_server_binary(...)
    spawn LanguageServer::new(binary, ...)
```

### BR-002_GracefulShutdownWithHardKillFallback

**Linked FR:** FR-003
**Source:** `crates/lsp/src/lsp.rs:61,1090-1110`
**Applies to:** `LanguageServer` shutdown path
**Rule:** On shutdown, the server is given `SERVER_SHUTDOWN_TIMEOUT` (5 seconds, `crates/lsp/src/lsp.rs:61`) to exit gracefully via the LSP `shutdown`/`exit` handshake; if the timer elapses first, the child process is killed forcibly.

**Pseudocode:**

```text
send_shutdown_and_exit_notifications()
race:
  - child_process.wait()
  - executor.timer(SERVER_SHUTDOWN_TIMEOUT)
if timer wins: child_process.kill()
```

### BR-003_ToolchainChoicePersistsAndReactivates

**Linked FR:** FR-004
**Source:** `crates/toolchain_selector/src/toolchain_selector.rs:909-951`, `crates/workspace/src/persistence.rs:2388-2423`
**Applies to:** `ToolchainSelectorDelegate::confirm`
**Rule:** Confirming a toolchain in the picker only proceeds if the workspace has a `database_id`; it writes an upsert (`INSERT ... ON CONFLICT DO UPDATE`) into the `toolchains` table keyed by `(workspace_id, worktree_root_path, relative_worktree_path, language_name)`, then calls `Project::activate_toolchain` to apply it live. A database write failure is logged (`.log_err()`) but does not block toolchain activation.

**Pseudocode:**

```text
on confirm(selected_toolchain):
  if workspace.database_id is None: return  # no-op, dismiss only
  db.set_toolchain(workspace_id, worktree_root, rel_path, selected_toolchain).await.log_err()
  project.activate_toolchain(project_path, selected_toolchain).await
```

### BR-004_LanguageOverrideFailsSilentlyOnLookupError

**Linked FR:** N/A (UI-only correction path)
**Source:** `crates/language_selector/src/language_selector.rs:209-227`
**Applies to:** `LanguageSelectorDelegate::confirm`
**Rule:** Selecting a language resolves it via `LanguageRegistry::language_for_name` (async — may involve loading a grammar); if that resolution errors (project or buffer dropped, language load failure), the error is logged via `detach_and_log_err` and no language change is applied — the buffer keeps its prior language.

**Pseudocode:**

```text
on confirm(selected_language_name):
  spawn:
    language = language_registry.language_for_name(name).await?  # may error
    project.set_language_for_buffer(buffer, language)
  # errors here are only logged, never surfaced to the user
```

### Decision Logic

N/A — no user-facing decision logic beyond DISC-### Polymorphic Behavior. The language/toolchain selector confirm handlers are single-branch dispatches (selected match found → apply; else dismiss) with no multi-predicate render, interaction-reveal, or in-feature flow routing.

### State Machines

None — no persisted or ≥3-state/≥2-transition UI state machine was found scoped to this feature; language-server health (Ok/Warning/Error, BL110) is a propagated enum, not a locally-owned state machine with transition rules.

### Algorithms

None.

### External Integrations

_(See itemized entries below.)_

### INT-001_VueLanguageServerTsServerBridge

**Linked FR:** N/A (cross-cutting protocol bridge)
**Source:** `crates/project/src/lsp_store/vue_language_server_ext.rs:1-133` (full file; per `behavior-logic.md:2507-2532`, `BL111_VueLanguageServerTsServerBridge`)
**Type:** event-publish (LSP custom notification proxy)
**Target:** sibling TypeScript language server (`vtsls` or `typescript-language-server`) registered for the same worktree
**Trigger:** Vue language server emits a non-standard `tsserver/request` notification
**Payload:** `VueServerRequest` params (forwarded verbatim)
**Failure handling:** if no sibling TS server is registered under `LspStore.language_server_ids` for that worktree, the forward silently has nothing to route to (no error surfaced to the Vue server; behavior documented, not independently verified beyond the BL entry)

**Pseudocode:**

```text
on vue_server.notification("tsserver/request", params):
  ts_server = lsp_store.find_running_server_by_name(worktree, "vtsls" | "typescript-language-server")
  if ts_server: response = ts_server.request(params); vue_server.notify("tsserver/response", response)
```

### INT-002_PrettierProcessAsFormatterServer

**Linked FR:** N/A
**Source:** `crates/project/src/prettier_store.rs:336-420`
**Type:** queue-job (background process lifecycle, not a network call)
**Target:** worktree-local Prettier install, or bundled default Prettier installed on demand via Node/npm
**Trigger:** a buffer needs formatting and no running Prettier instance is cached for its worktree
**Payload:** N/A (process spawn, not a message payload)
**Failure handling:** if the default-Prettier install task fails, the attempt counter increments and `anyhow::bail!` propagates a formatted error ("Cannot start default prettier due to its installation failure: …") to the formatting caller

**Pseudocode:**

```text
if default_prettier.state == NotInstalled:
  await installation_task
  if err: attempts += 1; bail("install failure: {err}")
  prettier = start_prettier(node, default_prettier_dir, worktree_id)
else:
  prettier = cached_instance or start_prettier(...)
```

### Verification

- **SC-001** A worktree that is not trusted never spawns a language-server child process (covers FR-001, BR-001)
- **SC-002** Restarting language servers for a set of buffers results in all matching `LanguageServerId`s being torn down and re-initialized within the affected buffers (covers FR-002, US051)
- **SC-003** Selecting a toolchain results in a row in the `toolchains` table matching `(workspace_id, worktree_root_path, relative_worktree_path, language_name)` (covers FR-004, BR-003)
- **SC-006** Shutting down a language server that does not respond within the fixed timeout is force-killed rather than left running indefinitely (covers FR-003)

---

**Client behavior:** see
[`behavior-logic.md`](../../behavior-logic.md) (client-side patterns — debounce, optimistic UI, polling, upload, realtime),
[`permissions.md`](../../permissions.md) (feature flags / experiments / env / locale gates),
[`architecture.md`](../../architecture.md) (guards / deep-link state restoration / unsaved-changes protection).

## User Stories

### US049_SwitchLanguageServerToolchain — Switch language server toolchain (Priority: should)

**What happens:** From the status bar, a developer opens the toolchain picker (`ToolchainSelectorDelegate::confirm`) and selects among detected toolchains (e.g. Python interpreters/venvs) for the language backing the active buffer's worktree.
**Why this priority:** Secondary/should — most projects auto-detect the correct toolchain; manual override matters only when auto-detection is ambiguous or wrong.
**Independent Test:** With two Python interpreters present, pick the non-default one and confirm the language server reconfigures against it (verifiable via the persisted `toolchains` row and live `activate_toolchain` call).

**Acceptance Scenarios:**

1. **Given** two Python interpreters are detected for the project, **When** the developer selects the non-default one from the toolchain picker, **Then** the Python language server reconfigures to use the selected interpreter and the choice is persisted for that worktree path.

**Requirements fulfilled:**

- **FR-004** Toolchain selection persists to `toolchains` table and reactivates — via `BL073_ToolchainSelectorActions`
  **Source:** `crates/toolchain_selector/src/toolchain_selector.rs:909-951`

**Rules enforced:**

### BR-003 (see Cross-Cutting Logic) — applies directly to this US's confirm handler.

**Verification:**

- **SC-003** (covers FR-004, BR-003)

---

### US050_SwitchBufferLanguage — Switch a buffer's language (Priority: should)

**What happens:** From the status bar, a developer opens the language picker (`BL038_LanguageSelectorToggleAction`) and manually reassigns the active buffer's language, re-associating it with that language's grammar and LSP.
**Why this priority:** Secondary/should — needed only when file-extension-based language detection misclassifies a buffer.
**Independent Test:** Rename a `.txt` buffer containing Rust source's language to Rust via the selector and observe Rust syntax highlighting and rust-analyzer activation without reopening the file.

**Acceptance Scenarios:**

1. **Given** a `.txt` file actually contains Rust code, **When** the developer sets its language to Rust via the status-bar selector, **Then** Rust syntax highlighting and the Rust language server activate for that buffer.

**Requirements fulfilled:**

- **FR-005** Buffer language reassignment invokes `Project::set_language_for_buffer` after async grammar resolution — via `Toggle` action
  **Source:** `crates/language_selector/src/language_selector.rs:209-227`

**Rules enforced:** BR-004_LanguageOverrideFailsSilentlyOnLookupError (see Cross-Cutting Logic).

**Verification:**

- **SC-004** Selecting a language in the picker results in `Buffer.language` changing to the selected grammar within the same render pass (covers FR-005, BR-004)

---

### US051_RestartLanguageServersForBuffer — Restart language servers for a buffer (Priority: should)

**What happens:** A developer triggers "Restart Language Servers" (`BL182_RestartLanguageServersForBuffers`) to tear down and respawn the server process(es) backing the target buffers, recovering from a hung/stale server without restarting the whole editor.
**Why this priority:** Secondary/should — a recovery path for a failure mode (hung server), not a primary daily workflow.
**Independent Test:** With an unresponsive rust-analyzer process, trigger the restart action and confirm a new `LanguageServerId` appears and diagnostics resume in the affected buffers.

**Acceptance Scenarios:**

1. **Given** the rust-analyzer process for a project has become unresponsive, **When** the developer triggers "Restart Language Servers", **Then** rust-analyzer respawns and diagnostics resume within the affected buffers.

**Requirements fulfilled:**

- **FR-002** Server processes are torn down and re-initialized on restart — via `LspStore::restart_language_servers_for_buffers`
  **Source:** `crates/project/src/lsp_store.rs:11717-11766`

**Rules enforced:** BR-002_GracefulShutdownWithHardKillFallback (see Cross-Cutting Logic) — applies to the teardown half of restart.

**Verification:**

- **SC-002** (covers FR-002, US051)

---

### Edge Cases

| Scenario                                                          | Behavior                                                                                                                                                                                                                          |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Toolchain confirmed while workspace has no `database_id` yet      | The picker skips the DB write entirely and only dismisses — `crates/toolchain_selector/src/toolchain_selector.rs:912-917`                                                                                                         |
| `db.set_toolchain` fails (e.g. locked/corrupt DB)                 | Error is swallowed via `.log_err()`; `activate_toolchain` still runs against the in-memory project so the session's behavior is correct even though persistence silently failed — `crates/workspace/src/persistence.rs:2388-2423` |
| Selected language fails to load (`language_for_name` errors)      | Error is logged via `detach_and_log_err`; buffer's language is left unchanged, no dialog shown — `crates/language_selector/src/language_selector.rs:215-224`                                                                      |
| Worktree not yet trusted when a buffer needing a server is opened | Server spawn is deferred on a `watch` channel until a `TrustedWorktreesEvent::Trusted` event covers that worktree; no server process exists in the interim — `crates/project/src/lsp_store.rs:448-486`                            |
| Restart requested while connected to a remote project             | Restart is proxied as a `proto::RestartLanguageServers` RPC to the remote host rather than torn down locally — `crates/project/src/lsp_store.rs:11723-11752`                                                                      |
| Default Prettier install fails                                    | `install_default_prettier`'s failure increments an attempt counter and `bail!`s a formatted error string up to the formatting caller rather than retrying silently — `crates/project/src/prettier_store.rs:352-373`               |

## Key Entities

| Entity                          | Table                           | Key Columns                                                                                     | Purpose                                                                                                                                                                                          |
| ------------------------------- | ------------------------------- | ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| LanguageServer                  | _(in-memory only, no DB table)_ | server_id, name, binary, capabilities, server (Child handle)                                    | The running LSP process and JSON-RPC channel this feature spawns/supervises — `crates/lsp/src/lsp.rs:99`                                                                                         |
| Toolchain (persisted selection) | `toolchains`                    | workspace_id, worktree_root_path, relative_worktree_path, language_name, name, path, raw_json   | Per-worktree, per-language toolchain override selected via US049 — `crates/workspace/src/persistence.rs:2401-2419`                                                                               |
| user_toolchains                 | `user_toolchains`               | remote_connection_id, workspace_id, relative_worktree_path, language_name, name, path, raw_json | Companion table (schema migration at `crates/workspace/src/persistence.rs:954-1017`) tracking toolchain choices scoped by remote connection; read/written by the same toolchain-persistence path |

**Note:** `MODEL017_LanguageServer` (`data-model.md:587-611`) is process/session state, not a persisted database row — the only DB-backed entity this feature owns is the `toolchains`/`user_toolchains` pair.

## Artifact References

| Artifact           | File                                                 | Codes Used                                                                                                                                                                                  | Reviewed |
| ------------------ | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| System Overview    | [system-overview.md](../../system-overview.md)       | —                                                                                                                                                                                           | [x]      |
| Architecture       | [architecture.md](../../architecture.md)             | —                                                                                                                                                                                           | [x]      |
| Feature List       | [feature-list.md](../../feature-list.md)             | F002                                                                                                                                                                                        | [x]      |
| Entities           | [entities.md](../../entities.md)                     | MODEL017                                                                                                                                                                                    | [x]      |
| Screens            | [screens.md](./screens.md)                           | N/A (no SCR### in this profile)                                                                                                                                                             | [x]      |
| Behavior Logic     | [behavior-logic.md](../../behavior-logic.md)         | BL038, BL039, BL041, BL042, BL043, BL050, BL058, BL073, BL107, BL108, BL109, BL110, BL111, BL112, BL115, BL116, BL131, BL136, BL142, BL170, BL175, BL182, BL183, BL184, BL190, BL191, BL192 | [x]      |
| Permissions Matrix | [permissions-matrix.md](../../permissions-matrix.md) | N/A — no PERM### rows reference Language Intelligence                                                                                                                                       | [x]      |
| User Stories       | [user-stories.md](../../user-stories.md)             | US049, US050, US051                                                                                                                                                                         | [x]      |

**Rule:** Every code listed in Codes Used exists in its source artifact. This `generic-source` profile has no `route-list.md`/`screen-list.md` — the `API Map`/`Screens` rows above are adapted accordingly (no ROUTE###/SCR### fabricated).

## Assumptions

- The `LspLogView`/`LspToolMenu`/`HighlightsTreeView`/`SyntaxTreeView` dev-tool panels (BL041–BL043, BL050, BL131) are treated as read-only observability surfaces over this feature, not separate features, per `feature-list.md:356-371` bundling them under F002.
- Jupyter/REPL kernel lifecycle (BL058, BL115, BL116, BL190–BL192) is bundled into F002 by the upstream feature-list grouping despite being a distinct execution runtime (Python kernels, not LSP) — documented here as-is without re-partitioning, since re-scoping the feature boundary is out of this pass's remit.
- `Toolchain`/`user_toolchains` DB rows are the only persistence this feature performs; `LanguageServer` state itself is never persisted (assumption drawn from finding zero `crates/db`/persistence references in `lsp_store.rs`, `lsp.rs`, `prettier_store.rs`, `language_selector.rs`, or `toolchain_selector.rs` outside the one `set_toolchain` call).

## Source Code References

| Order | Symbol                                           | Path                                                          | Purpose                                                                                      |
| ----- | ------------------------------------------------ | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| 1     | `LanguageServer` (struct)                        | `crates/lsp/src/lsp.rs:99`                                    | Entity definition — one running LSP process + JSON-RPC channel                               |
| 2     | `LspStore::start_language_server`                | `crates/project/src/lsp_store.rs:423-500`                     | Entry point — decides when/whether to spawn a server for a worktree+language, gated by trust |
| 3     | `LspStore::restart_language_servers_for_buffers` | `crates/project/src/lsp_store.rs:11717-11766`                 | Restart flow — local teardown+respawn vs. remote RPC proxy                                   |
| 4     | `ToolchainSelectorDelegate::confirm`             | `crates/toolchain_selector/src/toolchain_selector.rs:909-951` | UI confirm handler — persists + activates toolchain choice                                   |
| 5     | `LanguageSelectorDelegate::confirm`              | `crates/language_selector/src/language_selector.rs:209-227`   | UI confirm handler — reassigns buffer language                                               |
| 6     | `PrettierStore::start_default_prettier`          | `crates/project/src/prettier_store.rs:336-420`                | Formatter process lifecycle (install + spawn)                                                |
| 7     | `WorkspaceDb::set_toolchain`                     | `crates/workspace/src/persistence.rs:2388-2423`               | Persistence layer — writes the `toolchains` table                                            |

## Unresolved Questions

1. **Vue↔TS bridge failure mode**: `BL111_VueLanguageServerTsServerBridge` (`crates/project/src/lsp_store/vue_language_server_ext.rs`) was not directly read line-by-line in this pass (sourced from the behavior-logic.md synthesis); the exact behavior when no sibling TS server is registered is inferred, not independently verified against source.
2. **Feature-boundary question**: whether Jupyter/REPL kernel management (BL058, BL115, BL116, BL190-BL192) belongs under Language Intelligence at all, versus a separate "Notebooks/REPL" feature, is a partitioning decision made upstream in `feature-list.md` and outside this researcher's remit to re-litigate.
3. **`user_toolchains` write path**: the migration history in `crates/workspace/src/persistence.rs:954-1017` shows a `user_toolchains` table exists, but the write call site for it (as opposed to `toolchains`) was not located in this pass — likely gated on remote-connection scenarios not exercised in the code paths read here.

## Source Walkthrough

1. **File:** `crates/lsp/src/lsp.rs:61-110` — why start here: defines `LanguageServer`, the entity every other piece of this feature revolves around, plus the 5s `SERVER_SHUTDOWN_TIMEOUT` invariant.
2. **File:** `crates/project/src/lsp_store.rs:423-500` — next: the entry point (`start_language_server`) that decides, per worktree/language pair, whether/when to spawn one, gated by `TrustedWorktrees`.
3. **File:** `crates/toolchain_selector/src/toolchain_selector.rs:909-951` — next: the UI confirm handler that lets a developer override the toolchain feeding into that entry point, and its side-effecting DB write.
4. **File:** `crates/project/src/prettier_store.rs:336-420` — last: the sibling "server-lifecycle" integration (Prettier) that reuses the same start/install/hibernate shape as the LSP path.

### Call Hierarchy

```text
[status bar Toggle/Select actions]
  -> LanguageSelectorDelegate::confirm / ToolchainSelectorDelegate::confirm
       -> Project::set_language_for_buffer | Project::activate_toolchain
            -> LspStore::start_language_server (trust-gated)
                 -> LanguageServer::new (spawn child, JSON-RPC loop)
                      -> LanguageServer::initialize (BL170, handshake)
[buffer opened / language changed] -> LspStore::start_language_server (same entry point)
[restart action] -> LspStore::restart_language_servers_for_buffers -> stop + re-register
```

**Related files:** see `## Source Code References` above — the **Order** column is this section's related-files table.

## DB Impact per Event

| Event/Endpoint                      | Table        | Columns                                                                                       | Operation                        | Value Derivation                                                                                                                                       | Source                                          |
| ----------------------------------- | ------------ | --------------------------------------------------------------------------------------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------- |
| Confirm toolchain selection (US049) | `toolchains` | workspace_id, worktree_root_path, relative_worktree_path, language_name, name, path, raw_json | INSERT ... ON CONFLICT DO UPDATE | `workspace_id`/paths from the active workspace+worktree context; `name`/`path`/`raw_json` copied verbatim from the picker's selected `Toolchain` value | `crates/workspace/src/persistence.rs:2399-2422` |

All other events in this feature (language switch, server start/restart, Prettier lifecycle) are read/process-state-only — no other DB writes were found.
