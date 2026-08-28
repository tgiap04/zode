# The second test to certify a defect, and a comment that asserted something false

**Date**: 2026-08-27
**Severity**: High — the badge would have shed every agent's process tree for 27 of every 30 seconds
**Component**: `project_footprint`, `project`, `sidebar`
**Status**: Shipped, 4497/4497 green, and never once observed rendering

## What Happened

Two commissions. A status-bar badge showing the combined CPU and RAM of the child processes
each project owns, with a per-project breakdown on click; then a settings gear at the foot of
the project rail.

The design work went well, and it went well for one reason: **the expensive decisions were
measured before they were made, not after.** What is worth recording is the two defects that
still got through, and one factual claim of mine that quietly cost something.

## The Brutal Truth

### A test was pinning the bug. Again.

`collect()` is a pure function. It walked the process tree only on discovery ticks — one in
ten. On the other nine it attributed **root PIDs only**, so the badge would show an agent's
full tree for 3 seconds, drop every descendant for 27, then jump back. A sawtooth in which
every individual reading looks like a perfectly plausible number.

The reviewer caught it. What it also caught is worse: a test named
`discover_false_reuses_the_previous_pid_set` was asserting `Some(100)` — the root alone —
with a comment explaining that the memory "belongs to the caller (phase 04)". Phase 04 never
implemented that memory. **The test name promised reuse and the test body asserted its
absence**, and the comment made the gap look considered.

This is the second time in three sessions a well-argued test has certified a defect in this
repo. The [2026-08-25 journal](2026-08-25-usage-429-and-agent-grouped-history.md) records the
first, where `one_stale_source_makes_the_pair_stale` locked in a throttle that made the
reported problem three times worse. Both times the rationale in the comment was true in
isolation and wrong as a justification.

### A comment that asserted something checkable and false

On the rail settings button I wrote:

> Dispatched, not called: same reason as the panel toggle above — this body runs inside
> `Sidebar::update`, and the handler reaches back out through the workspace.

`Clickable::on_click` (`crates/ui/src/traits/clickable.rs:6`) hands the closure `&mut Window`
and `&mut App`, **never** `Context<Sidebar>`. There is no `Sidebar` borrow there to be
reentrant with. I had copied the neighbouring button's rationale into a place where it does
not apply.

The real constraint is duller and true: `crates/sidebar` has no `settings_ui` dependency to
call into, and dispatching keeps the button on the same action path as the command palette.
A comment that states something a reader can check and get wrong is worse than no comment —
it teaches a constraint that does not exist.

### "No spec layer in this repo"

I wrote that in a delivery manifest. It is false. `docs/features/` holds eleven features,
alongside `docs/flows/`, `docs/generated/`, `docs/system/` and a `docs/.rebuild-state.json`
cursor.

Because I believed the layer was absent, I did not look at it — and it is **three weeks
stale**, last rebuilt at `9e0737a` on 2026-08-07. Neither `keep_awake` nor
`project_footprint` nor the rail-agents work appears anywhere in it. `takumi.sddMode` is
`off`, so the staleness may well be deliberate; that is the user's call. But I did not
surface it as a question, and the reason I did not was an assertion I never checked.

## Technical Details

**Measure first, then design.** A standalone `sysinfo` probe on the target machine (817 live
processes, 10 physical cores) settled the architecture before a line was written:

| refresh                   | cost                       |
| ------------------------- | -------------------------- |
| full enumeration, mem+cpu | **12–15 ms**, 1326 records |
| narrow, 12 known PIDs     | **~165 µs** — ~80× cheaper |
| `refresh_memory()` alone  | ~5 µs                      |

That gap _is_ the design: full enumeration every ~30 s purely to discover descendants, on a
throwaway `System` dropped immediately; narrow refresh of the known PIDs every ~3 s.

**Three traps the probe found that the docs did not state:**

1. `remove_dead_processes: true` combined with `ProcessesToUpdate::Some(subset)` does **not**
   evict PIDs outside `subset` — refreshing 2 of 10 retained all 10 — and 0.37 exposes no
   per-PID removal. Terminals churn constantly, so the narrow `System` is dropped and rebuilt
   whenever the discovered set shrinks. Without that it is an unbounded cache.
2. `System::cpus()` is **empty** on a `System` that has only refreshed processes (measured:
   `0`), so normalizing CPU through `cpus().len()` divides by zero. `physical_core_count()`
   returned `Some(10)` correctly on the same object.
3. `parent()` **is** populated under `ProcessRefreshKind::nothing()` — 542 of 840 records
   reported one, and a live walk resolved `probe → zsh → claude → zode → launchd`. Which also
   means command lines, executable paths and environments of every process on the machine are
   never read into the address space.

**A bug in the plan, caught before implementing.** The phase's pseudocode primed the narrow
`System` on _every_ discovery tick. A priming refresh moments before `sample()` collapses the
interval `sysinfo` differences to compute CPU, so every 30 seconds the CPU column would have
read near-zero for every healthy process. Priming now happens only on the rebuild branch.

**The flaky LEAK, resolved by patience.** `recent_projects ssh_config::tests::does_not_fuzzy_match_host_aliases`
reported `LEAK` in one full-suite run. The crate has no dependency on any changed code and no
changed files. An isolated re-run timed out, so it went into the record as _unconfirmed_
rather than dismissed. Across three full runs of the same tree it appeared exactly once —
which is what finally settled it as flaky, and only then was it safe to say so.

## What Is Still Not Done

- **Nobody has opened the editor.** The badge, its popover, the two icons, and the settings
  gear have never been seen rendering; clicking has never been observed opening anything. The
  numbers above come from a standalone probe, not from the running app. Every manual box in
  both plans is deliberately unticked.
- **Windows and Linux are untested** for the footprint feature; `parent()` under the empty
  refresh kind was verified on macOS only.
- **No automated coverage** for the poll loop end to end, `Render`/popover output, window
  activation gating, or the multi-level descendant walk against a real tree.
- **`crates/sidebar/src/rail.rs` is 239 lines**, over the ~200 guidance — it was already 217
  before the gear was added. Left as separate work by agreement with the reviewer, worth
  doing before the footer grows a fourth button.
- **The spec layer is three weeks behind the branch**, as above.

## Lesson

**When a test's name and its assertion disagree, believe the assertion.** Both defects this
repo has shipped into a green suite were protected by a test whose _comment_ was persuasive.
A rationale written next to an assertion makes the assertion feel examined; it is the
assertion that runs.

**And a claim about the repository is a claim, not a background fact.** "No spec layer here"
took four seconds to check and I did not spend them, so a three-week drift sat unmentioned
through two deliveries.
