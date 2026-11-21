---
title: TypeScript
description: "Configure TypeScript language support in Zode, including language servers, formatting, and debugging."
---

# TypeScript

Zode hỗ trợ TypeScript và TSX sẵn có.

- Tree-sitter: [tree-sitter/tree-sitter-typescript](https://github.com/tree-sitter/tree-sitter-typescript)
- Language server: [yioneko/vtsls](https://github.com/yioneko/vtsls)
- Language server thay thế: [typescript-language-server/typescript-language-server](https://github.com/typescript-language-server/typescript-language-server)
- Debug adapter: [vscode-js-debug](https://github.com/microsoft/vscode-js-debug)

<!--
TBD: Document the difference between Language servers
-->

## Language server

Mặc định Zode dùng [vtsls](https://github.com/yioneko/vtsls) cho file TypeScript, TSX và JavaScript.
Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > TypeScript/TSX/JavaScript, hoặc thêm vào file settings:

```json [settings]
{
  "languages": {
    "TypeScript": {
      "language_servers": ["typescript-language-server", "!vtsls", "..."]
    },
    "TSX": {
      "language_servers": ["typescript-language-server", "!vtsls", "..."]
    },
    "JavaScript": {
      "language_servers": ["typescript-language-server", "!vtsls", "..."]
    }
  }
}
```

Mặc định Prettier cũng được dùng cho file TypeScript. Để tắt, cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > TypeScript, hoặc thêm vào file settings:

```json [settings]
{
  "languages": {
    "TypeScript": {
      "prettier": { "allowed": false }
    }
    //...
  }
}
```

## Dùng Tailwind CSS Language Server với TypeScript

Để có đầy đủ tính năng (autocomplete, linting...) của [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file TypeScript thuần (`.ts`), bạn có thể tùy chỉnh trường `classRegex` bên dưới nó trong `settings.json`:

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

## Dự án lớn

`vtsls` có thể hết bộ nhớ ở các dự án rất lớn. Chúng tôi đặt mặc định giới hạn là 8092 (8 GiB) thay cho mặc định 3072, nhưng có thể vẫn chưa đủ với bạn:

```json [settings]
{
  "lsp": {
    "vtsls": {
      "settings": {
        // For TypeScript:
        "typescript": { "tsserver": { "maxTsServerMemory": 16184 } },
        // For JavaScript:
        "javascript": { "tsserver": { "maxTsServerMemory": 16184 } }
      }
    }
  }
}
```

## Inlay Hint

Zode đặt các tùy chọn khởi tạo sau để language server gửi về inlay hint (tức là khi Zode đã bật inlay hint trong settings).

Bạn có thể ghi đè các setting này trong `settings.json` của Zode khi dùng `typescript-language-server`:

```json [settings]
{
  "lsp": {
    "typescript-language-server": {
      "initialization_options": {
        "preferences": {
          "includeInlayParameterNameHints": "all",
          "includeInlayParameterNameHintsWhenArgumentMatchesName": true,
          "includeInlayFunctionParameterTypeHints": true,
          "includeInlayVariableTypeHints": true,
          "includeInlayVariableTypeHintsWhenTypeMatchesName": true,
          "includeInlayPropertyDeclarationTypeHints": true,
          "includeInlayFunctionLikeReturnTypeHints": true,
          "includeInlayEnumMemberValueHints": true
        }
      }
    }
  }
}
```

Xem [tài liệu inlayhints của typescript-language-server](https://github.com/typescript-language-server/typescript-language-server?tab=readme-ov-file#inlay-hints-textdocumentinlayhint) để biết thêm.

Khi dùng `vtsls`:

```json [settings]
{
  "lsp": {
    "vtsls": {
      "settings": {
        // For JavaScript:
        "javascript": {
          "inlayHints": {
            "parameterNames": {
              "enabled": "all",
              "suppressWhenArgumentMatchesName": false
            },
            "parameterTypes": {
              "enabled": true
            },
            "variableTypes": {
              "enabled": true,
              "suppressWhenTypeMatchesName": true
            },
            "propertyDeclarationTypes": {
              "enabled": true
            },
            "functionLikeReturnTypes": {
              "enabled": true
            },
            "enumMemberValues": {
              "enabled": true
            }
          }
        },
        // For TypeScript:
        "typescript": {
          "inlayHints": {
            "parameterNames": {
              "enabled": "all",
              "suppressWhenArgumentMatchesName": false
            },
            "parameterTypes": {
              "enabled": true
            },
            "variableTypes": {
              "enabled": true,
              "suppressWhenTypeMatchesName": true
            },
            "propertyDeclarationTypes": {
              "enabled": true
            },
            "functionLikeReturnTypes": {
              "enabled": true
            },
            "enumMemberValues": {
              "enabled": true
            }
          }
        }
      }
    }
  }
}
```

## Gỡ lỗi

Zode hỗ trợ gỡ lỗi code TypeScript sẵn có bằng `vscode-js-debug`.
Những thứ sau gỡ lỗi được mà không cần viết thêm cấu hình:

- Task từ `package.json`
- Test viết bằng một số framework phổ biến (Jest, Mocha, Vitest, Jasmine, Bun, Node)

Chạy {#action debugger::Start} ({#kb debugger::Start}) để xem danh sách các task gỡ lỗi định sẵn theo ngữ cảnh.

> **Lưu ý:** Test Bun được tự phát hiện khi `@types/bun` có trong `package.json`.

> **Lưu ý:** Test Node được tự phát hiện khi `@types/node` có trong `package.json` (yêu cầu Node.js 20+).

Như với mọi ngôn ngữ, các cấu hình trong `.vscode/launch.json` cũng dùng được để gỡ lỗi trong Zode.

Nếu trường hợp của bạn không nằm trong số này, bạn có thể toàn quyền kiểm soát bằng cách thêm cấu hình gỡ lỗi vào `.zode/debug.json`. Xem các cấu hình ví dụ bên dưới.

### Cấu hình task gỡ lỗi JavaScript

Gỡ lỗi JavaScript phức tạp hơn các ngôn ngữ khác vì có hai môi trường khác nhau: Node.js và trình duyệt. `vscode-js-debug` có trường `type` để bạn chỉ định môi trường, `node` hoặc `chrome`.

- [Tài liệu cấu hình vscode-js-debug](https://github.com/microsoft/vscode-js-debug/blob/main/OPTIONS.md)

### Gắn debugger vào server đang chạy trong trình duyệt (`npx serve`)

Với một web server chạy bên ngoài (ví dụ bằng `npx serve` hoặc `npx live-server`), bạn có thể gắn vào nó và mở nó bằng trình duyệt.

```json [debug]
[
  {
    "label": "Launch Chrome (TypeScript)",
    "adapter": "JavaScript",
    "type": "chrome",
    "request": "launch",
    "url": "http://localhost:5500",
    "program": "$ZODE_FILE",
    "webRoot": "${ZODE_WORKTREE_ROOT}",
    "build": {
      "command": "npx",
      "args": ["tsc"]
    },
    "skipFiles": ["<node_internals>/**"]
  }
]
```

## Xem thêm

- [Tài liệu Zode về Yarn](./yarn.md) hướng dẫn cấu hình dự án dùng Yarn.
- [Tài liệu Zode về Deno](./deno.md)
