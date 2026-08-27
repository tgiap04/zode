# Zode

Zode is a fork of [Zed](https://github.com/zed-industries/zed), the code editor built
by Zed Industries. This fork exists to answer one question: what does Zed look like
with every account, cloud, AI, and telemetry code path removed rather than merely
disabled?

## What's different from upstream Zed

This fork removed:

- **Authentication and accounts** — no sign-in, no session, no credential storage.
- **Cloud and collaboration** — no real-time collaboration server, no channels, no
  shared projects. Editing is single-player.
- **AI and agent features** — no AI providers, no edit predictions, no agent panel.
- **Telemetry and crash reporting** — no usage analytics, no event tracking, no
  minidump upload. `telemetry::send_event` is a no-op by construction, not by setting.
- **Auto-update** — no in-app updater. Updates come from wherever you installed this
  from (see Installing, below).

What's kept: the editor itself, LSP integration, the terminal, git integration, the
debugger, and extensions.

SSH remote development is a middle case. The code is still here and still builds from
source, but the released installers **do not ship the `remote_server` binary** it needs
on the far end: building it doubles the build time and disk of every release, which does
not fit the free GitHub-hosted runners this project releases from. Build it yourself with
`cargo build --release --package remote_server` if you want it.

**One dependency was kept and disclosed rather than removed**: browsing and installing
extensions still queries Zed Industries' extension registry (`api.zed.dev`). Dropping
it meant either running an independent marketplace or losing the extension ecosystem
entirely; for now, this fork keeps the registry and says so plainly instead. See
[`legal/third-party-terms.md`](./legal/third-party-terms.md) for the full disclosure and
[`legal/privacy-policy.md`](./legal/privacy-policy.md) for what does and doesn't leave
your machine.

## Why

Not a judgment on Zed's product decisions — its cloud, AI, and collaboration features
are legitimate choices for a company building a business. This fork is for people who
want the editor itself without any of that surface area: nothing phoning home, nothing
to sign into, nothing to disable in settings because it was never wired up to begin
with.

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
   nothing phones home. When you want to know, use *Help → Check for Updates*: it reads
   this repository's latest published release, and if there is a newer one it downloads
   and installs it for you. A published release only — a draft is invisible to it.
3. **Debug symbols are stripped in release builds**, so a crash backtrace will be mostly
   empty. Reproduce against a source build if you want a useful one.

To open despite the warning — do this for the one file, and never disable Gatekeeper or
SmartScreen system-wide:

- **macOS**: `xattr -dr com.apple.quarantine /Applications/Zode.app`. Recursive, and not
  `-d`: the flag lands on 18 paths inside the bundle, six of them executables —
  `Contents/MacOS/{cli,git,zode,zode-db-mysql,zode-db-postgres,zode-db-sqlite}`. Clearing
  only the bundle root leaves the other seventeen flagged, and `script/bundle-mac` notes
  what that costs: a driver Gatekeeper blocks fails silently, so every database connection
  just fails to start one with nothing saying why.
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
