---
title: Zode on macOS
description: "Zode is developed primarily on macOS, making it a first-class platform with full feature support."
---

# Zode on macOS

Zode is developed primarily on macOS, making it a first-class platform with full feature support.

## Installing Zode

Download the `Zode-<arch>.dmg` file from the [releases page](https://github.com/tgiap04/zode/releases)—open it and drag Zode to your Applications folder.

After installation, Zode checks for updates automatically and prompts you when a new version is available.

### Building from Source

To build Zode from source, see the [macOS development documentation](./development/macos.md).

## System Requirements

- macOS 10.15.7 (Catalina) or later
- Apple Silicon (M1/M2/M3/M4) or Intel processor

Zode uses Metal for GPU-accelerated rendering, which is available on all supported macOS versions.

## Installing the CLI

Zode includes a command-line tool for opening files and projects from Terminal. To install it:

1. Open Zode
2. Open the command palette with `Cmd+Shift+P`
3. Run `cli: install`

This creates a `zode` command in `/usr/local/bin`. You can then open files and folders:

```sh
zode .                   # Open current folder
zode file.txt            # Open a file
zode project/ file.txt   # Open a folder and a file
```

See the [CLI Reference](./reference/cli.md) for all available options.

## Uninstall

1. Quit Zode if it's running
2. Drag Zode from Applications to the Trash
3. Optionally, remove your settings and extensions:

```sh
rm -rf ~/.config/zode
rm -rf ~/Library/Application\ Support/Zode
rm -rf ~/Library/Caches/Zode
rm -rf ~/Library/Logs/Zode
rm -rf ~/Library/Saved\ Application\ State/io.github.tgiap04.zode.savedState
```

If you installed the CLI, remove it with:

```sh
rm /usr/local/bin/zode
```

An older Zode created `/usr/local/bin/zed` instead. Remove it only if `readlink /usr/local/bin/zed` points into a `Zode*.app` bundle; otherwise it belongs to Zed. Do not run `/usr/local/bin/zed --uninstall` from that older Zode: see the warning on the [Uninstall](./uninstall.md) page.

## Troubleshooting

### Zode won't open or shows "damaged" warning

If macOS reports that Zode is damaged or can't be opened, it's likely a Gatekeeper issue. Try:

1. Right-click (or Control-click) on Zode in Applications
2. Select "Open" from the context menu
3. Click "Open" in the dialog that appears

This tells macOS to trust the application.

If that doesn't work, remove the quarantine attribute:

```sh
xattr -cr /Applications/Zode.app
```

### CLI command not found

If the `zode` command isn't available after installation:

1. Check that `/usr/local/bin` is in your PATH
2. Try reinstalling the CLI via `cli: install` in the command palette
3. Open a new terminal window to reload your PATH

### GPU or rendering issues

Zode uses Metal for rendering. If you experience graphical glitches:

1. Ensure macOS is up to date
2. Restart your Mac to reset the GPU state
3. Check Activity Monitor for GPU pressure from other apps

### High memory or CPU usage

If Zode uses more resources than expected:

1. Check for runaway language servers in the terminal output (`zed: open log`)
2. Try disabling extensions one by one to identify conflicts
3. For large projects, consider using [project settings](./reference/all-settings.md#file-scan-exclusions) to exclude unnecessary folders from indexing

For additional help, see the [Troubleshooting guide](./troubleshooting.md) or visit the [Zode Discord](https://discord.gg/zed-community).
