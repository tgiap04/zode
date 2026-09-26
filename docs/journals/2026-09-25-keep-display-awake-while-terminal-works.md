# Keep the display awake while a terminal is working, not just when an agent answers

**Date**: 2026-09-25 14:03
**Severity**: Medium — a feature request that half-existed already and exposed three bugs on the way to completion
**Component**: `keep_awake`, `gpui`, `agent_ui`, `terminal_view`, `workspace`
**Status**: Resolved (commits `308da0e`, `76ec400`, `3ca352a`)

## What Happened

The request came in two parts: keep the display awake while a shell command or long-running process is working in a terminal, and with several projects open, one project with an active agent should hold the screen on even when that window is unfocused.

Reading the code revealed that the second part was already true — the activity poll runs on the app foreground executor with no window-active check anywhere, and an unfocused window's agent kept its hold all along. The word "monitor" meant a long-running terminal process, not a technical term in this codebase. The real gap was that `keep_awake` had never seen a terminal; it watched agent tabs only.

## The Brutal Truth

The frustrating part is that **half the answer was already shipping**. An unfocused window held the display fine — there was simply nothing in the code checking window focus at all, and the poll is spawned on the app's foreground executor rather than a window's. Half the feature was already done before any code was written. That left only the terminal half, which was the half someone had actually asked for and the half the system did not do.

The galling part came after: three separate bugs surfaced on the way through. Only one of them — the poll that could never restart — would have shown up by using the feature on this machine; the assertion-per-workspace bug is invisible outside Windows, and the duplicated doc was never going to surface by running anything. A test that looked green 15 times and then failed 2 of 10 runs meant the fix was right but the test's premise was not. Splitting it cost clarity but bought correctness — and the second pass went red the moment the refcount was reverted, which is what a test is supposed to do.

## Technical Details

### The rule was already known: output-based, not liveness-based

Agents already obeyed this rule (in `agent_ui::AgentView::is_answering`). A terminal was extended to the same rule: a command that is printing holds the display; an idle `npm run dev` does not; `top` repainting at one or two hertz does not. This rule had a consequence: a foreground-process check to tell "a shell at its prompt" from "a shell running something" turned out to be unnecessary, because a shell at its prompt writes nothing and the rate rule already declines for it.

The implementation reused the existing rate threshold (`RESPONDING_WRITES` = 8 writes per second) from `agent_ui` by making it public (`pub const`), keeping the two rules synchronized in one place rather than duplicated.

### Every workspace took its own OS wake assertion

`App::keep_display_awake` had no registry and no refcount. With one caller per workspace, a window holding several retained project groups already took several OS assertions. Harmless stacking on macOS and Linux. On Windows a real bug: `SetThreadExecutionState` overwrites rather than nests, and each handle restores whatever it replaced, so the first release puts back "no hold" and silently kills a sibling's hold that still reports itself held. Fixed in commit `308da0e` by moving the count to `App` behind a weak handle, so one assertion goes out to the platform and the last handle to drop takes it back.

### The activity poll could never restart

When the poll's own loop decided to stop (no live tabs left), it never cleared its task handle. Later, when a terminal went quiet and then spoke again, `settled` checked if `activity_check` was `None`. Since the old handle was still there, the check returned false and no replacement was spawned. The terminal was watched once and then abandoned. Fixed by explicitly clearing the handle inside the loop's exit condition.

### Dock terminals were invisible to the event

`workspace::Event::ItemAdded` is emitted from exactly one place, which never sees the terminal panel's own panes, so dock terminals were never registered as watched. Widening `ItemAdded` was rejected — four unrelated crates subscribe, and one of them has a removal sweep that dock churn would start firing. A new `TerminalPanelEvent::TerminalsChanged`, with no existing subscribers, was the smaller change. It carries no payload and fires on both add and remove, because its one handler re-enumerates the panel's panes rather than reading an item out of the event.

## Lessons Learned

**A request names a symptom, not a cause.** "Keep the display awake while a terminal is working" meant "I am typing in a terminal and the screen goes to sleep." Reading the code first — checking whether window focus mattered, whether a running process was already being watched — turned half the request into a test that pinned an existing behavior.

**The expensive half of a scope can collapse into the cheap half with the right rule.** A foreground-process check sounded necessary until the rate rule was written down: a shell at its prompt prints nothing, so the rate rule already declines for it without being told what is running. The rule is not "something is running" — a silent build or an idle server is running and deliberately does not hold — which is precisely why no second source of truth was needed.

**A test that passes 15 times and fails on the 16th is not "flaky" — it is wrong.** A real shell's pty write arrived late and was stamped against a test clock the scheduler had already advanced, because virtual time folded real seconds spent parked. The fix that worked was to split the test: lock arithmetic proved with deterministic in-process state (rate calculation and assertions), and the real shell kept only for the wiring it alone can prove — that a terminal in the centre pane and one in the dock both reach the holder. Both tests were confirmed red with the refcount reverted, confirming the defect existed.

**Plan references in shipped comments resolve to nothing.** Review found two in this change's own test comments and they were rewritten to describe the code instead. The same sweep turned up nine more across other crates from earlier sessions; those were left alone to keep this change scoped, and they remain debt. `plans/` is gitignored, so any such reference is unreadable to whoever reads the comment next.

## What review caught

One reviewer flagged that a borrow is held across a platform call inside `App::keep_display_awake`, and releasing it first would let a re-entrant caller see a zero count and take a second assertion, reintroducing the exact bug just removed. Holding it turns that into a panic instead, which is louder and safer. Deliberately not fixed — it is a protection, not a flaw.

Another reviewer noted that it could not rule out a lock leak on the agent path. That rule is unchanged by this work, so any such hazard predates it.

The same setting is described twice in the docs — `docs/src/keep-display-awake.md` and `docs/src/reference/all-settings.md` — and only the first was being kept current. Both were corrected here, but they are still two hand-written descriptions with nothing keeping them in step, so the next change to this behaviour has to touch both.

## Gates

`cargo test -p keep_awake` 37, `-p gpui` 134, `-p agent_ui` 130, `-p terminal_view` 59, `-p terminal` 73 — all exit 0, re-run after the commits landed. `keep_awake` measured 10 of 10 green over consecutive full-suite runs. `cargo fmt --all -- --check` exit 0. `./script/clippy` over the five crates exit 0.

## Still unverified by machine

Windows behaviour: cross-compilation is blocked on this machine, so the fix deliberately changed no Windows logic — only the comment stating the invariant now enforced upstream. CI's Windows job is the only compile proof. Also by hand: a command in the dock holding the display, an idle server not holding it, and `pmset -g assertions` showing one entry with two projects open.
