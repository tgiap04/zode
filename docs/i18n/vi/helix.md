---
title: Helix Mode - Zode
description: Helix-style keybindings and modal editing in Zode. Selection-first editing built on top of Vim mode.
---

# Chế độ Helix

_Đang phát triển. Chưa phải phím tắt Helix nào cũng được hỗ trợ._

Chế độ Helix của Zode là lớp giả lập, mang phím tắt và cách soạn thảo theo chế độ (modal) kiểu Helix vào Zode. Nó xây trên [chế độ Vim](./vim.md) của Zode nên phần lớn chức năng cốt lõi dùng chung. Bật `helix_mode` thì `vim_mode` cũng được bật theo.

Các tính năng Vim cũng dùng được trong chế độ Helix được hướng dẫn trong [tài liệu chế độ Vim](./vim.md).

Muốn xem tiến độ hiện tại của chế độ Helix, hoặc đề nghị thêm một tính năng Helix còn thiếu, xem [thảo luận "Are we Helix yet?"](https://github.com/zed-industries/zed/discussions/33580).

Danh sách đầy đủ phím tắt mặc định của Helix có trong [tài liệu chính thức của Helix](https://docs.helix-editor.com/keymap.html).

## Khác biệt cốt lõi

Text object nào dùng được với `m i` hoặc `m a` thì cũng dùng được với `]` và `[`. Ví dụ `] (` chọn cặp dấu ngoặc đơn kế tiếp sau con trỏ.
