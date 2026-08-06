# Phase 06 Memory Measurements

**Date:** 2026-08-06
**Machine:** MacBook Pro, Apple Silicon (arm64), macOS 26.6 (build 25G72), 10 CPU cores, 32 GB RAM
**Server measured:** `rust-analyzer 1.94.1 (e408947b 2026-03-25)`, installed via `rustup` toolchain
`1.94.1-aarch64-apple-darwin`

## Scope and honesty notes

- **Only rust-analyzer was measured.** This sandbox has no `gopls` or `typescript-language-server`
  installed, and installing them was out of scope for this pass (see Next Steps). Every number
  below is Rust-only — do not extrapolate to Go/TS projects without separately measuring them.
- **Not measured against the zode repo itself.** zode is a ~300-crate Cargo workspace; pointing
  rust-analyzer at *any* subdirectory inside it resolves the whole workspace (confirmed
  empirically — see below), and this machine already has a live, user-owned rust-analyzer session
  on this exact repo. Reusing it would have contended with that live session and produced a
  number representative of "the zode monorepo," not of "a typical project." Instead, a synthetic,
  realistically-sized standalone project was built for this measurement (see below) — the numbers
  here characterize *a modest real Rust project*, not zode-sized monorepos.
- **This is one project shape, not a distribution.** Real projects vary widely in size and
  dependency graph. Treat these numbers as a single honest data point, not a statistical claim.

### Aborted first attempt — full-workspace resolution

The first attempt pointed rust-analyzer at `crates/paths` (a small, ~600-line crate) expecting a
small-crate measurement. Instead RSS climbed to **5.3 GB and was still rising** after 60 seconds,
because rust-analyzer walked up to the nearest `Cargo.toml` with `[workspace]` and indexed the
*entire* zode workspace — all ~300 crates — rather than just the pointed-at directory. This
number was discarded (not representative, and it may have contended with the live editor session
mentioned above). The lesson generalizes: **pointing a language server "at a project" inside a
monorepo does not scope it to that subdirectory** — this is relevant to Phase 6's own FR1/FR2
instrumentation, since a project's true LSP memory cost depends on its *workspace* root, not the
folder the user happens to have open.

### Sample project (used for all numbers below)

A standalone Cargo project (outside any workspace), with a realistic small-to-medium dependency
set and ~150 lines of real code across 4 modules:

```toml
[dependencies]
anyhow = "1.0.104"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.19"
tokio = { version = "1.53.1", features = ["full"] }
```

`cargo build` run once beforehand (target/ present, matching a project that's actually been
worked on, not a pristine `cargo new`).

## Method

A minimal Python LSP client (`initialize` → `initialized`, acking
`window/workDoneProgress/create`) drove a real `rust-analyzer` process per project. RSS was
polled every 5s via `ps -o rss=`. "Settled" = 8+ consecutive seconds with no `$/progress`
notification (rust-analyzer's own indexing-progress signal) after the process had run for at
least 20s. N separate copies of the sample project (identical dependency set, own `Cargo.lock`)
were used for N ≥ 2 so instances didn't share a lockfile/target directory.

## Results

| N projects open | Per-instance RSS (settled) | Total RSS | Time to settle |
|---|---|---|---|
| 1 | 674 MB | 674 MB | ~20s |
| 3 | 668 / 678 / 667 MB | 2,013 MB | ~20s (all three) |
| 5 | 690 / 687 / 688 / 665 / 681 MB | 3,412 MB | ~20s (all five) |

Per-instance average across all 9 runs: **~675 MB**, standard deviation ~9 MB (~1.3%) — tight and
consistent.

**Scaling is linear, not sub-linear.** 3× predicted from N=1 would be 2,022 MB (actual: 2,013 MB,
−0.4%); 5× predicted would be 3,375 MB (actual: 3,412 MB, +1.1%). No shared-cache benefit was
observed between concurrent instances of the same dependency set on this machine — each
rust-analyzer process builds its own independent in-memory analysis database. This directly
validates the plan's core premise: hibernating N−1 of N similar projects should free
approximately (N−1)/N of this cost, not some smaller fraction.

**Wake cost (cold start → settled) was ~20s** for this project size — most of which is the
`initialize` round-trip (10ms) plus initial crate-graph + macro-expansion indexing. This is a
*small/medium* project; the plan's own concern that "rust-analyzer has no on-disk index cache, so
wake pays the full indexing cost again" is not measured here for a large project — expect
materially longer wake times for anything zode-repo-sized, which this pass deliberately avoided
touching (see above).

## What this does and doesn't tell us about the defaults

- **`hibernate_after_ms` (Phase 2/3 default: 300,000 ms / 5 minutes):** no change proposed. A
  ~20s wake cost for a modest project is well inside what a 5-minute idle threshold already
  tolerates; this measurement doesn't surface a reason to move it.
- **`memory_pressure_threshold_percent` (Phase 6, currently a `10.0` placeholder):** this
  measurement does **not** directly calibrate this value — per the phase's own Risk Assessment,
  the fuse's threshold is deliberately based on *system-wide* available memory, not a per-project
  number, specifically so it doesn't need retuning as project sizes vary. What this pass *does*
  confirm is that the per-project cost it's protecting against is real and linear (~675 MB per
  modest Rust project on this measurement), which is the assumption the threshold's existence
  rests on. `10.0` (i.e., trigger when under 10% of total memory is free) is left as a reasonable,
  still-unmeasured judgment call — changing it would need a deliberate low-memory scenario test
  (e.g. a memory-constrained VM), which is out of scope for this pass.

## Next steps (not done in this pass)

- Install `gopls` and `typescript-language-server` and repeat this measurement for Go/TS
  projects — Rust-only coverage here is a real gap for any user whose workload isn't Rust.
  These are the two Todo items still open from phase-06 (see phase-06's own Todo List and
  `plan.md`'s phase table).
- Measure wake cost on a large, real-world-sized project (the zode repo is right there, but
  needs to be measured from a *separate* environment without a live competing rust-analyzer
  session on it, unlike this one).
