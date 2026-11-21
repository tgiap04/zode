---
title: HTML
description: "Configure HTML language support in Zode, including language servers, formatting, and debugging."
---

# HTML

HTML được hỗ trợ qua [extension HTML](https://github.com/tgiap04/zode/tree/develop/extensions/html).

- Tree-sitter: [tree-sitter/tree-sitter-html](https://github.com/tree-sitter/tree-sitter-html)
- Language Server: [microsoft/vscode-html-languageservice](https://github.com/microsoft/vscode-html-languageservice)

Extension này được cài tự động, nhưng nếu không muốn dùng, bạn có thể thêm đoạn sau vào cài đặt:

```json [settings]
{
  "auto_install_extensions": {
    "html": false
  }
}
```

## Định dạng

Mặc định Zode dùng [Prettier](https://prettier.io/) để định dạng HTML.

Cấu hình định dạng trong Settings ({#kb zode::OpenSettings}) ở mục Languages > HTML, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "HTML": {
      "format_on_save": "off",
    }
  }
```

Bạn vẫn có thể chạy định dạng thủ công bằng {#kb editor::Format} hoặc mở [bảng lệnh (command palette)](..//getting-started.md#command-palette) ({#kb command_palette::Toggle}) rồi chọn "Format Document".

### Định dạng bằng LSP

Để dùng tính năng tự định dạng của language server `vscode-html-language-server` thay cho Prettier, cấu hình formatter trong Settings ({#kb zode::OpenSettings}) ở mục Languages > HTML, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "HTML": {
      "formatter": "language_server",
    }
  }
```

Bạn có thể tuỳ chỉnh nhiều [tuỳ chọn định dạng](https://code.visualstudio.com/docs/languages/html#_formatting) cho `vscode-html-language-server` qua `settings.json` của Zode:

```json [settings]
  "lsp": {
    "vscode-html-language-server": {
      "settings": {
        "html": {
          "format": {
            // Indent under <html> and <head> (default: false)
            "indentInnerHtml": true,
            // Disable formatting inside <svg> or <script>
            "contentUnformatted": "svg,script",
            // Add an extra newline before <div> and <p>
            "extraLiners": "div,p"
          }
        }
      }
    }
  }
```

## Dùng Tailwind CSS Language Server với HTML

Để có đầy đủ tính năng (tự hoàn thành, lint...) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file HTML, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "experimental": {
          "classRegex": ["class=\"([^\"]*)\""]
        }
      }
    }
  }
}
```

Với các cài đặt này, bạn sẽ có gợi ý hoàn thành cho class Tailwind CSS trong thuộc tính `class` của HTML. Ví dụ:

```html
<div class="flex items-center <completion here>">
  <p class="text-lg font-bold <completion here>">Hello World</p>
</div>
```

## Xem thêm

- [CSS](./css.md)
- [JavaScript](./javascript.md)
- [TypeScript](./typescript.md)
