# Test Cases — F012_ExtensionSystem

**SIDECAR (v26.1.0):** this is a 5th, optional file alongside the 4 mandatory feature-spec files.
Its absence never blocks feature-spec promotion.

**Code Format**: `TC###` — 3-digit zero-padded, **resets per feature** (this file's own scope is
the reset boundary; unlike `JOB###`, which is file-global).

**Citation-source split**: `UT`/`IT` rows cite a `BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, a
`` `file:line` ``, or an `edge-cases.md` row. `UAT` rows cite a `screens.md`/`business-context.md`
section — NEVER a bare code (UAT is less code-traceable by design; see
`references/test-cases-researcher-contract.md`).

**CSV export**: out of scope v1 — Markdown is the sole output. See plan Decision.

---

## Test Cases

| Test-ID | Type (UT\|IT\|UAT) | Given | When | Then | Traces-to |
|---------|---------------------|-------|------|------|-----------|
| TC001 | UT | A `ProcessExec` command+args is declared in `extension.toml` AND the same entry exists in `granted_extension_capabilities` | The extension requests to spawn that command with those args | `grant_exec` returns Ok and the process spawns | `BR-001` |
| TC002 | UT | A `ProcessExec` command+args is declared in `extension.toml` but has NO matching entry in `granted_extension_capabilities` | The extension requests to spawn that command | `grant_exec` rejects the call — manifest declaration alone does not satisfy gate 2 | `BR-001` |
| TC003 | UT | `run_command`/`download_file`/`npm_install_package` is about to be invoked from WASM guest code with an unauthorized target | The host function executes | `capability_granter.grant_*` is evaluated as the first statement and returns `Err` before the process spawn / HTTP fetch / npm install line is ever reached | `BR-002` |
| TC004 | UT | `ProcessExecCapability` declares command `*` with a trailing `**` for args | `allows` is called with any command name and any number of trailing args | The predicate returns true (wildcard command + open-ended trailing args match) | `DISC-013` |
| TC005 | UT | `DownloadFileCapability` declares an exact host and a path pattern with a single `*` segment | `allows` is called with a matching host and a path where one segment differs but segment count matches | The predicate returns true; a path with a different segment count or a non-matching host returns false | `DISC-013` |
| TC006 | UT | `NpmInstallPackageCapability` declares an exact package name (no wildcard) | `allows` is called with a different package name | The predicate returns false (exact match required, no partial match) | `DISC-013` |
| TC007 | UT | `extension_index` shows a non-dev entry already installed for id `X` | `install_dev_extension` is called for a local dev source with id `X` | The published extension `X` is uninstalled first, then the dev source is compiled and symlinked | `BR-003` |
| TC008 | UT | A symlink already exists at the install output path from a prior dev install of the same id | `install_dev_extension` is called again for the same id | The existing symlink is removed and replaced with a symlink to the new source path | `BR-003` |
| TC009 | UT | A REAL (non-symlink) directory exists at the install output path | `install_dev_extension` is called targeting that path | Installation aborts with `bail!("extension {id} is still installed")` rather than overwriting | `BR-003` |
| TC010 | UT | A context server is in `Starting` state | The server's initialization fails | State transitions `Starting -> Error` | `SM-001` |
| TC011 | UT | A context server is in `Starting` state | The server responds with 401 during init | State transitions `Starting -> AuthRequired`, holding an `OAuthDiscovery` for the browser flow | `SM-001` |
| TC012 | UT | A context server is in `Authenticating` state after the user started the OAuth browser flow | The OAuth callback fails | State transitions `Authenticating -> Error` | `SM-001` |
| TC013 | IT | An extension's manifest declares no `ProcessExec` capability at all | The extension calls `run_command` for an arbitrary command via the WASM host function | `CapabilityGranter::grant_exec` fails through `ExtensionManifest::allow_exec`, the WASM caller receives an `Err`, and no process is spawned or host crash occurs | `edge-cases.md § Extension requests a process/command/download/package it never declared in its manifest` |
| TC014 | IT | Multiple extensions are loaded and "Reload Extensions" is triggered while one extension's reload will fail | The reload proceeds across all loaded extensions | The failing extension's error is reported in isolation; every other extension reloads successfully and the extension host process does not crash | `edge-cases.md § Reloading all extensions while one extension fails to reload` |
| TC015 | IT | A dev extension's source has a compile error introduced since the last successful build | `rebuild_dev_extension` is triggered | The compile error is logged and surfaced (not silently dropped, stale build stays as-is) and the `outstanding_operations` in-flight marker for that extension is cleared regardless of the failure | `edge-cases.md § Rebuilding a dev extension fails to compile` |
| TC016 | IT | A context server's `ContextServerStatus` is `Error` (connection previously dropped) | The developer triggers the `context_server::Restart` action | The existing connection is torn down, a new connection attempt begins (`Error -> Starting`), and the status converges to `Running` or `Error` depending on outcome | `SM-001` |
| TC017 | IT | The MCP Unix-socket listener is actively accepting connections | One `listener.accept()` call errors for a given connection attempt | Only that iteration's connection task ends; the outer accept loop continues serving subsequent connection attempts without crashing | `crates/context_server/src/listener.rs:33-80` |
| TC018 | IT | The MCP client `Client` transport delivers a JSON-RPC notification whose method has no registered callback in the handler table | `on_notification` dispatch runs for that incoming notification | The notification is silently dropped (no matching handler invoked) and no error propagates or crashes the connection | `crates/context_server/src/protocol.rs:118-124` |
| TC019 | IT | A valid local extension directory containing `extension.toml` has no existing install at its target path | The developer triggers "Install Dev Extension" against that directory | The source compiles, a symlink is created at the install path pointing to the source directory, and the extension loads and is marked as a dev extension | `crates/extension_host/src/extension_host.rs:929-1012` |
| TC020 | IT | A user previously dismissed the language-extension suggestion banner for a given file extension (kv_store row upserted) | The user opens another file of that same extension type in a new project | The banner does not reappear — the dismissal is honored via the persisted key-value marker, not re-evaluated per session | `edge-cases.md § A dismissed extension suggestion is later relevant again` |
| TC021 | UAT | A developer has a file open whose type has no active language support | The developer opens the file | The inline suggestion banner appears asking whether to install the matching extension | `screens.md § User Journey step 1` |
| TC022 | UAT | The suggestion banner is showing for an unsupported file type | The developer clicks "Yes" to accept the suggestion | The matching extension installs and activates immediately | `screens.md § User Journey step 2` |
| TC023 | UAT | The suggestion banner is showing for an unsupported file type | The developer dismisses the banner | The banner never reappears for that file type again | `business-context.md § What They Do (item 1)` |
| TC024 | UAT | The developer wants to test a local extension under development | The developer opens the Extensions page and clicks "Install Dev Extension", then picks a local folder | The picked folder is installed and appears on the Extensions page as active and dev-flagged | `screens.md § User Journey step 3` |
| TC025 | UAT | A dev extension is installed and its source has just been edited | The developer triggers a rebuild from the Extensions page | The extension recompiles and the running instance updates in place without a manual reinstall | `screens.md § User Journey step 4` |
| TC026 | UAT | A connected context server's connection has dropped mid-session | The developer opens its status view and triggers "Restart" | The connection re-establishes and the status view shows it connected | `screens.md § User Journey step 5` |

---

## Coverage Notes

- `FR-005` — [NO_TEST_CASE] no dedicated row beyond `INT-001`/`INT-002`; FR-005's own behavior (accept connection + dispatch notifications) is the exact mechanism those two integrations describe, so a third row citing the bare FR code would restate TC017/TC018 rather than add a new scenario.
- Verification criteria `SC-001`/`SC-002` — [NO_TEST_CASE] not expanded as standalone rows; each is a rollup already covered by the underlying `BR-001`/`BR-002`/`FR-001` rows above (TC001-TC003, TC013, TC014), and the contract's code family is `BR-###`/`SM-###`/`DEC-###`/`DISC-###`, not `SC-###`.
