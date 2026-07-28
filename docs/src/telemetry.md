---
title: Telemetry
description: "This build collects no telemetry and sends no usage or crash data."
---

# Telemetry

**This build collects no telemetry.** Nothing about your usage, your projects, or
your crashes is recorded or transmitted.

This is not a setting you have to find and switch off — it is a property of the
build. The code that gathered events, the code that uploaded them, and the crash
reporter were all removed rather than disabled:

- No usage or metric events are recorded, and there is no event queue or log to
  upload. The `telemetry::event!` macro that the rest of the editor calls discards
  its argument.
- No crash reports or [minidumps](https://learn.microsoft.com/en-us/windows/win32/debug/minidump-files)
  are captured or uploaded. Hang traces are still written, but only to a local file
  under your own data directory, and nothing reads them but you.
- There is no account, so nothing can be attributed to a user in the first place.
- There is no third-party analytics or crash-reporting service in the dependency
  tree.

## The `telemetry` setting

A `telemetry` key is still accepted in `settings.json`:

```json [settings]
"telemetry": {
    "diagnostics": false,
    "metrics": false
},
```

It is parsed, but **nothing acts on it** — there is no collection for it to
enable or disable. It remains only so that existing settings files keep loading
unchanged. Setting either value to `true` does not cause anything to be collected.

## What still reaches the network

Removing telemetry is not the same as never opening a socket. This build still
makes deliberate outbound requests, and it is worth being precise about them:

- **Extension downloads**, when you browse or install an extension.
- **Language server downloads**, when a language you open needs a server that is
  not already on your machine.
- **Remote development**, when you connect to a host you chose yourself.

These are downloads and connections you initiate. None of them carry usage data.

## Verifying this yourself

The claims above are meant to be checkable rather than taken on trust:

- `rg -n "MINIDUMP_ENDPOINT|multipart|telemetry/events" crates/` returns nothing.
- `crates/telemetry/src/telemetry.rs` is a no-op — read it; it is a few dozen lines.
- Watch the process with a network monitor and confirm that editing, saving and
  quitting produce no outbound traffic.
