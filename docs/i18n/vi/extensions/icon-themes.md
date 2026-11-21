---
title: Icon Themes
description: "Icon Themes for Zode extensions."
---

# Bộ biểu tượng (Icon Theme)

Extension có thể cung cấp bộ biểu tượng để đổi biểu tượng mà Zode dùng cho thư mục và file.

> **Lưu ý:** Trong bản build này, biểu tượng file được cố định là Material Icon Theme (xem [Bộ biểu tượng](../icon-themes.md)). Extension vẫn có thể khai báo thư mục `icon_themes` theo schema này và nó nạp không lỗi, nhưng không bao giờ được chọn — không có bộ chọn bộ biểu tượng, và không có cài đặt nào trỏ tới nó.

## Extension mẫu

[Material Icon Theme](https://github.com/zed-extensions/material-icon-theme) là ví dụ về cấu trúc của một extension chứa bộ biểu tượng.

## Cấu trúc thư mục

Extension bộ biểu tượng có hai thư mục quan trọng:

- `icon_themes`: Thư mục này chứa một hoặc nhiều file JSON định nghĩa bộ biểu tượng.
- `icons`: Thư mục này chứa các tài nguyên biểu tượng đi kèm extension. Bạn có thể tạo thư mục con trong đó tuỳ ý.

Mỗi file bộ biểu tượng phải tuân theo JSON schema tại [`https://zed.dev/schema/icon_themes/v0.3.0.json`](https://zed.dev/schema/icon_themes/v0.3.0.json).

Đây là ví dụ về cấu trúc một bộ biểu tượng:

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

Mỗi đường dẫn biểu tượng được tính tương đối so với thư mục gốc của extension.

Trong ví dụ này, extension sẽ có cấu trúc như sau:

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
