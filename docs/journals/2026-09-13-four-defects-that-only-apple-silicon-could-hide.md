# Four defects that only Apple Silicon could hide

**Date**: 2026-09-13 22:30
**Severity**: High — the badge overstated RAM by up to 8x on Linux and CPU by 2x on every SMT machine, for as long as the feature has existed
**Component**: `project_footprint`
**Status**: Resolved (PR #38)

## What Happened

The reader said: "thông tin ram, cpu trên linux đang sai … nó sai hoàn toàn luôn ấy." They
were right, and they were right about more than they knew — the check across the other
platforms turned up three further defects, one of which is wrong everywhere.

Nothing regressed. Every one of these shipped with the feature and has been wrong since.

## The Brutal Truth

**All four trace to one root: the feature was designed and measured on Apple Silicon, and
Apple Silicon is the one machine where three of them are invisible.**

The [2026-08-27 journal](2026-08-27-per-project-cpu-ram-footer.md) is not wrong about the
measurement discipline it praises — the expensive decisions really were measured before they
were made. The measurements were just all taken on one box, and their results were then
written into module comments as unqualified facts. A figure with no platform beside it reads
as a property of the code. It was a property of the machine.

That journal also has this, under "What Is Still Not Done":

> **Windows and Linux are untested** for the footprint feature; `parent()` under the empty
> refresh kind was verified on macOS only.

The item was known. It was written down. It was never converted into work. **A recorded
unknown is not a mitigated risk** — and the repo had `run_tests_linux` and
`run_tests_windows` in CI the entire time.

### Defect 1 — Linux only. RAM multiplied by thread count.

`ProcessRefreshKind::nothing()` is not "refresh nothing". sysinfo's own doc:

> Creates a new `ProcessRefreshKind` with every refresh set to `false`, **except for
> `tasks`**. By default, we want to list all processes and tasks are considered processes on
> their own in linux so we still fetch them by default.

With `ProcessesToUpdate::All`, sysinfo inserts every _thread_ as its own process whose
`parent()` is its thread-group leader. The parent-chain walk adopted all of them, and
`sample()` then read each one's memory from `/proc/<pid>/task/<tid>/statm` — which reports
the **whole process's** RSS. Summed once per thread.

Measured, one process with seven threads: **16,986,112 bytes reported against a true
2,138,112.** A `rust-analyzer` or a node-based agent carries far more than seven threads.

CPU was doubled by the same route: the leader's `/proc/<pid>/stat` utime/stime is already
thread-group-wide, and each task added its share again.

### Defect 2 — every platform. The CPU denominator was the wrong quantity.

`Process::cpu_usage()`'s doc: "If you want a value between 0% and 100%, divide the returned
value by **the number of CPUs**." That is logical CPUs. The code divided by
`System::physical_core_count()`.

| Machine                                         | Divisor used | Divisor wanted | Badge           |
| ----------------------------------------------- | ------------ | -------------- | --------------- |
| SMT x86 (most Linux, all Windows, Intel Macs)   | logical/2    | logical        | **2x too high** |
| Linux container, `--cpuset-cpus=0-1` (measured) | 10           | 2              | **5x too low**  |
| Apple Silicon                                   | 10           | 10             | correct         |

`std::thread::available_parallelism()` is the answer, respects cgroup quota and CPU
affinity, and was **already this repo's idiom at five call sites**. `physical_core_count`
appeared in no other crate.

### Defect 3 — every platform. The combined total was unbounded.

`sum()` clamps each project to 100%. `Footprints::combined()` then added those clamped
percentages with no ceiling. Two pegged projects read 200%.

The only one of the four that is deterministic everywhere, and the only test in this batch
that was red on the machine it was written on.

### Defect 4 — every platform. Summed RSS counts every shared page once per process.

Measured, a tree of five processes of the same binary:

| Platform                                | Summed RSS | What the OS says | Overstated by |
| --------------------------------------- | ---------- | ---------------- | ------------- |
| Linux (PSS, `/proc/<pid>/smaps_rollup`) | 25,124,864 | 18,908,160       | **33%**       |
| macOS (`ri_phys_footprint`)             | 46,743,552 | 24,594,160       | **90%**       |

macOS is worse because the dyld shared cache is mapped into every process.

The criterion the fix was built to: **the badge must match what that platform's own system
monitor reports**, because a number nobody can check against their own machine is not a
measurement. PSS on Linux, `phys_footprint` on macOS (Activity Monitor's "Memory" column),
`PrivateUsage` on Windows, each falling back to RSS when the read fails — never to zero. A
process that cannot be read is not a process using nothing.

## Verification

Every mechanism was removed and the matching test watched to fail. The two that carry real
evidence are the two that had to leave the development machine.

| Neutralised                         | Test that failed                                                                  | Where it had to run                                          |
| ----------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| `.without_tasks()` in `descendants` | `a_thread_is_not_a_descendant_process` — got 11 pairs, expected 1                 | **Linux container.** Green on macOS with the defect present. |
| the clamp in `combined()`           | `combined_cpu_cannot_exceed_the_whole_machine` — `Some(200.0)` vs `Some(100.0)`   | anywhere                                                     |
| `Pss:` → `Rss:` in the parser       | `parse_pss_bytes_reads_the_kilobyte_field` — `Some(25726976)` vs `Some(19361792)` | anywhere, by design                                          |
| `kilobytes * 1024` → `kilobytes`    | same test — `Some(18908)` vs `Some(19361792)`                                     | anywhere                                                     |

Cross-checks against the operating systems' own tools, not against our own code:

- **Linux.** The shipped sampler reported **18,896,896** bytes for a five-process tree. The
  kernel's `Pss:` fields, summed by an independent parser in the same probe, said
  **18,905,088** — 0.04% apart, the gap being the time between two reads. The old summed RSS
  was 25,079,808.
- **macOS.** `resident_cost` for a live 647 MB process reported **678,317,368 bytes
  (646.9 MB)**. `top -l 1 -stats mem`, which reads the same source as Activity Monitor's
  Memory column, said **647M**. `ps` RSS for the same PID said 683.6 MB.
- **CPU denominator.** Under `--cpuset-cpus=0-1`, the shipped `cpu_count()` returned **2**
  where `physical_core_count()` still returned **10**.
- **What the Linux memory read costs.** `smaps_rollup` walks every VMA of the target with
  its `mmap_lock` held; `statm`, the resident-set file it replaces, is O(1). Measured in the
  same aarch64 container:

  | mappings | `statm` | `smaps_rollup` | ratio |
  | -------- | ------- | -------------- | ----- |
  | ~74      | 12.2 µs | 35.0 µs        | 2.9x  |
  | ~525     | 10.2 µs | 61.1 µs        | 6.0x  |
  | ~2,027   | 9.0 µs  | 170.5 µs       | 19.0x |
  | ~8,027   | 13.3 µs | 745.8 µs       | 56.3x |

  Recorded here because the user documentation compares the two, and a comparison with only
  one side sourced is the same defect in a smaller font. It runs on a background thread
  every three seconds, so single-digit milliseconds is affordable.

`cargo test -p project_footprint`: 33 passed. `./script/clippy -p project_footprint`: exit 0.
The real sampler and its real test file were also compiled and run inside a Linux container
against a shim crate, which is how defect 1 was proven without a Zed-scale Linux build.

## A fifth one, found during review of the fix

Review turned up a test in this crate whose comment said:

> this is the assertion that would fail if the rebuild were removed

It would not. `self.tracked = discovered` runs on **both** branches of the superset check, so
every assertion in `shrinking_the_discovered_set_evicts_the_retained_map` passed with the
rebuild deleted — verified by deleting it: 9 of 9 sampler tests stayed green.

Worth recording how the repair went, because the first attempt was also wrong. Asserting
that the CPU baseline is gone after a shrink looked like it would pin the rebuild, and it
did not: the rebuild branch ends with a second `self.primed = false`, so the flag reads
false whether or not the `System` was replaced. It took reading the branch line by line to
find the one thing only the rebuild does — discard the retained process records — and that
was not observable at all until a `retained_len()` accessor was added for it. The assertion
now fails with `(2 -> 2)` when exactly one line, `self.narrow = System::new()`, is removed.

Two lessons sit in that, and they are not the same lesson. A test can be hollow while its
comment is confident. And an assertion written to catch a mutant has to be _run against that
mutant_, because reasoning about which flag moves is exactly the step that was wrong here.

## Still Unverified

Left visible on purpose. This is the section the previous journal had, and honouring it is
the point.

- **The Windows arm's fidelity.** `PrivateUsage` is private _commit_, not resident, so it
  reads a little higher than Task Manager's "private working set". CI's `run_tests_windows`
  proves it compiles and returns a number. **Nobody has held it next to Task Manager.**
- **The badge has still never been observed rendering** these numbers. Every check above is
  a probe or a test. The 2026-08-27 entry says the same thing about the original feature,
  which is how four defects reached a user.
- **`Project::resource_stats`** (`crates/project/src/project.rs:5095`) sums
  `process.memory()` over language-server PIDs and shares defect 4. It uses
  `ProcessesToUpdate::Some`, so defect 1 does not reach it. Not fixed here.

## Lessons

1. **A figure in a comment with no platform beside it is a claim about the world.** "12-15
   ms, touching ~1300 records" was true of one macOS box. Write the machine down, or write
   nothing.
2. **A green suite on your own platform is not a passing suite when CI has three.** Every
   existing test in this crate was green against all four defects. The Linux and Windows
   runners existed the whole time and were never asked.
3. **"Untested on X" in a doc is an open defect, not a caveat.** It was recorded honestly in
   August and cost four defects in September.
4. **Split the platform-independent logic out so it can be tested where you are.**
   `parse_pss_bytes` is pure string logic gated `cfg(any(target_os = "linux", test))` for
   exactly one reason: its two tests are the only ones covering the memory work that can
   fail against a no-op `resident_cost`, and they run on a Mac. Every other test there
   passes whether the fix exists or not. Both of those carry that admission in their own
   doc comment — and one of them only does because review caught that it did not, in the
   change whose journal entry this is.
5. **The reader found this by using the app.** Third time this week.
