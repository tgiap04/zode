# In-App Updater from GitHub Releases — A Falsified Design Inference, and Two Traps That Bite Silently

**Date**: 2026-08-24 21:40
**Severity**: medium
**Component**: `auto_update`, `auto_update_ui`, `auto_update_helper`, version stamping, release pipeline
**Status**: shipped to PR, verification incomplete by design

## What Happened

Two sessions ran back to back. The first closed out the release-pipeline plan (`260812-2207`); the second built the in-app updater on top of it (`260824-1914`). Both landed on `feat.release-v0.1.2`, which now carries seven commits into PR #22.

The updater request was simple on its face — "a button in the IDE that updates the app". The interesting part was that the answer already existed in this repository's own history: `crates/auto_update` (1544 lines) and `crates/auto_update_ui` (432 lines) were deleted in `c3e2ac3` and were recoverable intact. The work was not writing an updater; it was deciding what to keep, proving the parts that could not be kept, and being honest about what never got run.

**Delivered:** version stamped from the release tag at compile time; `auto_update` restored and retargeted at `/repos/OWNER/REPO/releases/latest`; `auto_update_ui` restored minus its announcement machinery; `auto_update_helper` restored for the Windows path; four UI surfaces wired; three documentation claims corrected.

**Metrics:** 7 tests green (4 version comparisons kept, 2 new for `asset_name`, 1 rewritten end-to-end). `./script/clippy`, `cargo-machete` and `typos` clean. `cargo xtask workflows` byte-stable. One dependency direction checked for a cycle that did not exist.

## The Brutal Truth

**The plan I wrote was wrong in six places, and one of those errors would have wasted somebody's afternoon.** Phase 06 prescribed `ZED_APP_VERSION=0.1.0 ./zode` as the way to test the updater without cutting a release. It does not work, for two reasons the plan never looked at: `check()` returns early unless `ReleaseChannel::poll_for_updates()` is true, and that is `!matches!(self, Dev)` — a local build reads `dev` from `crates/zed/RELEASE_CHANNEL`. And `ZED_RELEASE_CHANNEL`, the override that would fix that, only applies under `cfg!(debug_assertions)`, so a `--release` build ignores it entirely. The working command is a debug build with both variables set. A plan that hands someone a command which silently does nothing is worse than a plan that omits the command.

**"Keep the 15 version tests" was incoherent with "cut the nightly branch"** and I did not notice while writing it. Eight of those fifteen tested `VersionCheckType::Sha`, which only the nightly path used. Cutting the path left them testing nothing. They were deleted, not kept — and the plan now says so rather than leaving a number nobody can reconcile.

**The repo already had a GitHub release helper and the plan did not know.** `http_client::github::latest_github_release` exists. Reusing it would have been wrong in a way that would not have shown up in any test: it reads the `/releases` *list* rather than `/releases/latest`, and it attaches an ambient `GITHUB_TOKEN` when one is exported. Both together mean that on a developer machine with a token in the shell, **unpublished draft releases become visible to the updater** — destroying the exact property the whole release flow leans on ("keep releases as drafts, publish by hand"). The local `github_get` sends no credential at all. This is the one place where not reusing existing code was the correct call, and it needed a comment saying why, because the next reader's instinct will be to consolidate them.

**A behaviour change got made that is easy to describe as a cleanup and is not one.** Dropping the nightly branch means a Nightly-channel install is now offered the newest *stable* release, because `/releases/latest` passes over prereleases. That is a defensible outcome for a manual-only updater and it is still a change in what a user experiences, so it is recorded as one.

## Technical Details

### The version-stamping trap, and why `rerun-if-env-changed` is load-bearing

`script/determine-release-channel` writes `RELEASE_VERSION` from the git tag into the job environment before every bundling step. Nothing read it. The app took its version from `CARGO_PKG_VERSION`, and that same script's comments state outright that the tag is *not* required to match `Cargo.toml`.

Harmless while nothing compares the two. The moment a version check exists, it becomes a loop: the check sees a newer tag, downloads 106 MB, installs, and finds the same old number still reported. Then does it again.

`crates/zed/build.rs` now emits `ZODE_APP_VERSION` from `RELEASE_VERSION`. The accompanying `cargo:rerun-if-env-changed=RELEASE_VERSION` is not defensive garnish: `option_env!` reads the environment at compile time and **cargo does not track that as a dependency**, so with sccache warm — which CI has — a rebuild keeps whichever value was baked in first. Proven rather than assumed: built with `9.9.9`, confirmed the binary reported `9.9.9`; rebuilt with `8.8.8`, confirmed it reported `8.8.8`; built with the variable unset, confirmed the fallback to `0.1.1`.

`crates/cli/src/main.rs:886` reads `option_env!("RELEASE_VERSION")` today with no such directive. That is a latent instance of the same bug, left alone as out of scope and raised as a rules suggestion instead.

### `cargo xtask workflows` must run in the debug profile

Found the hard way, by the reviewer, not by reading. `tooling/xtask/src/tasks/workflows/steps.rs:547-561` derives each generated step's name from a **runtime backtrace**. A release build's inlining makes those frames unresolvable, every name becomes `<unknown>`, and the generated YAML is quietly gutted — two unrelated workflow files (`run_cron_unit_evals.yml`, `run_unit_evals.yml`) had all their job and step names blanked before anyone noticed. Restored from `git show HEAD:<path>`, verified against a clean debug rerun.

Nothing about the failure announces itself. The command exits zero.

### The Windows path is code, not verified behaviour

Windows will not let a running `.exe` be overwritten, so the app hands off to `tools\auto_update_helper.exe` on quit and the helper replaces the app. That crate was deleted too — eight files including a UAC manifest and an icon — and so was the installer wiring: `zed.iss` created a `tools` directory and put nothing in it. Restoring the code without the wiring would have produced an update path that fails at its final step, so both landed in one commit.

`cleanup_windows` also had to be rehomed. It cleared the `updates`, `install` and `old` directories a previous helper run could not delete, and it hung off the head of the polling loop that no longer exists; it now runs once at init.

What was verified: `auto_update_helper` type-checks for `x86_64-pc-windows-msvc`, which proves the restored crate is not stale against `windows` 0.61. The path contract between the two crates (`updates`/`install`/`old`, and `tools/auto_update_helper.exe`) was read on both sides rather than assumed.

What was not: anything at all. `auto_update`'s own `cfg(windows)` blocks **have never been read by a compiler**, because gpui pulls in `psm` and `stacker`, whose build scripts need an MSVC C toolchain absent on this machine. UAC, file locking, the quit hand-off, and whether the helper actually ships inside a real installer build are all untested.

### A failed experiment worth recording

The terminal test race fixed in the first session (`test_basic_terminal` asserting on `get_content()` the moment the child process exited — two unordered moments) was only ever observed failing under a full-workspace `nextest` run on Linux CI. An attempt to reproduce it locally **failed**: the pre-fix test passed 25 out of 25 in a CPU-constrained Ubuntu 24.04 container, and the full terminal suite passed there too. That container is therefore not a reproduction, and the post-fix pass in it proves nothing about the race. Whole-workspace load looks like the trigger rather than the platform. Recorded in the phase file so nobody repeats the same three-hour detour.

### Two documentation claims that the feature falsified

`docs/system/design-intent.md` inferred the motive behind `c3e2ac3` from a pattern: every removed subsystem needed "a hosted backend or a vendor account". Auto-update was on that list and did not — it needs a public release page, which this repository already had. The inference still holds for collab, auth, the cloud API, AI and crash reporting. It is recorded as falsified rather than quietly edited, because a design-intent document's value is that its reasoning stays inspectable, including where the reasoning failed.

The README told readers there was no in-app updater. The generated release notes said the same. Both were fixed; `docs/src/development/linux.md`'s documentation of `ZED_UPDATE_EXPLANATION` was checked and left alone, because that escape hatch still works and packagers still need it.

## Lessons

1. **Search the repo for the capability before designing around its absence.** `http_client::github` existed and the plan never looked. Here reuse happened to be the wrong choice, but that was luck: the decision should have been made deliberately at planning time, not discovered mid-implementation.
2. **A test affordance in a plan must be executed once before it is written down.** The `ZED_APP_VERSION` command looked obviously correct and was gated twice over.
3. **When a plan says "keep N tests" and also "cut feature X", check whether those tests test X.** Two clauses written minutes apart contradicted each other.
4. **`option_env!` needs `cargo:rerun-if-env-changed`, always.** Cargo will not tell you. sccache will happily serve the stale value.
5. **A green exit code is not evidence a generator worked.** `cargo xtask workflows --release` gutted two files and exited zero.
6. **Report the failed reproduction, not just the fix.** A passing test in an environment that cannot reproduce the bug is not evidence, and calling it evidence is how an unfixed race ships.

## Suggested .rules additions

Both meet the repo's bar — non-obvious, encountered in practice this session, specific enough to act on:

- `cargo xtask workflows` must run in the **debug** profile. `steps.rs` derives step names from a runtime backtrace; a release build silently produces workflow files with every job and step name replaced by `<unknown>`, and exits zero.
- Any build-time value reaching code through `option_env!` requires `cargo:rerun-if-env-changed=<VAR>` in the corresponding `build.rs`. Cargo does not track `option_env!` as a dependency, so a warm sccache serves the previously baked value. Live example lacking it: `crates/cli/src/main.rs:886`.

## Open

- Nobody has pressed the button in a running app. The "already current" path is safe to exercise (it downloads nothing); the update path overwrites `/Applications/Zode.app`, so it was left to the maintainer.
- `install_release_macos` and `MacOsUnmounter` have never run for real — the integration test routes around them through an `InstallOverride`.
- Linux and Windows: compiled only.
- `auto_update.rs` is 1193 lines against the repo's 200-line guidance, and `auto_update_helper` carries several restored-from-upstream `let _ =` discards. Both were left alone deliberately: splitting and rewriting code that has never executed on two of its three platforms adds risk instead of removing it. Worth a pass once a Windows machine exists.
