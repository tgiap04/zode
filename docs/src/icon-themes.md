---
title: Icon Themes
description: "Zode's file and folder icons come from the Material Icon Theme, in colour, and cannot be changed."
---

# Icon Themes

Zode ships one icon theme — the [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme), rendered in its real colours — and does not offer a choice of icon themes. There is no Icon Theme Selector, and no other icon theme takes effect, whether named in settings or contributed by an extension.

File and folder name matching for icons is case-insensitive, so names like `Dockerfile`, `README.MD`, or `.PNG` still resolve to the right icon.

## The `icon_theme` Setting

The `icon_theme` setting still exists in the settings schema — including the object form for light/dark variants — because the settings machinery resolves it unconditionally and a missing key would be a startup panic. Whatever name you put here has no effect on which icons are drawn:

```json [settings]
{
  "icon_theme": "Material Icon Theme"
}
```

This is the shipped default. Writing any other name, or the `{ "mode", "light", "dark" }` object form, does not change the icons shown.

## Icon Theme Development

Extensions may still declare an `icon_themes` directory (see [Developing Zode Icon Themes](./extensions/icon-themes.md)), but a contributed icon theme is never selected — the file icons are part of what this build is, not a user preference.
