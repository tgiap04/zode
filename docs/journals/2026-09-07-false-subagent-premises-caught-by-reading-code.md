# Worktree picker reload: three false subagent premises, all caught by reading the code

**Date**: 2026-09-07 19:15
**Severity**: Medium — feature shipped cleanly, but pipeline made checkable false claims and review caught a real regression
**Component**: `git_ui`, `fs` (test double), plus the delegation pipeline itself
**Status**: Resolved (commits `9dece91`, `c0405c2`)

## What Happened

A small feature shipped: add a reload button to the worktree picker. It went through the full pipeline — planning → implementation → test suite → review → reconciliation → landing. The feature works. What is worth recording is **what the pipeline claimed versus what the code actually contained**.

Two subagents — the `planner` and later the `project-manager` — made confident, specific, checkable assertions. Three of them were **false**, and all three were caught before landing by opening the relevant source files and reading them. No implementation subagent was involved: the code was written directly in the main thread, so the false premises were inherited into that code by trusting them, not delegated away.

A `reviewer` then found a real regression. It matters that the plan had recorded that very behavior change as an *intentional improvement* — the planner wrote it that way, and it was then implemented without challenge. The false premise was the planner's; the decision to build on it was not.

## The Brutal Truth

### False premise 1: "test harness does not exist"

**Planner's claim:** `git_ui` had no test harness for visual components that need picker state, so tests would require a real on-disk git repo. **Prescribed:** zero tests, test the feature by hand.

**Reality:** `crates/git_ui/src/stash_picker.rs:730` carries a full end-to-end test (`#[gpui::test]`) that drives a sibling picker through `FakeFs` → `Project::test` → `MultiWorkspace::test_new` → `VisualTestContext`. It re-reads `.git/worktrees/*` off the fake filesystem **on every call**, so inserting a fake admin entry, calling reload, and asserting the list changed is a real, testable behavior. Three tests were written where the plan said none were possible.

**How it was caught:** searching the codebase for existing picker tests found the stash picker and its harness immediately.

### False premise 2: "picker does not override editor_position()"

**Planner's claim:** The worktree picker does not customize `editor_position()`, so it uses the base implementation from `Picker`.

**Reality:** `worktree_picker.rs:408` overrides it explicitly:
```rust
fn editor_position(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Point<Pixels>> {
    // custom logic here
}
```

The override returns the same value, so the code was correct. But I had written a code comment repeating the false claim (`// does not override editor_position()`), and the plan carried it forward. Fixed the comment and noted it in the revision. This is the **same failure mode** as the August 27 entry: a comment stating a checkable property, unchecked.

**How it was caught:** grepping for `editor_position` in the file.

### False premise 3: "168 passed"

**Project-manager's reconciliation report:** "All 169 phase tests passed, reconciliation count 168 from before the third test was added."

**Reality:** The count was stale. The third test was written (one of the three that false premise #1 said was impossible), bringing the total to 169, but the reconciliation report's number had not been updated.

**How it was caught:** comparing the reconciliation report against the phase file it was supposed to summarize.

## Technical Details

### The real regression found by the reviewer

I had refactored the worktree reload to extract a shared fetch function that resolved `active_repository` itself:

```rust
async fn reload_worktrees(&self, cx: &mut AsyncAppContext) -> Result<Vec<GitWorktree>> {
    let repository = self.project.read(cx)?.active_repository()?;
    // ... fetch from repository
}
```

This broke the `GitWorktreeListChanged` subscription. The subscription had been registered to the original repository at construction time:

```rust
let subscription = cx.subscribe(repository, |this, _, event, cx| {
    this.reload_worktrees(cx).detach_and_log_err(cx);
});
```

But now `reload_worktrees` resolved `active_repository`, which might be **different at event time**. So the event came from Repository A, but the fetch happened against Repository B. This silently mismatched `current_branch_name` and `project_worktree_paths`, which had both been built once at construction from Repository A.

The plan had listed this refactor as an "intentional improvement — cleaner code, repository resolves inside the fetch." It was a defect dressed as a simplification, and it reached the reviewer because nobody between the plan and the code asked whether the weaker guarantee was actually better.

**Fix:** Split into two functions:
- `reload_worktrees_for(repository: Entity<Repository>)` — takes the repo explicitly, fetches
- `reload_worktrees(cx)` — resolves active repo, calls the explicit version
- The subscription passes its captured repo: `this.reload_worktrees_for(repository, cx)`

Now the subscription stays coupled to the repository it subscribed to.

### Secondary hardening

The reviewer also flagged a hardcoded `Divider::horizontal()` in the render method with no compile-time coupling to the `editor_position()` override. If one is changed without the other, the picker breaks silently at runtime. The plan had accepted this with "documented coupling" — I fixed it anyway with a comment explaining the dependency, because it is the **same shape** as the macOS DMG bug from earlier in the session: a value fixed at one end that another end can change without a compiler complaint.

## What We Tried

1. Wrote the plan and prescribed zero tests.
2. Implemented with three tests (after reading existing test patterns).
3. Ran the full gate set: fmt, clippy, 169 tests — all passed.
4. Reviewer found the subscription regression and the divider coupling.
5. Split the fetch function, fixed the subscription scope, added the coupling comment.
6. Re-ran tests — still 169 passed.

## Root Cause Analysis

The subagent claims were confident because they were plausible-sounding assertions without obvious triggers to check them:

- "test harness" requires knowing the codebase has one — searching for it answers it
- "does not override" requires grepping for the method — not obvious without the method name
- "168 vs 169" requires comparing two numbers in two different files — a consistency check, not obvious until you collate them

All three were caught by the **discipline of reading the actual code** rather than taking the subagent's word. The second regression was invisible to the subagent because it required **reasoning about side effects across async boundaries** — the subscription was captured in one phase, and the behavior change was in a later refactor, in different code. The subagent optimized for simplicity and did not see the coupling; the reviewer did.

## Lessons Learned

1. **Every claim from a subagent that talks about what code does is checkable, and worth checking.** "Does not override," "has no test," "the count is X" — grep, search, collate. Plausible-sounding is not the same as true.

2. **After accepting a subagent plan, read the code against it.** Spot-check the key assertions, especially:
   - "This component already does X" (verify with grep)
   - "This is not tested" (search for existing test patterns)
   - "The refactor simplifies because X" (reason through side effects)

3. **Async couplings need explicit naming.** When a subscription holds one entity and a refactored function resolves a different one, the bug is silent. Capture the scope at the call site: `reload_worktrees_for(repository)` makes the coupling visible in the subscription's callback.

4. **A compile-time guard is better than a comment.** The divider / `editor_position` coupling should fail to compile if one changes without the other. The current comment will drift; a enum tag coupling or a builder that enforces both would not.

## What Could Not Be Verified

- **No display environment**: The plan carried a 7-item manual GUI checklist (button placement, spacing, hover state, etc.). All measurements were cross-checked against source instead (e.g., `w_full()` at `ui_input/src/input_field.rs:159` implies the wrapper needs `flex_1`, checked). The reviewer deemed none of the 7 severe enough to block, but **nobody has opened this button on a screen**. It is in the PR body for a human to verify before merge.

- **No Windows compilation**: The worktree picker has no Windows-specific code and changed none, but the build matrix could not be checked. CI is the gate.

## Next Steps

- Verify the worktree picker UI on an actual screen before merge.
- Consider adding a `#[derive]` or builder pattern to couple the divider and position overrides at compile time.
- For future subagent planning, add a step: "Cross-check the top 5 assumptions against the actual codebase before accepting the plan."
