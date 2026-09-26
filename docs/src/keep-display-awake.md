# Keeping the Display Awake

While a terminal is actively producing output, Zode asks the operating system
to keep the display lit. This covers agent CLIs, task terminals run from the
terminal dock, and ordinary shell terminals alike — a long refactor, a test
run or a fan-out of subagents can go many minutes without printing anything,
and a machine that dims and locks in the middle of one is a machine you have
to come back and wake up.

This is on by default:

```json
{
  "keep_display_awake": true
}
```

## What holds it

The rule is a **rate**, not liveness: a terminal that is producing output
fast enough holds the display; one that has gone quiet does not, whether or
not its process is still running.

- Output arriving faster than roughly eight writes a second holds the
  display. That rate has to keep up — the hold lets go about a minute after
  the last such burst, so a build that pauses between steps keeps the display
  lit, while one that has finished does not.
- A dev server that has finished booting and now sits idle — or a `tail -f`
  with nothing new to print — **will** let the display sleep once that minute
  passes. That is deliberate, not a bug.
- A slow repaint — `top`, `htop`, a prompt clock, a `tmux` status line —
  redraws once or twice a second, under the threshold, so it does not hold
  the display either.
- A shell sitting at its prompt writes nothing and holds nothing.
- Typing at a prompt can cross the threshold, but it changes nothing in
  practice: the operating system already resets its own idle timer on
  keystrokes.
- An idle editor with terminals open costs nothing to watch: the poll behind
  this does not start until a terminal writes something, and stops itself
  again once every terminal has been quiet for a minute.

Subagents need no special handling in an agent tab: a subagent runs inside
the agent's own CLI process, so its output counts as that tab's output.

## The status bar switch

There is a bolt icon on the status bar. It is lit while the display is being
held and dimmed when it is not, and clicking it opens a menu with the switch and
one line saying what is actually happening:

| The menu says                   | Meaning                                                      |
| ------------------------------- | ------------------------------------------------------------ |
| The display is being held awake | Something is producing output and the hold is in place       |
| Nothing is producing output     | Nothing to hold it for                                       |
| Paused - running on battery     | Something is producing output, but the machine is on battery |
| The system refused the request  | Everything says yes and the OS still said no                 |

The line exists because three of those four states are the same dimmed icon.
The tooltip names the tab responsible when there is one — an agent's label or
a terminal's title.

The status behind that line actually has five values, not four: the fifth is
"turned off," and it has no row in the table above because switching the
feature off does not dim the icon, it removes it entirely — see [What lets it
go](#what-lets-it-go).

## What lets it go

- Every terminal producing output goes quiet — its command finishes, or its
  output rate drops below the threshold for a minute (see [What holds
  it](#what-holds-it)) — or you close its tab.
- The machine starts running on battery. The hold returns when you plug back in;
  the power source is re-checked about once a minute, so expect up to a minute
  of lag either way.
- You set `keep_display_awake` to `false` — by hand-editing `settings.json`,
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
ends a hold except the producing terminal going quiet, the tab closing, or
the setting going off. There is no time limit.

If that trade is wrong for where you work, set `keep_display_awake` to `false`
and lock the machine yourself, or lock it manually before you walk away —
a hold prevents the _idle_ lock, not a lock you ask for.

## Platform support

Two separate things are needed, and they were built separately: asking the OS to
hold the display, and noticing the machine is on battery.

| Platform | Holds the display                                                                                           | Detects battery |
| -------- | ----------------------------------------------------------------------------------------------------------- | --------------- |
| macOS    | Yes — an IOKit assertion, visible in `pmset -g assertions` and named after the tab producing output         | Yes             |
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
