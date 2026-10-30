---
title: CLI Reference
description: "Reference for Zode's command-line interface (CLI), including opening files and directories, integrating with tools, and controlling Zode from scripts."
---

# CLI Reference

Use Zode's command-line interface (CLI) to open files and directories, integrate with other tools, and control Zode from scripts.

## Installation

**macOS:** Run the `cli: install` command from the command palette ({#kb command_palette::Toggle}) to install the `zode` CLI to `/usr/local/bin/zode`.

**Linux:** The CLI is installed as `~/.local/bin/zode` by the installation script. See [Zode on Linux](../linux.md).

**Windows:** The CLI is included with Zode. The CLI is `zode.exe` in the `bin` folder of the installation directory. It is on your PATH when the installer's "Add to PATH" task is ticked; otherwise use the full path.

## Usage

```sh
zode [OPTIONS] [PATHS]...
```

## Opening Files and Directories

Open a file:

```sh
zode myfile.txt
```

Open a directory as a workspace:

```sh
zode ~/projects/myproject
```

Open multiple files or directories:

```sh
zode file1.txt file2.txt ~/projects/myproject
```

Open a file at a specific line and column:

```sh
zode myfile.txt:42        # Open at line 42
zode myfile.txt:42:10     # Open at line 42, column 10
```

## Options

### `-w`, `--wait`

Wait for all opened files to be closed before the CLI exits. When opening a directory, waits until the window is closed.

This is useful for integrating Zode with tools that expect an editor to block until editing is complete (e.g., `git commit`):

```sh
export EDITOR="zode --wait"
git commit  # Opens Zode and waits for you to close the commit message file
```

### `-n`, `--new`

Open paths in a new workspace window, even if the paths are already open in an existing window:

```sh
zode -n ~/projects/myproject
```

### `-a`, `--add`

Add paths to the currently focused workspace instead of opening a new window. When multiple workspace windows are open, files open in the focused window:

```sh
zode -a newfile.txt
```

### `-r`, `--reuse`

Reuse an existing window, replacing its current workspace with the new paths:

```sh
zode -r ~/projects/different-project
```

### `--diff <OLD_PATH> <NEW_PATH>`

Open a diff view comparing two files. Can be specified multiple times:

```sh
zode --diff file1.txt file2.txt
zode --diff old.rs new.rs --diff old2.rs new2.rs
```

### `--foreground`

Run Zode in the foreground, keeping the terminal attached. Useful for debugging:

```sh
zode --foreground
```

### `--user-data-dir <DIR>`

Use a custom directory for all user data (database, extensions, logs) instead of the default location:

```sh
zode --user-data-dir ~/.zode-custom
```

Default locations:

- **macOS:** `~/Library/Application Support/Zode`
- **Linux:** `$XDG_DATA_HOME/zode` (typically `~/.local/share/zode`)
- **Windows:** `%LOCALAPPDATA%\Zode`

### `-v`, `--version`

Print Zode's version and exit:

```sh
zode --version
```

### `--uninstall`

Uninstall Zode and remove all related files (macOS and Linux only):

```sh
zode --uninstall
```

### `--zode <PATH>`

Specify a custom path to the Zode application or binary:

```sh
zode --zode /path/to/Zode.app myfile.txt
```

## Reading from Standard Input

Read content from stdin by passing `-` as the path:

```sh
echo "Hello, World!" | zode -
cat myfile.txt | zode -
ps aux | zode -
```

This creates a temporary file with the stdin content and opens it in Zode.

## URL Handling

The CLI can open `zed://`, `http://`, and `https://` URLs:

```sh
zode zed://settings
zode https://github.com/zed-industries/zed
```

## Using Zode as Your Default Editor

Set Zode as your default editor for Git and other tools:

```sh
export EDITOR="zode --wait"
export VISUAL="zode --wait"
```

Add these lines to your shell configuration file (e.g., `~/.bashrc`, `~/.zshrc`).

## macOS: Switching Release Channels

On macOS, you can launch a specific release channel by passing the channel name as the first argument:

```sh
zode --stable myfile.txt
zode --preview myfile.txt
zode --nightly myfile.txt
```

## WSL Integration (Windows)

On Windows, the CLI supports opening paths from WSL distributions. This is handled automatically when launching Zode from within WSL.

## Exit Codes

| Code | Meaning                           |
| ---- | --------------------------------- |
| `0`  | Success                           |
| `1`  | Error (details printed to stderr) |

When using `--wait`, the exit code reflects whether the files were saved before closing.
