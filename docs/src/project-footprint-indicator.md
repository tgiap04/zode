# Project Footprint Indicator

A badge in the status bar shows the combined CPU and RAM of every tracked
project's child processes — its language servers and its terminals (the agent
CLIs running in them), plus their descendant process trees. Clicking it opens
a popover with a per-project breakdown.

This is on by default:

```json
{
  "project_footprint_indicator": true
}
```

## What it counts, and what it deliberately does not

A project's "child processes" are its language servers, its terminals, and
everything those terminals spawn — a shell that spawns an agent CLI that
spawns a build, for instance, all count toward that project once the process
tree is walked.

Two things it does **not** count, on purpose:

- **Zode's own heap and CPU.** Zode hosts every open project in one OS
  process, so its own memory and CPU use cannot be attributed to any single
  project and are left out entirely.
- **The rest of the machine.** This is not a system monitor. A project with
  nothing running shows nothing, and the badge's total will not match
  Activity Monitor, Task Manager, or `top` — those report the whole machine;
  this reports only what a tracked project's own processes are doing.

A PID reachable from more than one project (possible if a process gets
re-parented onto a shell that is itself a tracked terminal) is credited to
whichever project claims it first, never to both — the total can undercount
an edge case like that, but it can never double-count.

## Reading the badge and the popover

The badge sits at the left edge of the status bar's right-hand group, before
the buffer-specific controls. It draws nothing, and reserves no space, when
there is nothing to report — a freshly opened window with no terminals or
language servers running shows no icon at all. Once a tracked project has a
child process, the badge shows the combined RSS and CPU across every tracked
project, each behind its own icon: a database glyph for memory, a bolt for
CPU.

Those two icons are stand-ins. This repository ships no purpose-built CPU or
memory-module icon, so the badge's tooltip and every popover row spell out
"RAM" and "CPU" in words rather than leaving the icons to carry the meaning
by themselves.

Clicking it opens a popover with one line per project: its display name, its
RSS, and its CPU. A value that has not been measured yet — for example, CPU on
the very first sample, before there is an earlier reading to difference
against — reads "not measured", never "0%", since those mean different
things: zero says the project is idle, not measured says nothing is known yet.

CPU is normalized against the machine's physical core count and clamped to
100%, so a project busy across several cores reads as "busy", not as a number
past 100 that answers a different question than the one the badge is for.

## What polling this costs

Two cadences share one timer, so the expensive pass and the cheap one can
never overlap:

- Every **~3 seconds**, a narrow refresh of only the PIDs already known to
  belong to a tracked project.
- Every **~30 seconds** (every tenth narrow tick), a full process-tree
  enumeration, run once to discover which PIDs now descend from each
  project's roots, then thrown away.

Both stop entirely when the window is not the active one, and when the
setting is off. If no tracked project owns a single child process — the
common case for a freshly opened window — a tick does no background work at
all: there is nothing a scan could discover.

### The numbers behind that, and where they come from

These come from a standalone probe of the underlying `sysinfo` calls this
feature uses, run once on one machine (macOS, 10 physical cores, 817 live
processes) — not from watching the feature run inside the editor, because
nobody has done that yet:

- Full process enumeration: 12–15 ms, on a throwaway `sysinfo::System` that is
  dropped as soon as that pass ends.
- Narrow refresh of the already-known PIDs: ~165 µs — roughly 80x cheaper.
- A `System` given only the known PIDs to refresh retains only those PIDs (10
  records measured, not 819), so what stays resident between ticks is a
  handful of process records, not the whole machine's process table.

## Privacy

The discovery pass asks `sysinfo` for process data with
`ProcessRefreshKind::nothing()`, which does **not** read command lines,
executable paths, or environment variables of any process on the machine —
verified: it still reports each process's parent PID, which is all discovery
needs. Only PIDs, parent PIDs, RSS, and CPU are ever held by this feature, and
none of it is logged.

## Platform support

Verified on macOS only. The measurements above, and the confirmation that
`sysinfo` populates a process's parent PID under the empty refresh kind, both
come from a macOS run. Windows and Linux are untested for this feature —
nobody has run it on either platform yet.

## Turning it off

Set `project_footprint_indicator` to `false` and the badge, the popover, and
the polling loop behind them all stop — there is no partial state where the
loop keeps running with nothing to show.
