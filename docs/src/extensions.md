---
title: Extensions
description: "Extend Zode with themes, language support, AI tools, and more through the extension system."
---

# Extensions

Zode lets you add new functionality using user-defined extensions.

- [Installing Extensions](./extensions/installing-extensions.md)
- [Extension Capabilities](./extensions/capabilities.md)
- [Developing Extensions](./extensions/developing-extensions.md)
  - [Developing Language Extensions](./extensions/languages.md)
  - [Developing Debugger Extensions](./extensions/debugger-extensions.md)
  - [Developing Themes](./extensions/themes.md)
  - [Developing Icon Themes](./extensions/icon-themes.md)
  - [Developing Snippets](./extensions/snippets.md)

## Agent-server extensions are not supported

An extension can declare an agent server in its `extension.toml`:

```toml
[agent_servers.myagent]
name = "My Agent"
```

This build does not run one. The `agent_servers` field was removed along with the
ACP registry it was built to feed, and nothing replaced it — agents here are the
four built-in CLIs on the sidebar rail (`claude`, `codex`, `agy`, `copilot`), each
run as a terminal session, plus whatever command you name yourself in the
`agent_servers` **setting** (which is a different thing from the manifest field;
see [Configuring Zode](./configuring-zed.md)).

What this means in practice:

- An extension whose manifest carries an `agent_servers` stanza **still installs**,
  and every other capability it provides keeps working. The stanza is ignored, not
  rejected — this build's manifest parser does not deny unknown fields.
- Because it is ignored rather than rejected, nothing tells you at install time.
  Where the extension registry reports the capability, the extensions page labels
  it **"Agent Servers (unsupported)"**, which is the only warning you get.
- Upstream Zed's own banner used to say agent-server extensions were being
  deprecated "in favor of the ACP registry". That is not what happened here: the
  registry was removed too. There is no migration path, and the banner is gone.

Nothing in this build downloads an agent from a URL or a package registry. If you
want an agent, install its CLI yourself; the rail's button offers you the vendor's
install command when the CLI is missing.
