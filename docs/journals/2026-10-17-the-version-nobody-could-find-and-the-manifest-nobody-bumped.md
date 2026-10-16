# The version lived only in the About window, and the manifest sat at 0.1.1

**Date**: 2026-10-17 01:40
**Severity**: Medium — a CI-red test was pushed in the previous change and found here.
**Component**: `release_channel` (`AppVersion`), `crates/zed` version surfaces, release workflow
**Status**: ongoing — committed on PR #43; no manual check yet.

## What Happened

The user could not find the IDE's version anywhere in the app. They asked for it to be shown, for `crates/zed/Cargo.toml` to be bumped to 0.1.5, and for the number to follow each new release tag instead of being edited by hand.

The only version on screen was the About window (macOS "Zode > About Zode", or the command palette). Its headline used `env!("CARGO_PKG_VERSION")` and its "Version" line used `AppVersion::global`. Release builds report the tag, so on a release the headline named the stale manifest (0.1.1, while tags had reached v0.1.4) and the line under it named the tag.

Shipped on PR #43 (`feat.release-v0.1.5`): the version now shows in four places (status bar, rail account menu, Help menu, Welcome page). A pushed stable tag makes the release workflow commit the bump to develop. Prerelease tags are skipped.

## The Brutal Truth

The galling part is that the About window, the only place to look, gave two different versions on a release. Honestly it stings that the user had to ask where the number was.

The sharper sting is the test. `cargo test -p zode` had been red on this branch since 6c38e3e, the previous change. Its gates ran only the `status_bar_items` and `test_action_namespaces` filters, and never the whole crate. The tester's whole-crate run here is what caught it.

## Technical Details

- Headline: `release_channel::AppVersion::display` (major.minor.patch plus any prerelease, no build metadata). Release path: `crates/zed/build.rs` turns `RELEASE_VERSION` into `ZODE_APP_VERSION`; `main.rs` passes it to `AppVersion::load`.
- Surfaces: status-bar item `v0.1.5` opens About, hidden via `status_bar.app_version_button`; "About Zode" in Help; "About Zode (v0.1.5)" at the end of the rail account menu (signed in or offline only); "Version 0.1.5" on Welcome.
- Job `bump_manifest_on_develop` with `script/sync-zode-manifest-to-release-tag`: numeric compare without bash arithmetic; refuses prerelease, equal and older tags; edits Cargo.toml and Cargo.lock together or not at all; timeout 10 minutes.
- Red test: `init_list_tests::every_ui_crate_the_harness_initialises_is_also_initialised_by_the_app` flagged `keep_awake`. A false positive from a source-text scan: `keep_awake::init` runs inside `initialize_workspace`, which main.rs calls. Fixed by listing it in `ALLOWED_TO_DIFFER` beside `agent_notify` and `remote_control` (8d5ea63).
- Review fixes: `StatusBar::render_right_tools` uses `.rev()`, so an item added last sat at the left edge of the right group while its comment said far right. It is now added first; the remote-control indicator, when on, sits right of it. Also: rustfmt failed in `status_bar_items.rs`, clippy failed on a field read only by tests, and tests for numbers over 18 digits were added.
- Pre-existing, unrelated: `multi_workspace_tests::test_hibernate_after_ms_zero_disables_hibernation` fails on clean HEAD and on develop.
- Gates, all exit 0: `cargo test -p zode` 71 passed, twice; `-p xtask` 13; `-p release_channel` 2; `-p sidebar` 76; `-p settings_ui` 15; `-p agent_usage` 86; `-p keep_awake` 45; `cargo update --locked --workspace`; `cargo xtask workflows` (only release.yml changed); shellcheck; `./script/clippy`; `cargo fmt --check`; `./script/prettier`.
- Commits on PR #43: `8d5ea63` test, `b6c353c` bump, `fb45857` surfaces, `cc148a8` auto-bump, `accb6f7` docs.

## What We Tried

- **`verified-bot-commit` for the auto-bump:** the action the existing bump workflow pins. Reading it, the planner found it takes the parent commit from the remote branch at call time but the file contents from the job's local checkout, and it updates without force, so a push landing mid-job could be silently overwritten. Set aside. The new job runs a plain `git push origin HEAD:refs/heads/develop`; if develop moved, the push is rejected and the job can be re-run.
- **Version item registered last:** it rendered at the left edge of the right group. Moved to first.
- **Filtered gates on the previous change:** did not catch the red test.
- **`cargo xtask workflows`:** a planner memory note forbade running it here after an earlier run blanked two YAML files. That cause was already fixed, so it ran, and only release.yml changed.

## Root Cause Analysis

- **Drift:** two sources answered one question. The compile-time manifest (`CARGO_PKG_VERSION`) and the runtime tag (`AppVersion::global`) both appeared on the same window, and nothing moved the manifest when a release was cut straight from a tag, so it fell behind the tags.
- **Red test:** the gate was filtered to the changed modules, and `init_list_tests` sits in the same crate outside that filter. The scan matches source text, not call paths, so it flagged `keep_awake` even though its init is reached through `initialize_workspace`.
- **Status bar:** the code assumed add-order is visual order. `render_right_tools` reverses it, and the comment described the opposite.
- **Overwrite hazard:** it sits in the pinned action's contract. The parent commit comes from the remote at call time, the content from the local checkout, and nothing forces the push.

## Lessons Learned

- A test filtered to one module of `crates/zed` misses that crate's source-scan tests. Run the whole `-p zode` suite when you touch it.
- Check how a container renders before trusting "added last means rightmost".
- An action that commits from a local checkout onto a remote ref it reads at call time can overwrite a concurrent push. A plain non-force push fails instead.

## Next Steps

- **Manual check, not done:** run with `ZED_APP_VERSION=9.9.9`; all four surfaces and the About headline should say 9.9.9. Owner not recorded.
- **First run to watch:** the job runs from the tagged commit's workflow file, so v0.1.5's own run will skip because develop is already 0.1.5. The first real bump is v0.1.6.
- **Unverified path:** the script's bash 5 path was not run locally; only bash 3.2 was. Owner not recorded.
- **Docs gap:** the repo has no release-process page. The auto-bump is described only in the script header and the xtask job comments, and the header holds the mistaken-tag repair (revert the bump commit on develop).
