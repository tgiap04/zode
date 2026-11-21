---
title: Luau
description: "Configure Luau language support in Zode, including language servers, formatting, and debugging."
---

# Luau

[Luau](https://luau.org/) là ngôn ngữ script nhanh, nhỏ, an toàn, kiểu dữ liệu tăng dần (gradually typed), nhúng được, bắt nguồn từ Lua. Luau do Roblox phát triển và phát hành theo giấy phép MIT.

Hỗ trợ ngôn ngữ Luau trong Zode do [extension Luau](https://github.com/4teapo/zed-luau) do cộng đồng duy trì cung cấp.
Báo lỗi tại: [https://github.com/4teapo/zed-luau/issues](https://github.com/4teapo/zed-luau/issues)

- Tree-sitter: [4teapo/tree-sitter-luau](https://github.com/4teapo/tree-sitter-luau)
- Language Server: [JohnnyMorganz/luau-lsp](https://github.com/JohnnyMorganz/luau-lsp)

## Cấu hình

Hướng dẫn cấu hình có trong [README của extension Luau cho Zode](https://github.com/4teapo/zed-luau).

## Format code

Để tự động format code, bạn có thể dùng [JohnnyMorganz/StyLua](https://github.com/JohnnyMorganz/StyLua), một công cụ format code Lua.

Cài bằng:

```sh
# macOS via Homebrew
brew install stylua
# Or via Cargo
cargo install stylua --features lua52,lua53,lua54,luau
```

Cấu hình format trong Settings ({#kb zode::OpenSettings}) tại Languages > Luau, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Luau": {
      "formatter": {
        "external": {
          "command": "stylua",
          "arguments": ["-"]
        }
      }
    }
  }
```
