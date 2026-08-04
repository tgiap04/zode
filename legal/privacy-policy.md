---
title: Privacy Policy
slug: privacy-policy
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service.

## Summary

Zode collects nothing. There is no account, no telemetry, no analytics, no crash
reporting, and no server that Zode talks to about you, your code, or how you use the
editor. This isn't a setting you have to find and disable — the code paths that would
collect or transmit that information were removed, not switched off. You can verify
this yourself; see "How to verify" below.

## What Zode is

Zode is a fork of [Zed](https://github.com/zed-industries/zed), the code editor. It
removed Zed's account system, cloud sync, AI/agent features, real-time collaboration,
telemetry, crash reporting, and auto-updater. What's left is a local code editor with
no server-side component of its own.

There is no company operating Zode as a service. Nobody but you holds your settings,
your project files, or anything else Zode touches — it's all on your own machine.

## What actually leaves your machine

Two things, both something you ask for, not something Zode decides to do on its own:

1. **Installing or browsing extensions.** The extension registry is a retained,
   disclosed dependency on Zed Industries' infrastructure (`api.zed.dev`) — see
   `legal/third-party-terms.md` for why, and what that means.
2. **Downloading a language server.** When you open a file in a language whose server
   isn't installed yet, Zode downloads it from that language server's own distributor
   (e.g. a GitHub release, a package registry) — not from Zed Industries or Zode.

Neither of these transmits your source code, your settings, or anything identifying
beyond what's inherent in making an HTTP request (your IP address, visible to whichever
server you're downloading from — true of any internet request from any program).

Everything else — editing, LSP communication with a language server already running
on your machine, git operations, the terminal — stays local.

## What does not exist in this fork

- No account or sign-in of any kind.
- No telemetry: no usage analytics, no event tracking, nothing sent anywhere about how
  you use the editor. See `docs/src/telemetry.md` for the code-level detail and how to
  check it yourself.
- No crash reporting. No minidump is ever generated or uploaded.
- No real-time collaboration server, so no audio/video/screen-share processing.
- No AI features, so no AI provider ever sees your code or your prompts.
- No subscription, no payment processing, no billing data of any kind.

## How to verify

This is a privacy claim about open-source software — don't take it on faith. Build it
yourself and check:

```sh
rg -n 'zed\.dev|api\.zed\.dev|collab\.zed|MINIDUMP|sentry' crates/ --type rust
```

The only expected matches are the extension-registry path described above and
click-through documentation links (which open your browser, not a background
connection). `legal/third-party-terms.md` and `docs/src/telemetry.md` explain what you'll
find and why.

## Questions

This fork has no support address or company behind it. If something in the source
doesn't match what this document says, please open an issue on the repository — that's
the accuracy check this document exists to invite.
