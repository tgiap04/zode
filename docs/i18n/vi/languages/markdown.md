---
title: Markdown
description: "Configure Markdown language support in Zode, including language servers, formatting, and debugging."
---

# Markdown

Markdown được hỗ trợ sẵn trong Zode.

- Tree-sitter: [tree-sitter-markdown](https://github.com/tree-sitter-grammars/tree-sitter-markdown)
- Language Server: N/A

## Tô sáng cú pháp cho khối code

Zode hỗ trợ tô sáng cú pháp theo từng ngôn ngữ cho khối code trong markdown nhờ [ngữ pháp tree-sitter của ngôn ngữ](../extensions/languages.md#grammar). Mọi [ngôn ngữ Zode hỗ trợ](../languages.md), kể cả các ngôn ngữ do extension chính thức hoặc cộng đồng cung cấp, đều dùng được trong khối code markdown. Bạn chỉ cần ghi tên ngôn ngữ sau dấu rào mở <kbd>```</kbd> như sau:

````python
```python
import functools as ft

@ft.lru_cache(maxsize=500)
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
```
````

## Cấu hình

### Định dạng

Zode hỗ trợ dùng Prettier để tự động định dạng lại tài liệu Markdown. Bạn có thể kích hoạt thủ công bằng action {#action editor::Format} hoặc phím tắt {#kb editor::Format}. Ngoài ra, bạn có thể bật định dạng khi lưu.

Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > Markdown, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Markdown": {
      "format_on_save": "on"
    }
  },
```

### Tiếp tục danh sách

Zode tự động tiếp tục danh sách khi bạn nhấn Enter ở cuối một mục danh sách. Các loại danh sách được hỗ trợ:

- Danh sách không thứ tự (dấu `-`, `*` hoặc `+`)
- Danh sách có thứ tự (số tự động tăng)
- Danh sách việc cần làm (`- [ ]` và `- [x]`)

Nhấn Enter ở một mục danh sách trống sẽ xoá dấu đầu dòng và thoát khỏi danh sách.

Để tắt hành vi này, hãy cấu hình trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > Markdown, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Markdown": {
      "extend_list_on_newline": false
    }
  },
```

### Thụt lề danh sách

Zode thụt lề mục danh sách khi bạn nhấn Tab lúc con trỏ nằm trên dòng chỉ có dấu đầu dòng của danh sách. Nhờ vậy bạn tạo danh sách lồng nhau nhanh chóng.

Để tắt hành vi này, hãy cấu hình trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > Markdown, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Markdown": {
      "indent_list_on_tab": false
    }
  },
```

### Khoảng trắng cuối dòng

Theo mặc định Zode xoá khoảng trắng cuối dòng khi lưu. Nếu bạn dựa vào khoảng trắng cuối dòng không nhìn thấy được chuyển thành `<br />` trong file Markdown, bạn có thể tắt hành vi này.

Cấu hình trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > Markdown, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "Markdown": {
      "remove_trailing_whitespace_on_save": false
    }
  },
```
