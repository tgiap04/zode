---
title: Code Completions - Zode
description: Zode's code completions from language servers and edit predictions. Configure autocomplete behavior, snippets, and documentation display.
---

# Gợi ý code (Completions)

Zode có hai nguồn gợi ý:

1. "Code Completions" do các Language Server (LSP) cung cấp, được Zode tự động cài đặt hoặc cài qua [extension ngôn ngữ của Zode](languages.md).
2. "Edit Predictions" do model Zeta của chính Zode hoặc các nhà cung cấp bên ngoài như [GitHub Copilot](#github-copilot) cung cấp.

## Gợi ý code từ Language Server {#code-completions}

Khi có language server phù hợp, Zode gợi ý tên biến, hàm và các symbol khác trong file hiện tại. Bạn có thể tắt tính năng này bằng cách thêm đoạn sau vào file `settings.json` của Zode:

```json [settings]
"show_completions_on_input": false
```

Bạn có thể gọi gợi ý thủ công bằng `ctrl-space`, hoặc chạy action `editor::ShowCompletions` từ bảng lệnh (command palette).

> Lưu ý: Để dùng `ctrl-space` trong Zode, bạn phải tắt phím tắt toàn cục của macOS.
> Mở **System Settings** > **Keyboard** > **Keyboard Shortcut**s >
> **Input Sources** rồi bỏ chọn **Select the previous input source**.

Xem thêm:

- [Cấu hình các ngôn ngữ được hỗ trợ](./configuring-languages.md)
- [Danh sách ngôn ngữ Zode hỗ trợ](./languages.md)

## Edit Predictions {#edit-predictions}

Zode hỗ trợ sẵn việc dự đoán nhiều chỉnh sửa cùng lúc [qua Zeta](https://huggingface.co/zed-industries/zeta), model mã nguồn mở và dữ liệu mở của Zode.
Dự đoán chỉnh sửa hiện ra khi bạn gõ, và phần lớn thời gian bạn chỉ cần nhấn `tab` để chấp nhận.
