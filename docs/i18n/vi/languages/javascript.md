---
title: JavaScript
description: "Configure JavaScript language support in Zode, including language servers, formatting, and debugging."
---

# JavaScript

Zode hỗ trợ JavaScript sẵn có.

- Tree-sitter: [tree-sitter/tree-sitter-javascript](https://github.com/tree-sitter/tree-sitter-javascript)
- Language Server: [yioneko/vtsls](https://github.com/yioneko/vtsls)
- Alternate Language Server: [typescript-language-server/typescript-language-server](https://github.com/typescript-language-server/typescript-language-server)
- Debug Adapter: [vscode-js-debug](https://github.com/microsoft/vscode-js-debug)

## Định dạng mã

Định dạng khi lưu được bật mặc định cho JavaScript, dùng bộ định dạng mã tích hợp của TypeScript.
Nhưng nhiều dự án JavaScript dùng công cụ định dạng mã dòng lệnh khác, như [Prettier](https://prettier.io/).
Bạn có thể dùng một trong các công cụ này bằng cách chỉ định trình định dạng mã _bên ngoài_ cho JavaScript trong cài đặt.
Xem [tài liệu cấu hình](../reference/all-settings.md) để biết thêm.

Ví dụ, nếu đã cài Prettier và nó có trong `PATH`, bạn có thể dùng nó để định dạng file JavaScript.

Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > JavaScript, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "JavaScript": {
      "formatter": {
        "external": {
          "command": "prettier",
          "arguments": ["--stdin-filepath", "{buffer_path}"]
        }
      }
    }
  }
}
```

## JSX

Zode hỗ trợ tô màu cú pháp JSX sẵn có.

Trong chuỗi JSX, [`tailwindcss-language-server`](./tailwindcss.md) được dùng để gợi ý tự động các class Tailwind CSS.

## JSDoc

Zode hỗ trợ cú pháp JSDoc trong comment JavaScript và TypeScript khớp với cú pháp JSDoc.
Zode dùng [tree-sitter/tree-sitter-jsdoc](https://github.com/tree-sitter/tree-sitter-jsdoc) để phân tích và tô màu JSDoc.

## ESLint

Bạn có thể cấu hình Zode định dạng mã bằng `eslint --fix` bằng cách chạy code action của ESLint khi định dạng.

Cấu hình code action khi định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > JavaScript, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "JavaScript": {
      "code_actions_on_format": {
        "source.fixAll.eslint": true
      }
    }
  }
}
```

Bạn cũng có thể chỉ chạy một quy tắc ESLint khi dùng `fixAll`:

```json [settings]
{
  "languages": {
    "JavaScript": {
      "code_actions_on_format": {
        "source.fixAll.eslint": true
      }
    }
  },
  "lsp": {
    "eslint": {
      "settings": {
        "codeActionOnSave": {
          "rules": ["import/order"]
        }
      }
    }
  }
}
```

> **Lưu ý:** trình định dạng khác mà bạn đã cấu hình vẫn chạy, sau ESLint.
> Vì vậy nếu cấu hình language server hay Prettier của bạn không định dạng theo
> quy tắc của ESLint, chúng sẽ ghi đè những gì ESLint đã sửa và bạn sẽ
> gặp lỗi.

Nếu bạn **chỉ** muốn chạy ESLint khi lưu, bạn có thể cấu hình code action làm trình định dạng.

Cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > JavaScript, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "JavaScript": {
      "formatter": [],
      "code_actions_on_format": {
        "source.fixAll.eslint": true
      }
    }
  }
}
```

### Cấu hình `nodePath` của ESLint:

Bạn có thể cấu hình cài đặt `nodePath` của ESLint:

```json [settings]
{
  "lsp": {
    "eslint": {
      "settings": {
        "nodePath": ".yarn/sdks"
      }
    }
  }
}
```

### Cấu hình `problems` của ESLint:

Bạn có thể cấu hình cài đặt `problems` của ESLint.

Ví dụ, đây là cách đặt `problems.shortenToSingleLine`:

```json [settings]
{
  "lsp": {
    "eslint": {
      "settings": {
        "problems": {
          "shortenToSingleLine": true
        }
      }
    }
  }
}
```

### Cấu hình `rulesCustomizations` của ESLint:

Bạn có thể cấu hình cài đặt `rulesCustomizations` của ESLint:

```json [settings]
{
  "lsp": {
    "eslint": {
      "settings": {
        "rulesCustomizations": [
          // set all eslint errors/warnings to show as warnings
          { "rule": "*", "severity": "warn" }
        ]
      }
    }
  }
}
```

### Cấu hình `workingDirectory` của ESLint:

Bạn có thể cấu hình cài đặt `workingDirectory` của ESLint:

```json [settings]
{
  "lsp": {
    "eslint": {
      "settings": {
        "workingDirectory": {
          "mode": "auto"
        }
      }
    }
  }
}
```

## Dùng Tailwind CSS Language Server với JavaScript

Để có đủ tính năng (gợi ý tự động, lint, v.v.) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file JavaScript thuần (`.js`), bạn có thể tùy chỉnh trường `classRegex` bên dưới nó trong `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "experimental": {
          "classRegex": [
            "\\.className\\s*[+]?=\\s*['\"]([^'\"]*)['\"]",
            "\\.setAttributeNS\\(.*,\\s*['\"]class['\"],\\s*['\"]([^'\"]*)['\"]",
            "\\.setAttribute\\(['\"]class['\"],\\s*['\"]([^'\"]*)['\"]",
            "\\.classList\\.add\\(['\"]([^'\"]*)['\"]",
            "\\.classList\\.remove\\(['\"]([^'\"]*)['\"]",
            "\\.classList\\.toggle\\(['\"]([^'\"]*)['\"]",
            "\\.classList\\.contains\\(['\"]([^'\"]*)['\"]",
            "\\.classList\\.replace\\(\\s*['\"]([^'\"]*)['\"]",
            "\\.classList\\.replace\\([^,)]+,\\s*['\"]([^'\"]*)['\"]"
          ]
        }
      }
    }
  }
}
```

## Gỡ lỗi

Zode hỗ trợ gỡ lỗi mã JavaScript sẵn có bằng `vscode-js-debug`.
Có thể gỡ lỗi những thứ sau mà không cần viết thêm cấu hình:

- Task từ `package.json`
- Test viết bằng một số framework phổ biến (Jest, Mocha, Vitest, Jasmine, Bun, Node)

Chạy {#action debugger::Start} ({#kb debugger::Start}) để xem danh sách theo ngữ cảnh các task gỡ lỗi định sẵn này.

> **Lưu ý:** Bun test được tự động phát hiện khi có `@types/bun` trong `package.json`.

> **Lưu ý:** Node test được tự động phát hiện khi có `@types/node` trong `package.json` (yêu cầu Node.js 20+).

Như mọi ngôn ngữ khác, cấu hình từ `.vscode/launch.json` cũng dùng được để gỡ lỗi trong Zode.

Nếu trường hợp của bạn không nằm trong số này, bạn có thể toàn quyền kiểm soát bằng cách thêm cấu hình gỡ lỗi vào `.zode/debug.json`. Xem cấu hình mẫu bên dưới.

### Cấu hình task gỡ lỗi JavaScript

Gỡ lỗi JavaScript phức tạp hơn các ngôn ngữ khác vì có hai môi trường khác nhau: Node.js và trình duyệt. `vscode-js-debug` cung cấp trường `type`, bạn dùng để chỉ định môi trường, là `node` hoặc `chrome`.

- [Tài liệu cấu hình vscode-js-debug](https://github.com/microsoft/vscode-js-debug/blob/main/OPTIONS.md)

### Gỡ lỗi file hiện tại bằng Node

```json [debug]
[
  {
    "adapter": "JavaScript",
    "label": "Debug JS file",
    "type": "node",
    "request": "launch",
    "program": "$ZODE_FILE",
    "skipFiles": ["<node_internals>/**"]
  }
]
```

### Chạy web app trong Chrome

```json [debug]
[
  {
    "adapter": "JavaScript",
    "label": "Debug app in Chrome",
    "type": "chrome",
    "request": "launch",
    "file": "$ZODE_WORKTREE_ROOT/index.html",
    "webRoot": "$ZODE_WORKTREE_ROOT",
    "console": "integratedTerminal",
    "skipFiles": ["<node_internals>/**"]
  }
]
```

## Xem thêm

- [Tài liệu Yarn](./yarn.md) hướng dẫn cấu hình dự án dùng Yarn.
- [Tài liệu TypeScript](./typescript.md)
