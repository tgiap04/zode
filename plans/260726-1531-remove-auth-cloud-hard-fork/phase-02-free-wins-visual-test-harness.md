---
phase: 2
title: Free wins - visual test harness
status: completed
effort: 0.5d
---

# Phase 2: Free wins - visual test harness

## Context Links

- [`crates/zed` wiring map](./reports/scout-zed-wiring.md) §6 — binary targets
- [plan.md](./plan.md)

## Overview

**Priority:** P1 · **Build state:** GREEN (must stay green) · **Depends on:** Phase 1

Delete the macOS visual-regression test harness. This is the single highest value-to-risk action in the whole plan: **~4,100 lines and 5 dependency edges disappear for free**, while the build stays green and everything is independently revertible.

Doing this first shrinks the Phase 4 blast radius before the build ever goes red.

## Key Insights

- `crates/zed/src/visual_test_runner.rs` is **3,589 lines** and saturated with delete-set imports: `acp_thread::{AgentConnection, StubAgentConnection}` `:97`, `agent_servers::{AgentServer, AgentServerDelegate}` `:99`, `call::init` `:186`, `language_model::init` `:204`, `RefreshLlmTokenListener::register` `:205`, `language_models::init` `:210`, `agent_ui::init` `:217`, `agent_settings::AgentSettings::override_global` `:236`, `agent::{AgentTool, ToolInput, ReadFileTool, …}` `:2008-2062`, `:2686-2720`.
- Salvaging it means rewriting ~40% of a screenshot-diff harness. Not worth it for a fork that is deleting the features it screenshots.
- Deleting it removes the `acp_thread`, `action_log`, `agent_servers`, `clock`, `tempfile` edges from `crates/zed` **at zero cost**.
- Baseline images are already gitignored (`docs/src/development/macos.md:82-84`) — nothing committed is lost.
- **⚠ Correction found during execution: `.github/workflows/*.yml` are GENERATED, not hand-written.** `run_tests.yml:1-2` says *"Generated from xtask::workflows::run_tests / Rebuild with `cargo xtask workflows`"*. Worse, `run_tests.yml:730-733` contains a CI step that runs `cargo xtask workflows` and **fails the build if `.github` has uncommitted changes afterwards**. So hand-editing the YAML is not merely fragile — it is a guaranteed CI failure.

  The edit belongs in `tooling/xtask/src/tasks/workflows/run_tests.rs`:
  - `:76-78` — the `.then(build_visual_tests_binary())` job registration
  - `:623-637` — the `build_visual_tests_binary()` fn itself

  Then run `cargo xtask workflows` and commit the regenerated YAML alongside the source change. The generator also owns the `tests_pass` aggregation job, so removing the registration prunes the `needs:`, `check_result`, and `RESULT_*` env wiring automatically — four YAML touch points from one source edit.

## Requirements

**Functional**
- Remove the `zed_visual_test_runner` binary target, its source, its GPUI glue, and the `visual-tests` feature.
- Build must remain green afterwards.

**Non-functional**
- No CI job may reference the removed binary.

## Architecture

Four coupled removal points:

```
Cargo.toml [[bin]] zed_visual_test_runner  (:60-63, required-features = ["visual-tests"])
Cargo.toml [features] visual-tests         (:31-54  → names dep:action_log, dep:agent_servers,
                                                       acp_thread/test-support, agent_ui/test-support,
                                                       agent/test-support, language_model/test-support)
src/visual_test_runner.rs                  (3,589 lines)
src/zed/visual_tests.rs                    (551 lines, only ref: app_state.client.clone() at :88)
                                            └─ declared at zed.rs:11-12 behind #[cfg(feature="visual-tests")]
```

## Related Code Files

**To delete**
- `crates/zed/src/visual_test_runner.rs`
- `crates/zed/src/zed/visual_tests.rs`
- `crates/zed/test_fixtures/visual_tests/` (if present; gitignored)

**To modify**
- `crates/zed/Cargo.toml` — drop `[[bin]]` `:60-63` and `[features] visual-tests` `:31-54`
- `crates/zed/src/zed.rs` — drop `:11-12` module declaration
- `.github/workflows/*` — remove any job invoking the runner
- `docs/src/development/macos.md` — drop the "Visual Regression Tests" section `:61-107`

## Implementation Steps

1. **Check CI first** — this determines whether workflow edits are needed:
   ```sh
   rg -n "visual_test_runner|visual-tests|UPDATE_BASELINE" .github/ script/ docs/
   ```
   **Executed result:** a real job exists — `.github/workflows/run_tests.yml:539-560` (`build_visual_tests_binary`), wired into the `tests_pass` gate at `:805`, `:836`, `:857`. Plus docs at `docs/src/development/macos.md:77-103`.

   Then check whether the YAML is generated (it is — see Key Insights):
   ```sh
   head -2 .github/workflows/run_tests.yml
   ```
2. Delete `crates/zed/src/visual_test_runner.rs` and `crates/zed/src/zed/visual_tests.rs`.
3. `crates/zed/src/zed.rs` — remove the `#[cfg(all(macos, feature = "visual-tests"))] pub mod visual_tests;` block at `:11-12`.
4. `crates/zed/Cargo.toml` — remove the `[[bin]]` block `:60-63` and the whole `visual-tests` feature `:31-54`. Leave `test-support` `:18-30`, `tracy`, `track-project-leak` untouched.
5. Remove the CI job **via the generator, never the YAML**:
   - `tooling/xtask/src/tasks/workflows/run_tests.rs` — drop the `.then(build_visual_tests_binary())` registration and the `build_visual_tests_binary()` fn.
   - `cargo xtask workflows` to regenerate.
   - `git diff .github/` must show `build_visual_tests_binary` and its four wiring points gone. If the diff is empty, the generator edit did not take.
6. Delete the docs section `docs/src/development/macos.md:61-107` and any `SUMMARY.md` reference to it.
7. **Verify green:**
   ```sh
   cargo check --workspace
   cargo check -p zed --all-features    # proves no feature still names the deleted deps
   ```
8. Confirm the dependency edges actually dropped:
   ```sh
   cargo tree -i acp_thread -e normal | grep -c zed    # expect 0 from crates/zed
   cargo tree -i action_log -e normal
   ```
9. Commit alone: `chore: remove macOS visual regression test harness`.

## Todo List

- [ ] CI/script references audited
- [ ] `visual_test_runner.rs` deleted
- [ ] `zed/visual_tests.rs` deleted
- [ ] `zed.rs:11-12` module declaration removed
- [ ] `[[bin]]` target removed from Cargo.toml
- [ ] `visual-tests` feature removed from Cargo.toml
- [ ] CI job removed **in `tooling/xtask/.../run_tests.rs`** (not the YAML)
- [ ] `cargo xtask workflows` run; regenerated YAML committed with the source change
- [ ] `git diff .github/` confirms the job + its 4 wiring points are gone
- [ ] Docs section removed
- [ ] `cargo check --workspace` green
- [ ] `cargo check -p zed --all-features` green
- [ ] Dependency edges confirmed dropped
- [ ] Committed as a standalone commit

## Success Criteria

- `cargo check --workspace` **and** `cargo check -p zed --all-features` both green.
- `crates/zed` no longer has edges to `acp_thread`, `action_log`, `agent_servers`.
- ~4,100 lines removed in a single revertible commit.
- No CI job references `zed_visual_test_runner`.
- `cargo xtask workflows` produces no further diff — i.e. the committed YAML matches the generator, so the `:730-733` CI guard passes.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| A CI job depends on the runner and turns red | Broken pipeline | Step 1 audits before deleting — **one was found** |
| Hand-editing the generated YAML | `cargo xtask workflows` reverts it, and the `:730-733` guard fails CI | Edit the generator; regenerate; commit both |
| `--all-features` still names a deleted dep | Hidden breakage, surfaces much later | Step 7 explicitly runs `--all-features` |
| Losing visual regression coverage | No screenshot diffing for UI changes | Accepted — the fork removes most of the UI it covered; local baselines were gitignored anyway |

## Security Considerations

None — test-only code with no production or network surface.

## Next Steps

Phase 3 — sever the thin ties that let `edit_prediction_types`, `remote_connection`, `remote_server` and friends stay out of the delete set.
