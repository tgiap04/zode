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

You must be signed in to your Zode account. Then either open Settings and
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
   **The digits match** in the browser, then **The digits match: trust it** on A.
   If they differ, choose **Digits differ** (browser) or dismiss the prompt on A.
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
| Mirrored terminals     | Open one to see its screen live and type into it. Its size follows the host's terminal.        |
| **Files**, **Changes** | Read-only tabs: browse the open folders, read a file, see the uncommitted diff.                |
| Mobile key bar         | Enter, Escape, Tab, arrows, 1, 2, 3 and Control C, plus a line box you send with **Send**.     |

Nothing in the Files and Changes tabs writes or runs anything. Limits on what
they show are [listed below](#what-is-not-served).

## Control A from another Zode

From another machine, in Zode:

1. Open the **Remote Projects** dialog and choose **Open a Zode on your
   account**.
2. Pick the other Zode. If it is not paired yet, choose **Pair…**, compare the
   six digits, and confirm on both machines; otherwise choose **Connect**.
3. Choose **Open a project on** that Zode.

This opens an independent workspace: you edit the project as if it were local,
and its terminals run on A, not on your machine. It is a separate window from
anything else you have open.

**Both Zodes must run the same version.** The project server's messages are only
compatible between identical builds, so if the versions differ, Zode refuses and
asks you to update both.

## While you are being controlled

- The status bar shows **Controlled by** the device's name (or "N devices"). It
  stays visible even if `status_bar.show` hides the status bar, and a toast
  announces each device that takes control.
- Click the indicator to **Disconnect all devices**, **Turn off remote
  control**, or **Manage trusted devices...**.
- The same are available as the command-palette actions
  `remote_control::DisconnectAll`, `remote_control::Disable` and
  `remote_control::ManageDevices`.
- While a device is connected Zode holds the display awake, on the same terms as
  [`keep_display_awake`](./reference/all-settings.md).

## Revoke a device

| Where                                                                                  | What it does                                                                                               |
| -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| On A: **Manage trusted devices...** then **Forget**                                    | Ends that device's session and removes its key; it cannot come back without pairing again.                 |
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
| File requests being worked on, per device  | 4 at once; past that, requests are answered `rate_limited` |
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

The exact rules are in [Remote control protocol](./remote-control-protocol.md#files).

## Troubleshooting

| You see                                                             | What it means and what to do                                                                                                |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| "This Zode is version X but Y runs Z. Update both Zodes..."         | Version mismatch when opening a project. Update both Zodes to the same version.                                             |
| "That Zode is not online right now" / the Zode is greyed out        | A is off, asleep, signed out, or has remote control off. Turn it on there.                                                  |
| "This host's key is not the one expected..." / "Its key changed..." | The other device's key is not the one you paired with. Nothing was opened. If you reinstalled it, forget it and pair again. |
| "Zode did not start pairing..."                                     | A is busy with another pairing, pairing is locked, remote control is off, or three requests in an hour were already used.   |
| "Pairing is locked after repeated mismatches"                       | On A: Manage trusted devices, then **Unlock pairing**. The lock never expires by itself.                                    |
| "The relay's limit for your account was reached" / "Zode is busy"   | Rate or quota limit. Close some tabs or devices, wait, and try again.                                                       |
| "This browser was signed out of your account..."                    | The browser was revoked. Its key and pairings were deleted; pair again.                                                     |
