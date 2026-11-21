---
title: Bash
description: "Configure Bash language support in Zode, including language servers, formatting, and debugging."
---

# Bash

Hỗ trợ Bash có qua [extension Bash](https://github.com/zed-extensions/bash).

- Tree-sitter: [tree-sitter/tree-sitter-bash](https://github.com/tree-sitter/tree-sitter-bash)
- Language server: [bash-lsp/bash-language-server](https://github.com/bash-lsp/bash-language-server)

## Cấu hình

Khi có `shellcheck`, `bash-language-server` sẽ tự dùng nó để đưa ra chẩn đoán.

### Cài `shellcheck`:

```sh
brew install shellcheck             # macOS (HomeBrew)
apt-get install shellcheck          # Ubuntu/Debian
pacman -S shellcheck                # ArchLinux
dnf install shellcheck              # Fedora
yum install shellcheck              # CentOS/RHEL
zypper install shellcheck           # openSUSE
choco install shellcheck            # Windows (Chocolatey)
```

Rồi kiểm tra xem nó có trong PATH không:

```sh
which shellcheck
shellcheck --version
```

Muốn tùy chỉnh các cảnh báo/lỗi được báo, chỉ cần tạo file `.shellcheckrc` ở thư mục gốc của dự án hoặc trong thư mục home (`~/.shellcheckrc`). Xem thêm: [tài liệu shellcheck](https://github.com/koalaman/shellcheck/wiki/Ignore#ignoring-one-or-more-types-of-errors-forever).

### Xem thêm:

- [Tài liệu Zode: Hỗ trợ ngôn ngữ: Shell Scripts](./sh.md)
