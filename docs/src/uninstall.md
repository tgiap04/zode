---
title: Uninstall
description: "This guide covers how to uninstall Zode on different operating systems."
---

# Uninstall

This guide covers how to uninstall Zode on different operating systems.

> **Warning:** Do not run `--uninstall` from a leftover Zode 0.1.5 binary, such as `zed --uninstall`, `~/.local/bin/zed --uninstall` or `/usr/local/bin/zed --uninstall`. Zode 0.1.5 embeds the old uninstall script, which deletes `~/.local/share/zed`, `~/.config/zed` and `~/.zed_server`, and on macOS also `~/Library/Application Support/Zed` and `~/Library/Logs/Zed`. Those are the directories upstream Zed keeps its own data in. Remove Zode by hand with the steps below instead. The script that ships from 0.1.6 only touches Zode's own paths.

## macOS

### Standard Installation

If you installed Zode by downloading it from the website:

1. Quit Zode if it's running
2. Open Finder and go to your Applications folder
3. Drag Zode to the Trash (or right-click and select "Move to Trash")
4. Empty the Trash

### Removing User Data (Optional)

To completely remove all Zode configuration files and data:

1. Open Finder
2. Press `Cmd + Shift + G` to open "Go to Folder"
3. Delete the following directories if they exist:
   - `~/Library/Application Support/Zode`
   - `~/Library/Saved Application State/io.github.tgiap04.zode.savedState`
   - `~/Library/Logs/Zode`
   - `~/Library/Caches/io.github.tgiap04.zode`
   - `~/Library/Caches/Zode`
   - `~/.config/zode`
   - `~/.local/state/Zode`

If you installed the CLI with `cli: install`, also remove `/usr/local/bin/zode`.

## Linux

### Standard Uninstall

If Zode was installed using the default installation script, run:

```sh
zode --uninstall
```

You'll be prompted whether to keep or delete your preferences. After making a choice, you should see a message that Zode was successfully uninstalled.

If the `zode` command is not found in your PATH, try:

```sh
$HOME/.local/bin/zode --uninstall
```

or:

```sh
$HOME/.local/zode.app/bin/zode --uninstall
```

### Package Manager

If you installed Zode using a package manager (such as Flatpak, Snap, or a distribution-specific package manager), consult that package manager's documentation for uninstallation instructions.

### Manual Removal

If the uninstall command fails or Zode was installed to a custom location, you can manually remove:

- Installation directory: `~/.local/zode.app` (or your custom installation path)
- Binary symlink: `~/.local/bin/zode`
- Configuration and data: `~/.config/zode`, `~/.local/share/zode`

Paths named `zed` (`~/.local/zed.app`, `~/.local/bin/zed`, `/usr/local/bin/zed`) belong to Zed unless they point into a Zode bundle. A Zode 0.1.5 Linux install used `~/.local/zed.app` and `~/.local/bin/zed`; see [Upgrading from Zode 0.1.5](./linux.md#upgrading-from-zode-015) for how to tell the two apart. Only a Zode 0.1.5 on macOS created `/usr/local/bin/zed`; remove it only if `readlink /usr/local/bin/zed` points into a `Zode*.app` bundle.

## Windows

### Standard Installation

1. Quit Zode if it's running
2. Open Settings (Windows key + I)
3. Go to "Apps" > "Installed apps" (or "Apps & features" on Windows 10)
4. Search for "Zode"
5. Click the three dots menu next to Zode and select "Uninstall"
6. Follow the prompts to complete the uninstallation

Alternatively, you can:

1. Open the Start menu
2. Right-click on Zode
3. Select "Uninstall"

### Removing User Data (Optional)

To completely remove all Zode configuration files and data:

1. Press `Windows key + R` to open Run
2. Type `%APPDATA%` and press Enter
3. Delete the `Zode` folder if it exists
4. Press `Windows key + R` again, type `%LOCALAPPDATA%` and press Enter
5. Delete the `Zode` folder if it exists

## Troubleshooting

If you encounter issues during uninstallation:

- **macOS/Windows**: Ensure Zode is completely quit before attempting to uninstall. Check Activity Manager (macOS) or Task Manager (Windows) for any running Zode processes.
- **Linux**: If the uninstall script fails, check the error message and consider manual removal of the directories listed above.
- **All platforms**: If you want to start fresh while keeping Zode installed, you can delete the configuration directories instead of uninstalling the application entirely.

For additional help, see our [Linux-specific documentation](./linux.md) or visit the [Zode community](https://zed.dev/community-links).
