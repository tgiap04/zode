---
title: OpenTofu
description: "Configure OpenTofu language support in Zode, including language servers, formatting, and debugging."
---

# OpenTofu

OpenTofu được hỗ trợ qua [extension OpenTofu](https://github.com/ashpool37/zed-extension-opentofu).

- Tree-sitter: [MichaHoffmann/tree-sitter-hcl](https://github.com/MichaHoffmann/tree-sitter-hcl)
- Language Server: [opentofu/tofu-ls](https://github.com/opentofu/tofu-ls)

## Cấu hình

Để tự động dùng extension OpenTofu và language server khi sửa file `.tf` và `.tfvars`,
hãy gỡ extension Terraform hoặc thêm đoạn này vào settings.json:

```json
"file_types": {
  "OpenTofu": ["tf"],
  "OpenTofu Vars": ["tfvars"]
},
```

Xem [danh sách đầy đủ các cài đặt của server tại đây](https://github.com/opentofu/tofu-ls/blob/main/docs/SETTINGS.md).
