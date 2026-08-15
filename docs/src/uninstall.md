---
title: Uninstall
description: "This guide covers how to uninstall Zode on different operating systems."
---

# Uninstall

This guide covers how to uninstall Zode on different operating systems.

## macOS

### Standard Installation

If you installed Zode by downloading it from the website:

1. Quit Zode if it's running
2. Open Finder and go to your Applications folder
3. Drag Zode to the Trash (or right-click and select "Move to Trash")
4. Empty the Trash

### Homebrew Installation

If you installed Zode using Homebrew, use the following command:

```sh
brew uninstall --cask zed
```

Or for the preview version:

```sh
brew uninstall --cask zed@preview
```

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

## Linux

### Standard Uninstall

If Zode was installed using the default installation script, run:

```sh
zed --uninstall
```

You'll be prompted whether to keep or delete your preferences. After making a choice, you should see a message that Zode was successfully uninstalled.

If the `zed` command is not found in your PATH, try:

```sh
$HOME/.local/bin/zed --uninstall
```

or:

```sh
$HOME/.local/zed.app/bin/zed --uninstall
```

### Package Manager

If you installed Zode using a package manager (such as Flatpak, Snap, or a distribution-specific package manager), consult that package manager's documentation for uninstallation instructions.

### Manual Removal

If the uninstall command fails or Zode was installed to a custom location, you can manually remove:

- Installation directory: `~/.local/zed.app` (or your custom installation path)
- Binary symlink: `~/.local/bin/zed`
- Configuration and data: `~/.config/zode`

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
