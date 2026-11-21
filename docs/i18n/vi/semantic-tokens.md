---
title: Semantic Tokens and Syntax Highlighting - Zode
description: Enable and configure semantic token highlighting in Zode for richer, language-server-aware syntax coloring.
---

# Semantic Token

Semantic token cho phép tô màu cú pháp phong phú hơn nhờ thông tin từ language server. Khác với tô màu bằng tree-sitter vốn chỉ dựa trên cú pháp, semantic token hiểu ý nghĩa của code—phân biệt biến cục bộ với tham số, hay định nghĩa class với chỗ tham chiếu tới class.

## Bật semantic token

Semantic token do cài đặt `semantic_tokens` điều khiển. Mặc định semantic token bị tắt.

```json [settings]
{
  "semantic_tokens": "combined"
}
```

Cài đặt này nhận ba giá trị:

| Giá trị      | Mô tả                                                                                                                            |
| ------------ | -------------------------------------------------------------------------------------------------------------------------------- |
| `"off"`      | Không yêu cầu semantic token từ language server. Chỉ dùng tô màu của tree-sitter. (Mặc định)                                     |
| `"combined"` | Dùng semantic token của LSP cùng với tô màu của tree-sitter. Tree-sitter tô màu nền, semantic token phủ thêm thông tin lên trên. |
| `"full"`     | Chỉ dùng semantic token của LSP. Tô màu của tree-sitter bị tắt hẳn với các buffer có hỗ trợ semantic token.                      |

Bạn có thể cấu hình toàn cục hoặc theo từng ngôn ngữ:

```json [settings]
{
  "semantic_tokens": "off",
  "languages": {
    "Rust": {
      "semantic_tokens": "combined"
    },
    "TypeScript": {
      "semantic_tokens": "full"
    }
  }
}
```

> **Lưu ý:** Đổi chế độ `semantic_tokens` có thể cần khởi động lại language server mới có tác dụng. Dùng lệnh `lsp: restart language servers` từ bảng lệnh (command palette) nếu màu không cập nhật ngay.

## Tuỳ chỉnh màu của token

Semantic token được tô theo các quy tắc ánh xạ kiểu và modifier của token LSP sang kiểu của giao diện (theme) hoặc màu tuỳ chọn. Zode có sẵn mặc định hợp lý, nhưng bạn có thể tuỳ chỉnh trong settings.json: thêm quy tắc dưới khoá `global_lsp_settings.semantic_token_rules`.

Quy tắc được khớp theo thứ tự, quy tắc khớp đầu tiên thắng.
Quy tắc do người dùng định nghĩa có ưu tiên cao nhất, sau đó đến quy tắc ngôn ngữ do extension cung cấp, rồi mới tới mặc định của Zode.

### Cấu trúc quy tắc

Mỗi quy tắc có thể chỉ định:

| Thuộc tính         | Mô tả                                                                                                     |
| ------------------ | --------------------------------------------------------------------------------------------------------- |
| `token_type`       | Kiểu semantic token LSP cần khớp (ví dụ `"variable"`, `"function"`, `"class"`). Bỏ qua thì khớp mọi kiểu. |
| `token_modifiers`  | Danh sách modifier phải có đủ (ví dụ `["declaration"]`, `["readonly", "static"]`).                        |
| `style`            | Danh sách tên kiểu của giao diện để thử. Kiểu đầu tiên tìm thấy trong giao diện hiện tại sẽ được dùng.    |
| `foreground_color` | Ghi đè màu chữ, định dạng hex (ví dụ `"#ff0000"`).                                                        |
| `background_color` | Ghi đè màu nền, định dạng hex.                                                                            |
| `underline`        | Boolean hoặc màu hex. Nếu `true`, gạch chân bằng màu chữ.                                                 |
| `strikethrough`    | Boolean hoặc màu hex. Nếu `true`, gạch ngang bằng màu chữ.                                                |
| `font_weight`      | `"normal"` hoặc `"bold"`.                                                                                 |
| `font_style`       | `"normal"` hoặc `"italic"`.                                                                               |

### Ví dụ: tô nổi các tham chiếu chưa phân giải

Để làm các tham chiếu chưa phân giải nổi bật:

```json [settings]
{
  "global_lsp_settings": {
    "semantic_token_rules": [
      {
        "token_type": "unresolvedReference",
        "foreground_color": "#c93f3f",
        "font_weight": "bold"
      }
    ]
  }
}
```

### Ví dụ: tô nổi code unsafe

Để tô nổi các thao tác unsafe trong Rust:

```json [settings]
{
  "global_lsp_settings": {
    "semantic_token_rules": [
      {
        "token_type": "punctuation",
        "token_modifiers": ["unsafe"],
        "foreground_color": "#AA1111",
        "font_weight": "bold"
      }
    ]
  }
}
```

### Ví dụ: dùng kiểu của giao diện

Thay vì gõ cứng màu, hãy tham chiếu các kiểu từ giao diện của bạn:

```json [settings]
{
  "global_lsp_settings": {
    "semantic_token_rules": [
      {
        "token_type": "variable",
        "token_modifiers": ["mutable"],
        "style": ["variable.mutable", "variable"]
      }
    ]
  }
}
```

Kiểu đầu tiên tìm thấy trong giao diện hiện tại sẽ được dùng, nhờ đó có phương án dự phòng.

### Ví dụ: tắt một kiểu token

Để tắt tô màu cho một kiểu token cụ thể, thêm một quy tắc rỗng khớp với nó:

```json [settings]
{
  "global_lsp_settings": {
    "semantic_token_rules": [
      {
        "token_type": "comment"
      }
    ]
  }
}
```

Vì quy tắc người dùng có ưu tiên cao nhất và quy tắc khớp đầu tiên thắng, quy tắc rỗng này ngăn mọi kiểu dáng áp lên token comment.

## Quy tắc mặc định

Các quy tắc semantic token mặc định của Zode ánh xạ kiểu token LSP chuẩn sang các kiểu thông dụng của giao diện. Ví dụ:

- kiểu `function` → kiểu `function`
- kiểu `variable` có modifier `constant` → kiểu `constant`
- kiểu `class` → kiểu `type.class`, `class` hoặc `type` (cái nào tìm thấy trước)
- kiểu `comment` có modifier `documentation` → kiểu `comment.documentation` hoặc `comment.doc`

Có thể xem toàn bộ cấu hình mặc định trong Zode bằng lệnh `zode: show default semantic token rules`.

## Các kiểu token chuẩn

Language server báo token bằng các kiểu chuẩn hoá. Các kiểu thường gặp gồm:

| Kiểu            | Mô tả                                   |
| --------------- | --------------------------------------- |
| `namespace`     | Tên namespace hoặc module               |
| `type`          | Tên kiểu                                |
| `class`         | Tên class                               |
| `enum`          | Tên kiểu enum                           |
| `interface`     | Tên interface                           |
| `struct`        | Tên struct                              |
| `typeParameter` | Tham số kiểu generic                    |
| `parameter`     | Tham số của hàm/method                  |
| `variable`      | Tên biến                                |
| `property`      | Thuộc tính object hoặc field của struct |
| `enumMember`    | Biến thể của enum                       |
| `function`      | Tên hàm                                 |
| `method`        | Tên method                              |
| `macro`         | Tên macro                               |
| `keyword`       | Từ khoá của ngôn ngữ                    |
| `comment`       | Comment                                 |
| `string`        | Chuỗi literal                           |
| `number`        | Số literal                              |
| `operator`      | Toán tử                                 |

Các modifier thường gặp gồm: `declaration`, `definition`, `readonly`, `static`, `deprecated`, `async`, `documentation`, `defaultLibrary`, và modifier riêng của từng ngôn ngữ như `unsafe` (Rust) hay `abstract` (TypeScript).

Để xem đặc tả đầy đủ, tham khảo [tài liệu LSP Semantic Tokens](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#semanticTokenTypes).

## Xem semantic token

Để xem semantic token áp lên code theo thời gian thực, dùng lệnh `dev: open highlights tree view` từ bảng lệnh. Lệnh này mở một bảng hiển thị mọi highlight (gồm cả semantic token) của buffer hiện tại, giúp bạn dễ hiểu token nào đang được áp và gỡ lỗi các quy tắc tuỳ chỉnh.

## Xử lý sự cố

### Không thấy tô màu semantic

1. Đảm bảo `semantic_tokens` được đặt là `"combined"` hoặc `"full"` cho ngôn ngữ đó
2. Kiểm tra language server có hỗ trợ semantic token (không phải server nào cũng có)
3. Thử khởi động lại language server bằng `lsp: restart language servers`
4. Xem log LSP (`workspace: open lsp log`) để tìm lỗi

### Màu không cập nhật sau khi đổi cài đặt

Đổi chế độ `semantic_tokens` có thể cần khởi động lại language server. Dùng `lsp: restart language servers` từ bảng lệnh.

### Kiểu của giao diện không được áp dụng

Hãy chắc tên kiểu trong quy tắc khớp với kiểu được định nghĩa trong giao diện. Mảng `style` cung cấp phương án dự phòng—nếu không thấy kiểu đầu tiên, Zode thử kiểu tiếp theo.
