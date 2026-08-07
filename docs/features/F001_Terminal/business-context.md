# Business Context — F001_Terminal

## Why It Matters

Developers spend a large share of their time running commands — builds, tests, linters, git —
and having to switch to a separate terminal app breaks focus. The integrated terminal keeps that
work inside the editor window, alongside the code it operates on.

## Who Uses It

- **Developer working in the editor** — runs ad hoc shell commands (build, test, git) without leaving the window, and needs to see and interact with the output live.
- **Developer running a project-configured task** — runs a predefined command (from the project's task list) with its arguments and working directory already set up, and wants to know whether it succeeded or failed.
- **Developer reviewing long output** — needs to find earlier lines in a terminal that has scrolled past, without manually scrolling through everything.

## What They Do

1. Developer opens a terminal — a shell starts in the project's own folder and streams its output live.
2. Developer shows or hides the terminal panel at will — a command that is still running keeps running while the panel is hidden, so nothing is lost by tucking it away.
3. Developer selects a project-configured task instead of typing a command by hand — the task's command, arguments, and folder are already set up, and once it finishes, a summary line reports whether it succeeded or failed (and its exit code, if it failed).
4. If a task fails to even start, the developer is shown a clear alert saying the task could not be started; if an interactive terminal session itself fails to start, that failure is recorded quietly in the background rather than interrupting the developer with a popup.
5. Developer searches within a terminal's history for a word or phrase — every match is highlighted and the developer can jump to the next or previous one, though there is no "replace" option since a terminal's history is a record of what already happened, not editable text.
6. When the developer closes the editor and reopens the project, ordinary terminal tabs reopen in the same folder with any custom name kept — but a tab that was running a configured task does not attempt to resume; rerunning the task from scratch is the expected way to pick it back up.

## Unresolved Questions

- **Display-only terminal usage**: the codebase supports a terminal variant that shows fixed text without a live running process behind it, but no in-house feature was found that actually creates one in this pass — is this exclusively for a third-party/plugin-style integration, and if so which one should be documented as its business owner?
