# Business Context — F009_Diagnostics

## Why It Matters

Developers need to know what's broken in their code without hunting through logs or scrolling
through every open file one by one. Diagnostics turns the errors and warnings that language
servers already compute into two focused views — "what's wrong with the file I'm looking at" and
"what's wrong across the whole project" — plus a quick status-bar signal when a language server
itself has failed. In this fork specifically, diagnostics also has to stay honest with the user
during the multi-project hibernation lifecycle: when a background project wakes back up, its
error/warning counts may briefly be stale, and the product says so instead of quietly showing
possibly-wrong numbers.

## Who Uses It

- **Developer editing a file** — opens the current-file diagnostics view to see only the problems in the file they're actively working on, without the noise of the rest of the project.
- **Developer doing a health check** — opens the project-wide diagnostics view before a commit or handoff to confirm nothing is broken anywhere in the project, and can filter out warning-level noise to focus on errors.
- **Developer troubleshooting a stuck language server** — glances at the status bar to see whether a language server has failed, reads the last error message, and dismisses it once acknowledged.
- **Support/bug-report submitter** — copies system specs (OS, memory, app version, GPU) to the clipboard, or (on Windows) records a performance trace, to attach to a bug report.

## What They Do

1. A developer focuses a file with known problems and opens the current-file diagnostics view — they see only the excerpts around that file's errors and warnings, nothing from other files.
2. A developer opens the project-wide diagnostics view before wrapping up work — every file across the project that has an error or warning shows up in one combined list.
3. The developer decides warning-level issues are just noise right now and toggles them off — the list narrows to errors only, without losing track of which files still have warnings waiting.
4. If the project had been sitting idle and just woke back up, the developer sees a plain banner saying the project is re-indexing and counts may be temporarily stale, rather than being shown numbers that look final but might not be.
5. In the file browser, a file whose error count is left over from before that idle period shows its badge dimmed rather than fully colored, signaling "this hasn't been re-verified yet" without hiding the information outright.
6. If a language server itself crashes, the developer notices the status-bar indicator change, clicks it to read the last error message, and dismisses it once they've seen enough — the language server keeps running or restarting on its own regardless.
7. When filing a bug report, the developer copies system specs to the clipboard (or, on Windows, records a performance trace) to hand over to whoever is investigating.

## Unresolved Questions

- **Reindex completion guarantee**: is there a maximum time a project can sit in the "re-indexing, counts may be stale" state before the team considers it a bug worth surfacing more prominently to the user?
- **Warning-filter scope**: is it intentional that turning "include warnings" on or off applies to every diagnostics view currently open, rather than just the one the developer is looking at?
