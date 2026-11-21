---
title: Jsonnet
description: "Configure Jsonnet language support in Zode, including language servers, formatting, and debugging."
---

# Jsonnet

Hỗ trợ ngôn ngữ Jsonnet trong Zode do [extension Jsonnet](https://github.com/narqo/zed-jsonnet) do cộng đồng duy trì cung cấp.

- Tree-sitter: [sourcegraph/tree-sitter-jsonnet](https://github.com/sourcegraph/tree-sitter-jsonnet)
- Language Server: [grafana/jsonnet-language-server](https://github.com/grafana/jsonnet-language-server)

## Cấu hình

Các tuỳ chọn cấu hình workspace có thể truyền cho language server qua mục `lsp` trong `settings.json`.

Ví dụ sau cấu hình `jsonnet-language-server` để phân giải đường dẫn import của [tanka](https://tanka.dev):

```json [settings]
{
  "lsp": {
    "jsonnet-language-server": {
      "settings": {
        "resolve_paths_with_tanka": true
      }
    }
  }
}
```
