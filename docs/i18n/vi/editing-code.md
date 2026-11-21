---
title: Editing Code in Zode
description: Core code editing features in Zode including multi-cursor, refactoring, code actions, and language server integration.
---

# Chỉnh sửa code

Zode có sẵn các công cụ giúp bạn viết và sửa code nhanh hơn. Phần này giới thiệu các tính năng chỉnh sửa cốt lõi, hoạt động cùng language server của bạn.

## Nội dung phần này

- **[Gợi ý hoàn thành code](./completions.md)** — Tự động hoàn thành từ language server và dự đoán chỉnh sửa bằng AI
- **[Snippet](./snippets.md)** — Chèn các mẫu code dùng lại được, có tab stop
- **[Định dạng & Linting](./configuring-languages.md#formatting-and-linting)** — Cấu hình định dạng code tự động và tích hợp linter
- **[Chẩn đoán & Sửa nhanh](./diagnostics.md)** — Xem lỗi, cảnh báo và áp dụng bản sửa từ language server
- **[Multibuffer](./multibuffers.md)** — Sửa nhiều file cùng lúc bằng nhiều con trỏ

## Các tính năng phối hợp với nhau thế nào

Khi bạn chỉnh sửa code, Zode kết hợp thông tin từ nhiều nguồn:

1. **Language server** cung cấp gợi ý hoàn thành, chẩn đoán và sửa nhanh dựa trên kiểu dữ liệu và cấu trúc của dự án
2. **Dự đoán chỉnh sửa** đề xuất thay đổi nhiều ký tự hoặc nhiều dòng khi bạn gõ
3. **Multibuffer** cho phép áp dụng thay đổi trên nhiều file trong một thao tác

Ví dụ, bạn có thể:

- Đổi tên một hàm bằng tính năng rename của language server
- Xem kết quả trong một multibuffer liệt kê mọi file bị ảnh hưởng
- Dùng nhiều con trỏ để sửa thêm ở tất cả các vị trí
- Nhận phản hồi chẩn đoán ngay nếu có gì hỏng

## Tính năng liên quan

- [Cấu hình ngôn ngữ](./configuring-languages.md) — Thiết lập language server cho dự án
- [Phím tắt](./key-bindings.md) — Tùy chỉnh phím tắt cho các lệnh chỉnh sửa
