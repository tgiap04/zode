# The file that could not say whether it was finished

**Date**: 2026-09-14
**Severity**: Medium — a feature landed, but two of the three lessons are about checking, not about code
**Component**: `agent_sessions`, `agent_ui`, `git_ui` (`branch_panel/`)
**Status**: Resolved (commits `9c7b7c9`, `445baae`, `6e12d2a`, `4fbdec4` on `feat.drag-to-reorder-stacked-dock-panels`)

## What Happened

A session's subagents had to appear under it in the worktree panel, and a
subagent at work had to light the tab. The obvious design was wrong, and a
measurement said so before any of it was written. Then two separate checks
caught two false claims — one of them mine.

## The Brutal Truth

**The obvious liveness signal does not work, and only measurement said so.**
Each subagent writes its own transcript. "Still being written" reads like
"still running", and the code would have been four lines. Before writing them
the gaps between writes were measured on real finished logs:

| subagent          | lines | span   | p90 gap | **max gap** |
| ----------------- | ----- | ------ | ------- | ----------- |
| `agent-a0118bd7…` | 69    | 139.8s | 2.7s    | **58.6s**   |
| `agent-a181595…`  | 109   | 316.5s | 0.6s    | **165.4s**  |
| `agent-a67cc6a…`  | 136   | 150.6s | 4.3s    | 12.0s       |

A threshold under 165 seconds calls a working subagent finished. A threshold
over it leaves a spinner running for most of three minutes after the work is
done. There is no good number, which means there is no good version of that
design — and the whole commission was _stop the status flickering_. The obvious
fix would have introduced the exact fault it was asked to remove.

The exact signal was already on disk and cost one more measurement to find: the
sidecar records the `toolUseId` the parent used to spawn the subagent
(`{"agentType":"reviewer","description":"…","toolUseId":"toolu_…"}`), and the
parent's own transcript carries first a `tool_use` and later a `tool_result`
under that same id. Two lines of the parent's log bracket the subagent's life.
That is what `claude_log::completed_tool_uses`
(`crates/agent_sessions/src/claude_log.rs:126`) reads.

**The scan has to stop where a line stops, not where a file stops.** A live
transcript is being appended to while it is read, so a pass regularly ends
mid-line. `read_completed_tool_uses` (`crates/agent_sessions/src/claude.rs:878`)
therefore reports the offset of the last `\n`, not the file length — a result
missed once is missed forever, because the scan never looks back. Neutralising
that rule failed its test at **201 bytes (the whole file) against 101 (the last
complete line)**.

**The honest version of that rule has a hole, and it is recorded rather than
papered over.** A line that says `tool_result` and cannot be parsed is skipped,
and its bytes are marked read. Refusing to advance would re-read the same
unreadable bytes on every pass forever without progressing — so the choice is
between two bad options, not between a bad one and a good one. The skip is
logged and a test pins the behaviour, and the reviewer recorded it as `Defer`
rather than resolved, at my request. A spinner that will not stop is a bug
someone reports; that log line is what explains it.

## Two checks that caught two false claims

**I reported seven findings fixed when six were.** The fix for the last one was
applied by a script matching on `--` while the file contained an em dash, so its
`assert` raised on the _first_ replacement and that file was never written. I
read the traceback, assumed it belonged to the second replacement, and said
"all seven fixed" without re-reading the file. The reviewer refused to record
the finding as resolved, put it in `refuted`, and returned `REWORK` instead of
`SEALED`. It was right; the comment at
`crates/agent_sessions/src/claude.rs:852` was byte-for-byte unchanged.

The lesson is not "scripts are risky". It is that **a tool reporting failure is
not the same as knowing what failed**, and the gap between those two is exactly
where a false claim fits.

**`project-manager` corrected a true sentence into a false one — for the second
time, by the same mechanism.** On the previous plan it read a doc comment
describing an era _before_ `HEAD` as if it described `HEAD`. On this one it
compared a sentence about what the review _returned_ against a verdict file the
reviewer had _re-graded after the fixes landed_, and replaced "1 High, 4 Medium,
2 Low" with "0 High, 3 Medium, 4 Low" — leaving the paragraph saying "0 High"
immediately above a bullet beginning "The High …".

Both counts are true, of different moments. The failure mode is the same both
times: **a claim about the past checked against the present**, with no attention
to which moment the sentence was about. The repaired sentence now names both
counts and says why they differ.

## Technical Details

### What the design cost, and where it is

- `settle` (`crates/agent_ui/src/agent_view.rs:97`) is a pure function: a change
  must survive `RESPONDING_DEBOUNCE_TICKS` in a row before the mark follows it,
  on both edges. `RESPONDING_DEBOUNCE_TICKS`
  (`crates/agent_ui/src/agent_view.rs:72`) is _derived_ from the duration and
  the tick, so the two cannot drift apart when one is tuned.
- Ticks and not a clock, because `Instant::now` does not move under a test's
  fake timers and the alternative was a dependency on `scheduler` for one type.
  Five debounce tests run instantly as a result; with a real clock each would
  have waited two real seconds.
- `track_subagents` (`crates/agent_ui/src/agent_view.rs:805`) is a loop of its
  own, split out after review: the mark's loop also notices when the CLI exits,
  and a first pass over a multi-megabyte transcript would have held both behind
  however long a disk takes.
- `activity_for` (`crates/git_ui/src/branch_panel/tree.rs:133`) gives the
  session's mark to the subagent's row. The pty carries a subagent's output as
  the session's own, so the session cannot tell them apart — the row that can
  is the one beneath it.
- `follow_listed_subagents`
  (`crates/git_ui/src/branch_panel/lifecycle.rs:260`) trims three caches to what
  the panel is drawing. That is also the invalidation: a resumed session leaves
  the past set, so its stale entry is dropped and re-read when it closes again.
  One policy answering two review findings.
- The sidecar is re-listed only when the transcript grew
  (`crates/agent_ui/src/subagents.rs:102`). The gate is exact rather than
  approximate: a subagent cannot appear without the parent writing the tool call
  that spawned it.

## Lesson

**Measure the signal before building on it.** "Recently written" is a plausible
liveness test that a single afternoon of real logs disproves. The measurement
cost minutes; the design it rejected would have shipped the fault it was
commissioned to remove.

**A failed tool tells you something failed, not what.** Reading a traceback and
inferring which step it belonged to is a guess wearing evidence's clothes. Open
the file.

**When checking a claim, first ask what moment it is about.** Two counts of the
same review, taken before and after the fixes, are both true; a checker that
knows only "the current file" will call the older one wrong every time. This has
now happened twice in this repo, to the same agent, on consecutive plans.
