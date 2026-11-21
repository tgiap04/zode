---
title: Rego
description: "Configure Rego language support in Zode, including language servers, formatting, and debugging."
---

# Rego

Hỗ trợ ngôn ngữ Rego trong Zode do [extension Rego](https://github.com/StyraInc/zed-rego) do cộng đồng duy trì cung cấp.

- Tree-sitter: [FallenAngel97/tree-sitter-rego](https://github.com/FallenAngel97/tree-sitter-rego)
- Language server: [open-policy-agent/regal](https://github.com/open-policy-agent/regal)

## Cài đặt

Extension này chủ yếu dựa trên language server [Regal](https://docs.styra.com/regal/language-server), cần cài thêm để dùng extension. Xem hướng dẫn [bắt đầu](https://docs.styra.com/regal#getting-started) để biết chi tiết.

## Cấu hình

Hành vi của extension được cấu hình trong file `.regal/config.yaml`. Ví dụ dưới đây tắt rule `todo-comment`, tùy chỉnh rule `line-length` và bỏ qua các file test cho rule `opa-fmt`:

```yaml
rules:
  style:
    todo-comment:
      # don't report on todo comments
      level: ignore
    line-length:
      # custom rule configuration
      max-line-length: 100
      # warn on too long lines, but don't fail
      level: warning
    opa-fmt:
      # not needed as error is the default, but
      # being explicit won't hurt
      level: error
      # files can be ignored for any individual rule
      # in this example, test files are ignored
      ignore:
        files:
          - "*_test.rego"
```

Xem thêm [tài liệu cấu hình](https://docs.styra.com/regal#configuration) của Regal.
