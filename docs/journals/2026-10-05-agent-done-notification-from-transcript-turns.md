# Finished answering — read from the transcript, not guessed from the pty

**Date**: 2026-10-05
**Severity**: High — the notification fired while agents were still working, or before they had run anything
**Component**: `agent_notify`, `agent_ui`, `agent_sessions`, `terminal`
**Status**: Resolved on `feat.release-v0.1.5`; not yet observed end to end on a bundle

## What Happened

"Finished answering" notifications arrived while an agent was mid-turn, and sometimes when nothing had been asked at all. The notifier fired on `AgentView::is_answering()` going false and staying false for `quiet_period_ms` (12 s). `is_answering()` is inferred only from how often the pty is written to: at least `RESPONDING_WRITES` (8) writes in a 1 s window, held for a 2 s debounce on a 250 ms tick. The crate's own doc already admitted the notification "can and will fire while the agent is still working".

## The Brutal Truth

The rule was wrong in ways that took minutes to prove and that no amount of tuning could fix. Replaying the exact rule against real `claude` pty output:

- typing a prompt without pressing Enter (8–10 writes/s) produced an answering edge, then a notification;
- a 3 s window-resize drag (37 writes/s peak) did the same;
- a turn running `sleep 40` wrote nothing for about 38 s mid-turn, so the notification fired while the command was still running;
- an idle startup peaked at 5–7 writes/s, just under the threshold, which is why "nothing happened" was not every time.

A rate cannot say "the turn is over". The transcript can, and the editor was already reading it once a second for subagents.

## Technical Details

### The signal

For a tracked Claude tab, the existing incremental transcript pass now also yields turn marks (`claude_log::scan_chunk`): a prompt, a tool call, `end_turn`, `turn_duration`, an interrupt. `turn_tracker.rs` folds them into `TurnEvent { Started, Ended, Interrupted, ApprovalNeeded, ApprovalCleared }`. The notifier waits `TURN_END_CONFIRMATION` (3 s) after `Ended` so a turn that continues can retract it. Other agents and untracked tabs keep the pty heuristic unchanged; `is_answering()` itself did not change, so `keep_awake` and the branch panel behave as before.

Transcript facts the fold depends on, all measured on local transcripts:

- one reply writes `end_turn` on two lines (thinking and text) sharing one `message.id`, so the fold dedupes by id;
- `turn_duration` follows in newer CLIs but is missing in some (2.1.219), so an answered turn waits at most `ANSWERED_PATIENCE_PASSES` (8) for it;
- lines the CLI injects (subagent hand-backs, task notifications) carry `isMeta: true` and are not prompts, so a text-only reply to one has to start its own turn;
- on resume the CLI writes a `"<synthetic>"` "No response requested." reply, which is skipped unless it is an API error;
- the first pass over a transcript is a silent baseline, so reopening a session does not announce its history.

### Approval

Claude writes no bell or OSC for its permission dialog in a terminal with `TERM_PROGRAM=zed` — verified — so the dialog is read off the screen. `screen_asks_for_approval` looks for a line starting "Do you want to" with "1. Yes" below it, in `Terminal::visible_content()` (live screen rows only), at most once a second, and only while the transcript shows a tool call with no result and the pty has stayed under `APPROVAL_QUIET_MAX_WRITES` (15) for 3 s. A missed dialog loses only that notification. It cannot be withdrawn once answered: gpui has no API for that.

### Background agents

A `turn_duration` with `pendingBackgroundAgentCount > 0` holds the turn, by user decision, until a later turn ends with nothing pending. If a background agent hangs, the hold is released after `BACKGROUND_QUIET_LIMIT` (600 s) with no write to any `subagents/agent-*.jsonl`, or after `HELD_UNJUDGED_PASSES` (1800) when there is no such file to judge by.

## What Went Wrong On The Way

- **The first safety release was wrong on real data.** It released a held turn when `SubagentTracker::any_running()` was false. Checking this session's own transcript showed every background `Agent` call gets its tool_result ("Async agent launched successfully") within a second, while the agent runs for minutes. `any_running()` was false almost immediately, so the release would have announced within seconds. Replaced by the sidecar mtime check above. The tab's own subagent mark still has this inaccuracy; it predates this change and was left alone.
- **Exit and notify raced.** The 1 s transcript loop could deliver `Ended` after "Session ended" had posted. Found in review; an `exited` flag now drops it. The same review found an older bug: `fire_exit` cleared the awaited terminal id, so the next `reread` re-armed a waiter on the dead terminal and posted "Session ended" a second time.
- **"Start a New Session" silenced a tab for good.** The notifier captured `reports_turns()` once per tab, but starting a new session makes the tab untracked. It is now read on every event.
- **`live = from != 0` was not "first pass".** A pass that found no transcript yet left the cursor at 0, so the real first exchange was read as baseline. Replaced with an explicit flag, and a shrunk transcript resets the tracker and re-reads silently.

## Lessons Learned

- Before shipping a heuristic, replay it against recorded real input. A pty harness and the rule written out in a few lines of Python settled in minutes what the shipped feature had been guessing about.
- Any "is it still working" signal has to be checked against how background agents report: their tool call returns at once. Check on a real transcript, not on the shape of the data model.
- Three review rounds each found real defects in the previous round's fixes. For a state machine fed by two loops, plan for the second and third review, not just the first.

## Gates

`./script/clippy` exit 0. `cargo test`: `agent_notify` 30, `agent_sessions` 124, `agent_ui` 180, `keep_awake` 30, `terminal` 74, all passing; `agent_notify` and `agent_ui` run 5 times each with no flaky failure. `rustfmt --check` on touched files and prettier 3.5.0 on the touched docs and `default.json` exit 0. `zode`, `sidebar` and `floating_pane` compile. Linux and Windows not checked locally.

## Still Unverified

Nothing here has been seen as a real OS notification. Still to run on a debug bundle: typing without Enter, resize, `sleep 40`, a short reply, a Bash permission dialog, Esc mid-answer, resuming a session, a Codex tab, and quitting the CLI right after a reply.
