---
title: Fish
description: "Configure Fish language support in Zode, including language servers, formatting, and debugging."
---

# Fish

Hỗ trợ ngôn ngữ Fish trong Zode do [extension Fish](https://github.com/hasit/zed-fish) do cộng đồng duy trì cung cấp.
Báo lỗi tại: [https://github.com/hasit/zed-fish/issues](https://github.com/hasit/zed-fish/issues)

- Tree-sitter: [ram02z/tree-sitter-fish](https://github.com/ram02z/tree-sitter-fish)

### Định dạng mã

Zode hỗ trợ tự động định dạng mã fish bằng công cụ bên ngoài như [`fish_indent`](https://fishshell.com/docs/current/cmds/fish_indent.html), đi kèm sẵn với fish.

1. Đảm bảo `fish_indent` có trong path và kiểm tra phiên bản:

```sh
which fish_indent
fish_indent --version
```

2. Cấu hình Zode tự động định dạng mã fish bằng `fish_indent`:

```json [settings]
  "languages": {
    "Fish": {
      "formatter": {
        "external": {
          "command": "fish_indent"
        }
      }
    }
  },
```
