---
title: Diff
description: "Configure Diff language support in Zode, including language servers, formatting, and debugging."
---

# Diff

Zode hỗ trợ Diff sẵn, không cần cài thêm.

- Tree-sitter: [zed-industries/the-mikedavis/tree-sitter-diff](https://github.com/the-mikedavis/tree-sitter-diff)

## Cấu hình

Zode không định dạng file diff, đồng thời đặt [`remove_trailing_whitespace_on_save`](https://zodekit.com/docs/reference/all-settings#remove-trailing-whitespace-on-save) và [`ensure-final-newline-on-save`](https://zodekit.com/docs/reference/all-settings#ensure-final-newline-on-save) thành false.

Zode tự nhận file có đuôi `patch` và `diff` là file Diff. Để nhận thêm đuôi khác, thêm chúng vào `file_types` trong settings.json của Zode:

```json [settings]
  "file_types": {
    "Diff": ["dif"]
  },
```
