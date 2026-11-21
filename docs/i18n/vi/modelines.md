# Modeline

Modeline là các dòng comment đặc biệt ở đầu hoặc cuối file, dùng để cấu hình trình soạn thảo riêng cho file đó. Zode hỗ trợ cả định dạng modeline của Vim lẫn Emacs, cho phép bạn đặt ngay trong file các cài đặt như kích thước tab, kiểu thụt lề và loại file.

## Cấu hình

Dùng cài đặt [`modeline_lines`](./reference/all-settings.md#modeline-lines) để quy định Zode tìm modeline trong bao nhiêu dòng:

```json [settings]
{
  "modeline_lines": 5
}
```

Đặt `0` để tắt hoàn toàn việc phân tích modeline.

## Emacs

Zode hỗ trợ tương thích một phần với [biến file của Emacs](https://www.gnu.org/software/emacs/manual/html_node/emacs/Specifying-File-Variables.html).

Ví dụ:

```python
# -*- mode: python; tab-width: 4; indent-tabs-mode: nil; -*-
```

### Các biến Emacs được hỗ trợ

| Biến                       | Mô tả                             | Cài đặt Zode                                                                               |
| -------------------------- | --------------------------------- | ------------------------------------------------------------------------------------------ |
| `mode`                     | Major mode/ngôn ngữ               | Nhận diện ngôn ngữ                                                                         |
| `tab-width`                | Độ rộng hiển thị của tab          | [`tab_size`](./reference/all-settings.md#tab-size)                                         |
| `fill-column`              | Cột ngắt dòng                     | [`preferred_line_length`](./reference/all-settings.md#preferred-line-length)               |
| `indent-tabs-mode`         | `nil` dùng dấu cách, `t` dùng tab | [`hard_tabs`](./reference/all-settings.md#hard-tabs)                                       |
| `electric-indent-mode`     | Tự động thụt lề                   | [`auto_indent`](./reference/all-settings.md#auto-indent)                                   |
| `require-final-newline`    | Đảm bảo có dòng trống cuối file   | [`ensure_final_newline_on_save`](./reference/all-settings.md#ensure-final-newline-on-save) |
| `show-trailing-whitespace` | Hiện khoảng trắng thừa cuối dòng  | [`show_whitespaces`](./reference/all-settings.md#show-whitespaces)                         |

## Vim

Zode hỗ trợ tương thích một phần với [modeline của Vim](https://vimhelp.org/options.txt.html#modeline).

Ví dụ:

```python
# vim: set ft=python ts=4 sw=4 et:
```

### Các tùy chọn Vim được hỗ trợ

| Tùy chọn       | Bí danh | Mô tả                           | Cài đặt Zode                                                                               |
| -------------- | ------- | ------------------------------- | ------------------------------------------------------------------------------------------ |
| `filetype`     | `ft`    | Loại file/ngôn ngữ              | Nhận diện ngôn ngữ                                                                         |
| `tabstop`      | `ts`    | Số dấu cách mà một tab chiếm    | [`tab_size`](./reference/all-settings.md#tab-size)                                         |
| `textwidth`    | `tw`    | Độ rộng dòng tối đa             | [`preferred_line_length`](./reference/all-settings.md#preferred-line-length)               |
| `expandtab`    | `et`    | Dùng dấu cách thay cho tab      | [`hard_tabs`](./reference/all-settings.md#hard-tabs)                                       |
| `noexpandtab`  | `noet`  | Dùng tab thay cho dấu cách      | [`hard_tabs`](./reference/all-settings.md#hard-tabs)                                       |
| `autoindent`   | `ai`    | Bật tự động thụt lề             | [`auto_indent`](./reference/all-settings.md#auto-indent)                                   |
| `noautoindent` | `noai`  | Tắt tự động thụt lề             | [`auto_indent`](./reference/all-settings.md#auto-indent)                                   |
| `endofline`    | `eol`   | Đảm bảo có dòng trống cuối file | [`ensure_final_newline_on_save`](./reference/all-settings.md#ensure-final-newline-on-save) |
| `noendofline`  | `noeol` | Tắt dòng trống cuối file        | [`ensure_final_newline_on_save`](./reference/all-settings.md#ensure-final-newline-on-save) |

## Lưu ý

- Zode tìm modeline trong kilobyte đầu tiên của file.
- Khi có cả hai loại, modeline Emacs được ưu tiên hơn modeline Vim.
- Modeline ở vài dòng đầu được ưu tiên hơn modeline ở cuối file.
