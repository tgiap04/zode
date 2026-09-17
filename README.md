# Zode

Zode is a fork of [Zed](https://github.com/zed-industries/zed), the code editor built by
Zed Industries. It keeps Zed's editor — the same GPUI renderer, LSP integration, terminal,
and extension system — and replaces everything that pointed at a vendor's servers with
tooling that points at your own machine.

The result is a development environment rather than only an editor: your databases, your
containers, your git checkouts, and your coding agents each get a tab, and none of them
route through a service you did not choose.

## Features

Everything below is specific to this fork; none of it exists upstream.

**Agents as terminal tabs.** Claude Code, Codex, Antigravity, Copilot and opencode each
open in a pane beside your code. An agent here is a terminal session and nothing else —
there is no chat view, no message editor, no model or mode selector, and no Agent Client
Protocol stack underneath. It runs the CLI you installed, with your own credentials, and
zode never speaks to a model API itself. Sessions survive a restart, agents are tracked
per git checkout, and a rail mark shows which one is answering.

**Database client.** Postgres, MySQL, SQLite and MongoDB, in a tab. Zode ships no driver
of its own: each engine is a separate process speaking line-delimited JSON-RPC over its
stdio, the same shape language servers and debug adapters already use here. A driver that
hangs or crashes costs a process rather than the editor, and a third party can add an
engine without patching zode. Drivers download on first connect — see
[what leaves your machine](#what-leaves-your-machine).

**Containers.** Docker and Kubernetes in one list: what is running on this machine,
beside the code. The UI asks the backend which kinds and actions it offers rather than
branching on which engine it holds, so the two read the same way.

**Git tooling.** A worktree panel showing every checkout of the current project's
repositories and the agents that have run in each, plus branch and remote pickers, blame,
commit view and a commit graph.

**Multiple projects at once.** A left rail holds every open project, each with its own
avatar, colour, dock sizes and agent set. A footer badge reports CPU and RAM per project,
so a runaway build is attributable rather than just "the editor is slow".

## What's different from upstream Zed

Removed:

- **Zed accounts and sign-in** — no Zed session, no Zed credential storage.
- **Cloud and collaboration** — no real-time collaboration server, no channels, no shared
  projects. Editing is single-player.
- **In-editor AI** — no AI providers, no edit predictions, no assistant panel, no Agent
  Client Protocol. Zode holds no model credentials and makes no model calls. Coding
  agents are terminal sessions running a CLI you installed yourself (see Features above).
- **Telemetry and crash reporting** — no usage analytics, no event tracking, no minidump
  upload. `telemetry::send_event` is a no-op by construction, not by setting.

Kept: the editor, LSP integration, the terminal, git integration, the debugger, and
extensions.

Rebuilt rather than removed:

- **In-app updates.** Nothing checks in the background. *Zode → Check for Updates* reads
  this repository's published releases when you ask it to, and installs a newer one if
  there is one. No backend, no account, no identifier sent.
- **An optional account**, used only for syncing your own `settings.json` and
  `keymap.json` between machines. It is opt-in, end-to-end encrypted, and described
  below.

SSH remote development is a middle case. The code is still here and still builds from
source, but the released installers **do not ship the `remote_server` binary** it needs
on the far end: building it doubles the build time and disk of every release, which does
not fit the free GitHub-hosted runners this project releases from. Build it yourself with
`cargo build --release --package remote_server` if you want it.

## What leaves your machine

A fresh install talks to nothing. This is the complete list:

| When | Where | What |
|---|---|---|
| You browse or install an extension | `api.zed.dev` | Zed Industries' extension registry |
| You connect to a database for the first time | Zode's API | Downloads that engine's driver binary |
| You run *Check for Updates* | `api.github.com` | Reads this repository's published releases |
| You sign in and enable sync | `api.zodekit.site` | Your settings, keymap and extension list, encrypted |
| **Automatic**, if Claude Code is already signed in on this machine | `api.anthropic.com` | Reads your own plan quota for the status bar |

Every row but the last happens only when you ask for it. The last one is the exception and
is worth stating plainly: if Claude Code's credentials are already on your machine, the
status bar polls Anthropic's usage endpoint every 60 seconds while the window is focused,
authorised with Claude Code's own OAuth token. Zode reads that token fresh for each
request and never stores, logs or transmits it anywhere else. If Claude Code is not signed
in here, the request never happens. Codex's quota is read through the `codex app-server`
subprocess instead, so no credential leaves the editor for that one. See
[`docs/src/telemetry.md`](./docs/src/telemetry.md) for the full detail.

**The extension registry is a kept dependency, disclosed rather than removed.** Dropping
it meant either running an independent marketplace or losing the extension ecosystem
entirely; for now this fork keeps the registry and says so plainly.

**Settings sync is end-to-end encrypted and the server cannot read it.** Three things sync:
`settings.json`, `keymap.json`, and the list of extension identifiers you have installed —
the list only; pulling it never installs anything. Your projects, your files and your
editing history are not part of it. Each is sealed with AES-256-GCM under a 32-byte data
encryption key generated on your machine.
That key never leaves it, except as the recovery key you write down. There is no key
derivation step and no server-held wrapping key, because either would put something
crackable within reach of the server that already holds the ciphertext. Sync is off until
you sign in; if you never do, nothing is uploaded and no account exists.

See [`legal/third-party-terms.md`](./legal/third-party-terms.md) for the full third-party
disclosure and [`legal/privacy-policy.md`](./legal/privacy-policy.md) for the privacy
statement.

## Why

Not a judgment on Zed's product decisions — its cloud, AI, and collaboration features are
legitimate choices for a company building a business. This fork is for people who want
the editor without that surface area, and who would rather have the database, container
and agent tooling in the same window than in four more of them.

## Installing

Grab an installer from [Releases](https://github.com/tgiap04/zode/releases). Tagged
releases are built from the tag; `nightly` is a single rolling pre-release rebuilt from
`main` once a day, so its assets are replaced rather than accumulated.

| Platform | Asset |
|---|---|
| macOS, Apple silicon | `Zode-aarch64.dmg` |
| macOS, Intel | `Zode-x86_64.dmg` |
| Linux, x86_64 | `zode-linux-x86_64.tar.gz` |
| Linux, arm64 | `zode-linux-aarch64.tar.gz` |
| Windows, x86_64 | `Zode-x86_64.exe` |
| Windows, arm64 | `Zode-aarch64.exe` |

### Read this before you install

Three limits are real, not caveats to skim:

1. **The binaries carry an ad-hoc signature only, and are not notarized.** `codesign -dv`
   on a release bundle reports `Signature=adhoc` and `TeamIdentifier=not set` — enough for
   Apple silicon to execute them at all, and nothing beyond that. No Developer ID vouches
   for where they came from, so your OS will say so and it is right to: nothing has
   verified their origin except you. If that is not a trade you want, build from source
   below.
2. **Updates are manual, but they are in-app.** Nothing checks in the background and
   nothing phones home. When you want to know, use *Zode → Check for Updates*: it reads
   this repository's latest published release, and if there is a newer one it downloads
   and installs it for you. A published release only — a draft is invisible to it.
3. **Debug symbols are stripped in release builds**, so a crash backtrace will be mostly
   empty. Reproduce against a source build if you want a useful one.

To open despite the warning — do this for the one file, and never disable Gatekeeper or
SmartScreen system-wide:

- **macOS**: `xattr -dr com.apple.quarantine /Applications/Zode.app`. Recursive, and not
  `-d`, because the flag lands on every path inside the bundle, not just its root.
  Database drivers are **not** among them: the app ships none, so `-dr` on the bundle
  cannot cover them and does not need to. Each driver (`zode-db-postgres`,
  `zode-db-mysql`, `zode-db-sqlite`, `zode-db-mongodb`) is downloaded on first connect,
  from Zode's own API rather than from this bundle, into
  `~/Library/Application Support/Zode/database_drivers/<id>/<version>/` — outside the
  bundle's own quarantine seal entirely, so it needs no `xattr` step of its own. No
  route to fetch one, or want to install it by hand instead? Put the binary directly at
  that path (see `docs/src/development.md`'s "Database drivers" section for the exact
  shape and the other two platforms' data directories).
  Control-click → *Open* is gone — Apple removed that bypass in macOS 15, so on 15 and
  newer it does nothing. The GUI route is *System Settings → Privacy & Security* → the
  blocked-app notice → *Open Anyway*. That records an approval for the app; it does not
  clear the attribute, so the flags stay where they were. The `xattr` line is the one that
  actually removes them.
- **Windows**: on the SmartScreen prompt, *More info* → *Run anyway*.
- **Linux**: `tar -xzf zode-linux-$(uname -m).tar.gz` and run `zed.app/bin/zed`. The
  directory inside the archive is `zed.app`, not `zode.app` — the bundle keeps upstream's
  layout so `libexec/zed-editor` stays where the database drivers look for it.

Linux builds need **glibc 2.35 or newer** (Ubuntu 22.04 and newer, Debian 12 and newer).
They are built inside an `ubuntu:22.04` container so that floor holds regardless of which
runner image GitHub offers, and `script/check-glibc-floor` fails the build if anything in
the bundle asks for more.

### From source

```sh
git clone https://github.com/tgiap04/zode.git
cd zode
cargo build --release
```

The binary lands at `target/release/zode`. See [`docs/`](./docs/) for the full
development setup (this fork's docs still describe most of upstream Zed's build
process, since the build system itself wasn't touched).

## License

GPL-3.0-or-later for the application ([`LICENSE-GPL`](./LICENSE-GPL)) and Apache-2.0 for
`gpui` and related crates ([`LICENSE-APACHE`](./LICENSE-APACHE)). See
[`NOTICE`](./NOTICE) for attribution to the upstream Zed project.

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md), which still describes upstream Zed's
contribution process (largely unchanged by this fork) — apart from its references to
Zed's own CLA and hosted forums, which don't apply here.
