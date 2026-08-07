# Edge Cases — F001_Terminal

| Scenario | What Happens | User-Facing Message |
|----------|--------------|---------------------|
| An interactive terminal's shell fails to spawn (e.g. invalid configured shell path) | The failure is recorded in the application log only; no terminal tab opens | "None — silent handling (logged only, no popup)" |
| A configured task fails to spawn (e.g. its command binary is missing) | The workspace shows an alert immediately | "Task spawn failed: {error detail}" |
| A task's process exits with a non-zero code | A summary line is appended to the terminal after its output | "Task `<task name>` finished with exit code: N" |
| A task's process is killed by a signal instead of exiting normally | A summary line reports the signal instead of an exit code | "Task `<task name>` terminated by signal: N" |
| The project hibernates (goes idle) more than once without the user reopening it in between | The terminal's scrollback limit is only ever shrunk once; a second hibernate request changes nothing further, so the original limit can still be restored correctly on wake | "None — silent, no visible change" |
| User searches terminal scrollback and looks for a "replace" option | Not offered — terminal history is a read-only record, not editable text | "None — no replace control is shown in the search bar" |
| Editor is closed and reopened; a terminal tab was mid-way through running a configured task | That tab's task is not resumed or restored; only interactive-shell tabs' folder and custom name are remembered across restarts | "None — the tab reopens as a plain (idle) terminal rather than resuming the task" |
