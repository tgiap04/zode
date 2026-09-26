# macOS auto-update broken for every user: volume label mismatch in DMG mount

**Date**: 2026-09-07 18:30
**Severity**: Critical — broke update installation for all macOS users
**Component**: `auto_update`, `auto_update_helper`, `script/bundle-mac`
**Status**: Resolved (commits `6c55182`, `65022e7`)

## What Happened

A user reported "Update failed" with no details. The real story sat in `~/Library/Logs/Zode/Zode.log`:

```
downloaded and verified update. path:".../T/zed-auto-updatezfx2sn/Zed.dmg"
update failed: Failed to install update at: .../Zed.dmg
  failed to copy app: "rsync: .../zed-auto-updatezfx2sn/Zed/Zode.app/: (l)stat: No such file or directory"
Failed to unmount disk image: "hdiutil: detach failed - No such file or directory"
```

The installer downloaded the DMG, verified its checksum correctly, then tried to copy the app from a path that did not exist. After the copy failed, detach also failed, leaving the image mounted. Every update attempt hit this exact same wall.

## The Brutal Truth

This was a **name mismatch** between the bundling script and the installer code. The bundler at `script/bundle-mac:265` creates the DMG with `-volname Zode`. The installer (`install_release_macos` in `crates/auto_update_helper/src/updater.rs`) ran:

```bash
hdiutil attach -mountroot <tmp> <image.dmg>
# then read from <tmp>/Zed
```

`hdiutil attach -mountroot` mounts the volume **by its label** — so Zode mounted at `<tmp>/Zode`, not `<tmp>/Zed`. The installer tried to read from a directory that never existed. Download worked, checksum worked, extraction worked — only the install failed.

This is the **second time in this codebase** this exact fault has occurred. The Windows updater had already been burned by the same pattern: `APP_EXE` was hardcoded to `Zed.exe` while the bundler wrote `Zode.exe`. After the Windows fix landed, I audited Linux and Windows end to end and found them consistent. Linux only avoids the trap because `script/determine-release-channel` writes the channel to a file both the bundler and the binary read, so they cannot diverge the way a hardcoded name can.

## Technical Details

The broken sequence:

1. Bundler runs `hdiutil create -volname Zode -format UDZO` → creates volume labeled "Zode"
2. Installer runs `hdiutil attach -mountroot /tmp <image.dmg>` → mounts at `/tmp/Zode`
3. Installer then tries `rsync -a /tmp/Zed/ <destination>` → path does not exist, rsync fails with `(l)stat: No such file or directory`
4. Detach fails because the mount path was wrong, leaving the image attached
5. User sees "Update failed" with no context

The fix: use `-mountpoint` instead of `-mountroot`:

```bash
hdiutil attach -mountpoint /tmp/zode_mount <image.dmg>
rsync -a /tmp/zode_mount/ <destination>
hdiutil detach /tmp/zode_mount
```

`-mountpoint` names the mount directory outright; no ambiguity about which label gets used. Relevant: `auto_update_helper/src/updater.rs` lines 24–33 carry a doc comment recording the Windows nameplate bug for exactly this reason.

## Hardening Added

Rather than just fixing the mount call, three defensive layers were added to prevent this class from surfacing again:

1. **Fallback for the running app name**: macOS now checks if the bundled app's name matches the running one. If not, it finds the sole `.app` directory in the mounted image and uses that. So if the bundle is named anything, we still find it.

2. **Linux extraction validation**: Before trying to read files from an extracted archive, check that the expected directory exists. Report what was actually found if it does not.

3. **Clear error reporting**: When the app directory is missing, report its path and what the mounted/extracted volume actually contained. A person reading the log knows to check the bundler output.

The user-facing change: the auto-update flow now **stops at `AutoUpdateStatus::UpdateAvailable`** and waits for an explicit Download action (via notification or status-bar button). Previously it fetched ~110 MB automatically. This gives users control and buys time to notice and report download issues before install.

## What We Tried

1. First fix was just the mount path — worked locally with a DMG built exactly as `script/bundle-mac` does.
2. Then added the fallback and validation layers while testing against the real bundled image.

## Root Cause Analysis

The fault is a classic split-brain: **a value produced by one tool, guessed by another, with no compile-time or runtime coupling to keep them in sync.**

The bundler produces a volume label; the installer guesses a directory name. Both humans writing Rust: neither would catch this in a review because the label and the name look like independent decisions.

This exact shape is visible three places in the codebase now:

- Windows: `APP_EXE` hardcoded, bundler writes `Zode.exe` (fixed before)
- macOS: volume label hardcoded to read, bundler writes from the label (this fix)
- Linux: both bundler and binary read from a shared env var written to a file (no bug)

## Lessons Learned

1. **After a fork and rename, audit every hardcoded string in the bundler and the runtime.** The Zed→Zode fork renamed the volume and the app, but the installer code was not updated. A checklist: "Search `zed` (case-insensitive) in all bundler scripts and all auto-update code. Confirm every reference is either parameterized or updated."

2. **When a value is written by one system and read by another, prefer a shared source of truth.** Linux got this right: one file, both systems read it. Windows and macOS hardcoded separately and paid the price.

3. **Test end-to-end with a real artifact.** The installer had a test suite that did not exercise `hdiutil attach`, only the copy logic. A real DMG, a real mount, a real `rsync` — that caught it immediately.

## What Could Not Be Verified

- The fix could not be verified on Windows — no MSVC toolchain locally. `cargo check -p auto_update_helper` passes and no Windows-specific code changed, but CI is the real gate.

## Next Steps

- Monitor CI for any Windows-specific fallout.
- On next Zed/Zode fork or major rename, run the bundler audit checklist against every install path (macOS, Linux, Windows).
- Consider parameterizing the volume label in the DMG creation so it can't drift from the install code.
