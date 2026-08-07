# Business Context — F011_GitIntegration

## Why It Matters

Version control is how a developer's daily work gets checkpointed, shared, and undone safely.
Bringing git operations directly into the editor — instead of forcing a switch to a separate
terminal or git client — lets a developer stage, review, and commit changes without losing their
place in the code they're editing.

## Who Uses It

- **Developer working solo** — stages and commits their own changes, switches branches to work on
  different tasks, and discards experimental edits that didn't pan out.
- **Developer reviewing their own work-in-progress** — opens a combined view of every changed file
  before committing, to check the whole set of edits at once rather than file by file.
- **Developer investigating project history** — opens a visual commit graph to understand how
  branches were merged and when changes landed.

## What They Do

1. Developer edits files, then opens the git panel to see what changed.
2. Developer selects which specific changes to include — a whole file, or just part of it — and
   stages them, keeping the rest aside for a later, separate commit.
3. If the developer needs to switch context without losing in-progress work, they stash the
   uncommitted changes, which sets the project aside cleanly until they come back to it.
4. If an edit turns out to be unwanted, the developer discards it, returning that file to its
   last-saved state (or removing it entirely if it was never saved before).
5. Developer writes a commit message and confirms — the staged changes become a permanent
   checkpoint in the project's history.
6. Before committing, the developer can optionally review every changed file together in one
   place, or open the branch switcher to move to different line of work, or view the project's
   commit graph to see how past work has come together.

## Unresolved Questions

- **Stash conflict messaging**: it's unclear from the code whether restoring a stash that
  conflicts with the current work shows the developer a distinct, actionable message versus a
  generic error — worth confirming with whoever owns the git panel UX.
- **Push/pull scope**: this feature slice covers local operations (stage, commit, branch, stash,
  discard, diff review, history graph) but not pushing/pulling to a remote — confirm with
  stakeholders whether that belongs here or in a separate feature.
