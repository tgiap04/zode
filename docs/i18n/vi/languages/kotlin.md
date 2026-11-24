---
title: Kotlin
description: "Configure Kotlin language support in Zode, including language servers, formatting, and debugging."
---

# Kotlin

Hỗ trợ ngôn ngữ Kotlin trong Zode do [extension Kotlin](https://github.com/zed-extensions/kotlin) của cộng đồng cung cấp.
Báo lỗi tại: [https://github.com/zed-extensions/kotlin/issues](https://github.com/zed-extensions/kotlin/issues)

- Tree-sitter: [fwcd/tree-sitter-kotlin](https://github.com/fwcd/tree-sitter-kotlin)
- Language Server: [fwcd/kotlin-language-server](https://github.com/fwcd/kotlin-language-server)
- Language Server thay thế: [kotlin/kotlin-lsp](https://github.com/kotlin/kotlin-lsp)

## Cấu hình

Có thể truyền các tuỳ chọn cấu hình workspace cho language server qua cài đặt lsp
trong `settings.json`.

Danh sách đầy đủ các `settings` của lsp xem
[tại đây](https://github.com/fwcd/kotlin-language-server/blob/main/server/src/main/kotlin/org/javacs/kt/Configuration.kt)
trong `class Configuration`, còn initialization_options nằm trong `class InitializationOptions`.

### JVM Target

Ví dụ sau đổi JVM target từ `default` (là 1.8) sang
`17`:

```json [settings]
{
  "lsp": {
    "kotlin-language-server": {
      "settings": {
        "compiler": {
          "jvm": {
            "target": "17"
          }
        }
      }
    }
  }
}
```

### JAVA_HOME

Để dùng một bản cài java cụ thể, chỉ cần chỉ định biến môi trường `JAVA_HOME`:

```json [settings]
{
  "lsp": {
    "kotlin-language-server": {
      "binary": {
        "env": {
          "JAVA_HOME": "/Users/whatever/Applications/Work/Android Studio.app/Contents/jbr/Contents/Home"
        }
      }
    }
  }
}
```
