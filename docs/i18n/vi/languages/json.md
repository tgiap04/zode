---
title: JSON
description: "Configure JSON language support in Zode, including language servers, formatting, and debugging."
---

# JSON

JSON được hỗ trợ sẵn trong Zode.

- Tree-sitter: [tree-sitter/tree-sitter-json](https://github.com/tree-sitter/tree-sitter-json)
- Language Server: [zed-industries/json-language-server](https://github.com/zed-industries/json-language-server)

## JSONC

Zode cũng hỗ trợ một tập mở rộng của JSON gọi là JSONC, cho phép comment một dòng (`//`) trong file JSON.
Khi sửa các file này, bạn có thể dùng `cmd-/` (macOS) hoặc `ctrl-/` (Linux) để bật/tắt comment cho dòng hiện tại hoặc vùng chọn.

## Định dạng JSONC bằng Prettier

Nếu bạn dùng file có đuôi `*.jsonc` khi chạy `Format Document` hoặc đã bật `format_on_save`, Zode gọi Prettier làm formatter. Prettier có một [lỗi chưa được sửa](https://github.com/prettier/prettier/issues/15956): nó thêm dấu phẩy cuối vào file có đuôi `jsonc`. File JSONC có đuôi `.json` không bị ảnh hưởng.

Để khắc phục, bạn có thể thêm đoạn sau vào file cấu hình `.prettierrc`:

```json
{
  "overrides": [
    {
      "files": ["*.jsonc"],
      "options": {
        "parser": "json",
        "trailingComma": "none"
      }
    }
  ]
}
```

## JSON Language Server

Zode hỗ trợ sẵn việc kiểm tra JSON Schema cho file `package.json` và `tsconfig.json`, nhưng `json-language-server` còn dùng được các định nghĩa JSON Schema trong file của dự án, từ [JSON Schema Store](https://www.schemastore.org) hoặc các URL công khai khác để kiểm tra JSON.

### Khai báo schema ngay trong file

Để khai báo schema ngay trong file JSON, thêm khoá cấp cao nhất `$schema` trỏ tới file JSON schema của bạn.

Ví dụ với `.luarc.json` dùng cho [lua-language-server](https://github.com/LuaLS/lua-language-server/):

```json
{
  "$schema": "https://raw.githubusercontent.com/sumneko/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4"
}
```

### Khai báo schema qua cài đặt

Bạn có thể gắn JSON Schema với đường dẫn file bằng đường dẫn tương đối trong cài đặt language server. Zode phân giải đường dẫn tương đối so với thư mục gốc dự án:

```json [settings]
"lsp": {
  "json-language-server": {
    "settings": {
      "json": {
        "schemas": [
          {
            "fileMatch": ["config/*.json"],
            "url": "./schemas/custom-schema.json"
          },
          {
            "fileMatch": ["*.config.json"],
            "url": "~/global-schemas/shared.json"
          },
          {
            "fileMatch": ["*/*.luarc.json"],
            "url": "https://raw.githubusercontent.com/sumneko/vscode-lua/master/setting/schema.json"
          }
        ]
      }
    }
  }
}
```

Đường dẫn bắt đầu bằng `./` được phân giải theo thư mục gốc của worktree. Đường dẫn bắt đầu bằng `~/` được mở rộng thành thư mục home của bạn.

Bạn cũng có thể truyền bất kỳ [cài đặt được hỗ trợ](https://github.com/Microsoft/vscode/blob/main/extensions/json-language-features/server/README.md#settings) nào cho json-language-server bằng cách khai báo trong settings.json của Zode:

<!--
TBD: Add formatter (prettier) settings (autoformat, tab_size, etc)
-->
