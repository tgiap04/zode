# The gate found twice what I had counted by hand

**Date**: 2026-08-27
**Severity**: Medium — a shipped artifact that refused to start on a supported LTS
**Component**: `script/check-glibc-floor`, `tooling/xtask/.../workflows`, `README.md`, `docs/src/linux.md`
**Status**: Shipped to the branch, and never once run in CI

## What Happened

The published Linux tarballs required glibc 2.39, so they ran on Ubuntu 24.04 and nothing
older. The fix: build inside an `ubuntu:22.04` container, drop the floor to 2.35, and add a
gate that fails the build if anything in the bundle ever exceeds it again.

Three things are worth recording, and none of them is the fix.

## The Brutal Truth

### My measurement was a sample. The gate was a census.

I opened the shipped `v0.1.1` artifact, read its ELF headers, and reported a clean result:
**exactly four symbols** exceed `GLIBC_2.35` — `__isoc23_sscanf`, `__isoc23_strtol`,
`pidfd_spawnp`, `pidfd_getpid`. That number went into the plan, into the phase files, and
into my report to the user as an established fact.

It was taken from the two binaries I had decided were the ones that mattered:
`libexec/zed-editor` and `bin/zed`.

The first time the finished gate ran over the whole bundle it found **eight files** over the
floor:

| file                                                                             | needs | what it is                                                       |
| -------------------------------------------------------------------------------- | ----- | ---------------------------------------------------------------- |
| `bin/zed`, `libexec/zed-editor`                                                  | 2.39  | the two I checked                                                |
| `libexec/zode-db-mysql`, `libexec/zode-db-postgres`                              | 2.38  | database drivers I did not think of                              |
| `lib/libX11.so.6`, `lib/libxcb.so.1`, `lib/libbsd.so.0`, `lib/libxkbcommon.so.0` | 2.38  | **system libraries copied out of the build host** by `find_libs` |

The last row is the interesting one. Those are not our code at all — they are Ubuntu 24.04's
own `/usr/lib`, riding along inside the tarball because `script/bundle-linux` copies the
dynamic dependencies it finds. Nothing about "read the binary's symbols" would ever have led
me to them; I would have had to think about what else is in the archive.

The container fix covers them for free, since `find_libs` will now copy 22.04's libraries
instead. So the outcome was unaffected. But the _number I published was wrong_, and it was
wrong in the specific way hand-verification is always wrong: it measured what I had already
decided to look at.

The tool I built to replace hand-checking exposed the limits of my hand-checking on its
first run. That is the argument for the gate, made better than I made it.

### I filed a provable question under "only CI can answer this"

The plan's phase 04 was "prove it in CI", and I put everything in it — including the central
question the entire change exists to settle: _does building against glibc 2.35 actually
lower the floor?_

The user asked: "bạn không test ci qua docker container được à" — can't you test this through
a Docker container?

They were right and I was lazy. Two experiments, both of which ran in minutes:

1. A minimal Rust program using `std::process::Command`, built in `ubuntu:22.04`: maximum
   requirement `GLIBC_2.34`, nothing above 2.35, no `pidfd_*` at all.
2. **The repo's own `cli` and `zode-db-postgres`**, built in the same container in 4m49s:
   both `GLIBC_2.34` max, `above 2.35: NONE`. These are precisely two of the eight files
   measured over the floor in the shipped artifact.

Same source, two environments, two measurements. The mechanism stopped being an inference.

What genuinely needs CI is smaller and duller than what I claimed: whether `actions/checkout`
runs via git inside a container rather than degrading to a tarball, whether `$GITHUB_PATH`
puts cargo on `PATH`, whether the arm host pulls the arm64 manifest, and whether disk
survives the loss of ~14 GB of host cleanup. Runner semantics — not Linux.

"Only CI can prove it" is a claim about the world, and it deserves the same scepticism as
any other. I had not tried.

### The evidence gate caught me massaging a record

I wrote three negative tests into `temper-results.json` as `status: pass, exitCode: 1` — the
gate correctly _rejects_ a bad bundle, so 1 is the expected result. The evidence validator
blocked it: a `pass` over a non-zero exit is, by its rules, a forged green.

The easy fix was to edit the JSON until it validated. The honest one was to re-run each check
in the form that actually exits 0 — `! ./script/check-glibc-floor <bad input>` — so the
recorded exit code is the _test's_ real result rather than a number I typed. Three commands,
three real zeros, then SEALED.

A validator that only accepts consistent records is worth having precisely because the
inconsistent version was easier.

### Inherited claims, twice in one change

Two statements in this repo turned out to be upstream's, carried across and never checked:

- `runners.rs:11-20` said 22.04 was impossible because `webrtc-sys` needs clang 17+ and
  `script/linux` only installs clang-18 on its 20.04 branch. **This fork compiles no C++ at
  all** — zero `webrtc`/`livekit` rows in `Cargo.lock`, zero `.cpp`/`.cc` in the tree. The
  same comment claimed the x86 22.04 runner image carries clang-18; it ships 13/14/15.
- `docs/src/linux.md` claimed glibc ≥ 2.31 on x86_64 and ≥ 2.35 on aarch64. Both were 2.39.
  Those are upstream's numbers, true for upstream's 20.04 builders, never true here.

Both read as settled knowledge. Both were decoration.

### And the reasoning-vs-measuring split, from the other side

A research pass concluded that libstdc++/GLIBCXX was a second independent floor that would
also have to move — sound reasoning from how a 24.04 build against `libstdc++-14-dev`
normally behaves. It is wrong for this binary: `DT_NEEDED` contains no `libstdc++.so.6` and
the dynamic symbol table has zero `GLIBCXX_`/`CXXABI_` entries.

Generalised correctness lost to a two-second read of the actual artifact. It is the same
lesson as my own four-symbol error, pointing the other way: I over-trusted a measurement's
_scope_, the research over-trusted a _principle_.

## Technical Details

**Why the four symbols vanish.** `__isoc23_sscanf`/`__isoc23_strtol` are not feature
dependencies at all — glibc ≥ 2.38 headers redirect `sscanf`/`strtol` to those names at
compile time, so building against 2.35 headers emits the plain symbols. `pidfd_spawnp` and
`pidfd_getpid` appear as **weak** undefined references (`w` in `objdump -T`): Rust std uses
them when the libc it links against provides them and falls back when it does not. On 2.35
there is nothing to bind to, so std takes the other path.

**Why a container rather than `runs-on: ubuntu-22.04`.** The smaller diff was rejected on
lifecycle: that runner label's deprecation begins 2026-09-17 — three weeks from this entry —
and it is unsupported from 2027-04-17 (`actions/runner-images#14254`). Upstream holds a 20.04
floor by paying for Namespace.so runner profiles, which this fork cannot use. A container
decouples the userland from whatever GitHub calls its hosted image next.

**`objdump -T`, not `readelf --dyn-syms`.** Apple's `/usr/bin/objdump` is llvm-objdump and
reads ELF fine; the container has GNU binutils 2.38. They differ only in wrapping the version
in parentheses, which the extraction ignores. That one choice is why the gate is runnable on
a developer machine at all, and being runnable locally is what let phase 01 be finished and
proven before anything was pushed.

**What was lost.** `free_disk_space` on Linux drops roughly 14 GB of reclaim: `sudo` and
`docker` do not exist in the image and the host paths are not mounted into it. The `df -h /`
is kept so the first run reports a real number rather than a guess, and the fallback
(`volumes: ["/:/host"]`) is written into the function's doc comment so nobody has to design
it while a build is red.

## What Is Still Not Done

- **No CI run has happened.** No tarball has been produced by the containerised job, none
  downloaded, and nothing launched on an actual Ubuntu 22.04 machine. Phase 04 holds all of
  it and every box in it is unticked.
- **Four of the eight over-floor files were never re-measured.** The bundled system
  libraries will come from the container's own `/usr/lib` — reasonable, unobserved. The gate
  covers it on the first run, which is the right place for it.
- **`setup_linux`'s 20-minute timeout** is untested against a bare image where nothing is
  preinstalled.

## Lesson

**A hand measurement reports on what you chose to look at.** My four-symbol count was not
sloppy — the readings were right. Its scope was a judgement I made before measuring and then
stopped seeing. Any check that a person aims is a sample; the value of a gate is that it
cannot be aimed.

**And "only X can prove this" is a claim, not a constraint** — until you have tried the
cheaper thing and watched it fail.
