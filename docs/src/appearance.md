---
title: Appearance and Visual Customization - Zode
description: Customize Zode's themes, fonts, icons, UI density, and other visual settings to match your preferences.
---

# Appearance

Customize Zode's visual appearance to match your preferences. This guide covers themes, fonts, icons, and other visual settings.

For information on how the settings system works, see [All Settings](./reference/all-settings.md).

## Customize Zode in 5 Minutes

Here's how to make Zode feel like home:

1. **Pick a theme**: Press {#kb theme_selector::Toggle} to open the Theme Selector. Arrow through the list to preview themes in real time, and press Enter to apply.

2. **Toggle light/dark mode quickly**: Press {#kb theme::ToggleMode}. If you currently use a static `"theme": "..."` value, the first toggle converts it to dynamic mode settings with default themes.

3. **Set your font**: Open the Settings Editor with {#kb zed::OpenSettings} and search for `buffer_font_family`. Set it to your preferred coding font.

4. **Adjust font size**: In the same Settings Editor, search for `buffer_font_size` and `ui_font_size` to tweak the editor and interface text sizes.

That's it. You now have a personalized Zode setup.

## Themes

Install themes from the Extensions page ({#action zed::Extensions}), then switch between them with the Theme Selector ({#kb theme_selector::Toggle}).

Zode supports separate themes for light and dark mode with automatic switching based on your system preference:

```json [settings]
{
  "theme": {
    "mode": "system",
    "light": "One Light",
    "dark": "One Dark"
  }
}
```

You can also override specific theme attributes for fine-grained control.

→ [Themes documentation](./themes.md)

## Icon Themes

File and folder icons in the Project Panel and tabs come from the Material Icon Theme, in colour, and cannot be changed — there is no Icon Theme Selector, and no other icon theme takes effect.

→ [Icon Themes documentation](./icon-themes.md)

## Fonts

Zode uses three font settings for different contexts:

| Setting                | Used for                  |
| ---------------------- | ------------------------- |
| `buffer_font_family`   | Editor text               |
| `ui_font_family`       | Interface elements        |
| `terminal.font_family` | [Terminal](./terminal.md) |

Example configuration:

```json [settings]
{
  "buffer_font_family": "JetBrains Mono",
  "buffer_font_size": 14,
  "ui_font_family": "Inter",
  "ui_font_size": 16,
  "terminal": {
    "font_family": "JetBrains Mono",
    "font_size": 14
  }
}
```

### Font Ligatures

To disable font ligatures:

```json [settings]
{
  "buffer_font_features": {
    "calt": false
  }
}
```

### Line Height

Adjust line spacing with `buffer_line_height`:

- `"comfortable"` — 1.618 ratio (default)
- `"standard"` — 1.3 ratio
- `{ "custom": 1.5 }` — Custom ratio

## UI Elements

Zode provides extensive control over UI elements including:

- **Tab bar** — Show/hide, navigation buttons, file icons, git status
- **Status bar** — Language selector, cursor position, line endings, [agent usage](./agent-usage.md)
- **Scrollbar** — Visibility, git diff indicators, search results
- **Minimap** — Code overview display
- **Gutter** — Line numbers, fold indicators, breakpoints
- **Panels** — Project Panel sizing; Terminal sizing and docking (Project Panel always docks on the right, opposite the project rail, and cannot be moved)

→ [Visual Customization documentation](./visual-customization.md) for all UI element settings

## The Project Rail

Every open project has a square on the rail down the left edge. Clicking one
switches to that project; right-clicking opens its own menu:

- **Open Project in New Window** — moves the project out of this window. Only
  offered when there is more than one project open, since otherwise the window it
  would move to is this one.
- **Reveal in Finder** / **Copy Project Path** — a project opened over several
  folders copies all of them, one per line.
- **Change Initials…** — the two letters drawn on the square. Useful when two
  projects reduce to the same initials. This changes the square only: the
  project's name, its path and the window title are untouched.
- **Change Colour…** — picks a colour for the square, previewed on the square
  itself while you choose. **Use Default** returns it to the theme's colour. The
  letters switch between light and dark on their own so they stay readable on
  whatever you pick.
- **Remove Project** — takes the project off this window. Nothing on disk is
  touched and the project stays in your recent list.

Squares can be **dragged to reorder** them, and the order is remembered between
sessions. A newly opened project still appears at the top. Dragging a square off
the rail entirely moves that project to a new window — both that and **Remove**
ask for confirmation first, since both take the project off this window.

## What's Next

- [All Settings](./reference/all-settings.md) — Complete settings reference
- [Key bindings](./key-bindings.md) — Customize keyboard shortcuts
- [Vim Mode](./vim.md) — Enable modal editing
