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
  nothing running shows nothing, and the badge's total will not match the
  whole-machine totals in Activity Monitor, Task Manager, or `top`. Each
  process is accounted for the way that platform's own monitor accounts for
  it; the badge just adds up a much smaller set of them.

A PID reachable from more than one project (possible if a process gets
re-parented onto a shell that is itself a tracked terminal) is credited to
whichever project claims it first, never to both — the total can undercount
an edge case like that, but it can never double-count.

## Reading the badge and the popover

The badge sits at the left edge of the status bar's right-hand group, before
the buffer-specific controls. It draws nothing, and reserves no space, when
there is nothing to report — a freshly opened window with no terminals or
language servers running shows no icon at all. Once a tracked project has a
child process, the badge shows the combined memory and CPU across every
tracked project, each behind its own icon: a database glyph for memory, a bolt
for CPU.

Those two icons are stand-ins. This repository ships no purpose-built CPU or
memory-module icon, so the badge's tooltip and every popover row spell out
"RAM" and "CPU" in words rather than leaving the icons to carry the meaning
by themselves.

Clicking it opens a popover with one line per project: its display name, its
memory, and its CPU. A value that has not been measured yet — for example, CPU on
the very first sample, before there is an earlier reading to difference
against — reads "not measured", never "0%", since those mean different
things: zero says the project is idle, not measured says nothing is known yet.

### What "memory" means here

Whatever the platform's own system monitor calls it, so the badge can be
checked against `htop`, Activity Monitor or Task Manager rather than quietly
disagreeing with them:

| Platform | Source                                                          | Comparable to                                                                                                                                                                     |
| -------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux    | proportional set size, from `/proc/<pid>/smaps_rollup`          | the kernel's own summed `Pss:` fields, which is what it was checked against                                                                                                       |
| macOS    | physical footprint, via `proc_pid_rusage`                       | Activity Monitor's **Memory** column, and `top -l 1 -stats mem`, which reads the same source                                                                                      |
| Windows  | private commit, from `PROCESS_MEMORY_COUNTERS_EX::PrivateUsage` | Task Manager's private working set, but reading a little high: commit counts pages that have been paged out. Never checked against it, see [Platform support](#platform-support). |

Deliberately not resident set size. A page shared between a process and the
four children it forked appears in all five of their RSS figures, so adding
them up charges the same memory five times: measured at 33% too high on Linux
and 90% too high on macOS, where every process maps the shared library cache.
Each platform falls back to RSS if its own reading cannot be taken — a process
that cannot be read is not a process using no memory.

### What "CPU" means here

A share of the CPUs this application may actually run on
(`std::thread::available_parallelism()`, so a cgroup quota or a CPU affinity
mask is respected), clamped to 100%. A project busy across several cores reads
as "busy" rather than as a number past 100 that answers a different question
than the one the badge is for. The combined figure across projects is clamped
too, since each contributor is already a share of the whole.

Logical CPUs, not physical cores. The two are the same number only on hardware
without SMT, and where they differ, dividing by the smaller one makes the badge
read double. The count is re-read on the slow cadence below rather than at
startup, so raising a container's CPU quota or changing an affinity mask is
picked up within about 30 seconds instead of holding the badge wrong for the
life of the process.

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

Every figure below comes from a standalone probe of the `sysinfo` calls this
feature uses, not from watching the feature run inside the editor, because
nobody has done that yet. They come from two different machines and they do
not transfer between them, so each says which one it came from.

On a macOS box with 10 cores and no SMT (Apple Silicon), carrying 817 live
processes:

- Full process enumeration: 12–15 ms, on a throwaway `sysinfo::System` that is
  dropped as soon as that pass ends.
- Narrow refresh of the already-known PIDs: ~165 µs, roughly 80x cheaper.
- A `System` given only the known PIDs to refresh retains only those PIDs (10
  records measured, not 819), so what stays resident between ticks is a
  handful of process records, not the whole machine's process table.

Both timings predate the 2026-09 correctness fixes. The enumeration figure
survives them unchanged, because macOS never listed threads as processes and
that listing is the work the fix removed. The narrow refresh figure is now a
floor rather than the whole cost of that tick: every sampled process also has
its platform memory reading taken, and that was not part of what was measured
here. On Linux the enumeration used to do strictly more than it does now,
walking every thread of every process and reading the contents of
`/proc/<pid>/task` to do it. That is gone, but how much it had been costing was
never measured, so there is no Linux enumeration figure to quote.

On an aarch64 Linux container, the one cost that platform adds: reading a
process's proportional set size opens `/proc/<pid>/smaps_rollup`, which walks
every memory mapping the process owns, where the resident-set figure it
replaces is a constant-time read. Measured at 35 µs for a process with ~74
mappings, 171 µs at ~2,000, and 746 µs at ~8,000, so a handful of large
language servers can put single-digit milliseconds on a tick. That work runs on
a background thread every three seconds, so it does not touch frame time.

## Privacy

The discovery pass asks `sysinfo` for process data with
`ProcessRefreshKind::nothing().without_tasks()`. That reports each process's
parent PID, which is all discovery needs, and it does **not** read the command
line, executable path, or environment of any process on the machine. Only PIDs,
parent PIDs, memory figures and CPU figures are ever held by this feature, and
none of it is logged.

On Linux the memory reading additionally opens `/proc/<pid>/smaps_rollup`,
which the kernel only permits for processes this user owns; on macOS
`proc_pid_rusage` is likewise refused for processes outside this user's reach.
Both failures fall back to the reading `sysinfo` already had. That boundary is
the kernel's, and this feature only ever asks about processes descended from
its own children in any case.

## Platform support

The badge runs on Linux, macOS and Windows. How thoroughly each one has been
checked differs, and the difference is worth knowing: four defects were found
in 2026-09 that were invisible on macOS, the only machine the feature had ever
been measured on. The full account is in the repository's engineering journal,
under `2026-09-13-four-defects-that-only-apple-silicon-could-hide`.

- **Linux** — the sampler and its own tests were compiled and run inside a
  container, which is how the thread-counting defect was proven. The memory
  figure was checked against the kernel's summed `Pss:` fields for the same
  five processes, 0.04% apart. The CPU count was checked inside a
  `--cpuset-cpus=0-1` container, where it answered 2 and the physical-core
  count it replaced still answered 10.
- **macOS** — the memory figure was checked against `top -l 1 -stats mem`,
  which reads the same source as Activity Monitor's Memory column: 646.9 MB
  against 647M for one live process.
- **Windows** — CI builds the crate and runs its tests on Windows, so the
  Windows reading compiles and comes back positive and within a factor of
  eight of resident set size. That is the whole of what is known.
  **Nobody has held it next to Task Manager.** Its accuracy there is
  unconfirmed, and the one thing expected of it is directional: private commit
  counts pages that have been paged out, so it should read somewhat above Task
  Manager's private working set.

The timing figures in the section above each name the machine they came from,
and none has been re-taken on another. Read them as the shape of the difference
between the two cadences, not as numbers that hold everywhere.

Nobody has yet watched the badge render inside the editor on any platform.

## Turning it off

Set `project_footprint_indicator` to `false` and the badge, the popover, and
the polling loop behind them all stop — there is no partial state where the
loop keeps running with nothing to show.

The same setting is also reachable by right-clicking an empty part of the
status bar and switching off **Project Footprint** in the menu that opens —
see [Status Bar settings](./visual-customization.md#status-bar). That menu
writes the exact same setting a hand edit would, so the effect is identical
either way: the badge disappears and its polling loop stops with it.
