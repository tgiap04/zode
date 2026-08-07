# Business Context — F012_ExtensionSystem

## Why It Matters

Extensions let people add languages, tools, and integrations to the editor without the core team
having to build everything in. But letting someone else's code run on your machine is risky, so
this feature exists to give developers a fast way to install, reload, and iterate on extensions
while also making sure an extension can never do more than it explicitly said it would do up
front.

## Who Uses It

- **Everyday developer** — installs extensions from the Extensions page (or accepts a suggestion
  banner when opening a file type without support) to get language support, tools, or integrations
  they need, without worrying that an extension could silently reach beyond what it advertised.
- **Extension author** — writes and tests a local ("dev") extension before publishing it, declaring
  up front exactly which external commands, download hosts, and package installs their extension
  is allowed to use.
- **Developer troubleshooting a connected tool** — restarts a stalled context-server connection
  (an external tool integration reached over the Model Context Protocol) from its status view
  instead of restarting the whole editor.

## What They Do

1. A developer opens a file type the editor doesn't yet support and is shown a suggestion to
   install the matching extension — they accept it, or dismiss it so it's never shown again for
   that file type.
2. An extension author places their extension's manifest and code in a local folder, declares the
   external commands, download sources, and installable packages it needs, and installs it as a
   dev extension for testing.
3. The author edits their extension's source and triggers a rebuild — the editor recompiles it and
   swaps in the new version live, so they don't have to reinstall from scratch each time.
4. Whenever an installed extension's code changes on disk, the developer reloads all extensions at
   once so the update takes effect immediately, without restarting the editor.
5. If an extension ever tries to do something it didn't declare up front — run an unlisted command,
   fetch from an unlisted source, or install an unlisted package — the request is quietly refused
   and the extension receives an error, rather than the editor crashing or the action silently
   succeeding.
6. If a connected external tool (a context server) drops its connection or needs reconfiguring, the
   developer restarts it from its status view and watches it reconnect.

## Unresolved Questions

- **Granting capabilities**: it isn't clear from the code alone whether end users are expected to
  manually edit their settings file to grant an extension additional capabilities, or whether
  there's a planned in-app approval flow (like the "trust this folder" prompt used elsewhere) that
  simply wasn't found in this pass. Product/design confirmation needed.
