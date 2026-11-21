---
title: Snippets - Zode
description: Create and use code snippets in Zode with tab stops, placeholders, variables, and language-scoped triggers.
---

# Snippet

Dùng action {#action snippets::ConfigureSnippets} để tạo file snippet mới hoặc sửa file snippet có sẵn cho một [phạm vi](#scopes) cụ thể.

Các snippet nằm trong thư mục `~/.config/zode/snippets`, bạn mở nhanh thư mục này bằng action {#action snippets::OpenFolder}.

## Ví dụ cấu hình

```json
{
  // Each snippet must have a name and body, but the prefix and description are optional.
  // The prefix is used to trigger the snippet, but when omitted then the name is used.
  // Use placeholders like $1, $2 or ${1:defaultValue} to define tab stops.
  // The $0 determines the final cursor position.
  // Placeholders with the same value are linked.
  // If the snippet contains the $ symbol outside of a placeholder, it must be escaped with two slashes (e.g. \\$var).
  "Log to console": {
    "prefix": "log",
    "body": ["console.info(\"Hello, ${1:World}!\")", "$0"],
    "description": "Logs to console"
  }
}
```

## Phạm vi

Phạm vi được xác định bằng tên ngôn ngữ viết thường, ví dụ `python.json` cho Python, `shell script.json` cho Shell Script, nhưng có vài ngoại lệ:

| Phạm vi    | Tên file        |
| ---------- | --------------- |
| Global     | snippets.json   |
| JSX        | javascript.json |
| Plain Text | plaintext.json  |

Để tạo snippet JSX, bạn phải dùng file snippet `javascript.json` thay vì `jsx.json`. TSX và TypeScript thì không thuộc ngoại lệ này, vẫn theo quy tắc ở trên.

## Hạn chế đã biết

- Khi truyền vào một danh sách prefix, chỉ prefix đầu tiên được dùng.
- Hiện chỉ hỗ trợ định dạng file snippet `json`.
