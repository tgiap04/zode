---
title: Erlang
description: "Configure Erlang language support in Zode, including language servers, formatting, and debugging."
---

# Erlang

Erlang được hỗ trợ qua [extension Erlang](https://github.com/zed-extensions/erlang).

- Tree-sitter: [WhatsApp/tree-sitter-erlang](https://github.com/WhatsApp/tree-sitter-erlang)
- Language Server:
  - [erlang-ls/erlang_ls](https://github.com/erlang-ls/erlang_ls)
  - [WhatsApp/erlang-language-platform](https://github.com/WhatsApp/erlang-language-platform)

## Chọn language server

Extension Erlang hỗ trợ language server `erlang_ls` và `erlang-language-platform`.

`erlang_ls` được bật mặc định.

Cấu hình language server trong Settings ({#kb zode::OpenSettings}) ở mục Languages > Erlang, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Erlang": {
      "language_servers": ["elp", "!erlang-ls", "..."]
    }
  }
}
```

## Xem thêm:

- [Elixir](./elixir.md)
- [Gleam](./gleam.md)
