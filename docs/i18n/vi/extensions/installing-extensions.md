---
title: Installing Extensions
description: "Browse, install, and manage extensions from the Zode Extension Gallery."
---

# Cài đặt extension {#installing-extensions}

Extension bổ sung tính năng cho Zode, gồm ngôn ngữ, giao diện (theme) và công cụ AI. Bạn duyệt và cài chúng từ Extension Gallery.

Mở Extension Gallery bằng {#kb zode::Extensions}, hoặc chọn "Zode > Extensions" trên thanh menu.

## Vị trí cài đặt

- Trên macOS, extension được cài trong `~/Library/Application Support/Zode/extensions`.
- Trên Linux, extension nằm trong `$XDG_DATA_HOME/zode/extensions` hoặc `~/.local/share/zode/extensions`.
- Trên Windows, thư mục là `%LOCALAPPDATA%\Zode\extensions`.

Thư mục này gồm hai thư mục con:

- `installed`, chứa mã nguồn của từng extension.
- `work`, chứa các file do chính extension tạo ra, ví dụ language server đã tải về.

## Tự động cài đặt

Để tự động hoá việc cài/gỡ extension, xem tài liệu của [auto_install_extensions](../reference/all-settings.md#auto-install-extensions).
