---
title: Helm
description: "Configure Helm language support in Zode, including language servers, formatting, and debugging."
---

# Helm

Hỗ trợ Helm trong Zode do [extension Helm](https://github.com/cabrinha/helm.zed) do cộng đồng duy trì cung cấp.

- Tree-sitter: [tree-sitter-go-template](https://github.com/ngalaiko/tree-sitter-go-template/tree/master)
- Language Server: [mrjosh/helm-ls](https://github.com/mrjosh/helm-ls)

## Thiết lập

Bật ngôn ngữ Helm cho các file Helm bằng cách sửa `.zode/settings.json` và thêm:

```json [settings]
  "file_types": {
    "Helm": [
      "**/templates/**/*.tpl",
      "**/templates/**/*.yaml",
      "**/templates/**/*.yml",
      "**/helmfile.d/**/*.yaml",
      "**/helmfile.d/**/*.yml",
      "**/values*.yaml"
    ]
  }
```

Cách này cũng đánh dấu các file values.yaml là loại helm, vì helm-ls hỗ trợ điều đó.
