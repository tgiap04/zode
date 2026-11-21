---
title: Themes
description: "Themes for Zode extensions."
---

# Giao diện (Theme)

Thư mục `themes` trong extension nên chứa một hoặc nhiều file theme.

Mỗi file theme phải tuân theo JSON schema tại [`https://zed.dev/schema/themes/v0.2.0.json`](https://zed.dev/schema/themes/v0.2.0.json).

Xem [bài blog này](https://zed.dev/blog/user-themes-now-in-preview) để biết thêm bối cảnh về cách tạo theme.

## Cấu trúc JSON của theme

Cấu trúc của một theme Zode được định nghĩa trong [JSON Schema của Zode Theme](https://zed.dev/schema/themes/v0.2.0.json).

Một theme Zode gồm một đối tượng Theme Family với:

- `name`: Tên của theme family
- `author`: Tên tác giả của theme family
- `themes`: Mảng các Theme thuộc theme family

Các thành phần chính của một đối tượng Theme gồm:

1. Metadata của theme:

   - `name`: Tên theme
   - `appearance`: "light" hoặc "dark"

2. Thuộc tính style nằm dưới `style`, như:

   - `background`: Màu nền chính
   - `foreground`: Màu chữ chính
   - `accent`: Màu nhấn dùng để làm nổi bật

3. Tô màu cú pháp:

   - `syntax`: Đối tượng chứa định nghĩa màu cho các thành phần cú pháp (ví dụ keyword, string, comment)

4. Thành phần UI:

   - Màu cho các thành phần UI như:
     - `element.background`: Màu nền của các thành phần UI
     - `border`: Màu viền cho từng trạng thái (bình thường, focus, được chọn)
     - `text`: Màu chữ cho từng trạng thái (bình thường, mờ, nhấn)

5. Màu riêng cho trình soạn thảo:

   - Màu cho các thành phần của trình soạn thảo như:
     - `editor.background`: Màu nền trình soạn thảo
     - `editor.gutter`: Màu gutter
     - `editor.line_number`: Màu số dòng

6. Màu terminal:
   - Định nghĩa màu ANSI cho terminal tích hợp

## Thiết kế theme của bạn

Bạn có thể dùng [Theme Builder của Zed](https://zed.dev/theme-builder) để thiết kế theme riêng dựa trên một theme có sẵn.

Công cụ này cho phép bạn tinh chỉnh và xem trước giao diện các bề mặt trong Zode.
Sau đó bạn có thể xuất JSON và đăng lên kho extension của Zode.
