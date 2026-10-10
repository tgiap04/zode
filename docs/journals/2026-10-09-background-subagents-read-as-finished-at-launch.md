# Background subagents read as finished the moment they launched

**Date**: 2026-10-09
**Severity**: Medium — the branch panel's subagent spinner never ran for the most common kind of subagent
**Component**: `agent_sessions`, `agent_ui`, `git_ui`
**Status**: Resolved on `feat.release-v0.1.5`; not yet checked by hand in the app

## What Happened

Under each agent in the branch panel, the subagent list showed every subagent the session had ever spawned behind an "N subagents" disclosure, and the spinner for a running one never turned. The request was for the panel to show only subagents that are running, each with a working spinner, and to leave the main agent row as it was.

## The Brutal Truth

The rule for "finished" was right when it was written and wrong for how subagents are used now. A subagent counted as finished once the parent transcript held a `tool_result` for its call. A foreground subagent's result does arrive when it finishes. A background subagent's result is "Async agent launched successfully", written within about a second of the launch, while the agent goes on working for minutes. So every background subagent read as finished at birth. This was noticed while building the transcript-based notifications four days earlier and set aside as out of scope. It was the whole bug.

## Technical Details

### What a real transcript records

All of the following were measured on local transcripts.

- When a background agent stops, the parent transcript gets a line `{"type":"queue-operation","operation":"enqueue","content":"<task-notification><task-id>…</task-id><tool-use-id>…</tool-use-id>…<status>completed</status>…"}`. The same text appears again in `remove`, attachment and message copies, and those must be ignored.
- Statuses seen: completed, failed, killed and stopped. Any of them ends the run.
- Across 427 enqueued notifications, none carried more than one task.
- A resume shows up as a `tool_result` whose `toolUseResult.resumedAgentId` names the agent. Across 45 resumes, every one was followed by a notification.
- The sidecar `agent-<id>.meta.json` records `requestShape: "background"`.
- A finished subagent's own transcript ends with the tool result of its report, not with an `end_turn`, so the sidecar file cannot say when a subagent is done.

### The rule now

The existing incremental transcript pass also reads `Stopped` and `Resumed` events, in file order. A background subagent runs until its notification. A foreground one runs until its result or a notification, the latter covering an agent moved to the background mid-run. A resume makes either kind run again.

A tab that opens onto an existing transcript settles any subagent still open in that history as ended, because it belonged to a CLI process that no longer exists. The tracker's sets are pruned to the listed subagents, but only after an id is missing from two lists in a row. A sidecar that is unreadable for one pass therefore cannot lose a stop. `AgentView::running_subagents` returns nothing unless the CLI is alive.

### The panel

Running subagents are drawn as plain rows under the agent, with the spinner keyed by subagent id so one finishing does not restart the others. There is no disclosure. A finished session shows none, and the code that read a finished session's subagents from disk is gone. `activity_for` is unchanged, by the user's choice: any running subagent, background included, makes the main row show Ready.

A side effect, documented in `keep-display-awake.md`: the tab's answering mark and the display hold now stay on while a background subagent really runs, because `is_answering` already folds running subagents in.

## Proof on Real Data

A temporary test, since deleted, ran the real `ClaudeProvider` and `SubagentTracker` over this session's own transcript. It read the first quarter as history and the rest as live. Of 45 listed subagents, exactly one read as running: the background agent that was running at that moment. The other 44, all finished, read as not running.

## Lessons Learned

- "It received a result" is not "it finished" once calls can be asynchronous. Any completion rule keyed on a tool result needs checking against a background call.
- An out-of-scope note in one journal turned out to be the next bug. Worth re-reading the open items of recent journals before investigating a nearby report.
- A tester asked to print the running state on real data printed everything except the running state. Ask for the exact line that proves the claim, and check that it came back.

## Gates

`./script/clippy` exit 0. `cargo test`: `agent_sessions` 136, `agent_ui` 220, `git_ui` 257, `sidebar` 61, `agent_notify` 30, `keep_awake` 30 passed. `zode` and `floating_pane` compile. `rustfmt --check` and prettier exit 0.

## Still Open

- Not seen in the running app.
- A resumed run that never gets a notification would read as running until the CLI exits. It has not been observed, since all 45 local resumes ended with one.
