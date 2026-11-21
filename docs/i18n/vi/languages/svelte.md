---
title: Svelte
description: "Configure Svelte language support in Zode, including language servers, formatting, and debugging."
---

# Svelte

Hỗ trợ Svelte có qua [extension Svelte](https://github.com/zed-extensions/svelte).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-svelte](https://github.com/tree-sitter-grammars/tree-sitter-svelte)
- Language server: [sveltejs/language-tools](https://github.com/sveltejs/language-tools)

## Cấu hình thêm kiểu hiển thị cho giao diện (theme)

Bạn có thể chỉnh cách hiển thị một số kiểu, như directive và modifier, trong attribute:

```json
"syntax": {
  // Styling for directives (e.g., `class:foo` or `on:click`) (the `on` or `class` part of the attribute).
  "attribute.function": {
    "color": "#ff0000"
  },
  // Styling for modifiers at the end of attributes, e.g. `on:<click|preventDefault|stopPropagation>`
  "attribute.special": {
    "color": "#00ff00"
  }
}
```

## Inlay Hint

Khi bật inlay hint trong Zode, để language server gửi chúng về, Zode đặt các tùy chọn khởi tạo sau:

```json
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
    "suppressWhenTypeMatchesName": false
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
```

Để ghi đè các setting này, dùng cấu hình sau:

```json [settings]
"lsp": {
  "svelte-language-server": {
    "initialization_options": {
      "configuration": {
        "typescript": {
          // ......
        },
        "javascript": {
          // ......
        }
      }
    }
  }
}
```

Xem [`package.json` của language server TypeScript](https://github.com/microsoft/vscode/blob/main/extensions/typescript-language-features/package.json) để biết thêm.

## Dùng Tailwind CSS Language Server với Svelte

Để có đầy đủ tính năng (autocomplete, linting...) của [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file Svelte, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "svelte": "html"
        },
        "experimental": {
          "classRegex": [
            "class=\"([^\"]*)\"",
            "class='([^']*)'",
            "class:\\s*([^\\s{]+)",
            "\\{\\s*class:\\s*\"([^\"]*)\"",
            "\\{\\s*class:\\s*'([^']*)'"
          ]
        }
      }
    }
  }
}
```

Với các setting này, bạn sẽ có gợi ý hoàn thành cho class Tailwind CSS trong file Svelte. Ví dụ:

```svelte
<!-- Standard class attribute -->
<div class="flex items-center <completion here>">
  <p class="text-lg font-bold <completion here>">Hello World</p>
</div>

<!-- Class directive -->
<button class:active="bg-blue-500 <completion here>">Click me</button>

<!-- Expression -->
<div class={active ? "flex <completion here>" : "hidden <completion here>"}>
  Content
</div>
```
