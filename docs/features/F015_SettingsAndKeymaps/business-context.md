# Business Context — F015_SettingsAndKeymaps

## Why It Matters

Developers customize their editor constantly — font size, keybindings, color theme, and dozens of
other preferences. This feature lets those changes take effect instantly, without a restart, and
protects the developer's configuration from being silently broken or lost when the editor's
settings format changes during an update.

## Who Uses It

- **Every developer using the editor** — anyone who ever adjusts a setting, remaps a key, or
  switches a color theme touches this feature, even if only once.
- **Developers migrating from another editor** — they use the base keymap preset switcher to
  carry over muscle memory (e.g. VS Code or Sublime-style shortcuts) instead of relearning
  bindings from scratch.
- **Zed staff** — see a small set of in-progress features enabled by default through the
  feature-flag mechanism this feature also owns, so they can dogfood upcoming work before it
  reaches everyone else.

## What They Do

1. A developer opens the Settings editor and changes a value (for example, the editor font size)
   — the change is saved and takes effect immediately across every open file, with no restart.
2. A developer opens the keymap editor, records a new key combination for an action, and confirms
   it — the new shortcut works right away, and the old one stops working.
3. A developer switches the base keymap preset (for example, to a familiar layout from another
   editor) — that preset's shortcuts activate immediately across every open window.
4. When Zed detects that a developer's saved settings or keymap file uses an older format, it
   shows a notice offering to update the file. If the developer accepts:
   - Zed first saves a backup copy of the file exactly as it was, so nothing is lost.
   - Only after the backup is safely saved does Zed rewrite the live file in the new format.
5. If anything goes wrong partway through, the developer's original file is never left in a
   half-updated state — the backup step must fully succeed before the live file is touched at all.

## Unresolved Questions

- **Extension-contributed theme reload scope**: whether theme files shipped by third-party
  extensions hot-reload the same way a user's own bundled theme file does was not confirmed from
  the code in this pass.
