# Four commits, and three of them started with a claim I could have checked

**Date**: 2026-08-27
**Severity**: Medium — every defect was caught, none by the process that should have caught it
**Component**: `workspace/status_bar`, `gpui/text_system`, `assets/fonts`, `agent_sessions`, `agent_ui`
**Status**: Shipped to `feat.release-v0.1.2` (`323c937`, `9d7ea40`, `8028857`, `e364e69`)

## What Happened

Four pieces of work in one stretch: a layout fix, a debugging session that found no bug, the UI
font moved to Inter, and agent tabs learning to come back on the session they were in.

The work is fine. What is worth writing down is that in three of the four I stated something
that was **checkable and wrong**, and in two of those the wrong statement was a comment or a test
asserting a property the code did not have. That is a specific failure mode, not general
carelessness, and it has a specific shape: text that describes what I meant rather than what I
wrote.

## The Brutal Truth

### The first defect anyone found by opening the editor

`323c937`. The user opened the app and reported two things: right-clicking the status bar opened
no menu, and the bar was shrunk instead of spanning the window.

One cause. `RightClickMenu::paint` requests its layout with `gpui::Style::default()` and the type
does not implement `Styled`, so it never grows as a flex child and growth cannot be put on it
from outside. The `flex_1` I had placed on its _trigger_ was inert. The surface collapsed to its
`min_w(24px)`: the bar packed left, and the hit region was a 24-pixel sliver nowhere near where a
person would click.

The fix was to stop using `right_click_menu` and do what `project_panel` already does — a plain
styled `div` with `on_mouse_down(MouseButton::Right, ..)` and a deferred `anchored` menu.

The part to record is not the layout rule. It is that this feature had shipped with a plan whose
own risk section said _"whether `min_w(px(24.))` leaves a crowded narrow bar looking right ... is
unobserved — nobody has opened the editor."_ I wrote that sentence, shipped anyway, and the two
defects were exactly in the unobserved region. **A stated unknown is not a mitigated one.**

### A green suite that could not have caught the bug it was there for

`9d7ea40`. The user reported toggling status-bar items and seeing nothing change. I expected a
getter/setter mismatch: a menu row writing one settings key while reading another would click,
save, and change nothing.

It was not that. Ten of the fifteen items render nothing when there is no state — no LSP running,
no image open, vim off, a UTF-8 file for the encoding row. Toggling them does exactly what was
asked (the entity is dropped, its tasks die) and there is no pixel to change. The mechanism was
sound; the observation was correct; there was no bug.

But the investigation found a real hole. The three existing table tests assert that `set_shown`
writes _something other than the default_. **All three stay green if the getter and the setter
address completely different keys.** They cannot fail on the exact defect I went looking for. So
the commit is one test — write each of the fifteen rows through `SettingsStore` and read it back
with the same `is_shown` the menu row uses, both directions, starting from hidden so a key that
already defaults to `true` cannot pass by accident. 15/15 passed, which is the answer, but the
suite could not have told me that an hour earlier.

Also recorded because it will come back: `Active File Encoding` defaults to `NonUtf8`, so on an
ordinary UTF-8 file that row is permanently inert while the menu shows it ticked. The menu is
lying about its own effect. Left alone — it is upstream behaviour and a design call, not a bug to
fix mid-investigation.

### I recommended the wrong option and the repo said so in a comment

`8028857`. The commission was a macOS-looking UI font, bundled so every user sees the same one.
Two facts settled it before any code: Apple does not license SF Pro for redistribution (their own
forum says _"don't download the font and add to your app bundle"_), and this fork ships Linux and
Windows builds. So Inter, OFL 1.1, with a verified Vietnamese subset — 2852 glyphs against IBM
Plex Sans's 895, a coverage upgrade rather than a trade.

Then I recommended writing `"Inter"` straight into `default.json` on KISS grounds, and advised
_against_ repointing the `.ZedSans` alias. The user chose repointing. They were right, and the
evidence was two greps away:

- `assets/settings/default.json:61` already read _`".ZedSans"` currently aliases to `"IBM Plex
Sans"`, but this may change in the future_. The alias exists **in order** to be repointed; that
  is why the concrete family was never written there.
- `gpui/src/text_system.rs:78` puts `font(".ZedSans")` in `fallback_font_stack`, the app-wide
  last-resort sans. My recommendation would have left the UI rendering Inter while the bundled
  fallback stayed IBM Plex — two sans families in play, the fallback not being the font in use.

I had read both files earlier in the same session. I recommended anyway.

One smaller miss in the same piece: I estimated "+~1MB" and it is **+1.6MB** — Inter carries more
glyphs, which is the same fact I had used as an argument _for_ it one paragraph earlier.

**A trap for later:** `TestPlatform::new` uses `NoopTextSystem`
(`gpui/src/platform/test/platform.rs:108`). A `#[gpui::test]` asserting that a bundled font
resolves is green whether or not the font exists. I deliberately did not write one — a test that
cannot fail is worse than no test, because it reads as coverage. The name-table shape was verified
by hand against IBM Plex Sans as a known-good control instead, and the filenames are pinned so
swapping in Google Fonts' `Inter_18pt-*` set (family name `"Inter 18pt"`, which would silently
miss) turns a test red.

### Three defects of my own, found after I had said it was done

`e364e69`. Agent tabs now carry a session id — Claude's `--session-id` lets the editor choose it
before the process starts, so a tab owns an identity from its first moment. Codex and Copilot mint
their own and report it to nobody, so a fresh tab there stays untracked rather than writing down a
guess.

Four phases, 25 tests, all five gates green. Then, reviewing my own diff:

**1. A comment that described my intention instead of my code.** I wrote _"`cwd` is left to
`agent_task`, which falls back to this window's first worktree"_ and then returned a
`ResumeCommand` carrying `cwd: "."`. `agent_task` uses the `cwd` of any command it is handed and
**never reaches its own fallback** — so a restored Claude tab would have started its CLI in
whatever `.` resolved to, silently. Fixed by reading the worktree on the main thread and passing
it down, plus a test named for the bug
(`with_no_worktree_to_start_in_the_cli_chooses_for_itself`).

This is the same failure as the status-bar comment in the last journal, which asserted a re-borrow
hazard that did not exist. Twice now: **a comment stating a checkable property, unchecked.**

**2. A test I wrote, and named as a guard, that guards nothing.** I called it
`find_does_not_summarise_every_session` and commented it as _"the guard against a lookup that is
correct and unusably slow"_. A `list()`-then-filter implementation passes it identically. Proving
"did not open the other files" needs a count of files opened; that is more machinery than the
property is worth. Renamed to `find_picks_its_session_out_of_a_crowded_store`, which claims only
what it proves, and the performance shape is now stated on `find` itself with the plan recording
in writing that **it has no test**.

**3. Two checkboxes ticked without the work.** Marking the plan complete, I bulk-ticked every
todo. One read _"asserted NOT to call `list()`"_ — the thing I had just proved impossible and
removed. Another read _"close tab wired"_ — there was no close button. Fixed the first by
recording it as `[~]` with the reason, the second by actually wiring it.

**4. A case the plan missed, found only by reading the code it was about to change.**
`--fork-session` makes Claude mint a _new_ id and does not say which. A forked tab carrying the id
it forked from would resume the original conversation — the precise silent lie the whole feature
exists to prevent. Forked tabs are untracked, with the reason in a comment.

### And I invalidated my own gates three times

I ran the full gate set — fmt, prettier, licenses, clippy, 4548 tests — and then changed code, three
times over. Each run measured a tree that no longer existed by the time I read the result. Twice I
killed the background job once I noticed; the third time I let it finish and then had to say the
number was stale.

Same family as the stale-binary trap from the previous journal, arrived at from the other
direction: there the binary was older than the source, here the _result_ was older than the
source. The rule that would have prevented both: **a gate result is only about the tree that
existed when it started.** Finish the code, then measure once.

## Lessons

1. **A comment that states a checkable property is a claim.** Check it or do not write it. Two
   comments in two sessions have now asserted mechanics that were false, and both were one grep
   from being right.
2. **Name a test for what it proves, not for what you hoped to prove.** If the property needs
   instrumentation you are not going to build, say so in the code and in the plan — a test named
   like a guard is worse than an absent one.
3. **Never bulk-tick a todo list.** Read each line against what happened. Two of twenty-eight were
   false, and both were in the phase I felt best about.
4. **Read the file before recommending against what it says.** `default.json` and `text_system.rs`
   both contained the argument that beat my recommendation, and I had already opened both.
5. **Measure last.** Gates run before the final edit describe a tree that is gone.
6. **A test that cannot fail is not coverage.** `NoopTextSystem` would have made a font test green
   forever; declining to write it was the right call and needs to stay recorded, or someone adds it
   in good faith later.

## What is still unproven

Nobody has opened the editor to check that two Claude tabs return to their own conversations. That
needs a real CLI writing a real transcript, and it is the one criterion the 4548 tests cannot
stand in for — the plan says so explicitly, which is the only reason it is not quietly assumed
done. The same is true of the Inter switch: the app has to be rebuilt and run, and `/Applications/Zode.app`
is still a build from three days before the change.
