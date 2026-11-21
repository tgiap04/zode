---
title: Yarn
description: "Configure Yarn language support in Zode, including language servers, formatting, and debugging."
---

# Yarn

[Yarn](https://yarnpkg.com/) là trình quản lý package JavaScript, cho phép phân giải dependency xác định và cache offline.

## Thiết lập

1. Chạy `yarn dlx @yarnpkg/sdks base` để tạo thư mục `.yarn/sdks`.
2. Đặt language server của bạn (ví dụ VTSLS) dùng TypeScript SDK trong thư mục `.yarn/sdks/typescript/lib` qua [tùy chọn khởi tạo LSP](../reference/all-settings.md#lsp). Tên setting cụ thể tùy language server; ví dụ với VTSLS thì dùng [`typescript.tsdk`](https://github.com/yioneko/vtsls/blob/6adfb5d3889ad4b82c5e238446b27ae3ee1e3767/packages/service/configuration.schema.json#L5).

Sau khi cấu hình, các tính năng của language server (Go to Definition, gợi ý hoàn thành, tài liệu khi rê chuột) sẽ chạy đúng.
