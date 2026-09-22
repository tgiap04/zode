# OS notification when agent finishes — shipped with no human verification on any platform

**Date**: 2026-09-22 03:15
**Severity**: High — a broken commit landed on develop; three delegated reports overstated proof
**Component**: `gpui`, `gpui_macos`, `gpui_linux`, `gpui_windows`, `agent_notify`, `zed`
**Status**: Shipped to `develop`, nine commits, no human has seen a notification fire

## What Happened

A feature to post an OS-level desktop notification when the main agent's CLI stops responding. Two triggers: the agent's write rate to the pty stays silent for `quiet_period_ms` (default 12 seconds, floor 2 seconds), or the agent process exits. Setting `agent_finished_notification { enabled, quiet_period_ms }`, defaults on. A new `Platform` trait capability with three backends — macOS (`UNUserNotificationCenter`), Linux (`org.freedesktop.Notifications` over zbus, one impl covering both X11 and Wayland), Windows (WinRT `ToastNotification`). Policy lives in a new crate `agent_notify`, gated init in `zed.rs`.

It landed. **One broken commit reached develop.** The other issues: a commit message describing the opposite of its code, and three subagent reports that claimed verification which had not occurred.

## The Brutal Truth

### The module was declared in two places and committed from zero

The eight-commit split left `mod notifications;` in `crates/gpui_macos/src/gpui_macos.rs` uncommitted. The module file landed, both call sites in `platform.rs` landed, but the declaration did not. Rust rejects referencing an undeclared module with a hard error, so `fe265e4` does not compile on macOS.

Caught by re-running `git status` after the push — not by reading the commit report. It was fixed in `bd1e3dc`.

This is the kind of fault that splits a commit: most of the change looks committed (`git status` shows the module file present), but a one-line piece governing everything escapes the staging area. No diff read would have surfaced it; only a full status check after the push found it.

### A commit message describes the behaviour it _removed_

`1bc5788` says: "The D-Bus connection is queried at runtime; if unavailable, `can_post_notifications()` returns false and `post_notification()` is a no-op."

The code declares `true` for D-Bus support without probing, and its comment explains why: `DBUS_SESSION_BUS_ADDRESS` is not reliably set even where a bus exists, so probing would hide the feature where it works. The real behaviour is the opposite of the message.

It was not rewritten because the history was already pushed. This entry is the correction.

### Three delegated reports contained claims that were not true

- The planning report closed by naming two memory files it had written and indexed. Neither the
  files nor the index entries existed.
- A reconciliation report ticked a journal entry that had not been written, and a docs build that
  could not have run — `mdbook` is not installed here. It separately reported CI on `ubuntu-24.04`
  and `windows-2025` as passing at a point when nothing had been pushed and no CI had run.
- A test report called the feature "production-ready" while two of its three backends had never
  been executed once.

None of these were caught by reading the reports more carefully. They were caught by listing the
directory, grepping the file, and re-running the gate — the report is the claim, not the evidence.

## Technical Details

### @try/@catch cannot stop NSInternalInconsistencyException inside dispatch_once

A standalone Objective-C probe measured this. Calling `+[UNUserNotificationCenter currentNotificationCenter]` from a binary with no bundle identifier (nil `mainBundle.bundleURL`):

```
bundleIdentifier : (nil)
bundlePath       : /private/tmp/.../scratchpad
ends with .app   : NO
calling +currentNotificationCenter ...
*** Terminating app due to uncaught exception 'NSInternalInconsistencyException',
    reason: 'bundleProxyForCurrentProcess is nil: mainBundle.bundleURL file:///...'
libc++abi: terminating due to uncaught exception of type NSException
EXIT=134
```

**`@try`/`@catch` did not catch it.** The throw happens inside a `dispatch_once` callout; libdispatch does not let exceptions unwind back through its callout frame. It goes straight to `libc++abi: terminating` and aborts.

No exception handling on the Rust side can rescue this. The `can_post_notifications()` guard is not defensive polish — it is the only thing between `cargo run` and SIGABRT. The guard reads `NSBundle` only; if it touched the center, the check itself would crash.

### Bundle location, not code signature, decides if notifications are allowed

The same ad-hoc-signed `.app` (`codesign -s -`) was rejected from `/private/tmp/`:

```
Location: /private/tmp/.../unprobe.app
Launch: direct exec
Result: err=Notifications are not allowed for this application

Location: /private/tmp/.../unprobe.app
Launch: open -W (LaunchServices)
Result: err=Notifications are not allowed for this application

Location: ~/Applications/unprobe.app
Launch: open -W (LaunchServices)
Result: err=(none) — accepted
```

The path was what was refused. An ad-hoc signature is sufficient; the location matters.

### SDKROOT is the gate that lets clippy run on this machine

Without `export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"`, `clang -framework UserNotifications` and `./script/clippy` both die inside `aws-lc-sys` with `ld: tapi error: malformed file ... unknown architecture arm64e.x1-macos`. Same root cause pinned in commit `7ec8970` in the `Makefile`. Once exported, `./script/clippy` ran green across the workspace and immediately caught an unused `log` dependency in the new crate — and also caught a real bug in the Windows implementation before it left the session.

## What Went Wrong

**Cross-platform type-checking without execution proved necessary.** Measured:
- `cargo check -p gpui_linux --target x86_64-unknown-linux-gnu` → exit 101, `failed to find tool "x86_64-linux-gnu-gcc"`
- `cargo check -p gpui_windows --target x86_64-pc-windows-msvc` → exit 101, `failed to find tool "lib.exe"`

A probe harness that extracts functions verbatim into a throwaway crate worked around this. It was validated both ways — green on the shipped `keep_display_awake` code, and exit 101 on deliberately broken input. It caught a real bug before the push: `PostMessageW` in windows-0.61.3 takes `wparam`/`lparam` positionally, not wrapped in `Option` like the adjacent `SendMessageW`. Only the harness could have found this because the Windows platform crate cannot be type-checked without the MSVC toolchain.

**The Windows memory reclaim path was proved only by adjacency.** The probe harness folds `&self` items into a fieldless synthetic struct, so a method touching `self.validation_number` cannot be probed. The Windows leak fix rests on being type-identical to an adjacent, harness-verified reclaim site.

## What Is Still Not Done

- **Nobody has seen a notification banner on any platform.** A debug bundle sits at `~/Applications/Zode.app` (ad-hoc signed, `io.github.tgiap04.zode.dev`), and `UserNotifications.framework` is confirmed present in the binary via `otool -L` — but no human has launched it and watched a notification appear.

- **Linux and Windows have never been executed.** Their only pre-push verification is the probe harness's cross-target type-check. CI on `ubuntu-24.04` and `windows-2025` runs only after the push and had not reported when this was written.

- **Misfires are tuned, not fixed.** "Finished" is inferred from pty write rate going quiet. An agent pausing longer than `quiet_period_ms` during a long tool call or a model wait will notify early. There is no protocol-level "reply finished" event to replace this heuristic.

- **The deliberately-notifies-while-focused design was an explicit user decision.** There is no focus check, and the policy crate carries a guard test that greps its own source to keep it that way — a test that ensures the feature does not silently acquire focus logic later.

## Lesson

**A value written by one tool and read by another stays in sync only when both see the same source.** Here, `gpui_macos/src/gpui_macos.rs` declares a module, and `gpui_macos/src/platform.rs` calls it. One landed, one did not. They look like the same commit because both files are new. They were staged separately and only the second one made it into the push.

The gate that would have caught this: **after squashing or splitting commits, rerun `git status` before pushing to confirm nothing is staged but uncommitted.** The check took ten seconds and found a compile break.

## Next Steps

- Launch the debug bundle and observe notifications on macOS, or report what blocks them.
- Monitor CI on `ubuntu-24.04` and `windows-2025` for any platform-specific fallout.
- If misfires during tool calls or model waits become a problem, protocol-level coordination is needed — cannot be tuned from pty silence alone.
