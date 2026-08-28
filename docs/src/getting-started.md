---
title: Getting Started with Zode
description: Get started with Zode, the fast open-source code editor. Essential commands, environment setup, and navigation basics.
---

# Getting Started

Zode is an open-source code editor with built-in collaboration and AI tools.

This guide covers the essential commands, environment setup, and navigation basics.

## Quick Start

### Welcome Page

When you open Zode without a folder, you see the welcome page in the main editor area. The welcome page offers quick actions to open a folder, clone a repository, or view documentation. Once you open a folder or file, the welcome page disappears. If you split the editor into multiple panes, the welcome page appears only in the center pane when empty—other panes show a standard empty state.

To reopen the welcome page, close all items in the center pane or use the command palette to search for "Welcome".

### 1. Open a Project

Open a folder from the command line:

```sh
zed ~/projects/my-app
```

Or use `Cmd+O` (macOS) / `Ctrl+O` (Linux/Windows) to open a folder from within Zode.

### 2. Learn the Essential Commands

| Action          | macOS         | Linux/Windows  |
| --------------- | ------------- | -------------- |
| Command palette | `Cmd+Shift+P` | `Ctrl+Shift+P` |
| Go to file      | `Cmd+P`       | `Ctrl+P`       |
| Go to symbol    | `Cmd+Shift+O` | `Ctrl+Shift+O` |
| Find in project | `Cmd+Shift+F` | `Ctrl+Shift+F` |
| Toggle terminal | `` Ctrl+` ``  | `` Ctrl+` ``   |
| Open settings   | `Cmd+,`       | `Ctrl+,`       |

The command palette (`Cmd+Shift+P`) is your gateway to every action in Zode. If you forget a shortcut, search for it there.

### 3. Configure Your Editor

Open the Settings Editor with `Cmd+,` (macOS) or `Ctrl+,` (Linux/Windows). Search for any setting and change it directly.

Common first changes:

- **Theme**: Press `Cmd+K Cmd+T` (macOS) or `Ctrl+K Ctrl+T` (Linux/Windows) to open the theme selector
- **Font**: Search for `buffer_font_family` in Settings
- **Format on save**: Search for `format_on_save` and set to `on`

### 4. Set Up Your Language

Zode includes built-in support for many languages. For others, install the extension:

1. Open Extensions with `Cmd+Shift+X` (macOS) or `Ctrl+Shift+X` (Linux/Windows)
2. Search for your language
3. Click Install

See [Languages](./languages.md) for language-specific setup instructions.

### 5. Try AI Features

Zode includes built-in AI assistance. An agent opens as a tab beside your files rather than in a separate panel:

- Click the Claude Code, Codex, Antigravity, or Copilot icon in the sidebar rail to open that agent — it runs as a terminal session of the agent's own CLI, in a tab beside your files.
- Each agent needs its own CLI installed: `claude`, `codex`, `agy`, and `copilot` respectively. Clicking an agent whose CLI is missing offers you the vendor's install command rather than failing silently.
- To start a second session of an agent that's already open, use the `+` menu in the tab bar and pick "New Claude Code", "New Codex", "New Antigravity", or "New Copilot".

Two things are uneven across the four, because the CLIs themselves are uneven:

- **Session history** covers Claude Code, Codex and Copilot, each of which writes its conversations somewhere on this machine. Antigravity does not — `agy` keeps its conversations server-side and leaves nothing on disk, so it has no rows in the history panel.
- **Subscription quota** on the status bar covers Claude Code and Codex only. Neither Antigravity nor Copilot publishes a way to read it that does not require this editor to hold a credential of its own — see [Agent Usage Indicator](./agent-usage.md) for what was tried.

For inline assistance, use `Ctrl+Enter` (macOS, Linux, and Windows all use the same key here).

## Coming from Another Editor?

We have dedicated guides for switching from other editors:

- [VS Code](./migrate/vs-code.md) — Import settings, map keybindings, find equivalent features
- [IntelliJ IDEA](./migrate/intellij.md) — Adapt to Zode's approach to navigation and refactoring
- [PyCharm](./migrate/pycharm.md) — Set up Python development in Zode
- [WebStorm](./migrate/webstorm.md) — Configure JavaScript/TypeScript workflows
- [RustRover](./migrate/rustrover.md) — Rust development in Zode

You can also enable familiar keybindings:

- **Vim**: Enable `vim_mode` in settings. See [Vim Mode](./vim.md).
- **Helix**: Enable `helix_mode` in settings. See [Helix Mode](./helix.md).

## Join the Community

Zode is open source. Join us on GitHub or in Discord to contribute code, report bugs, or suggest features.

- [Discord](https://discord.com/invite/zedindustries)
- [GitHub Discussions](https://github.com/zed-industries/zed/discussions)
- [Zode Reddit](https://www.reddit.com/r/ZedEditor)
