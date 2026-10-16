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

Beside it sits a second, manual switch that holds the display for as long as a
Zode window is open, whatever the terminals are doing. It is off by default —
see [The always-on switch](#the-always-on-switch).

## What holds it

The automatic rule is a **rate**, not liveness: a terminal that is producing output
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

A background subagent in an agent tab also holds the display. While the agent
waits on one it only repaints a spinner, well under the threshold, so a
subagent that is still running counts as the tab answering even when the tab
is quiet. The hold ends once the subagent's task notification arrives, and
starts again if the subagent is resumed.

## The always-on switch

The automatic switch decides for you: it holds the display while something is
producing output and lets go when nothing is. If you would rather decide
yourself, a second, manual switch holds the display for as long as a Zode
window is open:

```json [settings]
{
  "keep_display_awake_always": true
}
```

While it is on, the display is held whatever the terminals are doing —
including when none is open — and whether or not the machine is on battery.
It is independent of `keep_display_awake`, which keeps its automatic meaning,
and it has no time limit. It is off by default, and it is an ordinary setting,
so a window opened with it on holds the display straight away.

Three ways to flip it, all of which end up as that one setting, so every open
window follows at once:

- Click the eye icon and choose **Always Keep Display Awake**, under **Keep
  Display Awake While a Tab Is Producing Output**. It is ticked while the
  switch is on.
- Run `keep_awake::ToggleAlwaysOn` from the command palette. It has no default
  key binding; [bind a key](./key-bindings.md#user-keymaps) of your own and it
  shows next to the menu entry.
- Edit `keep_display_awake_always` in `settings.json`.

If the operating system refuses the request, the menu line reads "The system
refused the request" and Zode asks again about once a minute until it is
granted or you turn the switch off, so a session bus that is not answering yet
at login is picked up without a restart. The warning in the log is written on
the first refusal only.

## The status bar switch

There is an eye icon on the status bar. It is lit while the display is being
held and dimmed when it is not, and clicking it opens a menu with the two
switches and one line saying what is actually happening:

| The menu says                   | Meaning                                                      |
| ------------------------------- | ------------------------------------------------------------ |
| The display is being held awake | A hold is in place, for output or for the always-on switch   |
| Nothing is producing output     | Nothing to hold it for                                       |
| Paused - running on battery     | Something is producing output, but the machine is on battery |
| The system refused the request  | Everything says yes and the OS still said no                 |

The line exists because three of those four states are the same dimmed icon.
The tooltip names the tab responsible when there is one — an agent's label or
a terminal's title. With the always-on switch on, the tooltip reads "Keeping
the display awake until turned off" instead, because closing every tab would
not end that hold. With that switch on and no hold in place, the line is always
"The system refused the request": the switch is reason enough on its own, so a
refusal is the only thing that can be missing it.

The status behind that line actually has five values, not four: the fifth is
"Turned off in settings," shown only when both switches are off. It has no row
in the table above because with both switches off the icon is not dimmed, it is
removed entirely — see [What lets it go](#what-lets-it-go).

## What lets it go

What ends a hold depends on which switch took it. Output going quiet and the
battery end only an automatic hold; an always-on hold is tied to neither.

- Every terminal producing output goes quiet — its command finishes, or its
  output rate drops below the threshold for a minute (see [What holds
  it](#what-holds-it)) — or you close its tab. Automatic hold only.
- The machine starts running on battery. The hold returns when you plug back in;
  the power source is re-checked about once a minute, so expect up to a minute
  of lag either way. Automatic hold only.
- You set the switch that took it to `false`: `keep_display_awake` for an
  automatic hold, `keep_display_awake_always` for an always-on one. Hand-edit
  `settings.json`, or untick the switch's entry in the eye icon's menu; for the
  always-on switch you can also run `keep_awake::ToggleAlwaysOn`. This takes
  effect immediately, including on a hold already in place — no restart. The
  two do not cover for each other: setting `keep_display_awake` to `false`
  while `keep_display_awake_always` is on changes nothing, and the display
  stays held.
- You right-click an empty part of the status bar and untick **Keep Display
  Awake** in the menu that opens (see [Status Bar
  settings](./visual-customization.md#status-bar)). This turns **both**
  switches off, so any hold ends at once, and it removes the eye icon from the
  bar itself, rather than leaving it dimmed: hiding a status-bar item this way
  drops the item's entity, and the icon is that entity. Ticking it again turns
  only `keep_display_awake` back on.

The icon is on the bar while either switch is on. When the last one goes off,
by any route above, the icon goes with it; turning the always-on switch on
brings it back.

## What this costs you

**Your screen will not lock on idle while a hold is in place.** That is the
feature working as asked, and it is also the risk: an editor left in a shared
office or a café stays readable to whoever walks past. The battery guard covers
a laptop on the move, but a desktop has no battery, so on a desktop nothing
ends an automatic hold except the producing terminal going quiet, the tab
closing, or the setting going off. There is no time limit.

**The always-on switch takes away every one of those exits.** It holds the
display, and so defeats the idle lock, for as long as a Zode window is open —
with every terminal quiet, with no terminal open at all, and on battery. There
is no battery guard and no time limit. A laptop left with it on keeps its screen
lit on battery too, readable to anyone who walks past, until you turn the
switch off or close every Zode window.

Hiding the status bar does not release it. Setting `experimental.show` under
`status_bar` to `false` removes the whole bar, the eye icon with it, while an
always-on hold carries on out of sight — as an automatic hold already did. The
command palette still reaches the switch: run `keep_awake::ToggleAlwaysOn` to
flip it, or check `keep_display_awake_always` in `settings.json`.

If that trade is wrong for where you work, set `keep_display_awake` to `false`
and leave `keep_display_awake_always` off, then lock the machine yourself, or
lock it manually before you walk away — a hold prevents the _idle_ lock, not a
lock you ask for.

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

A hold taken for the always-on switch is named "the display is kept awake until
turned off" where the OS records a reason, rather than after a tab.

A platform that cannot make the request at all gets none of this built: no
icon, no switch, no `keep_awake::ToggleAlwaysOn` action, no background work — a
dimmed control opening a switch that can never do anything would be worse than
nothing. That is a real code path
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
