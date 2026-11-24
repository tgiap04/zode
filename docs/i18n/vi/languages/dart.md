---
title: Dart
description: "Configure Dart language support in Zode, including language servers, formatting, and debugging."
---

# Dart

Hỗ trợ Dart có sẵn qua [extension Dart](https://github.com/zed-extensions/dart).

- Tree-sitter: [UserNobody14/tree-sitter-dart](https://github.com/UserNobody14/tree-sitter-dart)
- Language Server: [dart language-server](https://github.com/dart-lang/sdk)

## Điều kiện tiên quyết

Bạn cần cài Dart SDK.

Bạn có thể cài dart từ [dart.dev/get-dart](https://dart.dev/get-dart) hoặc qua [Flutter Version Management CLI (fvm)](https://fvm.app/documentation/getting-started/installation)

## Cấu hình

Extension dart không cần cấu hình nếu `dart` đã có trong path của bạn:

```sh
which dart
dart --version
```

Nếu muốn dùng một binary dart cụ thể hoặc dùng dart qua FVM, bạn có thể chỉ định binary `dart` trong file settings.json của Zode:

```json [settings]
{
  "lsp": {
    "dart": {
      "binary": {
        "path": "/opt/homebrew/bin/fvm",
        "arguments": ["dart", "language-server", "--protocol=lsp"]
      }
    }
  }
}
```

### Format code

Mặc định Dart dùng độ dài dòng tối đa rất khiêm tốn (80). Nếu muốn dart LSP cho phép dòng dài hơn khi tự động format, thêm đoạn sau vào settings.json của Zode:

```json [settings]
{
  "lsp": {
    "dart": {
      "settings": {
        "lineLength": 140
      }
    }
  }
}
```

Xem tài liệu Dart để biết thêm về [khả năng của dart language-server](https://github.com/dart-lang/sdk/blob/main/pkg/analysis_server/tool/lsp_spec/README.md).
