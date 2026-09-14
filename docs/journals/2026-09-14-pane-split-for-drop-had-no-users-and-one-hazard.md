# A hook with no users, and one hazard its doc comment did not name

**Date**: 2026-09-14 17:45
**Severity**: Medium — a feature landed solid, but six findings sit underneath. Three are about testing discipline; two are about comment hygiene; one is about a design choice nobody challenged.
**Component**: `floating_pane`, `workspace` (`pane.rs`)
**Status**: Resolved (commits `a0c76e0`, `52cdd67` on `feat.drag-to-reorder-stacked-dock-panels`, folding into PR #38)

## What Happened

`FloatingPane` needed to split tabs the way `terminal_panel` already did. The plan was six phases running tests to green after each one. It shipped that way — `cargo test -p floating_pane` 40 passed, `cargo test -p workspace` 292 passed, `./script/clippy` exit 0 — but the journal belongs to what those tests didn't catch and what the reviews found when they dug.

## The Brutal Truth

**The most dangerous trap in this crate's API had never been triggered.** `Pane::split_for_drop` (`crates/workspace/src/pane.rs:424`) is a hook that `Workspace` installs on each pane so it can respond to tab drops. Its doc comment says: "If this hook is not set, `handle_tab_drop` routes through `Workspace::split_pane` and the split silently lands nowhere." That is the visible failure — the one a user reports. The worse one is unwritten: the hook is invoked from inside `cx.defer_in` on a `Context<Workspace>` (`pane.rs:3832`), so `Workspace` is **leased** for the whole call. Anything that reads back through `self.workspace` during the hook hits `double_lease_panic` (`crates/gpui/src/app/entity_map.rs:164`) — a process abort, not an error. The naive implementation would have cached nothing and read `FloatingPane::project` inside `split_off`, which is inside the hook closure. That is process death on the first drop. The reason that didn't happen is that `FloatingPane::project` became a cached field (`host.rs:122`) in phase 2 before the hook was wired in phase 6. It was not obvious from the commission that a method read would cost an abort; the field change did not have its own commit. It landed as one line in a larger diff, with no special note in the review. The fact that it survived at all is luck, not discipline.

**An agent said "the harness cannot test this", and that turned out to be wrong.** Phase 4 skipped a test for `FloatingPane` dispatching split actions through a real workspace, because the test harness can only build a floating pane as a standalone entity decoupled from a workspace. The claim was unverified. Phase 5 proved it false: `workspace.register_floating_layer(floating, cx)` — the exact call `floating_pane::init` makes in production — then `cx.dispatch_action` on a real `FloatingPane`. The test run after that. The lesson is not "harnesses lie" — it is that **a limitation asserted without an attempt is a hypothesis**. The boundary between "cannot be done" and "have not tried" is where half the false design constraints live.

**A test that flaked 5% told us the environment was deterministic and we needed to listen.** `moving_the_only_tab_does_not_split_into_nothing` spawns a real terminal via `open_terminal()` and failed 3 times in 60 full-suite runs with `Detected activity on thread Some("PTY reader") ... Your test is not deterministic` from `crates/scheduler/src/test_scheduler.rs:111`. The scheduler was correct: the PTY event loop runs on a background thread, and when it wakes during the test's window, the test harness sees cross-thread activity and rejects the whole thing. The harness was right to reject it. The fix was one line: `cx.executor().allow_parking()` — the mechanism `terminal_panel.rs` uses in eight tests and which this crate's own drag test already had. 50/50 clean after. Running the test alone was 25/25 clean — the flake only appears under the full suite, so measuring it requires the suite.

**A green test that proved nothing.** The first draft of the drop test dragged the floating pane's own single tab onto the floating pane's own edge. That empties the pane; the collapse rule removes it; the tab count returns to one — passing while demonstrating nothing about whether the drop hook fired. It had to drag from a different source pane into the floating pane. A test that passes is not a test that works.

**Ten comments referencing the plan directory leaked into the shipped source, and only hand-stripping caught them.** `plans/` is gitignored, so every one was a pointer to a document no reader of the repo will ever have. Scattered across four of the six phases, two of them after an explicit instruction not to leave them. Stripped by hand because no test catches them. The durable lesson is that plan-shaped context bleeds into code unless **something** catches it — and that something has to be a review where someone reads `plans/` in parallel with the code.

**Two review findings were left unfixed, and both have reasons on record.** `FloatingPane::opening` holds a single `Option<Task<()>>`, so a second operation cancels the first silently — pre-existing design choice (the field's doc says "held... so a second operation replaces and cancels the first"), and this work didn't change that policy, only added more writers to it. The reviewer counted five writers before this work and noted it as "pre-existing and deliberate", so this is acknowledged rather than fixed. And the `if !self.open || self.is_empty(cx) { return; }` guard duplicated across four methods — the plan's own phase-5 risk table chose that trade-off over hoisting, so it was a documented decision to not refactor. The lesson here is about documentation: decisions that bypass clarity have to be _written down in a place that lasts_. They were, in this case. That is why they stayed unfixed.

**An unexplained behaviour turned up and was recorded rather than solved.** Calling `workspace.register_floating_layer(floating, cx)` a second time breaks action dispatch for every pane, including `ToggleFloatingPane` itself. Found while writing the dispatch test; traced through action dispatch and the render tree but the cause was not run to ground from outside `workspace.rs`. This entry is where that fact lives: a second `register_floating_layer` call costs you action dispatch everywhere, and nobody yet knows why.

## Technical Details

### The lease hazard

- `Pane::split_for_drop` is set at `crates/workspace/src/pane.rs:851`, invoked by `handle_tab_drop` at `pane.rs:3831` inside a `cx.defer_in` closure.
- `ctx.defer_in` (`crates/gpui/src/app/app_context.rs`) runs its callback with an exclusive borrow of the entity — `&mut Context<T>` — so `Workspace` is leased for the callback's whole life.
- `FloatingPane::project` was a method reading `self.workspace` synchronously. Phase 2 changed it to a cached field (`host.rs:122`), so the read happens once at construction.
- Removing `project()` method happened in phase 2 before phase 6 wired the hook, so it never appeared in the shipped code.

### The flaky test, step by step

1. `moving_the_only_tab_does_not_split_into_nothing` calls `handle_split` with `SplitMode::MovePane`.
2. `handle_split` (`split.rs:95-110`) checks `pane.items_len() <= 1` and falls back to `self.open_terminal()`.
3. `open_terminal` (`content.rs:114-118`) spawns a shell via `Pty::new`, which wakes the PTY reader thread.
4. PTY reader thread tries to schedule work on the test scheduler's foreground thread.
5. Test scheduler detects cross-thread activity and panics at `crates/scheduler/src/test_scheduler.rs:111`.
6. Fix: `cx.executor().allow_parking()` tells the scheduler "this test is allowed to have background threads." Borrowed from `terminal_panel.rs:1520` (eight instances), already used in this crate's `split_right_dispatched_through_a_real_workspace_splits_the_floating_group` test.

| Run batch | Passes | Fails | Rate |
|-----------|--------|-------|------|
| 1–10 | 9 | 1 | 10% |
| 11–30 | 19 | 1 | 5% |
| 1–30 combined | 28 | 2 | 6.7% |
| After fix, 1–50 | 50 | 0 | 0% |
| Isolated (alone) | 25 | 0 | 0% |

### Plan comments in the source code

Grepped the diff; found entries like:
- "Phase 2 introduced the PaneGroup" in a docstring
- "See phase-03 for why zoom works this way" in a comment
- "The guard is from phase 5's risk analysis" in an explanation

All stripped manually. No tool caught them.

### The double-writing hazard

`FloatingPane::opening` (`host.rs:188`) is one slot. Writers before this work:
1. `open_terminal` (`content.rs:114`)
2. `open_markdown_note` (`content.rs:202`)
3. `new_markdown_note` (`content.rs:221`)
4. `shut_down` (`host.rs:295`)

Writers added by this work:
5. Every `handle_split` non-`MovePane` branch (`split.rs:108-110`) calls `self.open_terminal` again.

The risk: if a user opens a file dialog (`Open Markdown Note`) and then triggers any split, the file-picker task is cancelled with no log line and no error. The dialog the OS is showing simply stops mattering. Reviewer rated this "High". It was left alone as a policy that predates this work, and is named in PR #38's known gaps.

### The dispatch test that could not exist, then did

`split_right_dispatched_through_a_real_workspace_splits_the_floating_group` (`floating_pane_tests.rs:1401-1445`):
1. Constructs a real `Workspace` via `cx.new_model`.
2. Creates a `FloatingPane` with one pane and one tab.
3. Calls `workspace.register_floating_layer(floating, cx)` — production's exact setup.
4. Dispatches `FloatingPane::SplitRight` action.
5. Asserts via `debug_bounds` that the `+` menu position changed (proving a second pane was added).

Why `debug_bounds`: the test cannot hold an `Entity<FloatingPane>` across the workspace mutation (the borrow model does not allow it), so direct pane counting is unreachable. Indirect verification via layout changes is sufficient — if no pane was added, the `+` button position would not move.

## Lessons Learned

**A method that reads an entity during another entity's lease is process death, not a correction loop.** Design the hazard away at the field level. If `project` needs to be live data, make it a field. If it never changes, cache it and call it at construction. Either way, say so in the comment — the next person will not guess that a simple read is an abort risk.

**"The harness cannot test this" is a hypothesis without an attempt.** Reach for the simplest matching call that production makes, and stage it in the test harness. If it does not work, then the limitation is real.

**Listen to the test scheduler.** When it says "not deterministic", it is right. It is also right about what thread the violation comes from. Follow the breadcrumb.

**A test's passing state is not its correctness state.** Run the test alone, then run it in the suite. Run it 50 times. A test that glows green while proving nothing is the worst kind — it wastes the next person's time.

**Plan references belong in a decision log, not in docstrings.** Docstrings reach forever; plans are temporary. If a choice is tied to a plan phase, write the choice in code and put the plan phase in a separate decision document (like `clarifications.md`), not in a `.rs` comment. When the plan is archived, the comment is still there pointing at nothing.

**A limitation is not disposed of by being written down in a working document.** Plans are scratch — this repository gitignores `plans/`, so anything whose only home is there has no home. A fact worth keeping has to reach somewhere a reader of the repository can actually open: this journal, the PR, or a comment beside the code it constrains.

## Next Steps

- `FloatingPane::opening` (High-1 from review) needs either per-pane task tracking or at least a log line when an in-flight task is cancelled. Flagged for post-PR work.
- `self.zoomed` defensive clear when upgrade fails (Medium-2): add `self.zoomed = None` to the `Option::None` arm in `render_window` so the field doesn't outlive a dropped view.
- Test for `MovePane` on a single-item pane falling back to terminal — simple to add, not yet written.
- The root cause of `register_floating_layer` breaking on a second call, pursued from outside `workspace.rs` — not run to ground.
- Manual drag-and-drop verification with live pointer input — every route was code-reviewed, nothing was visually tested. `split_for_drop` has never run in front of a person.


## Postscript: the hand test found it in a minute

Everything above shipped, and then someone dragged a tab. It did not split.

`split_for_drop` is half the wiring. `Pane::handle_drag_move` (`crates/workspace/src/pane.rs:3768`) records no edge as a split target unless `can_split_predicate` says so, and an unset predicate answers `false` — it returns before `drag_split_direction` is ever written, so `handle_tab_drop` reads `None` and performs an ordinary tab move. The editor's centre pane sets one (`crates/workspace/src/workspace.rs:1680`), the terminal panel sets one (`crates/terminal_view/src/terminal_panel.rs:1227`). This window set none. The hook was correct and unreachable.

**The tests were green because they started past the gate.** Every drop test wrote `drag_split_direction` itself, with a comment explaining that this is what real hover detection writes and that every other drag test in the crate does the same. Both statements are true. Neither is a reason: the field being writable by hand is exactly what let the test skip the thing that was broken. A test that stages its own precondition is only testing what comes after it, and nothing said so.

That is the third green-and-empty test on this branch, after `a_second_panel_sees_what_the_first_opened` and the drop test that dragged a pane's only tab onto itself. The shape repeats: the test constructs the state it means to observe, and so cannot observe the code that was supposed to produce it.

**And an error of my own worth recording.** Mid-investigation I grepped for `set_can_split_predicate` — the field's name — found nothing, and concluded drag-to-split was dead across the whole fork, editor included. The method is `set_can_split`, with four live callers. A grep that returns nothing is evidence about the grep before it is evidence about the code, and the larger the conclusion it invites, the more that is worth remembering.

The predicate now also refuses the one drag that undoes itself: a pane's only tab dropped on that pane's own edge, which empties the source and lets the collapse rule remove the split as it is made. Both drop tests assert the gate is open before they begin, and both fail when the predicate is removed — checked, not assumed.
