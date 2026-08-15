---
title: CLI Reference
description: "Reference for Zode's command-line interface (CLI), including opening files and directories, integrating with tools, and controlling Zode from scripts."
---

# CLI Reference

Use Zode's command-line interface (CLI) to open files and directories, integrate with other tools, and control Zode from scripts.

## Installation

**macOS:** Run the `cli: install` command from the command palette ({#kb command_palette::Toggle}) to install the `zed` CLI to `/usr/local/bin/zed`.

**Linux:** The CLI is included with Zode packages. The binary name may vary by distribution (commonly `zed` or `zeditor`).

**Windows:** The CLI is included with Zode. Add Zode's installation directory to your PATH, or use the full path to `zed.exe`.

## Usage

```sh
zed [OPTIONS] [PATHS]...
```

## Opening Files and Directories

Open a file:

```sh
zed myfile.txt
```

Open a directory as a workspace:

```sh
zed ~/projects/myproject
```

Open multiple files or directories:

```sh
zed file1.txt file2.txt ~/projects/myproject
```

Open a file at a specific line and column:

```sh
zed myfile.txt:42        # Open at line 42
zed myfile.txt:42:10     # Open at line 42, column 10
```

## Options

### `-w`, `--wait`

Wait for all opened files to be closed before the CLI exits. When opening a directory, waits until the window is closed.

This is useful for integrating Zode with tools that expect an editor to block until editing is complete (e.g., `git commit`):

```sh
export EDITOR="zed --wait"
git commit  # Opens Zode and waits for you to close the commit message file
```

### `-n`, `--new`

Open paths in a new workspace window, even if the paths are already open in an existing window:

```sh
zed -n ~/projects/myproject
```

### `-a`, `--add`

Add paths to the currently focused workspace instead of opening a new window. When multiple workspace windows are open, files open in the focused window:

```sh
zed -a newfile.txt
```

### `-r`, `--reuse`

Reuse an existing window, replacing its current workspace with the new paths:

```sh
zed -r ~/projects/different-project
```

### `--diff <OLD_PATH> <NEW_PATH>`

Open a diff view comparing two files. Can be specified multiple times:

```sh
zed --diff file1.txt file2.txt
zed --diff old.rs new.rs --diff old2.rs new2.rs
```

### `--foreground`

Run Zode in the foreground, keeping the terminal attached. Useful for debugging:

```sh
zed --foreground
```

### `--user-data-dir <DIR>`

Use a custom directory for all user data (database, extensions, logs) instead of the default location:

```sh
zed --user-data-dir ~/.zed-custom
```

Default locations:

- **macOS:** `~/Library/Application Support/Zode`
- **Linux:** `$XDG_DATA_HOME/zed` (typically `~/.local/share/zed`)
- **Windows:** `%LOCALAPPDATA%\Zode`

### `-v`, `--version`

Print Zode's version and exit:

```sh
zed --version
```

### `--uninstall`

Uninstall Zode and remove all related files (macOS and Linux only):

```sh
zed --uninstall
```

### `--zed <PATH>`

Specify a custom path to the Zode application or binary:

```sh
zed --zode /path/to/Zode.app myfile.txt
```

## Reading from Standard Input

Read content from stdin by passing `-` as the path:

```sh
echo "Hello, World!" | zed -
cat myfile.txt | zed -
ps aux | zed -
```

This creates a temporary file with the stdin content and opens it in Zode.

## URL Handling

The CLI can open `zed://`, `http://`, and `https://` URLs:

```sh
zed zed://settings
zed https://github.com/zed-industries/zed
```

## Using Zode as Your Default Editor

Set Zode as your default editor for Git and other tools:

```sh
export EDITOR="zed --wait"
export VISUAL="zed --wait"
```

Add these lines to your shell configuration file (e.g., `~/.bashrc`, `~/.zshrc`).

## macOS: Switching Release Channels

On macOS, you can launch a specific release channel by passing the channel name as the first argument:

```sh
zed --stable myfile.txt
zed --preview myfile.txt
zed --nightly myfile.txt
```

## WSL Integration (Windows)

On Windows, the CLI supports opening paths from WSL distributions. This is handled automatically when launching Zode from within WSL.

## Exit Codes

| Code | Meaning                           |
| ---- | --------------------------------- |
| `0`  | Success                           |
| `1`  | Error (details printed to stderr) |

When using `--wait`, the exit code reflects whether the files were saved before closing.
