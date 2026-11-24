---
title: Terraform
description: "Configure Terraform language support in Zode, including language servers, formatting, and debugging."
---

# Terraform

Hỗ trợ Terraform có qua [extension Terraform](https://github.com/zed-extensions/terraform).

- Tree-sitter: [MichaHoffmann/tree-sitter-hcl](https://github.com/MichaHoffmann/tree-sitter-hcl)
- Language server: [hashicorp/terraform-ls](https://github.com/hashicorp/terraform-ls)

## Cấu hình

<!--
TBD: Add example using `rootModulePaths` to match upstream example https://github.com/hashicorp/terraform-ls/blob/main/docs/SETTINGS.md#vs-code
-->

Bạn có thể cấu hình language server của Terraform trong `settings.json`, ví dụ:

```json [settings]
{
  "lsp": {
    "terraform-ls": {
      "initialization_options": {
        "experimentalFeatures": {
          "prefillRequiredFields": true
        }
      }
    }
  }
}
```

Xem [danh sách đầy đủ cài đặt của server tại đây](https://github.com/hashicorp/terraform-ls/blob/main/docs/SETTINGS.md).
