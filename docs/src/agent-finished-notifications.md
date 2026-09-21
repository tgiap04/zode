# Agent Finished Notifications

Zode can ask the operating system to post a real desktop notification -- the
OS's own notification UI, not an in-app toast -- when an agent finishes. This
is for the moment you have switched away to something else and want to know
without watching the tab.

This is on by default:

```json
{
  "agent_finished_notification": {
    "enabled": true,
    "quiet_period_ms": 12000
  }
}
```

## The two triggers

- **The agent stops answering.** An agent tab has an "is producing a
  response" edge, and once that edge goes from true to false and stays false
  for `quiet_period_ms`, a notification fires.
- **The agent's CLI process exits.** Independent of the first trigger --
  covers an agent that crashes, is killed, or ends its session outright.

Either one can fire on its own; neither depends on the other.

## The quiet period, and why notifications can fire early

`quiet_period_ms` (default `12000`, floored at `2000`) exists because there is
no better signal available. These agents run as CLIs inside a pty, not as
something Zode speaks a structured protocol to -- there is no protocol-level
"reply finished" event to read. Whether an agent is "still answering" is
inferred from how often its terminal is being written to, and that same
signal goes quiet during a long tool call or while the agent is waiting on a
model response, not only when it is actually done.

**This means a notification can and will fire while the agent is still
working**, whenever it pauses longer than the configured quiet period -- a
slow model response, a long build, a large file read. Raising
`quiet_period_ms` trades promptness for fewer of these early notifications; it
cannot be raised away entirely, because the underlying signal does not carry
more information than this. If you are seeing notifications while an agent is
clearly still going, raise the value rather than filing it as a bug.

## Not suppressed while Zode is focused

Notifications post regardless of window focus, including while a Zode window
is the frontmost app. This is deliberate, not an oversight -- there is no
focus-gating setting. If you only want to be told about a finished agent
while you are elsewhere, put the window in the background yourself; Zode will
not do that judgment for you.

## What a notification carries

The title is the agent tab's own label. The body is a fixed phrase -- never
transcript content, and never anything from the conversation. Notification
history is visible on the lock screen on all three platforms, which is the
reason nothing from the conversation ever reaches the OS layer.

## Platform support

| Platform | Delivery mechanism | Verification |
| -------- | ------------------- | ------------- |
| macOS | `UNUserNotificationCenter` | Bundle-identity, bundle-location, and permission-timing behavior measured directly against the real framework (below) via a standalone probe. **No banner has been visually confirmed by a human** -- an API-level accept is not an observation |
| Linux | `org.freedesktop.Notifications` over the session bus (zbus), the same call under X11 and Wayland | **Compile-verified only.** Type-checked against the real crates for the Linux target and built in CI. No one has run this and watched a notification appear |
| Windows | WinRT `ToastNotification` | **Compile-verified only,** same as Linux. No toast from this feature has been observed by any person building it |

No notification from this feature has been observed by a human on any of the three platforms.

A platform with no implementation gets nothing built at all -- no timers, no
subscriptions, nothing watching agent tabs -- the same "build nothing where
the answer is no" rule [Keeping the Display
Awake](./keep-display-awake.md#platform-support) follows.

### macOS: requires a real, correctly-located app bundle

`UNUserNotificationCenter` needs a non-nil bundle identifier. A plain
`cargo run` binary has none, so on macOS this feature is silently inert
outside a bundled build -- and the check that decides this is not a defensive
nicety: calling into the notification center with no bundle identifier
crashes the process outright, in a way no exception handler on either side of
the FFI boundary can catch. Zode checks the bundle identity first,
unconditionally, before anything else touches the notification center.

A bundle also has to live somewhere the OS is willing to trust. An ad-hoc
signature is enough, but a `.app` run from a temporary directory is refused
permission outright; the identical bundle in `~/Applications` is accepted. A
locally bundled build (`script/bundle-mac`) must live in `~/Applications` or
`/Applications`, not a scratch or temp location, or notifications will be
silently denied.

The first time this feature tries to notify, macOS shows its usual
notification-permission prompt. If you dismissed it, or later want to
re-grant it, that's under **System Settings → Notifications → Zode**. The
permission request is asynchronous and is not guaranteed to resolve quickly,
so don't be surprised if the first notification after installing doesn't
appear immediately.

### Windows: requires an installed, shortcut-launched build

Toast notifications key off an AppUserModelID (AUMID), which Zode's installed
build gets from its Start Menu shortcut. An unpackaged or shortcut-less launch
has no registered AUMID, and in that case the Windows toast API returns
success while rendering nothing -- there is no error to catch. API success is
not evidence that a toast appeared.

### Turning it off

```json
{
  "agent_finished_notification": {
    "enabled": false
  }
}
```
