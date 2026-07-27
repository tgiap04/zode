---
phase: 1
title: Preparation and baseline
status: completed
effort: 0.5d
---

# Phase 1: Preparation and baseline

## Context Links

- [plan.md](./plan.md) · [Consultation record](./brainstorm-report.md)
- [Workspace surgery research](./reports/research-workspace-surgery.md)
- [Final delete set script](./research/final-delete-set.py)

## Overview

**Priority:** P1 · **Build state:** GREEN (stays green)

Establish the branch, the measurement baseline, and the tooling that makes the red period navigable. Nothing is deleted here. This phase exists precisely because Hướng B removes all checkpoints — the only remaining safety net is knowing exactly what "correct" looked like before the cut.

## Key Insights

- `cargo check -p <crate>` in reverse-topological order beats `cargo check --workspace`. The latter re-diagnoses the whole closure every run and floods you with noise from crates you have not started fixing.
- `script/crate-dep-graph` already wraps `cargo depgraph --workspace-only` — no new tooling needed for the topological order.
- `profile.dev` already sets `incremental = true` (`Cargo.toml:884`). `sccache` is CI-only here (`.github/workflows/release.yml:37-47`) — do not enable locally; it does not help an already-incremental edit-check loop.
- `mold` is **not** viable on macOS (upstream calls it alpha). `.cargo/config.toml:19-20` correctly forces `lld` only for `aarch64-linux`. Leave the linker config alone.
- `cargo machete` is already a repo convention (`script/clippy:14-16`, ignore-list `Cargo.toml:1007-1016`). Prefer it over `cargo udeps` (nightly, slower, wrong tool here).

## Requirements

**Functional**
- A dedicated branch isolating all 12 phases.
- A recorded, reproducible baseline: green `cargo check --workspace`, green tests, working binary.
- A reverse-topological survivor fix order, generated rather than guessed.

**Non-functional**
- Every measurement re-runnable later, so Phase 11 can prove no regression beyond the intended deletions.

## Architecture

Three artifacts feed the rest of the plan:

```
script/crate-dep-graph          ──►  survivor topological order  ──►  Phases 7–8 fix sequence
cargo metadata                  ──►  final-delete-set.py         ──►  Phase 4 delete list
cargo run -- --dump-all-actions ──►  baseline action list        ──►  Phase 9 keymap diff
```

The third is critical: Phase 9 must diff the **post-deletion** action list against the keymap assets. Capturing the **pre-deletion** list now yields the exact set of actions that disappear.

## Related Code Files

**To read**
- `Cargo.toml` — members `:3-253`, `[workspace.dependencies]` `:260-493`, `[patch.crates-io]` `:872-880`
- `.cargo/config.toml`, `script/clippy`, `script/crate-dep-graph`, `rust-toolchain.toml`

**To create**
- `research/baseline-actions.txt`, `research/survivor-fix-order.txt`
- `research/baseline-test-names.txt`, `research/baseline-tests.json`, `research/baseline-metrics.md`

**To modify:** none.

## Implementation Steps

1. Branch off a clean `main`: `git switch -c fork/remove-auth-and-cloud`.
2. Verify the baseline is green and record timings:
   ```sh
   time cargo check --workspace 2>&1 | tail -5
   time cargo build 2>&1 | tail -5          # default-members = crates/zed only
   ls -la target/debug/zed
   ```
3. Capture the pre-deletion action list — **Phase 9 depends on this**:
   ```sh
   cargo run -p zed -- --dump-all-actions > research/baseline-actions.txt
   ```
   Emitter lives at `crates/zed/src/main.rs:1882-1925`.
4. Generate the dependency graph, then derive the reverse-topological order for the 17 survivors **from `cargo metadata` JSON, not by reading the SVG**. Expected shape (leaves first):
   `settings_content → cloud_api_types → edit_prediction_types → remote_connection → remote_server → notifications → activity_indicator → diagnostics → language_tools → project → file_finder → client → workspace → onboarding → title_bar → git_ui → settings_ui → zed`
5. Snapshot the test baseline so Phase 10/11 can separate "broken by us" from "already failing".
   **Capture per-test names, not a summary tail** (red team finding 7). An earlier draft used `| tail -40`, which keeps only the trailing aggregate — useless for diffing once 53 crates' tests simply cease to exist rather than fail. Aggregate counts would move for reasons unrelated to any regression, and a real failure inside a surviving crate could hide in the shift.
   ```sh
   cargo install cargo-nextest --locked   # if absent
   cargo nextest list --workspace > research/baseline-test-names.txt
   cargo nextest run --workspace --no-fail-fast --message-format libtest-json \
     > research/baseline-tests.json 2> research/baseline-tests.log
   ```
   Phase 10 diffs **test names**, so "this test disappeared with its crate" and "this test started failing" are distinguishable.
   The repo documents nextest as the fix for `Too many open files` (`docs/src/development/macos.md:167-172`).
6. Record a clean `cargo machete` baseline so Phase 11 findings are attributable to this work.
7. Re-run `research/final-delete-set.py`; commit its output as the authoritative Phase 4 list.
8. Write `baseline-metrics.md`: check time, build time, binary size, **exact counts**, test counts, machete findings.
   Count precisely — the plan's headline figure conflated two things (red team finding 17):
   ```sh
   ls -d crates/*/ | wc -l                                    # 232 crate directories
   sed -n '/^members = \[/,/^\]/p' Cargo.toml | grep -c '"'    # 239 workspace members
   cargo metadata --no-deps --format-version 1 | jq '.packages|length'   # 240 packages
   ```
   The extra members are `extensions/` (4) and `tooling/` (3). All 53 deletions are under `crates/`.

## Todo List

- [ ] Branch `fork/remove-auth-and-cloud` created from clean `main`
- [ ] `cargo check --workspace` green, time recorded
- [ ] `cargo build` green, binary size recorded
- [ ] `baseline-actions.txt` captured (**blocks Phase 9**)
- [ ] `survivor-fix-order.txt` written (**blocks Phases 7–8**)
- [ ] `baseline-test-names.txt` + `baseline-tests.json` captured — **per-test names, not a summary tail** (blocks Phases 10–11)
- [ ] `cargo machete` baseline recorded
- [ ] `final-delete-set.py` re-run and committed
- [ ] `baseline-metrics.md` written

## Success Criteria

- Branch exists, `git status` clean.
- All blocking artifacts exist and are non-empty: `baseline-actions.txt`, `survivor-fix-order.txt`, `baseline-test-names.txt`.
- `baseline-metrics.md` records all three count methods, not one conflated number.
- Baseline build is green — **if it is not, stop here.**

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Baseline already red (Metal toolchain, env) | Every later error becomes ambiguous | Hard gate — do not enter Phase 2 |
| `--dump-all-actions` needs a full build | Slow start | Accept; Phase 9 cannot be done correctly without it |
| Topological order eyeballed from the SVG | Wrong fix order downstream | Derive programmatically from `cargo metadata` |

## Security Considerations

Read-only phase. Note for later: this branch will carry removal of crash reporting and telemetry, making the cumulative diff security-relevant — Phase 11 includes a dedicated review pass.

## Next Steps

Phase 2 — delete the visual-test harness: ~4,100 lines and 5 dependency edges removed while the build is still green.
