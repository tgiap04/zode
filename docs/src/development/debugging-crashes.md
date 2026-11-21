---
title: Debugging Crashes
description: "Guide to debugging crashes for Zode development."
---

# Debugging Crashes

When Zode panics or crashes, it sends a message to a sidecar process that inspects the editor's memory and creates a [minidump](https://chromium.googlesource.com/breakpad/breakpad/+/master/docs/getting_started_with_breakpad.md#the-minidump-file-format) in `~/Library/Logs/Zode` or `$XDG_DATA_HOME/zode/logs`. You can use this minidump to generate backtraces for all thread stacks.

This build does not upload crash reports anywhere; see [Telemetry](../telemetry.md).

These crash reports include useful data, but they are hard to read without spans or symbol information. You can still analyze them locally by downloading source and an unstripped binary (or separate symbols file) for your Zode release, then running:

```sh
zstd -d ~/.local/share/zode/logs/<uuid>.dmp -o minidump.dmp
minidump-stackwalk minidump.dmp
```

Alongside the minidump in your logs directory, you should also see a `<uuid>.json` file with metadata such as the panic message, span, and system specs.

## Using a Debugger

If you can reproduce the crash consistently, use a debugger to inspect program state at the crash point.

For setup details, see [Using a debugger](./debuggers.md#debugging-panics-and-crashes).
