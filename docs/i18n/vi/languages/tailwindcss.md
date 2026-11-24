---
title: Tailwind CSS
description: "Configure Tailwind CSS language support in Zode, including language servers, formatting, and debugging."
---

# Tailwind CSS

Zode hỗ trợ sẵn tự hoàn thành, lint và xem trước khi di chuột cho Tailwind CSS.

- Language Server: [tailwindlabs/tailwindcss-intellisense](https://github.com/tailwindlabs/tailwindcss-intellisense)

Các ngôn ngữ dùng được với Tailwind CSS trong Zode:

- [Astro](./astro.md#using-the-tailwind-css-language-server-with-astro)
- [CSS](./css.md)
- [ERB](./ruby.md#using-the-tailwind-css-language-server-with-ruby)
- [Gleam](./gleam.md)
- [HEEx](./elixir.md#using-the-tailwind-css-language-server-with-heex-templates)
- [HTML](./html.md#using-the-tailwind-css-language-server-with-html)
- [TypeScript](./typescript.md#using-the-tailwind-css-language-server-with-typescript)
- [JavaScript](./javascript.md#using-the-tailwind-css-language-server-with-javascript)
- [PHP](./php.md#using-the-tailwind-css-language-server-with-php)
- [Svelte](./svelte.md#using-the-tailwind-css-language-server-with-svelte)
- [Vue](./vue.md#using-the-tailwind-css-language-server-with-vue)

## Cấu hình

Nếu mặc định language server chưa đủ để Tailwind chạy cho một ngôn ngữ nào đó, bạn có thể cấu hình language server và thêm vào mục `lsp` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "classFunctions": ["cva", "cx"],
        "experimental": {
          "classRegex": ["[cls|className]\\s\\:\\=\\s\"([^\"]*)"]
        }
      }
    }
  }
}
```

Xem thêm [tài liệu cài đặt của Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense?tab=readme-ov-file#extension-settings).

### Dùng chế độ Tailwind CSS trong file CSS

Zode hỗ trợ chế độ ngôn ngữ Tailwind CSS, cung cấp đầy đủ IntelliSense cho CSS ngay cả khi dùng các at-rule riêng của Tailwind như `@apply`, `@layer` và `@theme`.
Cấu hình language server trong Settings ({#kb zode::OpenSettings}) ở mục Languages > CSS, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "CSS": {
      "language_servers": [
        "tailwindcss-intellisense-css",
        "!vscode-css-language-server",
        "..."
      ]
    }
  }
}
```

Language server `tailwindcss-intellisense-css` là lựa chọn thay thế cho CSS language server mặc định, giữ mọi tính năng IntelliSense CSS chuẩn và bổ sung hỗ trợ cú pháp riêng của Tailwind.

### Plugin Prettier

Zode hỗ trợ Prettier sẵn, nghĩa là nếu bạn đã cài [plugin Tailwind CSS cho Prettier](https://github.com/tailwindlabs/prettier-plugin-tailwindcss), chỉ cần thêm nó vào cấu hình Prettier là chạy tự động:

```json
// .prettierrc
{
  "plugins": ["prettier-plugin-tailwindcss"]
}
```
