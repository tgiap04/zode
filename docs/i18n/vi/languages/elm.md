---
title: Elm
description: "Configure Elm language support in Zode, including language servers, formatting, and debugging."
---

# Elm

Hỗ trợ Elm có qua [extension Elm](https://github.com/zed-extensions/elm).

- Tree-sitter: [elm-tooling/tree-sitter-elm](https://github.com/elm-tooling/tree-sitter-elm)
- Language Server: [elm-tooling/elm-language-server](https://github.com/elm-tooling/elm-language-server)

## Thiết lập

Để Zode hỗ trợ Elm, bạn cần cài `elm`, `elm-format` và `elm-review`.

1. [Cài Elm](https://guide.elm-lang.org/install/elm.html) (hoặc chạy `brew install elm` trên macOS).
2. Cài `elm-review` để có lint code:
   ```sh
   npm install elm-review --save-dev
   ```
3. Cài `elm-format` để tự động định dạng
   ```sh
   npm install -g elm-format
   ```

## Cấu hình `elm-language-server`

Bạn có thể cấu hình language server của Elm trong `settings.json`, ví dụ:

```json [settings]
{
  "lsp": {
    "elm-language-server": {
      "initialization_options": {
        "disableElmLSDiagnostics": true,
        "onlyUpdateDiagnosticsOnSave": false,
        "elmReviewDiagnostics": "warning"
      }
    }
  }
}
```

`elm-format`, `elm-review` và `elm` phải được cài sẵn và có trong môi trường, hoặc được khai báo trong cài đặt. Xem [danh sách đầy đủ các cài đặt của server tại đây](https://github.com/elm-tooling/elm-language-server?tab=readme-ov-file#server-settings).
