---
title: Run and Test Code in Zode
description: Run and test code in Zode with Terminal, Tasks, REPL, and debugger workflows without leaving the editor.
---

# Chạy và kiểm thử

Dùng phần này để chạy và kiểm thử code trong Zode, rồi gỡ lỗi mà không phải
rời trình soạn thảo.

## Có gì ở đây

- **[Terminal](./terminal.md)**: terminal tích hợp sẵn của Zode. Mở nhiều terminal, tuỳ chỉnh shell và tích hợp với trình soạn thảo. Task và lệnh chạy ở đây.

- **[Task](./tasks.md)**: Định nghĩa và chạy lệnh shell với quyền truy cập ngữ cảnh của trình soạn thảo như file hiện tại, vùng chọn hay symbol. Dùng task để build, lint, chạy script hoặc thực thi bất kỳ quy trình lặp lại nào.

- **[Trình gỡ lỗi (debugger)](./debugger.md)**: Đặt breakpoint, chạy từng bước và xem biến bằng trình gỡ lỗi tích hợp của Zode. Hoạt động với C, C++, Go, JavaScript, Python, Rust, TypeScript và nhiều ngôn ngữ khác qua Debug Adapter Protocol.

- **[REPL](./repl.md)**: Chạy code tương tác bằng Jupyter kernel. Thực thi vùng chọn hoặc cell và xem kết quả ngay trong dòng, hữu ích cho Python, TypeScript (Deno), R, Julia và các ngôn ngữ được hỗ trợ khác.

## Bắt đầu nhanh

**Mở terminal**: Nhấn `` Ctrl+` `` để bật/tắt bảng terminal, hoặc `Ctrl+~` để mở terminal mới.

**Chạy một lệnh**: Nhấn `Cmd+Shift+R` (macOS) hoặc `Ctrl+Shift+R` (Linux/Windows) để mở bộ chọn task, rồi gõ lệnh shell bất kỳ.

**Bắt đầu gỡ lỗi**: Nhấn `Cmd+Shift+D` (macOS) hoặc `Ctrl+Shift+D` (Linux/Windows) để mở bảng gỡ lỗi và chọn một cấu hình.

**Chạy code tương tác**: Trong file Python hoặc TypeScript, chọn một đoạn code và nhấn `Ctrl+Shift+Enter` để chạy trong phiên REPL.
