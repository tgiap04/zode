---
title: XML
description: "Configure XML language support in Zode, including language servers, formatting, and debugging."
---

# XML

XML được hỗ trợ qua [extension XML](https://github.com/sweetppro/zed-xml/).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-xml](https://github.com/tree-sitter-grammars/tree-sitter-xml)

## Cấu hình

Nếu có thêm đuôi file chưa được tự nhận là XML, bạn chỉ cần thêm vào [file_types](../reference/all-settings.md#file-types) trong cài đặt Zode:

```json [settings]
  "file_types": {
    "XML": ["rdf", "gpx", "kml"]
  }
```
