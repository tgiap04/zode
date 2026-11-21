---
title: Glob Patterns - Zode
description: How glob patterns work in Zode for file matching, search filtering, and configuration. Syntax reference and examples.
---

# Glob

Zode hỗ trợ [glob](<https://en.wikipedia.org/wiki/Glob_(programming)>), tên chính thức của các ký tự đại diện khớp đường dẫn kiểu shell Unix như `*.md` hay `docs/src/**/*.md`, được sh, bash, zsh... hỗ trợ. Glob giống nhưng khác với [regex (biểu thức chính quy)](https://en.wikipedia.org/wiki/Regular_expression). Trong Zode, glob thường dùng để khớp tên file.

## Kiểu glob

Zode dùng hai crate Rust khác nhau để khớp mẫu glob:

- [crate ignore](https://docs.rs/ignore/latest/ignore/) để khớp các mẫu glob lưu trong file `.gitignore`
- [crate glob](https://docs.rs/glob/latest/glob/) để khớp đường dẫn file trong Zode

Biểu thức đơn giản thì dùng được ở mọi môi trường (ví dụ chạy `ls *.py` hoặc `*.tmp` trong gitignore), nhưng với tính năng nâng cao (lớp ký tự, loại trừ, `**`...) thì mức hỗ trợ và cú pháp khác nhau khá nhiều giữa các bản triển khai. Phần còn lại của tài liệu này mô tả glob theo cách Zode hỗ trợ qua crate `glob`. Xem [Tham khảo](#references) bên dưới để có liên kết tài liệu cú pháp glob của `.gitignore`, shell và các ngôn ngữ lập trình khác.

Crate `glob` được viết hoàn toàn bằng Rust và không dựa vào giao diện `glob` / `fnmatch` của libc trên nền tảng của bạn. Nghĩa là glob trong Zode hoạt động tương tự nhau trên các nền tảng.

## Giới thiệu

Một "mẫu" glob dùng để khớp tên file hoặc đường dẫn file đầy đủ. Ví dụ, khi dùng "Search all files" {#kb project_search::ToggleFocus}, bạn có thể bấm nút "Toggle Filters" hình phễu hoặc {#kb project_search::ToggleFilters} để hiện thêm các ô tìm kiếm "Include" và "Exclude", cho phép chỉ định mẫu glob khớp đường dẫn và tên file.

### Nhiều mẫu

Bạn có thể chỉ định nhiều mẫu glob trong bộ lọc Project Search bằng cách ngăn cách bằng dấu phẩy. Với các mẫu ngăn cách bằng dấu phẩy, Zode xử lý đúng dấu ngoặc nhọn bên trong từng mẫu:

- `*.ts, *.tsx` — Khớp file TypeScript và TSX
- `src/{components,utils}/**/*.ts, tests/**/*.test.ts` — Khớp file TypeScript trong một số thư mục cụ thể cùng các file test

Mỗi mẫu được đánh giá độc lập. Dấu phẩy trong ngoặc nhọn (như `{a,b}`) được coi là một phần của mẫu, không phải dấu phân cách.

**Quan trọng:** Dù ngoặc nhọn được giữ nguyên trong mẫu, Zode không mở rộng chúng thành nhiều mẫu. Mẫu `src/{a,b}/*.ts` khớp đúng cấu trúc đường dẫn theo nghĩa đen, không phải `src/a/*.ts` HOẶC `src/b/*.ts`. Điều này khác với hành vi của shell.

Khi tạo mẫu glob, bạn có thể dùng một hoặc nhiều ký tự đặc biệt:

| Ký tự đặc biệt | Ý nghĩa                                                 |
| -------------- | ------------------------------------------------------- |
| `?`            | Khớp một ký tự bất kỳ                                   |
| `*`            | Khớp một dãy ký tự bất kỳ (có thể rỗng)                 |
| `**`           | Khớp thư mục hiện tại và các thư mục con ở mọi cấp      |
| `[abc]`        | Khớp một ký tự bất kỳ trong ngoặc                       |
| `[a-z]`        | Khớp một ký tự bất kỳ trong khoảng (xếp theo Unicode)   |
| `[!...]`       | Phủ định của `[...]` (khớp ký tự không nằm trong ngoặc) |

Ghi chú:

1. Ký tự ngoặc nhọn `{` và `}` là ký tự mẫu theo nghĩa đen, không phải toán tử mở rộng. Mẫu `src/{a,b}/*.ts` khớp các đường dẫn chứa đúng chuỗi `{a,b}`, không phải đường dẫn khớp `src/a/*.ts` hay `src/b/*.ts` như glob của shell.
2. Để khớp ký tự `-` theo nghĩa đen trong ngoặc vuông, nó phải đứng đầu `[-abc]` hoặc cuối `[abc-]`.
3. Để khớp ký tự `[` theo nghĩa đen, dùng `[[]` hoặc đặt nó làm ký tự đầu của nhóm `[[abc]`.
4. Để khớp ký tự `]` theo nghĩa đen, dùng `[]]` hoặc đặt nó làm ký tự cuối của nhóm `[abc]]`.

## Ví dụ

### Khớp phần mở rộng file

Nếu chỉ muốn tìm file Markdown, thêm `*.md` vào ô "Include".

### Khớp không phân biệt hoa thường

Glob trong Zode phân biệt hoa thường, nên `*.c` sẽ không khớp `main.C` (kể cả trên hệ thống file không phân biệt hoa thường như HFS+/APFS của macOS). Thay vào đó hãy dùng ngoặc vuông để khớp ký tự. Vậy thay vì `*.c` dùng `*.[cC]`.

### Khớp thư mục

Nếu muốn tìm trong [repository zed](https://github.com/zed-industries/zed) các ví dụ về [Cấu hình language server](https://zodekit.com/docs/configuring-languages#configuring-language-servers) (dưới `"lsp"` trong settings.json của Zode), bạn có thể tìm `"lsp"` và ở bộ lọc "Include" chỉ định `docs/**/*.md`. Mẫu này chỉ khớp các file có đường dẫn nằm dưới thư mục `docs` hoặc bất kỳ thư mục con lồng nhau `**/` nào của nó, với tên file kết thúc bằng `.md`.

Nếu muốn giới hạn chỉ trong các trang [Tài liệu riêng cho từng ngôn ngữ của Zode](https://zodekit.com/docs/languages), bạn có thể dùng mẫu hẹp hơn: `docs/src/languages/*.md`. Mẫu này khớp [`docs/src/languages/rust.md`](https://github.com/tgiap04/zode/blob/develop/docs/src/languages/rust.md) và [`docs/src/languages/cpp.md`](https://github.com/tgiap04/zode/blob/develop/docs/src/languages/cpp.md) nhưng không khớp [`docs/src/configuring-languages.md`](https://github.com/tgiap04/zode/blob/develop/docs/src/configuring-languages.md).

### Ký tự đại diện ngầm

Khi dùng bộ lọc "Include" / "Exclude" trong Project Search, mỗi glob được bọc bằng ký tự đại diện ngầm. Ví dụ để loại các file có license trong đường dẫn hoặc tên file khỏi kết quả tìm, chỉ cần gõ `license` vào ô exclude. Phía sau, Zode biến `license` thành `**license**`. Nghĩa là các file tên `license.*`, `*.license` hoặc nằm trong thư mục con `license` đều bị lọc ra. Nhờ vậy người dùng lọc `*.ts` dễ dàng mà không phải nhớ gõ `**/*.ts` mỗi lần.

Ngược lại, nếu trong cài đặt Zode bạn muốn một ghi đè [`file_types`](./reference/all-settings.md#file-types) chỉ áp dụng cho một thư mục nhất định, bạn phải ghi rõ ký tự đại diện glob. Ví dụ, nếu có một thư mục file template đuôi `html` mà bạn muốn nhận là template Jinja2, bạn có thể dùng:

```json [settings]
{
  "file_types": {
    "C++": ["[cC]"],
    "Jinja2": ["**/templates/*.html"]
  }
}
```

## Tham khảo

Dù glob trong Zode được triển khai như mô tả ở trên, khi viết code dùng glob ở ngôn ngữ khác, hãy xem tài liệu glob của nền tảng bạn dùng:

- [macOS fnmatch](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/fnmatch.3.html) (thư viện chuẩn C của BSD)
- [Linux fnmatch](https://www.gnu.org/software/libc/manual/html_node/Wildcard-Matching.html) (thư viện chuẩn C của GNU)
- [POSIX fnmatch](https://pubs.opengroup.org/onlinepubs/9699919799/functions/fnmatch.html) (đặc tả POSIX)
- [node-glob](https://github.com/isaacs/node-glob) (gói `glob` của Node.js)
- [Python glob](https://docs.python.org/3/library/glob.html) (thư viện chuẩn Python)
- [Golang glob](https://pkg.go.dev/path/filepath#Match) (thư viện chuẩn Go)
- [gitignore patterns](https://git-scm.com/docs/gitignore) (định dạng mẫu Gitignore)
- [PowerShell: About Wildcards](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_wildcards) (ký tự đại diện trong PowerShell)
