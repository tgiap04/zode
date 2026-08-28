# I wrote the rule into the PR, then broke it in the same session

**Date**: 2026-08-27
**Severity**: Medium — nothing was lost, twice, by luck rather than by process
**Component**: `workspace/status_bar*`, `zed/status_bar_items`, `agent_usage`, `settings_content`, `docs`
**Status**: Shipped to the branch, and never once run

## What Happened

Right-clicking an empty part of the status bar now opens a menu of fifteen switches. Hiding an
item removes it from the bar and **drops its entity**, so any `Task` or `Subscription` it owned
dies with it — the commission was explicitly that a hidden item stop costing RAM and CPU, not
merely stop drawing.

Seven phases, three of them run in parallel. The feature works. What is worth recording is that
the same coordination mistake happened twice in one session, the second time by me, one turn
after I had written the rule against it.

## The Brutal Truth

### A `git stash` in a shared tree, and an exit code that lied

Three implementer agents ran concurrently against one working tree. To check whether a clippy
failure was pre-existing, the phase-02 agent ran `git stash`.

The phase-06 agent then had a `cargo test` run **compile and execute a stale binary — 77 tests
instead of 79 — and still report exit 0.** It caught this only because it grepped the output for
its own new test names instead of trusting the pass count.

That is a nastier variant of a trap this repo has hit four times already in cruder form (piping
cargo through `tail` and reading the pipe's exit code). Here nothing was piped and the exit code
was genuinely 0. It was 0 for a binary that did not contain the code under test.

My briefs forbade `cargo fmt`, `cargo fix`, `cargo clippy --fix` and `cargo xtask workflows`. They
said nothing about `git`. The agent did nothing it had been told not to do.

### Then I did the same thing

One turn after writing _"Never run a writing git command from a subagent sharing the working
tree"_ into the PR description as a proposed rule, I committed `docs/` while the `doc-writer`
agent was still writing to it. Commit `67c1b13` captured its edits mid-flight.

I knew it was running. I had said, in the previous turn, that I would wait for it so its changes
went into the same commit. Then the user said commit, and I committed without re-checking.

Nothing was lost — the agent later confirmed the captured diffs were byte-identical to what it
had written, and the remainder was one heading rename that went in a follow-up commit. Both
recoveries were luck. Neither was process.

The lesson is not "be careful". It is that **writing a rule down is not the same as installing
it.** I had the rule in front of me, in text I had authored minutes earlier, and it did not fire.

### The test that proves the feature was the one that got omitted

Phase 03 delivered five tests for the reconciler. None used `WeakEntity`. Its removal test
asserted that the item had left the vector — which the plan itself had already called out as
insufficient:

> a removal that leaves the entity alive somewhere — a build closure that captured a strong
> handle, a registered action holding one — still empties the vector and still polls forever.

So the one claim the whole commission rests on was the one thing not demonstrated. I added
`hiding_an_item_drops_its_entity`: `downgrade()` first, `upgrade().is_some()` before the hide as
a negative control, `upgrade().is_none()` after. Phase 05 then added a stronger version driving
the same proof through the real `update_settings_file` path.

Root cause is my brief. I listed the phase's success criteria without marking which one was the
point. A list of eight criteria reads as eight equal items; it was seven plus one.

### Over-correcting one constraint created another

After the stash incident I tightened file ownership hard. The consequence: phase 05's four
integration tests and their helpers had exactly one permitted destination, and
`status_bar_toggles_tests.rs` is now **802 lines** against a ~200-line guideline. The agent
disclosed it and attributed it correctly — to the boundary, not to a design choice.

A constraint added in reaction to one failure bought a smaller failure somewhere else. Worth
noticing before adding the next one.

### Two false alarms from stale diagnostics

Twice the IDE diagnostics reported what looked like hard compile errors — `E0583: unresolved
module` for `zed/status_bar_items.rs`, then `E0599: get_global not found`. Both were snapshots
taken while an agent was mid-write; `cargo check` returned 0 both times. A diagnostic and a build
are not the same evidence, and during concurrent work the diagnostic is the less current of the two.

### And one agent was simply right about something nobody asked it to look at

The `doc-writer` found a **pre-existing broken link**: `visual-customization.md` carried two
`### Status Bar` headings, so mdBook slugged the second `#status-bar-1` and every one of five
links written as `#status-bar` landed on the first — a section about the line indicator and dock
buttons. Three of those five predate this branch.

It also found a state the docs described that can no longer occur: `keep_display_awake: false`
now causes the reconciler to **remove** the indicator's entity, while `KeepAwake::render` still
carries a `Status::Disabled` → "Turned off in settings" branch the docs presented as an observable
dimmed icon. The icon is gone, not dimmed. That branch is now dead code — a side effect of this
feature that no phase anticipated.

## Technical Details

**Why a filler sibling and not a wrapper.** GPUI dispatches bubble-phase listeners in reverse
registration order, and `RightClickMenu::paint` paints its child _before_ registering its own
listener. Wrapping the bar would make the outer menu register last, fire first, `stop_propagation`,
and silently swallow the agent-usage indicator's own right-click. The surface is therefore a
`flex_1`/`min_w` sibling occupying the gap between the two groups.

**Why ranks.** `remove_item_at`/`insert_item_after` index flat across left-then-right while the
right group renders reversed. Reasoning through that at each call site is how an off-by-one gets
in, so every item carries a monotonic rank assigned at registration and one pure
`insertion_index` holds all of it.

**Why the menu never touches the bar.** It writes a setting and returns; a single settings
observer reconciles. One code path, so a hand edit of `settings.json` behaves identically — there
is a test that drives both and asserts they converge.

**A real defect closed on the way.** `agent_usage` consulted its per-agent switch only in
`visible_sources`, i.e. at render time, while `start_polling`/`restart_timer` never read it. So
`claude_usage_button: false` still issued an HTTP request every 60 seconds and merely hid the
number. A disabled agent's arm of the `join` is now `std::future::ready`, making `fetch`
unreachable rather than merely unused.

Only four new settings keys were needed: eleven of the fifteen items already owned a switch. A
uniform `status_bar.items` map would have given five of them a second one, and the two would
have disagreed the first time anyone edited either.

## What Is Still Not Done

- **Nobody has opened the editor.** No right-click performed, no menu seen, no item watched
  disappearing or returning. Seventeen manual checks across the plans are recorded unticked.
- **Two visual unknowns:** whether `min_w(px(24.))` leaves a crowded narrow bar looking right,
  and whether a fifteen-row menu opens upward correctly.
- **`KeepAwake`'s `Status::Disabled` branch stays**, and the claim in this entry's first draft
  that it should be removed was wrong. `Holds::status()` still produces that variant on every
  render when the setting is off, so deleting only the `explanation()` arm does not compile, and
  deleting the variant means deleting its producer — after which `status()` reports "The system
  refused the request" for a state whose truth is "you switched it off", because the `Disabled`
  check sits ahead of the `running.is_empty()` check. It is also not provably dead: on a settings
  change the `StatusBar` reconciler and `KeepAwake`'s own observer both fire, GPUI does not specify
  their order, and a frame may paint before the entity is removed. Dead-but-correct beats
  live-and-wrong. What was genuinely wrong was the documentation, which described it as a dimmed
  icon the user would see; that is fixed.
- **`cargo clippy -p workspace --all-targets` fails** with a pre-existing `E0004` at
  `persistence.rs:1618` from cargo feature unification. The real gate (`./script/clippy`) exits 0
  and never surfaces it, and the diff touches neither that file nor the `remote` crate. I did not
  re-verify it on a clean tree, because doing so needs the same `git stash` that caused the race
  above.

## Lesson

**A rule written down is not a rule installed.** I authored the ban on writing git commands in a
shared tree and violated it in the same session, minutes later, on a tree I knew an agent was
editing. If a constraint only exists as prose, it fires exactly when you happen to remember it.
The version that works is structural: isolated worktrees, or a check before the commit.

**And when a plan names one test as the proof of the whole feature, say so in the brief.** Eight
success criteria in a list read as eight equal items. One of them was the feature.
