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
| Turned off in settings          | You switched it off                                |
| The system refused the request  | Everything says yes and the OS still said no       |

The line exists because four of those five states are the same dimmed icon. The
tooltip names the agent responsible when there is one.

## What lets it go

- Every working agent finishes, or you close its tab.
- The machine starts running on battery. The hold returns when you plug back in;
  the power source is re-checked about once a minute, so expect up to a minute
  of lag either way.
- You set `keep_display_awake` to `false`. This takes effect immediately,
  including on a hold already in place — no restart.

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

Two separate things are needed, and they landed separately: asking the OS to hold
the display, and noticing the machine is on battery.

| Platform        | Holds the display                                                                    | Detects battery |
| --------------- | ------------------------------------------------------------------------------------ | --------------- |
| macOS           | Yes — an IOKit assertion, visible in `pmset -g assertions` and named after the agent | Yes             |
| Windows         | **Not yet**                                                                          | Yes             |
| Linux (Wayland) | **Not yet**                                                                          | Yes             |
| Linux (X11)     | **Not yet**                                                                          | Yes             |

So today this does something only on macOS. **On every other platform there is
no icon at all** — the editor asks the OS whether it could ever hold the display
and, where the answer is no, builds none of this: no icon, no switch, no
background work. A dimmed control opening a switch that can never do anything
visible would be worse than nothing. The `keep_display_awake` setting is still
read, so this appears on its own once a platform gains an implementation.
