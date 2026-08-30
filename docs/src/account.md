# Your Zode Account

Signing in is **optional**. Zode is fully usable signed out, and while you are
signed out it makes no network requests at all — not at startup, not on a
timer, not to check anything. Everything below happens only because you asked
for it.

## Signing in

Click the person icon at the bottom of the left rail. Zode shows an eight
character code and offers to open your browser with it already filled in;
approve there and the editor picks up the session within a few seconds.

If there is no browser to open — a remote or SSH session, for example — the
window also prints the address and the code so you can enter them from another
machine. That is why Zode uses this flow rather than a redirect: it is the only
one that works in both places.

Your session is stored in the operating system's keychain (Keychain Access on
macOS, libsecret on Linux, Credential Manager on Windows) and survives a
restart.

## Syncing your settings

**Account → Sync Settings…** in the rail menu. Three things can travel:

| | What moves |
|---|---|
| Settings | your whole `settings.json`, comments and formatting included |
| Key bindings | your whole `keymap.json` |
| Extensions | the list of installed extension IDs — never the extensions themselves |

Nothing syncs automatically. There is no background reconciliation and no sync
at startup; every transfer is one button press, and you are shown exactly what
would change before anything is written.

### Your recovery key

Your settings are encrypted **on your machine**, before they leave it. The
server stores ciphertext and has no key — it cannot read your settings, and
neither can anyone who obtains its database.

That is possible only because you hold the key. The first time you sync, Zode
generates a recovery key and shows it to you once:

```
ZODE-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXX
```

**Write it down.** If you lose it, your synced data is unrecoverable. Not
difficult to recover — unrecoverable. There is no support request that undoes
it, because there is nothing on our side to undo it with. That is what
end-to-end encryption means, and it is the reason `settings.json` can be synced
at all: that file can hold `language_models.*.api_key`, `terminal.env`, and
`remote_env`.

You can see it again at any time from **Account → Show Recovery Key**.

On a second machine, sign in and choose **Account → Enter Recovery Key…**.

### When both sides have changed

Pulling never overwrites your file silently. Zode shows the difference and
three choices: keep this machine's copy, take the server's copy, or cancel.
Whichever you pick, the file being replaced is copied to `settings_backup.json`
(or `keymap_backup.json`) first.

If two machines push at once, the second one is refused rather than allowed to
destroy the first one's work — you get the same diff and the same choice.

### Extensions are listed, never installed

Pulling the extension list shows you what is installed elsewhere and missing
here. It does not install anything. Installing is a separate button you press
after reading the list; a sync payload that could install code would be a
supply-chain problem with your own account as the key.

## Managing your machines

**Account → Account on the Web** opens the list of editors signed in to your
account, where you can rename or sign out any of them.

Pointing the editor at a different backend with `ZODE_API_URL` moves this link
with it. If the browser app is not simply the API host without its `api.`
prefix, set `ZODE_WEB_URL` as well.

Signing a device out ends its session. It does **not** change your recovery
key, so a machine you no longer control can still read synced data it had
already downloaded. To cut that off, use **Account → Rotate Recovery Key…**,
which replaces the key and re-encrypts everything stored under it. Every other
machine will then ask for the new key.

## What is never sent

- Telemetry. The account crates cannot even reach the telemetry code — it is
  enforced against the dependency graph in CI, not left to review.
- Anything at all while you are signed out.
- Anything you have not pressed a button to send.
