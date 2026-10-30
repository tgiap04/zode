# The update shipped the new icon, and macOS kept the old one

**Date**: 2026-10-23 02:37
**Severity**: Medium — the installed app was correct, but the user saw the old icon on every surface after updating
**Component**: auto_update (macOS in-place install), app bundle icon refresh
**Status**: ongoing — fix written and tested, uncommitted on `feat.release-v0.1.6`; one open consequence for older installs

## What Happened

After the auto-update to 0.1.5, the release that first shipped the new orange icon (`7d5aec1`), the Dock, Finder/Launchpad and Cmd+Tab all still showed the previous icon: dark background, orange Z. The user named all three surfaces.

The fix moves the copy step into `copy_app_bundle_over_running_app` and touches the bundle afterwards. Not committed yet; the user chose to wait.

## The Brutal Truth

The bundle on disk was right the whole time. `/Applications/Zode.app` reported 0.1.5 and its `Zode.icns` rendered the new mark. The galling part is that every check that looked at the file passed, and the user still saw the old icon. The update was "correct" and wrong at once.

## Technical Details

- `install_release_macos` runs `rsync -av --delete --exclude Icon?` over the running bundle. `-a` keeps the source's mtime, so the installed bundle carried **2026-10-22 01:37:25** (the CI build time), while the install happened at **06:15**.
- The Dock had been running since 2026-10-09. Its `com.apple.dock.iconcache` was last written 2026-10-21 19:23, before the update.
- LaunchServices re-registered the bundle at 06:15 with build 0.1.5. A fresh process calling `NSWorkspace.icon(forFile:)` got the new icon.
- Experiment on the user's machine: `touch /Applications/Zode.app` alone, then quit and relaunch Zode. All three surfaces showed the new icon. No `lsregister` needed.
- Sparkle does the same after an install: `SUPlainInstaller.m` calls `updateModificationAndAccessTimeOfItemAtURL:` with the comment "Register the new bundle with LaunchServices and the system".
- Regression test `test_copying_an_update_marks_the_running_bundle_modified` (macOS only). Source mtime is pinned to 946684800. Without the touch: `the running bundle kept the update's build-time mtime SystemTime { tv_sec: 946684800, tv_nsec: 0 }`.
- Fixture trap: "new" and "old" were both 3 bytes, written in the same second. rsync's quick check skipped the file, so the first RED run failed on the content assertion. The contents now differ in length.
- Final code: new test passes; `cargo test -p auto_update` 13/13; rustfmt and `./script/clippy` clean; review 9/10 with no critical, high or medium findings.

## What We Tried

- **Touch after rsync.** The experiment confirmed it. The touch failure is logged with `log::error!` and the install still returns `Ok(None)`, since the files are already in place.
- **LaunchServices call.** Not added; the touch experiment showed it was not needed.

## Root Cause Analysis

The copy preserved the CI build's mtime, so the bundle ended up dated earlier than the moment it was installed, and the Dock, Finder and Cmd+Tab kept their cached icon. Setting the mtime to now made all three refresh. Which cache checks which timestamp was not established; the experiment only shows that a current mtime is enough. Sparkle touches the new bundle after every install; our in-place rsync did not.

## Lessons Learned

- An in-place rsync update on macOS needs an explicit `touch` of the bundle. Preserved mtimes hide the change.
- A test fixture whose files match in size and mtime gets skipped by rsync. Make the contents differ in length.
- A RED run is not a temper result. The evidence gate rejected `status: pass` with exit code 101; the RED run now lives in its own file.
- A fix inside an updater only reaches installs made by the version that contains it.

## Next Steps

- **Open, not resolved:** the install step runs in the version being replaced. An update to 0.1.6 is installed by the user's current updater, which has no touch. Anyone on 0.1.4 or earlier who updates straight to 0.1.6 goes through the old updater; whether their icon refreshes was not tested. Option under consideration, not decided: the app touches its own bundle once on the first launch after an update.
- **Low, deferred:** the touch failure should log at `warn`, not `error`, since it is harmless.
- **Low, deferred:** the existing `.with_context(|| "failed to rsync: {cmd}")` is a plain string, so `{cmd}` is never filled in.
- **Not verified:** Linux and Windows builds (cross-target check blocked here; CI only). Windows is unaffected: its Inno Setup script already sets `ChangesAssociations=true`. A real DMG install end to end has not been run.
- **Owner:** user to decide on committing and on the first-launch touch.
