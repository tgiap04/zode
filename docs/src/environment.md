---
title: Environment Variables - Zode
description: How Zode detects and uses environment variables. Shell integration, dotenv support, and troubleshooting.
---

# Environment Variables

_**Note**: The following only applies to Zode 0.152.0 and later._

Multiple features in Zode are affected by environment variables:

- [Tasks](./tasks.md)
- [Built-in terminal](./terminal.md)
- Look-up of language servers
- Language servers

To make the best use of these features, it helps to understand where Zode gets environment variables and how it uses them.

## Where does Zode get its environment variables from?

How Zode starts affects which environment variables it can use. That includes launching from the macOS Dock, a Linux window manager, or the `zed` CLI.

### Launched from the CLI

If Zode is opened via the CLI (`zed`), it will inherit the environment variables from the surrounding shell session.

That means if you do

```
$ export MY_ENV_VAR=hello
$ zed .
```

the environment variable `MY_ENV_VAR` is now available inside Zode. For example, in the built-in terminal.

Starting with Zode 0.152.0, the CLI `zed` will _always_ pass along its environment to Zode, regardless of whether a Zode instance was previously running or not. Prior to Zode 0.152.0 this was not the case and only the first Zode instance would inherit the environment variables.

### Launched via window manager, Dock, or launcher

When Zode has been launched via the macOS Dock, or a GNOME or KDE icon on Linux, or an application launcher like Alfred or Raycast, it has no surrounding shell environment from which to inherit its environment variables.

To still have a useful environment, Zode spawns a login shell in the user's home directory and reads its environment. This environment is then set on the Zode _process_, so all Zode windows and projects inherit it.

Since that can lead to problems for users who need different environment variables per project (for example with `direnv`, `asdf`, or `mise`), Zode spawns another login shell when opening a project. This second shell runs in the project's directory. The environment from that shell is _not_ set on the process, because opening a new project would otherwise change the environment for all Zode windows. Instead, that environment is stored and passed along when running tasks, opening terminals, or spawning language servers.

## Where and how are environment variables used?

There are two sets of environment variables:

1. Environment variables of the Zode process
2. Environment variables stored per project

The variables from (1) are always used, since they are stored on the process itself and every spawned process (tasks, terminals, language servers, ...) will inherit them by default.

The variables from (2) are used explicitly, depending on the feature.

### Tasks

Tasks are spawned with a combined environment. In order of precedence (low to high, with the last overwriting the first):

- the Zode process environment
- if the project was opened from the CLI: the CLI environment
- if the project was not opened from the CLI: the project environment variables obtained by running a login shell in the project's root folder
- optional, explicitly configured environment in settings

### Built-in terminal

Built-in terminals, like tasks, are spawned with a combined environment. In order of precedence (low to high):

- the Zode process environment
- if the project was opened from the CLI: the CLI environment
- if the project was not opened from the CLI: the project environment variables obtained by running a login shell in the project's root folder
- optional, explicitly configured environment in settings

### Look-up of language servers

For some languages the language server adapters lookup the binary in the user's `$PATH`. Examples:

- Go
- Zig
- Rust (if [configured to do so](./languages/rust.md#binary))
- C
- TypeScript

For this look-up, Zode uses the following environment:

- if the project was opened from the CLI: the CLI environment
- if the project was not opened from the CLI: the project environment variables obtained by running a login shell in the project's root folder

### Language servers

After looking up a language server, Zode starts them.

These language server processes always inherit Zode's process environment. But, depending on the language server look-up, additional environment variables might be set or overwrite the process environment.

- If the language server was found in the project environment's `$PATH`, then that project environment is passed along to the language server process. Where the project environment comes from depends on how the project was opened (via CLI or not). See the previous section on language server look-up.
- If the language server was not found in the project environment, Zode tries to install and start it globally. In that case, the process inherits Zode's process environment and, if the project was opened via CLI, the CLI environment.
