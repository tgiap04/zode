---
title: Nim
description: "Configure Nim language support in Zode, including language servers, formatting, and debugging."
---

# Nim

Hỗ trợ ngôn ngữ Nim trong Zode do cộng đồng duy trì qua [extension Nim](https://github.com/foxoman/zed-nim).
Báo lỗi tại: [https://github.com/foxoman/zed-nim/issues](https://github.com/foxoman/zed-nim/issues)

- Tree-sitter: [alaviss/tree-sitter-nim](https://github.com/alaviss/tree-sitter-nim)
- Language Server: [nim-lang/langserver](https://github.com/nim-lang/langserver)

## Định dạng

Để dùng [arnetheduck/nph](https://github.com/arnetheduck/nph) làm formatter, làm theo [hướng dẫn cài đặt nph](https://github.com/arnetheduck/nph?tab=readme-ov-file#installation).

Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) tại Languages > Nim, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Nim": {
      "formatter": {
        "external": {
          "command": "nph",
          "arguments": ["-"]
        }
      }
    }
  }
```
