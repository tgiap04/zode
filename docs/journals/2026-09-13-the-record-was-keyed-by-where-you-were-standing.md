# The record was keyed by where you were standing, not by the repository

**Date**: 2026-09-13 21:40
**Severity**: Medium — a user-facing defect that survived the fix meant to close it, and the whole test suite that shipped with it
**Component**: `git_ui` (`branch_panel/`)
**Status**: Resolved (`feat.drag-to-reorder-stacked-dock-panels`, PR #38)

## What Happened

Hours after [2026-09-13-agent-list-survives-checkout-switch.md](2026-09-13-agent-list-survives-checkout-switch.md)
declared defect 1 fixed, the reader came back with the same symptom in their own words: open a
checkout's agent list while standing in checkout A, switch to B, and A's list is closed again.

That journal's fix was real and necessary — the record did move off the per-workspace key and onto
one process-global entity, and two panels really do share it now. It just was not sufficient,
because the *contents* of the key were wrong in a second, independent way that moving the key could
not touch.

## The Brutal Truth

`StoredKey` is `(repository, checkout)`. The checkout half was always right. The repository half was
`RepoData::path`, which is `RepositorySnapshot::work_directory_abs_path` — and that is **the checkout
the workspace is open at**, not the repository.

`git_store.rs` says so twice, in code:

- `linked_worktrees` is built by filtering `work_directory_abs_path` back out of `git worktree list`
  — "linked" means "every checkout except the one you are standing in".
- `is_main_worktree()` is literally `work_directory_abs_path == original_repo_abs_path`.

So standing in `wt-a`, opening `wt-a`'s agent list wrote `WorktreeAgents("/repo/wt-a", "/repo/wt-a")`.
Switching to `wt-b` builds a second workspace whose repository reports `work_directory_abs_path =
/repo/wt-b`, and that panel asks for `WorktreeAgents("/repo/wt-b", "/repo/wt-a")`. Different key.
Nothing found. Draws closed.

Every row re-keyed itself on every switch. The sharing worked perfectly; both panels were simply
looking in different drawers of the same shared cabinet.

`RepositorySnapshot` has carried the right value the whole time: `original_repo_abs_path`, the
original repository's working directory, identical seen from any of its checkouts, set at repository
creation from the git common dir rather than after a scan. `RepoData` now carries it as `anchor`,
and the four sites that resolve a `RowKey` into a `StoredKey` use it.

## Why the suite did not catch it

`a_second_panel_sees_what_the_first_opened` (T1) was written for exactly this scenario and passed
throughout. Its doc comment stated the premise:

> A second panel over the same checkout — a different `RepositoryId`, the same path, exactly what a
> checkout switch produces

The first clause is right and the last is wrong. A switch produces a different `RepositoryId` **and**
a different path. The test built both panels with the same `repo_data(id)` fixture, so both stood in
the same checkout — and two panels standing in the same place agree on the key whether or not the key
is the right one. The test could not fail. It was a test of the sharing, mislabelled as a test of the
switch.

This is the fourth time in this piece of work that a green suite was not evidence, and the first
where the blind spot was written down in prose in the test's own doc comment. Nobody had to infer the
false premise; it was there to read.

## The migration nobody would have noticed

Changing the key orphans every entry already on disk: `prune` only ever matched entries whose
repository component equalled the repository it was pruning for, so the old namespaces would have sat
in the record unread and unprunable, one per checkout the reader had ever stood in, forever. That is
the unbounded growth CLAUDE.md forbids, arriving by way of a fix.

`prune` now re-keys them instead of dropping them: the second component always named the right
checkout, so the entry is still exactly true — only its namespace was wrong. Scoped to namespaces
that are live checkouts of the repository being pruned, so a panel still never rewrites a project it
cannot see. The reader keeps what they opened, and the orphans are gone after one rebuild.

The same pass covers a race nobody set out to fix: `RepositorySnapshot::empty` falls back to
`original_repo_abs_path = work_directory_abs_path` before the real value is known, so a toggle in
that window writes an old-shape key. The next rebuild migrates it.

## Verification

Each mechanism was neutralised in place and the matching test watched to fail:

| Neutralised | Test that failed |
|---|---|
| `anchor` → `path` at the four resolution sites | `an_agent_list_opened_in_one_checkout_is_still_open_from_another`, `a_repository_closed_in_one_checkout_is_closed_from_another` |
| re-key filter made unreachable | `prune_rekeys_an_entry_written_under_a_checkouts_own_namespace` |
| re-key filter made unscoped | `prune_rekeys_nothing_outside_its_own_checkouts` |

With `anchor` reverted, the eight pre-existing tests in `restoring_expansion` still passed — which is
the measurement that matters, and the reason the new tests exist at all.

`cargo test -p git_ui`: 209 passed. `./script/clippy -p git_ui`: clean.

## Lessons

1. **A test's doc comment can carry the false premise.** T1 spelled out "exactly what a checkout
   switch produces" and was wrong about it. Read the premise, not just the assertion — an assertion
   is only as good as the situation it is set in.
2. **Two identifiers that are equal in the common case are not the same identifier.**
   `work_directory_abs_path == original_repo_abs_path` for every repository with no linked worktrees,
   which is most of them, which is why `path` read as stable for as long as it did.
3. **Ask what a field is, not what it is used for.** `RepoData::path` was documented as "Stable
   across sessions, unlike `id`. What the expanded set is keyed by on disk." Both sentences were
   true. Neither was the question.
4. **Changing a key is a migration whether or not you write one.** The orphans left behind are
   real state in a real store, and "nothing reads them" is not the same as "they are gone".
5. **The reader found this by opening the app.** Same as the last two defects in this batch. No
   amount of suite-green substitutes for one switch between two checkouts.
