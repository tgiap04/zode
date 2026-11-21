---
title: Snippets
description: "Snippets for Zode extensions."
---

# Snippet

Extension có thể cung cấp snippet cho một hoặc nhiều ngôn ngữ.

Mỗi file chứa snippet được khai báo trong trường `snippets` của file `extensions.toml`.

Đường dẫn được tham chiếu phải tương đối so với `extension.toml`.

## Định nghĩa snippet

Một extension có thể cung cấp một hoặc nhiều snippet. Mỗi snippet phải được đăng ký trong `extension.toml`.

Zode khớp file snippet theo tên ngôn ngữ viết thường (ví dụ `rust.json` cho Rust).
Bạn có thể dùng `snippets.json` làm tên file để định nghĩa snippet dùng được với mọi ngôn ngữ của buffer hiện tại.

Ví dụ, đây là extension cung cấp snippet cho Rust và TypeScript:

```toml
snippets = ["./snippets/rust.json", "./snippets/typescript.json"]
```

Để biết cách tạo snippet, xem [tài liệu Snippets](../snippets.md).
