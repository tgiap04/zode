# Remote control

Remote control lets you watch and type into the agents and terminals running in
one Zode (call it **A**) from another device: a browser at
`zodekit.site/remote`, or Zode on another machine. The two devices talk through
the zodekit.site relay, and everything they exchange is end-to-end encrypted;
the relay only forwards bytes it cannot read. For what that does and does not
protect, see [Remote control security](./remote-control-security.md).

It is **off by default**. While it is off, Zode opens no connection for it and
watches no terminal.

- [Turn it on](#turn-it-on)
- [Pair a device](#pair-a-device)
- [Control A from a browser](#control-a-from-a-browser)
- [Control A from another Zode](#control-a-from-another-zode)
- [While you are being controlled](#while-you-are-being-controlled)
- [Revoke a device](#revoke-a-device)
- [Limits](#limits)
- [What is not served](#what-is-not-served)
- [Troubleshooting](#troubleshooting)

## Turn it on

You must be signed in to your Zode account. Then click the account button at
the bottom of the sidebar rail and check **Remote Control**, open Settings and
switch on **Allow Remote Control**, or edit your settings file:

```json [settings]
{
  "remote_control": {
    "enabled": true,
    "idle_timeout_minutes": 30
  }
}
```

`idle_timeout_minutes` is how long a connected device may send nothing before
it is disconnected; `0` turns the limit off. Both keys are described in
[All Settings](./reference/all-settings.md#remote-control).

Switching it off again disconnects every device at once and stops listening.

## Pair a device

A device must be approved once on A before it can control it. Pairing teaches
each side the other's key and makes the two people at the two screens check
that nobody sits in between.

1. On the controlling device, choose the Zode to pair with. In a browser, open
   `zodekit.site/remote`, pick the Zode from the list (it must be online, with
   remote control on and signed in to the same account) and choose **Pair**.
2. A shows a prompt: "wants to control this Zode", with six digits.
3. The controlling device shows six digits too. **If they are the same**, choose
   **The digits match** in the browser, then **The Digits Match: Trust It** on A.
   If they differ, choose **Digits differ** (browser) or **Reject** on A.
4. Both sides finish the connection. The device is now trusted.

Confirm on A only when you started the pairing yourself. Trusting a device
gives it a shell on your machine, as [the security page](./remote-control-security.md#a-paired-device-has-a-shell-on-a)
explains.

Pairing is limited: A takes one pairing at a time, a request expires after 120
seconds, it accepts at most three requests an hour, and three mismatches lock
pairing until you unlock it. See [Troubleshooting](#troubleshooting).

## Control A from a browser

Open `zodekit.site/remote`, sign in, pick a paired Zode and choose **Connect**.

| You get                | Notes                                                                                          |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| **Agents**             | The agents and terminals open on A, with their status (running, awaiting approval, idle, ...). |
| **History**            | Past agent sessions of the projects open on A. See [History](#history).                        |
| Mirrored terminals     | Open one to see its screen live and type into it. Its grid follows the host's terminal.        |
| **Files**, **Changes** | Read-only tabs: browse the open folders, read a file, see the uncommitted diff.                |
| Mobile key bar         | Enter, Escape, Tab, Up, Down, 1, 2, 3 and Control C, plus a line box you send with **Send**.   |

The terminal sizes its text so the whole screen fits the pane, and follows the
site's Light or Dark theme. On a narrow screen such as a phone, an open terminal
fills the screen as a sheet with the key bar under it; **Back to agents** closes
it.

Nothing in the Files and Changes tabs writes or runs anything. Limits on what
they show are [listed below](#what-is-not-served).

### History

The **History** tab lists the past agent sessions of the projects open on A,
newest first, grouped by agent and folder. Each row shows the session's title,
when it was last used and its branch, if it has one. The tab appears only when A
supports it; with an older Zode it is not there and nothing else changes.
**Show older sessions** loads the next page.

| Button     | What it does                                                                                                                                   |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **Resume** | Continues the session in a new tab on A, and opens that tab's terminal in the browser.                                                         |
| **Open**   | Replaces Resume when the session is already running in a tab on A. It takes you to that tab and starts nothing.                                |
| **Fork**   | Branches the conversation into a new session, in a new tab, and opens it. Offered for Claude Code and opencode. Codex and Copilot cannot fork. |

- Resume and Fork start an agent on A, as if you had picked the session in the
  history panel there. A shows a [toast](#while-you-are-being-controlled) saying
  which device did it.
- A refuses a Resume or Fork from your browser for 3 seconds after the previous
  one opened a tab, and the page asks you to try again.
- A session appears only if it ran in a folder open on A, and not under a path
  A keeps private or hides from the scan. See
  [What is not served](#what-is-not-served).

## Control A from another Zode

From another machine, in Zode:

1. Click the account button at the bottom of the sidebar rail and choose
   **Open a Zode on Your Account…** (it is also in the **Remote Projects**
   dialog).
2. Pick the other Zode. If it is not paired yet, choose **Pair…**, compare the
   six digits, and confirm on both machines — **The Digits Match** here,
   **The Digits Match: Trust It** on the other, or **Digits Differ** here if they
   are not the same; otherwise choose **Connect**. If a Zode's key has changed
   since you paired, its row offers **Pair Again** instead, which forgets the old
   key and starts a new pairing.
3. Choose **Open a Project on** that Zode.

This opens an independent workspace: you edit the project as if it were local,
and its terminals run on A, not on your machine. It is a separate window from
anything else you have open.

**Both Zodes must run the same version.** The project server's messages are only
compatible between identical builds, so if the versions differ, Zode refuses and
asks you to update both.

## While you are being controlled

While remote control is on and nobody is connected, the status bar shows a
small screen icon: grey when it is ready, yellow while it reconnects or nobody
is signed in, red when it has stopped. Hover it for the reason; click it to turn
remote control off or manage trusted devices.

- The status bar shows **Controlled by** the device's name (or "N devices"). It
  stays visible even if `status_bar.show` hides the status bar, and a toast
  announces each device that takes control.
- When a device resumes or forks a past session, a toast names the device and
  the session, for example `My phone resumed “Fix the build”`. It appears where
  the tab opens, and in the window's front workspace too when that is a
  different one, and goes away after five seconds. Opening a session that is
  already running shows no toast.
- Click the indicator to **Disconnect All Devices**, **Turn Off Remote
  Control**, or **Manage Trusted Devices…**.
- The same are available as the command-palette actions
  `remote_control::DisconnectAll`, `remote_control::Disable` and
  `remote_control::ManageDevices`. The account menu offers turning it off and
  **Manage Trusted Devices…** at any time, not only while a device is connected.
- While a device is connected Zode holds the display awake, on the same terms as
  [`keep_display_awake`](./reference/all-settings.md).

## Revoke a device

| Where                                                                                  | What it does                                                                                               |
| -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| On A: **Manage Trusted Devices…** then **Forget**                                      | Ends that device's session and removes its key; it cannot come back without pairing again.                 |
| The web devices page, `zodekit.site/account/devices` (**Browsers**, then **Sign out**) | Removes that browser from your account. A's connection to it is closed and the browser deletes its key.    |
| Signing the device out of your account                                                 | The relay closes its connections; the device deletes its own key and every pairing, so it must pair again. |

If a device is lost or stolen, revoke it first and ask questions afterwards.

## Limits

| Limit                                      | Value                                                      |
| ------------------------------------------ | ---------------------------------------------------------- |
| Zodes online as hosts, per account         | 4                                                          |
| Controlling devices connected, per account | 8                                                          |
| Sessions open to one host                  | 4 (A also stops at four connected devices)                 |
| Bandwidth through the relay                | A daily byte quota per account (2 GiB by default)          |
| File and session list requests, per device | 4 at once; past that, requests are answered `rate_limited` |
| Session resumes, per device                | One at a time, 3 seconds apart; sooner is `rate_limited`   |
| Idle disconnect                            | `idle_timeout_minutes`, 30 by default                      |

The relay's numbers are its defaults and can be changed by whoever runs it. When
the quota is spent, connections close and new ones are refused until the quota resets at midnight UTC.

## What is not served

Browsing is read-only and goes through what A has already scanned, not its disk.

- Files you have listed in `private_files`, files hidden by
  `file_scan_exclusions`, and symlinks are not served.
- In the viewer, files over 1 MiB and files that are not UTF-8 text are not shown.
- A diff over 2 MiB is cut at a line boundary and marked truncated.
- The diff is of the working tree against `HEAD` for the folder that is open;
  untracked files are not in it, and a folder that is not a git repository has
  no diff.
- A folder listing shows at most 2000 entries.
- History lists only sessions that ran in a folder open on A. Sessions under a
  path in `private_files` or `file_scan_exclusions` are not listed and cannot be
  resumed, and neither are sessions of projects that are not on A's own disk,
  such as a remote project.

The exact rules are in [Remote control protocol](./remote-control-protocol.md#files)
and, for History, [Sessions](./remote-control-protocol.md#sessions).

## Troubleshooting

| You see                                                             | What it means and what to do                                                                                                |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| "This Zode is version X but Y runs Z. Update both Zodes..."         | Version mismatch when opening a project. Update both Zodes to the same version.                                             |
| "That Zode is not online right now" / the Zode is greyed out        | A is off, asleep, signed out, or has remote control off. Turn it on there.                                                  |
| "This host's key is not the one expected..." / "Its key changed..." | The other device's key is not the one you paired with. Nothing was opened. If you reinstalled it, forget it and pair again. |
| "Zode did not start pairing..."                                     | A is busy with another pairing, pairing is locked, remote control is off, or three requests in an hour were already used.   |
| "Pairing is locked after repeated mismatches"                       | On A: Manage Trusted Devices, then **Unlock Pairing**. The lock never expires by itself.                                    |
| "The relay's limit for your account was reached" / "Zode is busy"   | Rate or quota limit. Close some tabs or devices, wait, and try again.                                                       |
| "This browser was signed out of your account..."                    | The browser was revoked. Its key and pairings were deleted; pair again.                                                     |
| "A session was just opened. Try again in a few seconds."            | A allows one Resume or Fork per device every 3 seconds. Wait a moment and try again.                                        |
| "That session is no longer on this Zode, or its folder is closed."  | The session was deleted, its folder is gone, or no window on A has it open any more. Pick another session.                  |
| "Zode resumed the session, but its terminal has not appeared..."    | The tab was opened on A but its terminal did not show up in time. It is listed under **Agents** when it does.               |
