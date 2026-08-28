# The throttle that asked the wrong question

**Date**: 2026-08-25
**Severity**: High — the first fix made the reported problem three times worse
**Component**: `agent_usage`, `agent_ui/session_history`
**Status**: Fixed, reviewed, 4450/4450 green

## What Happened

Three things were asked for. One turned out not to exist, one was a bug fix, one was a UI
change.

The **usage bar for Copilot and Antigravity** was dropped, on evidence. `copilot -p "/usage"`
does answer — with `Total usage est: 0 Premium requests`, `Total session time: 5s`. That is
the _session's_ spend, not the subscription's remaining quota, and the status bar shows
"% of quota used / resets in". Putting one into the other is two units in one row. Antigravity
had already been shown to keep nothing locally. Running that probe also closed a gap this
same session had flagged and left open — _"never actually ran `/usage`"_ — so the earlier
conclusion now rests on output rather than on the CLI's description of itself.

The **429** was real, and its cause is structural: `api.anthropic.com/api/oauth/usage` is
undocumented and shared with the Claude Code CLI on one OAuth token. Two clients, one account,
one endpoint. On top of that, `observe_window_activation` called `start_polling`, which
fetches immediately — so alt-tabbing in and out five times was five requests in a few seconds.
The editor was earning the 429 it then displayed.

The **history panel** went from one grouping level to two: agent outside, project inside, both
collapsible.

## The Brutal Truth

### The fix made the reported problem three times worse

The first version throttled activation on `read_recently()`: _is every source's data fresh?_
It reads as the careful question. It is the wrong one.

`SourceState::apply` only stamps `fetched_at` on `Outcome::Windows`. `Outcome::Keep` — the
branch a persistent 429 always takes — leaves it untouched. `Outcome::Clear` nulls it. So:

| Condition              | Why the throttle never engaged                        | Cost                                                                |
| ---------------------- | ----------------------------------------------------- | ------------------------------------------------------------------- |
| Sustained 429          | `Keep` never stamps `fetched_at`                      | each activation became a 3-attempt retry chain instead of 1 request |
| No Codex CLI installed | `NotInstalled → Clear` nulls `fetched_at` permanently | the throttle never engaged at all, for what is probably most users  |

The first row is precisely the condition the change exists for. The load went _up_, by up to
3×, exactly when it mattered.

And the phase's own risk table had named it:

> _"Retry làm nặng thêm chính cái đang bị rate limit — countermeasure: plus the activation
> throttle, total load **goes down**, not up."_

The risk was identified correctly, a countermeasure was written down, and then the
countermeasure was implemented in the one way that made the risk worse. Writing the mitigation
down is what stopped it being checked: the table read as settled.

The right question was never about data. It is _have I already asked recently_ — so the answer
is an unconditional timestamp at the moment a request goes out, not a property of what came
back.

### A test that certified the defect

Worse than the bug: a test called `one_stale_source_makes_the_pair_stale` asserted this
behaviour was correct, with a rationale that sounded like judgement —

> _"One source answering is not the pair answering. Trusting the screen while half of it was
> never filled in is how a permanently blank row survives."_

Every clause of that is true. It is also an argument for the wrong mechanism, and having
written it down made the mechanism look considered. The test is now deleted, replaced by two
that assert the opposite: a failing source and a source that will _never_ answer both leave
the throttle working.

A test can lock in a defect as firmly as it can lock in a fix, and a well-argued one is
harder to see.

### Three tests that never ran while the suite said green

Three new tests landed nested _inside_ another test function. `cargo test` reported ok. The
tests did nothing.

`cargo clippy --all-targets` said `cannot test inner items`. Fixing the nesting moved the count
70 → 73. Had the lint not run, three tests would have sat there indefinitely while being cited
as coverage.

That is the second time in two sessions that a non-`cargo test` gate caught something
`cargo test` could not see. The first was `cargo fmt --check` and `check-licenses` being red
on a branch reported as done.

## Technical Details

**Retry**, in `claude.rs`: `request_once` returns `Attempt::{Done, Retry{after}, Fatal}`.
Retryable is `408`, `425`, `429` and any `5xx`; every other `4xx` is fatal, because a `401`
will answer the same next time and asking again only spends the limit. Three attempts,
backoff 1s then 4s, and any `Retry-After` clamped to 5s — 600 seconds is a legal answer, and
honouring it literally would hold a task open past the 60-second poll that supersedes it.

**Throttle**, in `agent_usage.rs`: `last_polled_at` stamped unconditionally where the request
is issued; `polled_recently()` compares against a 30-second floor; `PollReason::Manual` from
the refresh glyph is never throttled. A clock stepped backwards by NTP can read "fresh" early,
which only suppresses the on-focus fetch and never the unconditional 60-second loop — it
self-heals within one interval.

**Grouping**, in `session_history/`: the load-bearing detail is the collapse key. The same
project appears under every agent used in it, so `collapsed_groups` is keyed by
`(AgentKind, PathBuf)`, not by path. Keyed by path alone, closing `/project` under Claude would
close it under Codex. There is a test for exactly that.

Final: `agent_usage` 75, `agent_ui` 33, workspace **4450/4450**, and all four gates —
`clippy`, `cargo fmt --check`, `prettier`, `check-licenses` — at exit 0.

## What Is Still Not Done

- **Nobody has watched the indicator recover from a real 429**, or collapsed a real agent
  section. Both are GUI checks; the phase files record them unchecked rather than ticked.
- **No automated scroll-position test** exists for the panel, before or after. Collapsing an
  agent changes the row count by more than collapsing a project did, and `list_state.reset` is
  unchanged — so it is not a regression, but it is unverified.
- **Copilot and Antigravity still have no quota row**, by decision, with the probe evidence
  written into `docs/src/agent-usage.md` rather than left as an absence.

## Lesson

A risk table entry is not a mitigation. Writing "countermeasure: X" makes the risk feel
handled and is the moment it stops being checked — the entry needs a test, not a sentence.

When a throttle, cache or guard decides whether to skip work, be precise about which question
it asks. _Do I have good data_ and _did I recently try_ look interchangeable and are opposites
the moment things start failing — which is when the guard matters.
