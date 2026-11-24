---
title: Astro
description: "Configure Astro language support in Zode, including language servers, formatting, and debugging."
---

# Astro

Astro được hỗ trợ qua [extension Astro](https://github.com/zed-extensions/astro).

- Tree-sitter: [virchau13/tree-sitter-astro](https://github.com/virchau13/tree-sitter-astro)
- Language Server: [withastro/language-tools](https://github.com/withastro/astro/tree/main/packages/language-tools/language-server)

## Dùng Tailwind CSS Language Server với Astro

Để có đủ tính năng (tự động hoàn thành, linting, ...) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file Astro, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "astro": "html"
        },
        "experimental": {
          "classRegex": [
            "class=\"([^\"]*)\"",
            "class='([^']*)'",
            "class:list=\"{([^}]*)}\"",
            "class:list='{([^}]*)}'"
          ]
        }
      }
    }
  }
}
```

Với các cài đặt này, bạn sẽ được gợi ý hoàn thành cho class Tailwind CSS trong file template Astro. Ví dụ:

```astro
---
const active = true;
---

<!-- Standard class attribute -->
<div class="flex items-center <completion here>">
  <p class="text-lg font-bold <completion here>">Hello World</p>
</div>

<!-- class:list directive -->
<div class:list={["flex", "items-center", "<completion here>"]}>
  Content
</div>

<!-- Conditional classes -->
<div class:list={{ "flex <completion here>": active, "hidden <completion here>": !active }}>
  Content
</div>
```
