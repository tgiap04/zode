---
title: Privacy Policy
slug: privacy-policy
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service.

## Summary

Zode has no telemetry, no analytics, and no crash reporting. Those are not settings you
have to find and disable — the code paths that would collect or transmit that information
were removed, not switched off.

Zode does have one optional account, used for a single purpose: syncing your own
`settings.json` and `keymap.json` between your machines. It is off until you sign in, and
what it stores is encrypted on your machine with a key the server never receives. If you
never sign in, no account exists and nothing is uploaded.

You can verify all of this yourself; see "How to verify" below.

## What Zode is

Zode is a fork of [Zed](https://github.com/zed-industries/zed), the code editor. It
removed Zed's account system, cloud sync, real-time collaboration, in-editor AI, telemetry
and crash reporting. What it added is local tooling — a database client, a container list,
git tooling, and coding agents that run as terminal sessions — plus the optional settings
sync described below.

Your project files never leave your machine. Zode does not read them, index them, upload
them, or send them anywhere.

## What leaves your machine

Five things. The first four happen only when you ask for them; the fifth is the one
exception and is described as such:

1. **Installing or browsing extensions.** The extension registry is a retained, disclosed
   dependency on Zed Industries' infrastructure (`api.zed.dev`) — see
   `legal/third-party-terms.md` for why, and what that means.
2. **Downloading a language server.** When you open a file in a language whose server
   isn't installed yet, Zode downloads it from that language server's own distributor
   (e.g. a GitHub release, a package registry) — not from Zed Industries or Zode.
3. **Downloading a database driver.** Zode ships no database drivers. The first time you
   connect to a Postgres, MySQL, SQLite or MongoDB database, the matching driver binary is
   fetched from Zode's own API. The request carries the driver name and version, not your
   connection string, credentials, or queries.
4. **Checking for updates, and signing in.** *Check for Updates* reads this repository's
   published releases from `api.github.com` when you trigger it; nothing checks in the
   background. Signing in and enabling sync uploads your encrypted settings to
   `api.zodekit.site` — see the next section. The sign-in request also carries this
   machine's name, so that the account's device list can show you which machine it was.
5. **Reading your Claude plan quota — automatically.** This is the one request Zode makes
   without you triggering it each time, and it only exists if Claude Code is already
   signed in on this machine. While the window is focused, the status bar polls
   `api.anthropic.com/api/oauth/usage` every 60 seconds, authorised with Claude Code's own
   OAuth access token. Zode reads that token fresh from the keychain (or
   `~/.claude/.credentials.json`) for each request, and never stores, logs or sends it
   anywhere else. It carries no editor usage data and touches none of the removed
   telemetry pipeline. If Claude Code is not signed in here, the request never happens.
   Codex's quota is read through a local `codex app-server` subprocess that holds its own
   session, so nothing credential-shaped leaves the editor for that one.
   `docs/src/telemetry.md` covers this in more detail.

Beyond what is inherent in making an HTTP request at all — your IP address, visible to
whichever server you are contacting, as with any program — none of these transmit your
source code.

Everything else stays local: editing, LSP communication with a language server already
running on your machine, git operations, the terminal, database queries, and container
management.

## Settings sync, in detail

Sync is opt-in. Until you sign in, `crates/zode_sync` does nothing and no account exists.

When you do enable it, three things are synced: `settings.json`, `keymap.json`, and the
list of extension identifiers you have installed. That last one is a list of names only —
no versions, no settings, and pulling it never installs anything on its own. Nothing else
is synced: not your projects, not your files, not your editing history.

They are sealed on your machine with AES-256-GCM under a 32-byte data encryption key
generated locally. That key never leaves your machine, except as the recovery key you
write down and keep. There is no key-derivation step and no server-held wrapping key,
because either would put something crackable within reach of the server that already holds
the ciphertext. The server stores bytes it cannot read.

The consequence is worth stating plainly: **lose the recovery key and the data is gone.**
Nobody can reset it for you, because nobody else ever had it.

Signing in uses a device-code flow — Zode shows you a code, you approve it in your browser
at `zodekit.site/activate`. Your password is never typed into the editor.

## Coding agents

Zode's agents — Claude Code, Codex, Antigravity, Copilot, opencode — are not AI features
built into the editor. Zode holds no model credentials and makes no model API calls. Each
agent is a command-line tool **you** installed, running in a terminal tab, authenticated
with **your** account at that vendor.

This matters for privacy in a way that is easy to get backwards: when you use one of those
tools, your prompts and whatever code you show it go to that vendor under that vendor's
terms. Zode is not in that path and cannot be — it is running their CLI, not proxying it.
What Zode does not do is send anything of its own, or add your usage of those tools to any
record it keeps, because it keeps none.

## What does not exist in this fork

- No telemetry: no usage analytics, no event tracking, nothing sent anywhere about how you
  use the editor. `telemetry::send_event` is an empty function. See
  `docs/src/telemetry.md` for the code-level detail.
- No crash reporting. No minidump is ever generated or uploaded.
- No real-time collaboration server, so no audio/video/screen-share processing.
- No in-editor AI provider, no edit predictions, no assistant panel — so no model API ever
  receives your code from Zode itself (see "Coding agents" above for what your own CLI
  tools do).
- No subscription, no payment processing, no billing data of any kind.
- No advertising, no profiling, no data sold or shared with anyone.

## How to verify

This is a privacy claim about open-source software — don't take it on faith. Build it
yourself and check every host this document names:

```sh
rg -n 'zed\.dev|zodekit|api\.github\.com|anthropic|MINIDUMP|sentry' crates/ --type rust
```

What you should find: the extension-registry path, the account and sync code under
`crates/zode_account` and `crates/zode_sync`, the update check in `crates/auto_update`,
the quota read in `crates/agent_usage`, and click-through documentation links.

Two matches deserve a word so they don't mislead you. `MINIDUMP` matches only the name of
a `ZED_GENERATE_MINIDUMPS` environment variable in the remote and dev-container transports'
pass-through lists — there is no minidump or crash-reporter code behind it. And
`anthropic` matches three things: the quota read in `crates/agent_usage`, a settings-schema
remnant in `crates/settings_content`, and settings-migration code in `crates/migrator`. The
last two exist so old settings files keep loading and migrating; no crate consumes that
schema for inference, and the URL builders that once pointed at a model endpoint have no
remaining call sites.

To satisfy yourself about telemetry specifically:

```sh
rg -n 'fn send_event' -A3 crates/telemetry/
```

The body is empty. `legal/third-party-terms.md` and `docs/src/telemetry.md` explain what
else you will find and why.

## Questions

This fork has no support address or company behind it. If something in the source doesn't
match what this document says, please open an issue on the repository — that's the
accuracy check this document exists to invite.
