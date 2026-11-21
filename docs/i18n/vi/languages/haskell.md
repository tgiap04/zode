---
title: Haskell
description: "Configure Haskell language support in Zode, including language servers, formatting, and debugging."
---

# Haskell

Hỗ trợ Haskell có sẵn qua [extension Haskell](https://github.com/zed-extensions/haskell).

- Tree-sitter: [tree-sitter-haskell](https://github.com/tree-sitter/tree-sitter-haskell)
- Language Server: [haskell-language-server](https://github.com/haskell/haskell-language-server)

## Cài đặt HLS

Cách được khuyến nghị để [cài haskell-language-server](https://haskell-language-server.readthedocs.io/en/latest/installation.html) (HLS) là dùng [ghcup](https://www.haskell.org/ghcup/install/) (`curl --proto '=https' --tlsv1.2 -sSf https://get-ghcup.haskell.org | sh
`):

```sh
ghcup install hls
which haskell-language-server-wrapper
```

## Cấu hình HLS

Nếu cần cấu hình haskell-language-server (hls), bạn thêm các tùy chọn vào settings.json của Zode:

```json [settings]
{
  "lsp": {
    "hls": {
      "initialization_options": {
        "haskell": {
          "formattingProvider": "fourmolu"
        }
      }
    }
  }
}
```

Xem thêm tài liệu chính thức [configuring haskell-language-server](https://haskell-language-server.readthedocs.io/en/latest/configuration.html) để biết các tùy chọn khác.

Nếu muốn dùng một binary hls cụ thể, hoặc dùng [static-ls](https://github.com/josephsumabat/static-ls) thay thế trực tiếp, bạn chỉ định đường dẫn binary và các đối số:

```json [settings]
{
  "lsp": {
    "hls": {
      "binary": {
        "path": "static-ls",
        "arguments": ["--experimentalFeatures"]
      }
    }
  }
}
```
