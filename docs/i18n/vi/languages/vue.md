---
title: Vue
description: "Configure Vue language support in Zode, including language servers, formatting, and debugging."
---

# Vue

Vue được hỗ trợ qua [extension Vue](https://github.com/zed-extensions/vue).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-vue](https://github.com/tree-sitter-grammars/tree-sitter-vue)
- Language Server: [vuejs/language-tools](https://github.com/vuejs/language-tools)

## Tuỳ chọn khởi tạo

### Chỉ định vị trí của TypeScript SDK

Mặc định, extension này giả định bạn đang làm việc trong dự án có thư mục `node_modules`, và tìm
TypeScript SDK trong thư mục đó.

Điều này không phải lúc nào cũng đúng; ví dụ, khi làm việc trong dự án dùng Yarn PnP thì không có `node_modules`. Để
có hỗ trợ trình soạn thảo, cách [được tài liệu hoá](https://yarnpkg.com/getting-started/editor-sdks) là chạy một lệnh như
`yarn dlx @yarnpkg/sdks`. Khi đó, bạn có thể đặt các tuỳ chọn khởi tạo sau trong cài đặt Zode:

```json
{
  "lsp": {
    "vue": {
      "initialization_options": {
        "typescript": {
          "tsdk": ".yarn/sdks/typescript/lib"
        }
      }
    }
  }
}
```

## Tuỳ chọn cài đặt

`lsp.vue.settings` được truyền thẳng cho Vue language server (Volar / [`vuejs/language-tools`](https://github.com/vuejs/language-tools)). Các cài đặt sau được bật mặc định:

```json
{
  "lsp": {
    "vue": {
      "settings": {
        // Display inlay hints for the `$event` parameter in inline event handlers.
        "vue.inlayHints.inlineHandlerLeading": true,
        // Display hints when required component props are missing in templates.
        "vue.inlayHints.missingProps": true,
        // Display inlay hints for patterns that wrap component options.
        "vue.inlayHints.optionsWrapper": true,
        // Display inlay hints related to `v-bind` shorthand (`:`).
        "vue.inlayHints.vBindShorthand": true
      }
    }
  }
}
```

Bạn có thể xem schema cấu hình cài đặt của upstream [`here`](https://github.com/vuejs/language-tools/blob/ee5041d27940cf6f9a5150635d3b13140a9dff54/extensions/vscode/package.json#L252).

> Lưu ý: Một số cài đặt (ví dụ `vue.editor.focusMode`) có thể không có tác dụng.

## Dùng Tailwind CSS Language Server với Vue

Để có đầy đủ tính năng (tự hoàn thành, lint...) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file Vue, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "vue": "html"
        },
        "experimental": {
          "classRegex": [
            "class=\"([^\"]*)\"",
            "class='([^']*)'",
            ":class=\"([^\"]*)\"",
            ":class='([^']*)'"
          ]
        }
      }
    }
  }
}
```

Với các cài đặt này, bạn sẽ có gợi ý hoàn thành cho class Tailwind CSS trong file template Vue. Ví dụ:

```vue
<template>
  <!-- Static class attribute -->
  <div class="flex items-center <completion here>">
    <p class="text-lg font-bold <completion here>">Hello World</p>
  </div>

  <!-- Dynamic class binding -->
  <div
    :class="
      active ? 'bg-blue-500 <completion here>' : 'bg-gray-200 <completion here>'
    "
  >
    Content
  </div>

  <!-- Array syntax -->
  <div :class="['flex', 'items-center', '<completion here>']">Content</div>

  <!-- Object syntax -->
  <div
    :class="{
      'flex <completion here>': isFlex,
      'block <completion here>': isBlock,
    }"
  >
    Content
  </div>
</template>
```
