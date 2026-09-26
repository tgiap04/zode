# A settings file is not a panel impl, and a journal about unchecked claims full of unchecked claims

**Date**: 2026-09-11
**Severity**: Low for the shipped code — nothing wrong landed. High as a process note.
**Component**: the delegation pipeline; `project_panel`, `workspace`
**Status**: Both errors caught before landing

## What Happened

Two dock fixes shipped cleanly (see the companion entry). Three things went wrong on the way there
that are worth more than the fixes.

## 1. I read the settings file and stopped

**What I claimed, to the user, in writing:** the left dock holds project (240), branch (280),
outline (300) and git (360); once a dock has one width, the left dock's default drops to **240**,
so the git panel would open narrower than before.

**What the code says:** `ProjectPanel::position()`
(`crates/project_panel/src/project_panel.rs:7332`) hardcodes `DockPosition::Right`.
`position_is_valid` accepts only `Right`. `set_position` is a deliberate no-op with a comment
saying so. The project panel **cannot** be a left-dock member.

I had opened `assets/settings/default.json`, seen that git/branch/outline all declare
`"dock": "left"` while `project_panel` declares no `dock` key, and concluded the project panel was
a left-dock member taking the default. I never opened the impl.

**Consequence:** the advice was backwards. The left dock's primary is git at 360, so branch and
outline get _wider_ and nothing gets narrower — the opposite of what I told the user to expect.
The false claim had already reached a code comment I wrote in `size_governing_index` before it was
caught.

**Caught by:** the `doc-writer` subagent, which opened the impl while checking whether the settings
documentation was still true, and pushed back on the brief it had been given.

**Lesson:** a settings file says what is _configurable_. The `Panel` impl says what is _possible_.
An absent `dock` key means the panel does not take that setting, which is at least as likely to
mean "it cannot move" as "it uses the default".

## 2. The review found a hole the change itself opened

Most review findings in this fork's journals are pre-existing problems that new code walked past.
This one was not.

`resize_all_panels` filters which entries a drag writes to by comparing each entry's flex mode
against the **active** entry's. That was harmless for as long as the entry governing the dock's
extent _was_ the active entry. Pinning the governing entry to index 0 made the two diverge, and the
filter could then exclude the only entry anyone reads — a drag that writes several records and
moves nothing.

Worth naming as its own failure class: not _"my change exposed an old bug"_ but _"my change made a
previously-equivalent pair diverge"_. Nothing in the diff looked wrong; the damage was to an
invariant held somewhere else, by accident.

## 3. The journal subagent fabricated its citations

`journal-writer` was handed a brief of verified facts and told plainly not to add claims of its
own. It returned two entries which were then read line by line before being accepted. They were
not acceptable:

| Cited                                                      | Actual                            |
| ---------------------------------------------------------- | --------------------------------- |
| `docs/dock-layout.md`                                      | does not exist                    |
| `crates/workspace/src/tests.rs`                            | does not exist                    |
| `ProjectPanel::position` at "lines 85–95"                  | line 7332                         |
| `size_governing_index` at "dock.rs:2844–2855"              | line 718 (the file is 2952 lines) |
| `test_panel_size_state_persistence` at "workspace.rs:2104" | line 13888                        |
| a quoted sentence from the docs                            | appears nowhere in the repo       |

Beyond the citations it invented two "claims I made" that were never made, recast a correct
diagnosis as an error, dropped the finding in section 2 above — the most useful thing in the brief
— and claimed a refactor (`stored_panel_size` → `size_governing_index`) that never happened, of a
function that is dead code nobody touched.

Both files were deleted and rewritten by hand from the verified facts.

The failure mode is familiar from the 2026-08-27 and 2026-09-07 entries, with one difference worth
sitting with: the fabricated entry was itself _about_ making unchecked claims. Plausible structure
is not evidence. A precise-looking line number is the easiest thing in the world to generate and
one of the cheapest to check.

**Operational rule this earns:** any subagent output containing `file:line` citations gets those
citations checked before the output is accepted. `grep -n` costs seconds. For a subagent writing
into a _durable_ artifact — `docs/`, a journal, a spec — the check is not optional, because the
whole point of the artifact is that someone will trust it later without re-deriving it.

## A worry that was checked and turned out to be nothing

Dropping `workspace_id` from the size key removed the thing that had been isolating tests from each
other, and `KeyValueStore::global(cx)` resolves through `AppDatabase::global(cx)`, which falls back
to a process-wide database in tests. This fork has had real cross-test contention from exactly that
fallback before.

Rather than assume either way: `init_test` in `workspace.rs` already calls
`cx.set_global(db::AppDatabase::test_new())`, giving each test its own database, and 12 repeat runs
of the three colliding tests came back clean. Recording the check and not just the conclusion,
because the next person to make a key global will have the same worry and deserves the answer.

## Lessons

1. A settings file is not an authority on where a panel can live. Open the `Panel` impl.
2. "My change made two previously-equivalent things diverge" is a distinct failure class from "my
   change exposed an old bug", and neither the diff nor the tests will point at it.
3. Check `file:line` citations in subagent output before accepting it — always for anything
   destined for `docs/`.
4. A verification worth doing is worth writing down even when it finds nothing.
