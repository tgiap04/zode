# A mark that spans the tab cannot come from the item

**Date**: 2026-09-14
**Severity**: Low — a feature landed clean; the value here is two traps it walked around and one subagent claim that was false
**Component**: `workspace` (`item.rs`, `pane.rs`), `agent_ui` (`agent_view.rs`)
**Status**: Resolved (commits `567efc1`, `e50bb42` on `feat.drag-to-reorder-stacked-dock-panels`)

## What Happened

An agent tab was asked to show that its agent is answering. The signal already
existed and the visual vocabulary already existed, so the work looked like
twenty lines. It was not, and neither of the two reasons was visible from the
commission.

## The Brutal Truth

**The layout constraint is the whole design.** The intended mark was a bar
spanning the tab's bottom edge. The obvious home for it is the item — the item
already hands the pane a `tab_content` element, so let it hand over a bar too.
That cannot work, and the reason is not obvious from reading GPUI's styling API:
**an absolutely positioned element resolves against its direct parent, not
against the nearest positioned ancestor.** GPUI lays out through taffy, and
taffy gives every node its own absolute children. CSS intuition says the bar
would find `Tab`'s `.relative()` row two levels up; taffy says it finds
whatever `div` it was handed to.

`Tab::render` (`crates/ui/src/components/tab.rs:111`) puts its children inside
one row that does span the tab — `h_flex().relative().h(Tab::content_height(cx))`
at `tab.rs:168-176`. But an item's `tab_content` becomes _one child inside that
row_, so anything it positions absolutely spans only itself. The existing rename
overlay in `agent_view.rs:1678` is the proof the codebase already had: it sets
`.relative()` on its own `div` precisely because it cannot reach further out.

So the bar had to be drawn by the pane, which meant a new `Item::is_busy`
(`crates/workspace/src/item.rs:208`, declared on `ItemHandle` at `item.rs:485`)
and `render_tab_busy_bar` (`crates/workspace/src/pane.rs:4985`) attached to the
`Tab` itself at `pane.rs:3084`. A bool crossing the trait boundary instead of an
element — not the shape anyone would pick first, and the only one that works.

The second half of that trap is quieter: the natural way to build something
spanning its parent is `w_full` in flow, which widens every busy tab and shoves
the strip sideways the moment an agent starts answering. The test measures the
tab in both states; neutralising `.absolute()` failed it at **45.5px × 28px vs
42px × 28px**. A 3.5px defect nobody would have reported and everybody would
have felt.

**`should_serialize` was a live wire, and its own doc comment hides that.**
`AgentView::should_serialize` answered `true` for every event
(`git show b16c664:crates/agent_ui/src/agent_view.rs`, line 1729). An agent
pauses to read a file or wait on a model several times inside a single answer,
so routing the new activity signal through the existing `UpdateTab` would have
put a database write behind every one of those, for a row that comes back byte
for byte the same. Hence a separate `AgentViewEvent::Activity` that maps to the
same outward `ItemEvent` but is refused by `should_serialize`
(`agent_view.rs:1853`).

The trap is the comment above it. `agent_view.rs:1844` reads "The previous life
of this impl answered `false` here" — describing an era _before_ `b16c664`, not
`b16c664` itself. A `project-manager` subagent asked to verify the plan record
read that line, concluded the shipped code returned `false`, and rewrote an
accurate sentence in the plan into a false one. One `git show` disproved it. The
comment is accurate and still misleading, which is the worst combination a
comment can have; the plan record now names the trap explicitly.

## Technical Details

### What the design costs

- `responding: bool` (`agent_view.rs`) is **sampled**, not computed at draw time.
  The marks animate, `AnimationElement::request_layout` calls
  `window.request_animation_frame()`, and that notifies the _current view_ —
  which, for a tab, is the `Pane`. So a draw-time `is_responding` would walk the
  terminal's write history 60 times a second to answer a question that changes 4
  times a second.
- The field is also the only way the rules are testable. Making `is_responding`
  true needs a real pty; `record_pty_output` is private to `terminal`, reachable
  only from `process_event(AlacTermEvent::Wakeup)`. A settable field turns two
  render rules into assertions.
- `track_responding` (`agent_view.rs:676`) polls at 250ms, matching the branch
  panel's `ACTIVITY_TICK` (`crates/git_ui/src/branch_panel/lifecycle.rs:27`)
  exactly, so the same fact does not surface in two places a quarter-second
  apart.

### Each rule proved by neutralising it

| Neutralised                      | Test failed with                                                                  |
| -------------------------------- | --------------------------------------------------------------------------------- |
| `.absolute()` on the bar         | `45.5px × 28px` vs `42px × 28px`                                                  |
| `tab_icon` always `Some`         | "the spinner takes this slot, and two agent marks on one tab reads as two agents" |
| `should_serialize` always `true` | "nothing an activity tick reports appears in the persisted row"                   |

### Three subagents, three accuracy outcomes

- `reviewer` — substantively right, and caught a real Medium (a `tab_content`
  doc comment that still claimed "The label only — no icon" while the same
  function now draws a spinner). Two of its line citations had drifted ~10 lines
  from the file it was describing.
- `doc-writer` — accurate, returned "no changes needed", and flagged an
  unrelated pre-existing error it correctly refused to fix under this scope
  (`docs/system/architecture.md` called `agent_ui` "2,270-line" when
  `agent_view.rs` alone was 4,287 at `b16c664`). Corrected in `e50bb42`.
- `project-manager` — verified seven claims correctly and then replaced one true
  sentence with a false one, from a misread comment.

Same instruction to all three: verify every citation before writing it down. The
pattern from `2026-09-07-false-subagent-premises-caught-by-reading-code.md` holds
— a subagent's report is a lead, and the only thing that settles it is reading
the code.

## Lesson

**Ask where an element's parent is before deciding who draws it.** In GPUI,
"absolute" means absolute to the `div` it was handed to. A mark that must span a
container cannot be produced by something the container merely places — that is
a trait-boundary question, not a styling question, and finding out late means
redesigning the seam rather than moving a `.child()`.

**A comment describing history has to say which history.** "The previous life of
this impl" is unambiguous to whoever wrote it and unreadable to everyone after,
because "previous" is relative to a commit the reader is not standing on. Name
the commit, or name the behaviour at `HEAD` first.
