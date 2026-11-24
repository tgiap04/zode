---
title: Finding and Navigating Code - Zode
description: Navigate your codebase in Zode with file finder, project search, go to definition, symbol search, and the command palette.
---

# Tìm kiếm & điều hướng

Zode có nhiều cách để di chuyển nhanh trong codebase. Dưới đây là tổng quan các công cụ điều hướng chính.

## Bảng lệnh (Command Palette)

Bảng lệnh ({#kb command_palette::Toggle}) là cửa ngõ tới gần như mọi thứ trong Zode. Gõ vài ký tự để lọc lệnh, rồi nhấn Enter để chạy.

[Tìm hiểu thêm về bảng lệnh →](./command-palette.md)

## Tìm file

Mở bất kỳ file nào trong dự án bằng {#kb file_finder::Toggle}. Gõ một phần tên file hoặc đường dẫn để thu hẹp kết quả.

## Tìm trong dự án

Tìm trên mọi file bằng {#kb pane::DeploySearch}. Cứ gõ vào ô tìm kiếm là kết quả hiện ra ngay.

Kết quả hiện trong một [multibuffer](./multibuffers.md), nên bạn sửa trực tiếp được các kết quả khớp.

## Đi tới định nghĩa

Nhảy tới nơi một symbol được định nghĩa bằng {#kb editor::GoToDefinition} (hoặc `Cmd+Click` / `Ctrl+Click`). Nếu có nhiều định nghĩa, chúng sẽ mở trong một multibuffer.

## Đi tới symbol

- **File hiện tại:** {#kb outline::Toggle} mở outline các symbol của file đang mở
- **Toàn dự án:** {#kb project_symbols::Toggle} tìm symbol trên mọi file

## Bảng Outline

Bảng Outline ({#kb outline_panel::ToggleFocus}) hiển thị cây symbol của file hiện tại và giữ nguyên trên màn hình. Bảng này đặc biệt hữu ích khi dùng cùng [multibuffer](./multibuffers.md) để duyệt kết quả tìm kiếm hoặc chẩn đoán.

[Tìm hiểu thêm về bảng Outline →](./outline-panel.md)

## Chuyển tab

Chuyển nhanh giữa các tab đang mở bằng {#kb tab_switcher::Toggle}. Tab được sắp theo lần dùng gần nhất. Giữ Ctrl và nhấn Tab để duyệt qua từng tab.

[Tìm hiểu thêm về chuyển tab →](./tab-switcher.md)

## Tra cứu nhanh

| Tác vụ             | Phím tắt                         |
| ------------------ | -------------------------------- |
| Bảng lệnh          | {#kb command_palette::Toggle}    |
| Mở file            | {#kb file_finder::Toggle}        |
| Tìm trong dự án    | {#kb pane::DeploySearch}         |
| Đi tới định nghĩa  | {#kb editor::GoToDefinition}     |
| Tìm tham chiếu     | {#kb editor::FindAllReferences}  |
| Symbol trong file  | {#kb outline::Toggle}            |
| Symbol trong dự án | {#kb project_symbols::Toggle}    |
| Bảng Outline       | {#kb outline_panel::ToggleFocus} |
| Chuyển tab         | {#kb tab_switcher::Toggle}       |
