# Test Cases — F001_Terminal

**SIDECAR (v26.1.0):** this is a 5th, optional file alongside the 4 mandatory feature-spec files.
Its absence never blocks feature-spec promotion.

**Code Format**: `TC###` — 3-digit zero-padded, **resets per feature** (this file's own scope is
the reset boundary; unlike `JOB###`, which is file-global).

**Citation-source split**: `UT`/`IT` rows cite a `BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, a
`file:line`, or an `edge-cases.md` row. `UAT` rows cite a `screens.md`/`business-context.md`
section — NEVER a bare code (UAT is less code-traceable by design; see
`references/test-cases-researcher-contract.md`).

**CSV export**: out of scope v1 — Markdown is the sole output. See plan Decision.

---

## Test Cases

| Test-ID | Type (UT\|IT\|UAT) | Given | When | Then | Traces-to |
|---------|---------------------|-------|------|------|-----------|
| TC001 | UT | Terminal's `terminal_type` is `Pty { pty_tx, info }` with a live child process | `client_side_working_directory()` is queried | It returns the live process's cwd from `info.current`; the view renders streamed output and a real child-process PID | `DISC-012` |
| TC002 | UT | Terminal's `terminal_type` is `DisplayOnly` (fixed content injected via `write_output`, no PTY event loop) | `client_side_working_directory()` is queried and the item is later serialized | It always returns `None`; the `terminals` table's `working_directory` column is never written for this item, though `custom_title` may still be saved | `DISC-012` |
| TC003 | UT | A terminal was already hibernated once (`pre_hibernate_scroll_history.is_some()`) | `limit_scroll_history` is invoked again by a second hibernate request, without waking in between | The call is a no-op; `pre_hibernate_scroll_history` and `term_config.scrolling_history` are left unchanged, so `restore_scroll_history_limit` can still restore the original cap correctly on wake | `BR-001`; `edge-cases.md § hibernates more than once without reopening` |
| TC004 | IT | A `tasks.json` task with a defined `env` map is run against a project whose base directory environment has already been resolved (incl. venv activation) | `Project::create_terminal_task` spawns the task | The task's own `env` entries are merged on top of the resolved directory environment, and `RevealStrategy`/`HideStrategy` decide whether the terminal is shown or focused | `BR-002` |
| TC005 | IT | A developer opens an interactive shell (no associated `tasks.json` task) | `Project::create_terminal_shell`/`create_local_terminal` spawns the shell | Only the resolved directory environment is applied (no task `env` to merge), and the terminal always reveals per its caller | `BR-002` |
| TC006 | IT | A terminal is running a `tasks.json` task (`terminal.task().is_some()`) | The workspace item serialization pass calls `TerminalView::serialize()` | It returns `None` immediately; nothing is written to the `terminals` table for that item | `BR-003` |
| TC007 | IT | Zed is quit and relaunched; one closed tab was an interactive shell (custom-titled), another was mid-task | Zed restores the workspace's terminal tabs | The interactive-shell tab reopens in its remembered folder with its custom title; the task tab reopens as a plain idle terminal — the task is not resumed | `BR-003`; `edge-cases.md § tab was mid-way through running a configured task` |
| TC008 | IT | A `tasks.json` task is running inside a terminal (`SM-001` state `Running`) | The task's process exits with code `0` | State transitions `Running → Completed{success:true}`; the terminal prints "Task `<label>` finished successfully" | `SM-001` |
| TC009 | IT | A `tasks.json` task is running inside a terminal | The task's process exits with a non-zero code | State transitions `Running → Completed{success:false}`; a summary line reads "Task `<task name>` finished with exit code: N" | `SM-001`; `edge-cases.md § process exits with a non-zero code` |
| TC010 | IT | A `tasks.json` task is running inside a terminal | The task's process is killed by a signal instead of exiting normally | State transitions `Running → Completed{success:false}`; a summary line reads "Task `<task name>` terminated by signal: N" instead of an exit code | `SM-001`; `edge-cases.md § process is killed by a signal` |
| TC011 | IT | A `tasks.json` task is running inside a terminal | The terminal's event loop shuts down before an exit status is observed | State transitions `Running → Unknown`; no summary line is guaranteed | `SM-001` |
| TC012 | UT | A project's configured shell path is invalid | The developer opens a terminal and the shell fails to spawn | The failure is recorded via `detach_and_log_err` in the application log only; no toast is shown and no terminal tab opens | `edge-cases.md § interactive terminal's shell fails to spawn` |
| TC013 | IT | A configured task's command binary is missing | The developer runs the task and the spawn fails | The workspace immediately shows a toast: "Task spawn failed: {error detail}" | `edge-cases.md § configured task fails to spawn` |
| TC014 | UT | A developer has the terminal search bar open over scrollback content | The developer looks for a "replace" control | None is shown — `SearchOptions.replacement` is `false` for `TerminalView`, since terminal history is read-only | `edge-cases.md § searches terminal scrollback and looks for a "replace" option` |
| TC015 | UAT | Developer arrives at the editor workspace with the terminal panel closed | Developer toggles the TerminalPanel open (keybinding or command palette) | A shell spawns in the project directory and its output starts streaming into a TerminalView tab | `screens.md § User Journey step 1` |
| TC016 | UAT | Developer has the terminal panel open | Developer runs a project-configured task instead of typing a command by hand | A new (or reused) TerminalView tab shows the task's command output, followed by a summary line once it finishes | `screens.md § User Journey step 2` |
| TC017 | UAT | A session is running inside the TerminalPanel | Developer hides the panel to focus on code, then reopens it later | The session kept running in the background while hidden and is exactly where the developer left it on reopen | `screens.md § User Journey steps 3-4` |
| TC018 | UAT | Developer has a TerminalView tab focused with scrollback content | Developer opens the search bar (Cmd-F) and enters a query | Matches highlight in the scrollback and the developer can jump to the next/previous match | `screens.md § User Journey step 5` |
| TC019 | UAT | Developer starts either an interactive terminal or a configured task | The spawn fails for either one | If it was a task, the developer sees a clear alert that the task could not be started; if it was an interactive terminal, the failure is recorded quietly with no popup | `business-context.md § What They Do (item 4)` |
| TC020 | UAT | Developer closes the editor while several terminal tabs are open, one of them mid-task | Developer reopens the project | Ordinary terminal tabs reopen in the same folder with any custom name kept; the tab that was running a task does not resume — rerunning it from scratch is expected | `business-context.md § What They Do (item 6)` |

---

## Coverage Notes

All `BR-###`/`SM-###`/`DISC-###` codes in `technical-spec.md` have at least one tracing test case
above (`DISC-012`: TC001-TC002; `BR-001`: TC003; `BR-002`: TC004-TC005; `BR-003`: TC006-TC007;
`SM-001`: TC008-TC011). `technical-spec.md`'s Decision Logic section is explicitly `N/A — no
user-facing decision logic beyond DISC-012` for this feature, so no `DEC-###` codes exist to
cover. `INT-001` (cross-platform process spawn) is an External Integration code, not a
`BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, so it falls outside this pass's coverage-gap family
by contract definition — no `[NO_TEST_CASE]` entry is needed for it.
