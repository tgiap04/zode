# Keep-display-awake gets a manual always-on switch

**Date**: 2026-10-16 14:15
**Severity**: Low — a new feature, not an incident
**Component**: `keep_awake` crate, status bar eye icon, command palette, `settings`
**Status**: ongoing — committed on PR #43; no manual check yet

## What Happened

Until now Zode held the display awake only while a tab (agent, task or shell) was producing output. The user asked for a manual on/off that persists across restarts, holds on battery, has no time limit, and toggles from the eye-icon menu and the command palette.

The design is two independent switches. `keep_display_awake` keeps its automatic meaning. The new `keep_display_awake_always` (default false) holds whenever a window is open, regardless of output, battery or the automatic switch. The eye icon shows while either is on, and hiding the row from the status bar's right-click menu turns both off. `Holds::sync` in `crates/keep_awake/src/keep_awake.rs` stays the only place the lock is taken or dropped. Remote control's hold is unchanged.

Committed as `6c38e3e` (feat) and `de612ce` (docs), on `feat.release-v0.1.5`.

## The Brutal Truth

The galling part is that the first review caught a CI break no planned gate would have caught. A new namespace would have failed `cargo test -p zode` in CI, and our gates ran only the `status_bar_items` tests in that crate. The tester's gate run missed it too.

Two more were real bugs: a refused always-on hold was never retried, and two quick toggle presses both wrote "on". The rule that hiding the row turns both switches off had no test, and deleting it left every test green.

The tests cover the switch logic. Nothing has yet watched the OS assertion from outside, so until the macOS check runs we have not seen the display stay awake.

## Technical Details

- `test_action_namespaces` (`crates/zed/src/zed.rs`) asserts the exact namespace set. `keep_awake` was missing.
- `cargo fmt --check` exited 1 on three hunks. Fixed.
- Refused hold: the power-check timer ran only while a tab produced output. With no tab running, nothing asked the OS again. Realistic case from review: Linux at login, before the session bus answers.
- Toggle race: `toggle_always_on` read the switch when called, not inside the queued settings write.
- Fix: a `power_check_wanted` predicate (a tab producing output, or always on with no lock), shared by `settled` and the timer loop. `KeepAwake::new` calls `settled`, because the status bar drops `KeepAwake` when its row is hidden and rebuilds it on show. The refusal warning now logs once per run of refusals.
- `status_bar."experimental.show": false` hides the icon while an always-on hold continues. Decided: document it, not force the bar visible.
- Planner found a `build_menu` comment calling `toggleable_entry`'s fourth argument an icon. It is an action, drawn only as its key binding; the comment is corrected in `6c38e3e`.
- Mutation checks: five mutants, each failing its named tests (sync dropped in `new`, `always` ignored in `sync`, Unsupported arm dropped in `status`, always leg dropped from `power_check_wanted`, toggle reverted to call-time read).
- Tests added: `hiding_keep_awake_turns_off_the_always_on_switch_too` (zode), `two_quick_toggles_cancel_out`.
- Gates, all exit 0: `cargo test -p keep_awake` 45 passed, twice (7 ignored, pre-existing real-shell tests); `-p zode status_bar_items` 7; `-p zode test_action_namespaces` 1; `-p remote_control` 273; `-p settings` 31; `./script/clippy`; `cargo fmt --check`; `./script/prettier`. Tester repeated keep_awake, status_bar_items and remote_control 10 runs each, all green.

## What We Tried

- **Planned gates, `status_bar_items` tests only:** did not catch the missing namespace. The first review did.
- **Power-check timer tied to tab output:** served no always-on hold once no tab was running.
- **Toggle reading state at call time:** two quick presses both wrote "on".

## Root Cause Analysis

- **Namespace:** the planned gate for the `zode` crate was filtered to the module we changed, `status_bar_items`. The exact-set test lives in `zed.rs` of the same crate, outside that filter.
- **Refused hold:** the timer's lifetime was tied to one reason to hold, tab output. Always-on arrived as a second reason, and the timer never served it.
- **Toggle:** state was read before the queued write, so the write carried a stale value.
- **Hide rule:** no test pinned it, so deleting it went unnoticed.

## Lessons Learned

- A new `actions!` namespace has a test elsewhere (`test_action_namespaces`) that a gate filtered to the changed module never runs. When you add one, run that test by name.
- A timer whose lifetime is tied to one reason to hold silently stops serving a second reason added later. Key it off a predicate that covers every reason, as `power_check_wanted` does.
- A toggle that reads state before a queued write inverts a stale value. Read inside the write.

## Next Steps

- **User:** manual check on macOS: palette action, menu tick, `pmset -g assertions`, restart with the switch on. Not done. No deadline recorded.
- **Owner not recorded:** Linux and Windows were not built here. On Linux, watch the refused-hold case at login.
- **Accepted debt:** if always is turned off while a tab still holds the lock, the OS assertion keeps its reason string. Per the existing rule, do not recreate the assertion just to refresh the name.
