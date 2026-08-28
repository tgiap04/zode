# Keeping the Display Awake

While an agent's CLI is working, Zode asks the operating system to keep the
display lit. A long refactor, a test run or a fan-out of subagents can go many
minutes without printing anything, and a machine that dims and locks in the
middle of one is a machine you have to come back and wake up.

This is on by default:

```json
{
  "keep_display_awake": true
}
```

## What holds it

An agent tab whose CLI is **still running**. Not an agent tab that is merely
open — a finished session keeps its tab so you can read the transcript, and that
tab holds nothing.

Subagents need no special handling. A subagent runs inside the agent's own
process, so while it works the CLI has not exited and the hold stands.

## The status bar switch

There is a bolt icon on the status bar. It is lit while the display is being
held and dimmed when it is not, and clicking it opens a menu with the switch and
one line saying what is actually happening:

| The menu says                   | Meaning                                            |
| ------------------------------- | -------------------------------------------------- |
| The display is being held awake | An agent is working and the hold is in place       |
| No agent is working             | Nothing to hold it for                             |
| Paused - running on battery     | An agent is working, but the machine is on battery |
| The system refused the request  | Everything says yes and the OS still said no       |

The line exists because three of those four states are the same dimmed icon. The
tooltip names the agent responsible when there is one.

There is no "turned off" row in that table: switching the feature off does not
dim this icon, it removes it — see [What lets it go](#what-lets-it-go).

## What lets it go

- Every working agent finishes, or you close its tab.
- The machine starts running on battery. The hold returns when you plug back in;
  the power source is re-checked about once a minute, so expect up to a minute
  of lag either way.
- You set `keep_display_awake` to `false` — by hand, from the Settings Editor,
  or by right-clicking an empty part of the status bar and switching off
  **Keep Display Awake** in the menu that opens (see [Status Bar
  settings](./visual-customization.md#status-bar)). This takes effect
  immediately, including on a hold already in place — no restart. It also
  removes the bolt icon from the bar itself, rather than leaving it dimmed:
  hiding a status-bar item this way drops the item's entity, and the icon is
  that entity.

## What this costs you

**Your screen will not lock on idle while a hold is in place.** That is the
feature working as asked, and it is also the risk: an editor left in a shared
office or a café stays readable to whoever walks past. The battery guard covers
a laptop on the move, but a desktop has no battery, so on a desktop nothing
ends a hold except the agent finishing, the tab closing, or the setting going
off. There is no time limit.

If that trade is wrong for where you work, set `keep_display_awake` to `false`
and lock the machine yourself, or lock it manually before you walk away —
a hold prevents the _idle_ lock, not a lock you ask for.

## Platform support

Two separate things are needed, and they were built separately: asking the OS to
hold the display, and noticing the machine is on battery.

| Platform | Holds the display                                                                                           | Detects battery |
| -------- | ----------------------------------------------------------------------------------------------------------- | --------------- |
| macOS    | Yes — an IOKit assertion, visible in `pmset -g assertions` and named after the agent                        | Yes             |
| Windows  | Yes — `SetThreadExecutionState` with `ES_DISPLAY_REQUIRED`                                                  | Yes             |
| Linux    | Yes — an `org.freedesktop.ScreenSaver` inhibit on the session bus, the same call under both X11 and Wayland | Yes             |

All three are new, and none has been exercised end to end in a running editor
yet. The evidence behind them is not equal, so it is worth stating plainly: on
macOS the exact assertion call was verified out of band and shows up in
`pmset -g assertions`; on Windows and Linux the code has been compiled against
the real APIs for those targets and nothing more. If the display does not stay
awake for you, or the switch claims a hold the screen does not honor, that is
exactly the kind of thing worth filing an issue about.

A platform that cannot make the request at all gets none of this built: no
icon, no switch, no background work — a dimmed control opening a switch that
can never do anything would be worse than nothing. That is a real code path
(it is what a platform with no implementation gets by default), but none of
the three platforms above take it any more: each one now answers yes, so on
every supported platform the icon is always there.

### What each platform cannot tell you

- **Windows** has nowhere to record _why_ the display is held —
  `SetThreadExecutionState` takes flags and nothing else. The status-bar switch
  is the only place to find out.
- **Linux** goes through the `org.freedesktop.ScreenSaver` session-bus
  convention, so it behaves the same under X11 and Wayland — but only where
  something answers that name. GNOME and KDE do. **wlroots-based compositors —
  sway, Hyprland, river — do not by default**: they implement idle inhibition
  through a Wayland protocol instead, which this does not use yet, so the switch
  there will read "The system refused the request". Two further cases: some
  environments answer the call and then do not actually hold the screen, which
  cannot be detected from this side, so the switch may claim a hold while the
  screen sleeps anyway; and with no session bus at all — a container, or over
  SSH — the request simply fails and the menu says so.
