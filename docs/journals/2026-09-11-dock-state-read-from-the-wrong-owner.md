# Dock state read from the wrong owner, twice

**Date**: 2026-09-11
**Severity**: Medium — two reported bugs, both fixed; one latent issue recorded, not fixed
**Component**: `workspace` (`dock.rs`, `workspace.rs`), `git_ui` (`worktree_service.rs`)
**Status**: Resolved (commits `a3736de`, `526f002`, branch `fix.remove-last-project-leaves-it-on-rail`, PR #35)

## What Happened

Two separate reports, a few hours apart, turned out to be the same shape of mistake: a dock's
state was being read from a panel that did not own it.

- A worktree you had never opened came up with its docks shut.
- The left sidebar changed width whenever you switched panels on the rail, or switched project.

Neither was a subtle bug in new code. In both cases the wrong owner had been chosen deliberately,
and in the second case the code comment explaining the choice was still sitting there, describing
the symptom and then drawing the wrong conclusion from it.

## Bug A — a worktree opened with its docks shut

Two gaps stacked.

**The switch path carried nothing.** `open_worktree_workspace`
(`crates/git_ui/src/worktree_service.rs:774`) built its `init` closure only when
`is_creating_new_worktree`; switching passed `None`. So creating a worktree carried the layout and
switching to one did not.

**The create path carried one panel out of however many were up.**
`DockStructure`/`DockData` records a single `active_panel` per dock. This fork's left dock _stacks_
— several panels visible at once — so "the git panel was open beside the worktree panel" could not
survive the trip.

Which docks stack is worth writing down, because it is not obvious from the names.
`takes_turns()` is `dock_header_draws_panel(position)` (`crates/workspace/src/dock.rs:2085`), which
is `matches!(position, Left | Right) && position != Workspace::OWN_COLUMN_POSITION`, and
`OWN_COLUMN_POSITION` is `Left` (`crates/workspace/src/workspace.rs:7782`). So **Left and Bottom
stack; only Right takes turns.**

### The gate that cost nothing

The wanted behaviour was: inherit the layout only when the destination checkout has none of its
own, so a dock deliberately shut over there stays shut. No condition was written for this.
`Workspace::load_workspace` already runs after `init` and overwrites both records, and it is only
reached when a `SerializedWorkspace` exists.

The ordering is structural rather than lucky: `init` runs synchronously inside `Workspace::new`'s
`cx.new(...)` closure, and `open_items` comes later in the same async fn with no await between
them. Worth stating plainly, because a free gate that depends on an accident is a trap, and this
one does not.

### Shape of the fix

`DockStacks` carried alongside `DockStructure` in `PreviousWorkspaceState`; `set_dock_structure`
renamed and widened to `set_dock_layout(docks, stacks, ..)`
(`crates/workspace/src/workspace.rs:2313`), now the single way in for **both** the stored layout
`load_workspace` reads back and the inherited one. Both records go on before `restore_state` runs,
because the restore reads both.

Guard: `a_captured_layout_carries_every_panel_that_was_up` — fails `left: 1, right: 2` with the
stack assignment removed, which is precisely "only one of my two panels came back".

## Bug B — the sidebar would not hold a width

Three independent causes, all live at once:

1. Each panel carries its own `default_width`: git 360, outline 300, branch 280.
2. The stored size was keyed by panel.
3. The same key also carried `workspace_id`, so every project and worktree remembered its own.

Underneath all three: `Dock::size_governing_index()` (`crates/workspace/src/dock.rs:718`) returned
`active_panel_index` for stacking docks and `0` for takes-turns docks.

**The comment already on that function described the symptom** — a column changing width every
time you switched, because a drag only ever wrote to the panel showing at the time. It then
excluded stacking docks with this reasoning: _"A dock that stacks has no switcher and each section
keeps its own size."_

That is the wrong axis. A stack divides the column along the **other** one — `stack_flexes` says
how the sections share its height — so the sections were never describing their own width to begin
with. Reading a width off the active section was reading a field that meant nothing there.

So the fix **deleted** a branch rather than adding one. `size_governing_index()` now always returns
`0`, and Left and Bottom behave as Right already did. The key dropped `workspace_id`; a migration
fallback reads the old `{workspace_id}:{panel_key}` record once and writes it forward.

Consequence worth knowing at the time of this entry: the left dock's primary panel was **git**
(`activation_priority` 3, lowest among git 3 / branch 4 / outline 6), so the shared width was
360 — branch and outline got wider and nothing got narrower. (As of 2026-09-12, git and branch
swapped `activation_priority` values — branch is now 3 and git is 4 — so branch is the left
dock's primary panel and git and outline follow it instead. Branch's `default_width` was raised
280 → 360 in the same change, precisely so the column it now governs keeps the width it had:
the swap was asked for as an icon reorder, and a narrower column would have been an uninvited
second effect.) The bottom dock's primary is the terminal (2, against the debugger's 7).

### A knock-on the change forced into the open

Once records went global, `test_panel_size_state_persistence`
(`crates/workspace/src/workspace.rs:13888`) started failing: a fixed 350px write leaked into a
flexible panel's record under the same key. `resize_panel_entry`
(`crates/workspace/src/dock.rs:507`) now clears the axis it is _not_ writing. The test had
asserted that invariant all along — it had only ever held by accident, because each workspace got a
fresh record.

### A hole the change itself opened

Found in review, not by any test. `resize_all_panels` (`crates/workspace/src/dock.rs:1537`) is the
left dock's **default** resize path (`resize_all_panels_in_dock` defaults to `["left"]`). It filters
which entries get written by comparing each entry's `panel_uses_flexible_width` against the
_active_ entry's.

Before the change, governing == active for the left dock, so the entry written was always the entry
read. After it, the governing entry could be filtered out — the drag writes to entries nothing reads
and the column does not move at all. Reproduced: the dock sits at its 300px default while the drag
asks for 420px. Fixed by exempting the governing entry from the mode filter, which is safe because
`resize_panel_entry` picks fixed-or-flexible from the entry's own mode.

## Latent, recorded, not fixed

These have no home outside this file — `plans/` is gitignored.

**1. `starts_open` can collapse a restored stack.** `Dock::add_panel` calls `restore_state()` and
then honours `starts_open` unconditionally (`crates/workspace/src/dock.rs:960`), and
`activate_panel` is exclusive — it sets `entry.visible = ix == panel_ix` for _every_ entry
(`crates/workspace/src/dock.rs:1048`). So a panel with `starts_open: true` added **last** collapses
a restored stack to itself.

Only `project_panel` defaults to `starts_open: true`. It does not bite today because it is always
added first, and that is not a coincidence: `ProjectPanel::load`
(`crates/project_panel/src/project_panel.rs:1026`) contains no `await` at all, while
`BranchPanel::load` (`crates/git_ui/src/branch_panel/lifecycle.rs:78`) awaits a database read — so
in the `futures::join!` inside `initialize_panels` (`crates/zed/src/zed.rs:593`) the project panel
completes on the first poll. **That ordering is load-bearing, implicit, and untested.** Add an
`await` to `ProjectPanel::load`, or give a second left-dock panel `starts_open: true`, and restored
stacks start collapsing.

The fix, when someone takes it: gate the force-open on `self.serialized_dock.is_none()` — "starts
open" should mean "when nothing else says otherwise". One line, but it changes restore behaviour for
every workspace, so it wants its own change and its own decision.

**2. The size record is not scoped by dock position.** `panel_uses_flexible_width` gates on
`Axis::Horizontal`, and both the terminal and debug panels accept every position, so a panel moved
between the bottom dock and a side reads back a value meant for the other axis. Pre-existing; the
axis-clearing above narrows it without closing it. Real fix: put the position in the key.

**3. `render_dock` applies `min_w` and never `min_h`.** Dormant — no panel in the tree overrides
`Panel::min_size`. It matters more now that "the governing entry is not the visible entry" is an
ordinary state for the bottom dock.

**4. `Dock::stored_panel_size`** (the un-suffixed reader) has zero callers. Dead.

## Verification

780 tests green — workspace 268, git_ui 174, project_panel 100, agent_ui 79, terminal_view 58,
debugger_ui 55, outline_panel 12, sidebar 34. `cargo check --workspace --all-targets` and
`./script/clippy` both exit 0.

Each fix was proved by neutralising it in place and watching the matching test fail with the number
predicted beforehand: `1 vs 2` and `0 vs 2` for the carried stack, `360 vs 240` for the panel
switch, `300 vs 420` for the project switch, `300 vs 420` for the swallowed drag.

## Lesson

Both bugs were a field being read from the wrong owner, and in both cases the code said so. Bug A's
`init` branch and Bug B's `takes_turns()` branch were each written deliberately, with a comment. The
comment on `size_governing_index` had the symptom exactly right and the conclusion exactly wrong —
it reasoned about "size" without asking _which axis_, in a dock that divides along one and shares
the other.

When a comment justifies excluding a case, the exclusion is the part to check.
