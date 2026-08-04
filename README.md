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
debugger, extensions, and SSH remote development (rebuilt on a direct connection path
rather than through a relay server).

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

<!-- TODO(packaging): fill in once a signed, notarized release artifact and a
     Homebrew cask (or equivalent) exist. Until then, build from source: -->

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
