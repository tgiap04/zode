# Test Cases — F002_LanguageIntelligence

**SIDECAR (v26.1.0):** this is a 5th, optional file alongside the 4 mandatory feature-spec files.
Its absence never blocks feature-spec promotion.

**Code Format**: `TC###` — 3-digit zero-padded, **resets per feature** (this file's own scope is
the reset boundary; unlike `JOB###`, which is file-global).

**Citation-source split**: `UT`/`IT` rows cite a `BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, a
`file:line`, or an `edge-cases.md` row. `UAT` rows cite a `screens.md`/`business-context.md`
section — NEVER a bare code.

**CSV export**: out of scope v1 — Markdown is the sole output.

---

## Test Cases

| Test-ID | Type (UT\|IT\|UAT) | Given | When | Then | Traces-to |
|---------|---------------------|-------|------|------|-----------|
| TC001 | IT | A worktree is not yet trusted (`TrustedWorktrees::can_trust` returns false) and a buffer in a recognized language is opened in it | The buffer open triggers `LspStore::start_language_server` | The binary lookup/spawn is deferred behind a `watch` channel keyed by `worktree_id`; once a `TrustedWorktreesEvent::Trusted` event for that path fires, `get_language_server_binary` runs and the language server spawns | `BR-001` |
| TC002 | UT | A running language server is asked to shut down | `SERVER_SHUTDOWN_TIMEOUT` (5s) elapses before the LSP `shutdown`/`exit` handshake completes | The child process is force-killed rather than left running | `BR-002` |
| TC003 | IT | The active workspace has a `database_id`, and two toolchains are detected for the active buffer's language | The developer confirms a non-default toolchain in the Toolchain Selector | An upsert is written to the `toolchains` table keyed by `(workspace_id, worktree_root_path, relative_worktree_path, language_name)`, and `Project::activate_toolchain` applies it live | `BR-003` |
| TC004 | IT | A buffer's language is being reassigned via the Language Selector, and `LanguageRegistry::language_for_name` will error (e.g. grammar load failure) | The developer confirms the new language selection | The error is logged via `detach_and_log_err`, no language change is applied, and the buffer keeps its prior language | `BR-004` |
| TC005 | IT | A Vue language server and a sibling TypeScript server (`vtsls`/`typescript-language-server`) are both registered for the same worktree | The Vue server emits a `tsserver/request` notification | The request is forwarded verbatim to the sibling TS server and its response is relayed back as a `tsserver/response` notification | `crates/project/src/lsp_store/vue_language_server_ext.rs:1-133` |
| TC006 | IT | No Prettier instance is cached for a buffer's worktree, and no worktree-local Prettier install exists | The buffer needs formatting | The bundled default Prettier is installed on demand via Node/npm (or the cached default is reused) and used to format the buffer | `crates/project/src/prettier_store.rs:336-420` |
| TC007 | IT | A project folder has not yet been explicitly trusted | The developer opens a file in that folder | The file opens normally with no language server spawned; the developer sees no error, only the absence of live language features until trust is granted | `edge-cases.md § Opening a file in an untrusted project folder` |
| TC008 | IT | The workspace has not finished initializing and has no `database_id` yet | The developer selects a toolchain in the Toolchain Selector | The picker closes without writing to the `toolchains` table; the choice is not saved and does not take effect | `edge-cases.md § Selecting a toolchain before the workspace has finished initializing (no database id yet)` |
| TC009 | IT | The workspace has a `database_id`, but the underlying storage write will fail (e.g. locked/corrupt DB) | The developer confirms a toolchain selection | The write failure is logged (`.log_err()`) and swallowed; `activate_toolchain` still runs against the in-memory project, so the current session behaves correctly even though persistence silently failed | `edge-cases.md § Saving a toolchain choice fails (storage error)` |
| TC010 | IT | The developer is connected to a remote project | The developer triggers "Restart Language Servers" for a buffer | The restart request is proxied as a `proto::RestartLanguageServers` RPC to the remote host instead of tearing servers down locally; remote-side failures are logged rather than raised | `edge-cases.md § Restarting language servers while connected to a remote project` |
| TC011 | IT | No default Prettier is installed yet, and the install task will fail | A buffer needs formatting via the default Prettier path | The attempt counter increments and formatting fails with the message "Cannot start default prettier due to its installation failure: …" propagated to the caller, rather than retrying silently | `edge-cases.md § Default formatter (Prettier) fails to install on first use` |
| TC012 | IT | The rust-analyzer process for a project has become unresponsive | The developer manually triggers "Restart Language Servers" (no confirmation dialog) | The stuck process is torn down (graceful shutdown, then hard-killed after 5s if unresponsive) and a fresh server starts; diagnostics/completions resume once it initializes | `edge-cases.md § Language server becomes unresponsive mid-session` |
| TC013 | UAT | A developer opens the editor and opens a file in a recognized language | No panel is visible yet | The matching language server starts silently in the background with no visible action required from the developer | `screens.md § User Journey step 1` |
| TC014 | UAT | A developer has several Python interpreters installed and wants a specific one used | The developer opens the Toolchain Selector from the status bar and picks a detected option | The active language server reconfigures using the selected toolchain | `screens.md § User Journey step 2` |
| TC015 | UAT | A file's language was misdetected | The developer opens the Language Selector, sees the language list with the current one highlighted, and picks the correct language | Syntax highlighting updates immediately for the buffer | `screens.md § User Journey step 3` |
| TC016 | UAT | A developer suspects a language server is stuck | The developer opens the LSP Tool status-bar menu and jumps to the Language Server Logs view, or triggers a restart from there | The developer can inspect recent server activity in the logs view, or trigger a restart of the affected server | `screens.md § User Journey step 4` |
| TC017 | UAT | A zode contributor wants to debug how a buffer is parsed/highlighted | The contributor opens the Syntax Tree or Highlights Tree dev view for the active buffer | The live tree-sitter parse tree or highlight-layer breakdown is shown, with layer visibility toggles available | `screens.md § User Journey step 5` |
| TC018 | UAT | A developer opens a project folder for the first time | Before any language helper spawns | The project must first be marked as trusted, protecting the developer from a folder silently running arbitrary tooling on open | `business-context.md § What They Do step 1` |
| TC019 | UAT | A developer saves or formats a file for the first time in a project with no formatter installed | The developer triggers a save/format action | A formatting helper (Prettier) starts the same way as a language server — preferring a project-specific install, or installing a shared default automatically the first time it's needed | `business-context.md § What They Do step 6` |

---

## Coverage Notes

All BR-###/INT-### codes present in `technical-spec.md` are traced by at least one test case above
(`BR-001`→TC001/TC007, `BR-002`→TC002/TC012, `BR-003`→TC003/TC008/TC009, `BR-004`→TC004,
`INT-001`→TC005, `INT-002`→TC006/TC011). No `SM-###`/`DEC-###`/`DISC-###` codes exist for this
feature (technical-spec.md's State Machines, Decision Logic, and Polymorphic Behavior sections are
all explicitly `None`/`N/A`), so none are listed here.

- Jupyter/REPL kernel lifecycle (BL058, BL115, BL116, BL190–BL192) — [NO_TEST_CASE] technical-spec.md
  carries no BR-###/SM-###/DEC-###/DISC-### code for this sub-area (it is flagged only as an
  `## Assumptions` bundling note, not itemized cross-cutting logic), so there is no code to expand
  into a test case in this pass.
