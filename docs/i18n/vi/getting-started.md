---
title: Getting Started with Zode
description: Get started with Zode, the fast open-source code editor. Essential commands, environment setup, and navigation basics.
---

# Bắt đầu

Zode là trình soạn thảo code mã nguồn mở, tích hợp sẵn công cụ cộng tác và AI.

Hướng dẫn này nói về các lệnh thiết yếu, thiết lập môi trường và cách di chuyển cơ bản.

## Bắt đầu nhanh

### Trang chào mừng

Khi mở Zode mà không chọn thư mục, bạn thấy trang chào mừng trong vùng soạn thảo chính. Trang này có các thao tác nhanh để mở thư mục, clone repository hoặc xem tài liệu. Khi bạn mở một thư mục hoặc file, trang chào mừng biến mất. Nếu bạn chia trình soạn thảo thành nhiều khung, trang chào mừng chỉ hiện ở khung giữa khi khung trống — các khung khác hiện trạng thái trống thông thường.

Để mở lại trang chào mừng, đóng hết mọi mục trong khung giữa hoặc dùng bảng lệnh (command palette) tìm "Welcome".

### 1. Mở một dự án

Mở thư mục từ dòng lệnh:

```sh
zode ~/projects/my-app
```

Hoặc dùng `Cmd+O` (macOS) / `Ctrl+O` (Linux/Windows) để mở thư mục ngay trong Zode.

### 2. Học các lệnh thiết yếu

| Thao tác         | macOS         | Linux/Windows  |
| ---------------- | ------------- | -------------- |
| Bảng lệnh        | `Cmd+Shift+P` | `Ctrl+Shift+P` |
| Đi tới file      | `Cmd+P`       | `Ctrl+P`       |
| Đi tới symbol    | `Cmd+Shift+O` | `Ctrl+Shift+O` |
| Tìm trong dự án  | `Cmd+Shift+F` | `Ctrl+Shift+F` |
| Bật/tắt terminal | `` Ctrl+` ``  | `` Ctrl+` ``   |
| Mở cài đặt       | `Cmd+,`       | `Ctrl+,`       |

Bảng lệnh (`Cmd+Shift+P`) là cửa ngõ tới mọi action trong Zode. Quên phím tắt thì cứ tìm ở đó.

### 3. Cấu hình trình soạn thảo

Mở Settings Editor bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows). Tìm bất kỳ cài đặt nào và đổi trực tiếp.

Những thay đổi đầu tiên thường gặp:

- **Giao diện (theme)**: Nhấn `Cmd+K Cmd+T` (macOS) hoặc `Ctrl+K Ctrl+T` (Linux/Windows) để mở bộ chọn theme
- **Font**: Tìm `buffer_font_family` trong Settings
- **Format khi lưu**: Tìm `format_on_save` và đặt thành `on`

### 4. Thiết lập ngôn ngữ của bạn

Zode hỗ trợ sẵn nhiều ngôn ngữ. Với ngôn ngữ khác, hãy cài extension:

1. Mở Extensions bằng `Cmd+Shift+X` (macOS) hoặc `Ctrl+Shift+X` (Linux/Windows)
2. Tìm ngôn ngữ của bạn
3. Nhấn Install

Xem [Ngôn ngữ](./languages.md) để biết cách thiết lập cho từng ngôn ngữ.

### 5. Thử các tính năng AI

Zode tích hợp sẵn trợ lý AI. Agent mở thành một tab bên cạnh các file của bạn thay vì trong một bảng riêng:

- Nhấn biểu tượng Claude Code, Codex, Antigravity, Copilot hoặc opencode trên thanh bên để mở agent đó — nó chạy như một phiên terminal của CLI riêng của agent, trong một tab bên cạnh các file của bạn.
- Mỗi agent cần cài CLI riêng: lần lượt là `claude`, `codex`, `agy`, `copilot` và `opencode`. Nhấn vào agent chưa có CLI thì Zode đưa ra lệnh cài của nhà cung cấp, không im lặng báo lỗi.
- Để mở phiên thứ hai của một agent đã mở, dùng menu `+` trên thanh tab và chọn "New Claude Code", "New Codex", "New Antigravity", "New Copilot" hoặc "New opencode".

Có hai điểm không đồng đều giữa năm agent, vì bản thân các CLI không đồng đều:

- **Lịch sử phiên** gồm Claude Code, Codex, Copilot và opencode, mỗi cái ghi hội thoại ở đâu đó trên máy này. Antigravity thì không — `agy` giữ hội thoại ở phía server và không để lại gì trên đĩa, nên nó không có dòng nào trong bảng lịch sử.
- **Hạn mức đăng ký (subscription quota)** trên thanh trạng thái chỉ gồm Claude Code và Codex. Antigravity và Copilot không công bố cách đọc hạn mức nào mà không bắt trình soạn thảo này phải giữ một credential riêng; opencode dùng key của bạn nên vốn không có hạn mức đăng ký để công bố. Xem [Agent Usage Indicator](./agent-usage.md) cho cả hai.

Với hỗ trợ ngay trong dòng, dùng `Ctrl+Enter` (macOS, Linux và Windows đều dùng cùng phím này).

## Chuyển từ trình soạn thảo khác sang?

Chúng tôi có hướng dẫn riêng cho việc chuyển từ các trình soạn thảo khác:

- [VS Code](./migrate/vs-code.md) — Nhập cài đặt, ánh xạ phím tắt, tìm tính năng tương đương
- [IntelliJ IDEA](./migrate/intellij.md) — Làm quen cách điều hướng và refactor của Zode
- [PyCharm](./migrate/pycharm.md) — Thiết lập phát triển Python trong Zode
- [WebStorm](./migrate/webstorm.md) — Cấu hình quy trình làm việc với JavaScript/TypeScript
- [RustRover](./migrate/rustrover.md) — Phát triển Rust trong Zode

Bạn cũng có thể bật phím tắt quen thuộc:

- **Vim**: Bật `vim_mode` trong cài đặt. Xem [Vim Mode](./vim.md).
- **Helix**: Bật `helix_mode` trong cài đặt. Xem [Helix Mode](./helix.md).

## Tham gia cộng đồng

Zode là mã nguồn mở. Hãy tham gia với chúng tôi trên GitHub để đóng góp code, báo lỗi hoặc đề xuất tính năng.

- [GitHub Issues](https://github.com/tgiap04/zode/issues)
- [Zode repository](https://github.com/tgiap04/zode)
- [Zode website](https://zodekit.com)
