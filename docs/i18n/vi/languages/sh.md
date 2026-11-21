---
title: Shell Script
description: "Configure Shell Script language support in Zode, including language servers, formatting, and debugging."
---

# Shell Script

Zode hỗ trợ sẵn Shell Script (bash, zsh, dash, sh).

- Tree-sitter: [tree-sitter/tree-sitter-bash](https://github.com/tree-sitter/tree-sitter-bash)

## Cài đặt

Cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Shell Script, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Shell Script": {
      "tab_size": 2,
      "hard_tabs": false
    }
  }
```

### Định dạng mã

Zode hỗ trợ tự động định dạng Shell Script bằng công cụ bên ngoài như [`shfmt`](https://github.com/mvdan/sh).

1. Cài `shfmt`:

```sh
brew install shfmt            # macos (homebrew)
sudo apt-get install shfmt    # debian/ubuntu
dnf install shfmt             # fedora
yum install shfmt             # redhat
pacman -Sy shfmt              # archlinux
choco install shfmt           # windows (chocolatey)
```

2. Đảm bảo `shfmt` có trong path và kiểm tra phiên bản:

```sh
which shfmt
shfmt --version
```

3. Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Shell Script, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Shell Script": {
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "shfmt",
          // Change `--indent 2` to match your preferred tab_size
          "arguments": ["--filename", "{buffer_path}", "--indent", "2"]
        }
      }
    }
  }
```

## Xem thêm:

- [Tài liệu Zode: Hỗ trợ ngôn ngữ: Bash](./bash.md)
- [Tài liệu Zode: Hỗ trợ ngôn ngữ: Fish](./fish.md)
