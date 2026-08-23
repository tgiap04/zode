---
title: Icon Themes
description: "Icon Themes for Zode extensions."
---

# Icon Themes

Extensions may provide icon themes to change the icons Zode uses for folders and files.

> **Note:** In this build, the file icons are fixed to the Material Icon Theme (see [Icon Themes](../icon-themes.md)). An extension can still declare an `icon_themes` directory following this schema, and it loads without error, but it is never selected — there is no icon theme picker, and no setting resolves to it.

## Example extension

The [Material Icon Theme](https://github.com/zed-extensions/material-icon-theme) serves as an example for the structure of an extension containing an icon theme.

## Directory structure

There are two important directories for an icon theme extension:

- `icon_themes`: This directory will contain one or more JSON files containing the icon theme definitions.
- `icons`: This directory contains the icon assets distributed with the extension. You can create subdirectories in this directory as needed.

Each icon theme file should adhere to the JSON schema specified at [`https://zed.dev/schema/icon_themes/v0.3.0.json`](https://zed.dev/schema/icon_themes/v0.3.0.json).

Here is an example icon theme structure:

```json [icon-theme]
{
  "$schema": "https://zed.dev/schema/icon_themes/v0.3.0.json",
  "name": "My Icon Theme",
  "author": "Your Name",
  "themes": [
    {
      "name": "My Icon Theme",
      "appearance": "dark",
      "directory_icons": {
        "collapsed": "./icons/folder.svg",
        "expanded": "./icons/folder-open.svg"
      },
      "named_directory_icons": {
        "stylesheets": {
          "collapsed": "./icons/folder-stylesheets.svg",
          "expanded": "./icons/folder-stylesheets-open.svg"
        }
      },
      "chevron_icons": {
        "collapsed": "./icons/chevron-right.svg",
        "expanded": "./icons/chevron-down.svg"
      },
      "file_stems": {
        "Makefile": "make"
      },
      "file_suffixes": {
        "mp3": "audio",
        "rs": "rust"
      },
      "file_icons": {
        "audio": { "path": "./icons/audio.svg" },
        "default": { "path": "./icons/file.svg" },
        "make": { "path": "./icons/make.svg" },
        "rust": { "path": "./icons/rust.svg" }
        // ...
      }
    }
  ]
}
```

Each icon path is resolved relative to the root of the extension directory.

In this example, the extension would have this structure:

```
extension.toml
icon_themes/
  my-icon-theme.json
icons/
  audio.svg
  chevron-down.svg
  chevron-right.svg
  file.svg
  folder-open.svg
  folder.svg
  rust.svg
```
