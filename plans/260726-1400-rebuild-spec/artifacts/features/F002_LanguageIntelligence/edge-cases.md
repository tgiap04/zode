# Edge Cases — F002_LanguageIntelligence

| Scenario | What Happens | User-Facing Message |
|----------|--------------|----------------------|
| Opening a file in an untrusted project folder | The language server is not spawned yet; the spawn is deferred until the folder is explicitly trusted, then it starts automatically | "None — silent deferral; the file opens normally without live language features until trust is granted" |
| Selecting a toolchain before the workspace has finished initializing (no database id yet) | The picker just closes; the toolchain choice is not saved and does not take effect | "None — silent no-op; user may need to reopen the picker after the workspace finishes loading" |
| Saving a toolchain choice fails (storage error) | The failure is logged internally; the toolchain still applies for the current session, it just won't be remembered next time | "None — silent handling; choice appears to work but may not persist across restarts" |
| Setting a buffer's language to one that fails to load | The buffer keeps its previous language; the failure is only recorded in logs | "None — silent handling; user may notice highlighting/behavior didn't change and can retry" |
| Restarting language servers while connected to a remote project | Instead of restarting locally, the request is forwarded to the remote host, and failures there are logged rather than raised | "None — silent handling on failure; on success, diagnostics resume as expected" |
| Default formatter (Prettier) fails to install on first use | The install attempt is counted, and formatting fails with a specific message rather than retrying silently | "Cannot start default prettier due to its installation failure" |
| Language server becomes unresponsive mid-session | User manually triggers "Restart Language Servers"; the stuck process is torn down (graceful shutdown, then a hard kill after 5 seconds) and a fresh one starts | "None — restart is a deliberate user action with no confirmation dialog; diagnostics/completions reappear once the new server initializes" |
