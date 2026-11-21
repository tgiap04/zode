---
title: Vim Mode - Zode
description: Full Vim emulation in Zode with motions, text objects, visual mode, macros, and Zode-specific extensions.
---

# Vim Mode

Zode có lớp giả lập Vim. Trang này nói về cách bật/tắt vim mode, phím tắt, các tính năng riêng của Zode và tùy chọn cấu hình.

## Thiết kế vim mode của Zode

Vim mode tái tạo hành vi của motion và lệnh ở chỗ hợp lý, và dùng chức năng riêng của Zode ở chỗ cách của Zode tốt hơn. Mục tiêu là trải nghiệm quen thuộc, dùng được ngay mà không cần cấu hình.

Trong đó có hỗ trợ điều hướng theo ngữ nghĩa, nhiều con trỏ, hoặc các tính năng thường do plugin cung cấp như bao quanh văn bản (surround).

Vì vậy vim mode của Zode không sao chép Vim một-một, mà kết hợp thiết kế modal của Vim với các tính năng hiện đại của Zode để mang lại trải nghiệm mượt hơn. Nó cũng cấu hình được, nên bạn có thể thêm phím tắt riêng hoặc ghi đè mặc định.

### Khác biệt cốt lõi

Có bốn loại tính năng trong vim mode dùng chức năng lõi của Zode, dẫn tới một vài khác biệt về hành vi:

1. **Motion**: vim mode dùng bộ phân tích ngữ nghĩa của Zode để chỉnh hành vi motion theo từng ngôn ngữ. Ví dụ trong Rust, nhảy tới dấu ngoặc khớp bằng `%` hoạt động với ký tự pipe `|`. Trong JavaScript, `w` coi `$` là ký tự của từ.
2. **Chọn khối trực quan (visual block)**: vim mode dùng nhiều con trỏ của Zode để giả lập visual block, giúp chọn khối linh hoạt hơn nhiều. Ví dụ, bất cứ thứ gì bạn chèn sau vùng chọn khối đều cập nhật trên mọi dòng theo thời gian thực, và bạn có thể thêm hoặc bớt con trỏ bất cứ lúc nào.
3. **Macro**: vim mode dùng hệ thống ghi của Zode cho macro vim. Nhờ vậy bạn ghi và phát lại được các thao tác phức tạp hơn, như tự hoàn thành.
4. **Tìm và thay thế**: vim mode dùng hệ thống tìm kiếm của Zode, nên cú pháp biểu thức chính quy hơi khác Vim. [Xem mục khác biệt về Regex](#regex-differences) để biết chi tiết.

> **Lưu ý:** Nền tảng của vim mode trong Zode đã bao phủ nhiều trường hợp dùng, và chúng tôi luôn muốn cải thiện nó. Nếu bạn thấy thiếu tính năng mà bạn cần trong quy trình làm việc, hãy [tạo issue trên GitHub](https://github.com/tgiap04/zode/issues).

## Bật và tắt vim mode

Khi mở Zode lần đầu, bạn sẽ thấy một ô chọn trên màn hình chào mừng cho phép bật vim mode.

Nếu bỏ lỡ, bạn có thể bật/tắt vim mode bất cứ lúc nào bằng cách mở bảng lệnh (command palette) và dùng lệnh workspace `toggle vim mode`.

> **Lưu ý**: Lệnh này bật/tắt thuộc tính sau trong cài đặt người dùng:
>
> ```json [settings]
> {
>   "vim_mode": true
> }
> ```

## Tính năng riêng của Zode

Zode xây dựng trên nền tảng hiện đại, trong đó (cùng nhiều thứ khác) dùng Tree-sitter và language server để hiểu nội dung file bạn đang sửa, và hỗ trợ nhiều con trỏ ngay từ đầu.

Vim mode có vài phím tắt "lõi Zode" giúp bạn tận dụng tối đa bộ tính năng riêng của Zode.

### Language server

Các lệnh sau dùng language server để giúp bạn điều hướng và refactor code.

| Lệnh                                  | Phím tắt mặc định |
| ------------------------------------- | ----------------- |
| Đi tới định nghĩa                     | `g d`             |
| Đi tới khai báo                       | `g D`             |
| Đi tới định nghĩa kiểu                | `g y`             |
| Đi tới phần triển khai                | `g I`             |
| Đổi tên (đổi định nghĩa)              | `c d`             |
| Đi tới mọi tham chiếu của từ hiện tại | `g A`             |
| Tìm symbol trong file hiện tại        | `g s`             |
| Tìm symbol trong toàn dự án           | `g S`             |
| Đi tới chẩn đoán kế tiếp              | `g ]` or `] d`    |
| Đi tới chẩn đoán trước                | `g [` or `[ d`    |
| Hiện lỗi trong dòng (hover)           | `g h`             |
| Mở menu code action                   | `g .`             |

### Git

| Lệnh                                    | Phím tắt mặc định |
| --------------------------------------- | ----------------- |
| Đi tới thay đổi git kế tiếp             | `] c`             |
| Đi tới thay đổi git trước               | `[ c`             |
| Mở rộng diff hunk                       | `d o`             |
| Bật/tắt staged                          | `d O`             |
| Stage và tới mục kế (trong diff view)   | `d u`             |
| Unstage và tới mục kế (trong diff view) | `d U`             |
| Khôi phục thay đổi                      | `d p`             |

### Tree-sitter

Tree-sitter là parser Zode dùng để hiểu cấu trúc code của bạn. Zode cung cấp motion thay đổi vị trí con trỏ hiện tại, và text object có thể dùng làm đích của các action.

| Lệnh                              | Phím tắt mặc định           |
| --------------------------------- | --------------------------- |
| Đi tới method kế tiếp/trước       | `] m` / `[ m`               |
| Đi tới cuối method kế tiếp/trước  | `] M` / `[ M`               |
| Đi tới section kế tiếp/trước      | `] ]` / `[ [`               |
| Đi tới cuối section kế tiếp/trước | `] [` / `[ ]`               |
| Đi tới comment kế tiếp/trước      | `] /`, `] *` / `[ /`, `[ *` |
| Chọn node cú pháp lớn hơn         | `[ x`                       |
| Chọn node cú pháp nhỏ hơn         | `] x`                       |

| Text object                                                  | Phím tắt mặc định |
| ------------------------------------------------------------ | ----------------- |
| Quanh một class, định nghĩa, v.v.                            | `a c`             |
| Bên trong một class, định nghĩa, v.v.                        | `i c`             |
| Quanh một hàm, method, v.v.                                  | `a f`             |
| Bên trong một hàm, method, v.v.                              | `i f`             |
| Một comment                                                  | `g c`             |
| Một đối số, hoặc phần tử danh sách, v.v.                     | `i a`             |
| Một đối số, hoặc phần tử danh sách, v.v. (gồm dấu phẩy cuối) | `a a`             |
| Quanh một thẻ kiểu HTML                                      | `a t`             |
| Bên trong một thẻ kiểu HTML                                  | `i t`             |
| Cấp thụt lề hiện tại, cùng một dòng trước và sau             | `a I`             |
| Cấp thụt lề hiện tại, cùng một dòng trước                    | `a i`             |
| Cấp thụt lề hiện tại                                         | `i i`             |

Lưu ý định nghĩa đích của họ motion `[m` giống với
ranh giới do `af` định nghĩa. Đích của `[[` giống với đích do `ac` định nghĩa, tuy
nếu không có class thì hàm cũng được dùng. Tương tự `gc` dùng để tìm `[ /`. `g c`

Định nghĩa của hàm, class và comment phụ thuộc ngôn ngữ, và có thể thêm hỗ trợ
vào extension bằng cách thêm [`textobjects.scm`]. Định nghĩa của đối số và thẻ hoạt động
ở mức Tree-sitter, nhưng tìm một số mẫu nhất định trong parse tree và hiện chưa cấu hình được
theo từng ngôn ngữ.

### Nhiều con trỏ

Các lệnh này giúp bạn quản lý nhiều con trỏ trong Zode.

| Lệnh                                                                       | Phím tắt mặc định |
| -------------------------------------------------------------------------- | ----------------- |
| Thêm con trỏ chọn bản sao kế tiếp của từ hiện tại                          | `g l`             |
| Thêm con trỏ chọn bản sao trước của từ hiện tại                            | `g L`             |
| Thêm con trỏ ở cuối mỗi dòng trong vùng chọn visual hiện tại               | `g A`             |
| Thêm con trỏ ở ký tự đầu tiên của mỗi dòng trong vùng chọn visual hiện tại | `g I`             |
| Thêm vùng chọn visual cho mọi bản sao của từ hiện tại                      | `g a`             |
| Bỏ qua vùng chọn từ mới nhất, và thêm từ kế tiếp                           | `g >`             |
| Bỏ qua vùng chọn từ mới nhất, và thêm từ trước                             | `g <`             |

### Quản lý khung

Các lệnh này mở khung mới hoặc nhảy tới khung cụ thể.

| Lệnh                                             | Phím tắt mặc định  |
| ------------------------------------------------ | ------------------ |
| Mở tìm kiếm toàn dự án                           | `g /`              |
| Mở đoạn trích kết quả tìm kiếm hiện tại          | `g <space>`        |
| Mở đoạn trích tìm kiếm hiện tại trong khung tách | `<ctrl-w> <space>` |
| Đi tới định nghĩa trong khung tách               | `<ctrl-w> g d`     |
| Đi tới định nghĩa kiểu trong khung tách          | `<ctrl-w> g D`     |

### Trong insert mode

Các lệnh sau giúp bạn mở menu gợi ý của Zode, yêu cầu gợi ý từ GitHub Copilot, hoặc mở trợ lý AI trong dòng mà không rời insert mode.

| Lệnh                                                          | Phím tắt mặc định |
| ------------------------------------------------------------- | ----------------- |
| Mở menu gợi ý                                                 | `ctrl-x ctrl-o`   |
| Yêu cầu gợi ý từ GitHub Copilot (cần cấu hình GitHub Copilot) | `ctrl-x ctrl-c`   |
| Mở trợ lý AI trong dòng (cần cấu hình trợ lý)                 | `ctrl-x ctrl-a`   |
| Mở menu code action                                           | `ctrl-x ctrl-l`   |
| Ẩn mọi gợi ý                                                  | `ctrl-x ctrl-z`   |

### Plugin được hỗ trợ

Vim mode của Zode có các tính năng thường do plugin cung cấp trong hệ sinh thái Vim:

- Bạn có thể bao quanh text object bằng `ys` (yank surround), đổi phần bao quanh bằng `cs`, và xóa phần bao quanh bằng `ds`.
- Bạn có thể comment và bỏ comment vùng chọn bằng `gc` ở visual mode và `gcc` ở normal mode.
- Bảng dự án hỗ trợ nhiều phím tắt theo mẫu plugin Vim `netrw`: điều hướng bằng `hjkl`, mở file bằng `o`, mở file trong tab mới bằng `t`, v.v.
- Bạn có thể thêm phím tắt vào keymap để điều hướng tên "camelCase". [Xuống mục Phím tắt tùy chọn](#optional-key-bindings) để biết cách làm.
- Bạn có thể dùng `gR` để làm [ReplaceWithRegister](https://github.com/vim-scripts/ReplaceWithRegister).
- Bạn có thể dùng `cx` cho chức năng [vim-exchange](https://github.com/tommcdo/vim-exchange). Lưu ý nó không có phím mặc định ở visual mode, nhưng bạn có thể thêm vào keymap (xem mục [phím tắt tùy chọn](#optional-key-bindings)).
- Bạn có thể điều hướng tới độ thụt lề tương đối với con trỏ bằng plugin [indent wise](https://github.com/jeetsukumaran/vim-indentwise) `[-`, `]-`, `[+`, `]+`, `[=`, `]=`.
- Bạn có thể chọn văn bản trong ngoặc kép bằng text object AnyQuotes và văn bản trong ngoặc bằng AnyBrackets. Zode cũng cung cấp MiniQuotes và MiniBrackets với hành vi chọn thay thế dựa trên plugin Neovim [mini.ai](https://github.com/echasnovski/mini.nvim/blob/main/readmes/mini-ai.md). Xem mục [Text object cho dấu nháy và dấu ngoặc](#quote-and-bracket-text-objects) bên dưới để biết chi tiết.
- Bạn có thể cấu hình text object AnyQuotes, AnyBrackets, MiniQuotes và MiniBrackets để chọn văn bản trong dấu nháy và dấu ngoặc theo các chiến lược chọn khác nhau. Xem mục [Chức năng Any Bracket](#any-bracket-functionality) bên dưới để biết chi tiết.

### Chức năng Any Bracket

Zode có hai chiến lược khác nhau để chọn văn bản nằm trong bất kỳ dấu nháy hay dấu ngoặc nào. Các text object này **không được bật mặc định** và phải cấu hình trong keymap mới dùng được.

#### Các ký tự được bao gồm

Mỗi loại text object làm việc với các ký tự cụ thể:

| Text object              | Ký tự                                                                      |
| ------------------------ | -------------------------------------------------------------------------- |
| AnyQuotes/MiniQuotes     | Nháy đơn (`'`), Nháy kép (`"`), Dấu backtick (`` ` ``)                     |
| AnyBrackets/MiniBrackets | Ngoặc tròn (`()`), Ngoặc vuông (`[]`), Ngoặc nhọn (`{}`), Ngoặc góc (`<>`) |

Cả hai biến thể "Any" và "Mini" làm việc với cùng tập ký tự, nhưng khác chiến lược chọn.

#### AnyQuotes và AnyBrackets (hành vi Vim truyền thống)

Các text object này triển khai hành vi Vim truyền thống:

- **Ưu tiên chọn**: Tìm cặp nháy hoặc ngoặc trong cùng (gần nhất) trước
- **Cơ chế dự phòng**: Nếu không thấy, quay về dòng hiện tại
- **Khớp theo ký tự**: Chỉ nhìn ký tự mở và đóng mà không xét cú pháp
- **Giống Vim gốc**: AnyBrackets khớp hành vi của các lệnh như `ci<`, `ci(`, v.v., trong Vim gốc, kể cả các trường hợp biên có thể có (như coi `>` trong `=>` là dấu đóng)

#### MiniQuotes và MiniBrackets (hành vi mini.ai)

Các text object này triển khai hành vi của plugin Neovim [mini.ai](https://github.com/echasnovski/mini.nvim/blob/main/readmes/mini-ai.md):

- **Ưu tiên chọn**: Tìm trong dòng hiện tại trước khi mở rộng ra ngoài
- **Tích hợp Tree-sitter**: Dùng truy vấn Tree-sitter để chọn theo ngữ cảnh tốt hơn
- **Khớp theo cú pháp**: Phân biệt được dấu ngoặc thật với các ký tự giống nó ở ngữ cảnh khác (như `>` trong `=>`)

#### Chọn giữa các cách tiếp cận

- Dùng **AnyQuotes/AnyBrackets** nếu bạn:

  - Thích hành vi Vim truyền thống
  - Muốn chọn theo ký tự nhất quán, ưu tiên dấu phân cách trong cùng
  - Cần hành vi sát với text object của Vim gốc

- Dùng **MiniQuotes/MiniBrackets** nếu bạn:
  - Thích hành vi của plugin mini.ai
  - Muốn chọn theo ngữ cảnh tốt hơn bằng Tree-sitter
  - Thích ưu tiên dòng hiện tại khi tìm

#### Cấu hình ví dụ

Để dùng các text object này, bạn cần thêm binding vào keymap. Đây là cấu hình ví dụ giúp chúng dùng được khi dùng toán tử text object (`i` và `a`) hoặc đổi phần bao quanh (`cs`):

```json [keymap]
{
  "context": "vim_operator == a || vim_operator == i || vim_operator == cs",
  "bindings": {
    // Traditional Vim behavior
    "q": "vim::AnyQuotes",
    "b": "vim::AnyBrackets",

    // mini.ai plugin behavior
    "Q": "vim::MiniQuotes",
    "B": "vim::MiniBrackets"
  }
}
```

Với cấu hình này, bạn có thể dùng các lệnh như:

- `cib` - Đổi nội dung bên trong ngoặc theo hành vi AnyBrackets
- `ciB` - Đổi nội dung bên trong ngoặc theo hành vi MiniBrackets
- `ciq` - Đổi nội dung bên trong dấu nháy theo hành vi AnyQuotes
- `ciQ` - Đổi nội dung bên trong dấu nháy theo hành vi MiniQuotes

## Bảng lệnh (command palette)

Vim mode cho phép mở bảng lệnh của Zode bằng `:`. Sau đó bạn gõ để dùng bất kỳ lệnh Zode thông thường nào. Ngoài ra, vim mode thêm alias cho các lệnh Vim phổ biến để thói quen của bạn chuyển sang Zode. Ví dụ, bạn có thể viết `:w` hoặc `:write` để lưu file.

Bên dưới là các bảng liệt kê lệnh bạn có thể dùng trong bảng lệnh. Chúng tôi đặt ký tự tùy chọn trong ngoặc vuông để chỉ rằng bạn có thể bỏ chúng.

> **Lưu ý**: Chúng tôi chưa giả lập hết sức mạnh của dòng lệnh Vim. Cụ thể, các lệnh hiện không hỗ trợ đối số. Hãy [tạo issue trên GitHub](https://github.com/tgiap04/zode/issues) khi bạn thấy thiếu gì trong bảng lệnh.

### Quản lý file và cửa sổ

Bảng này liệt kê lệnh quản lý cửa sổ, tab và khung. Vì lệnh hiện không hỗ trợ đối số, bạn không thể chỉ định tên file khi lưu hoặc tạo file mới.

| Lệnh            | Mô tả                                              |
| --------------- | -------------------------------------------------- |
| `:w[rite][!]`   | Lưu file hiện tại                                  |
| `:wq[!]`        | Lưu file và đóng buffer                            |
| `:q[uit][!]`    | Đóng buffer                                        |
| `:wa[ll][!]`    | Lưu mọi file đang mở                               |
| `:wqa[ll][!]`   | Lưu mọi file đang mở và đóng mọi buffer            |
| `:qa[ll][!]`    | Đóng mọi buffer                                    |
| `:[e]x[it][!]`  | Đóng buffer                                        |
| `:up[date]`     | Lưu file hiện tại                                  |
| `:cq`           | Thoát hoàn toàn (đóng mọi instance Zode đang chạy) |
| `:bd[elete][!]` | Đóng file đang hoạt động trong mọi khung           |
| `:vs[plit]`     | Tách khung theo chiều dọc                          |
| `:sp[lit]`      | Tách khung theo chiều ngang                        |
| `:new`          | Tạo file mới trong khung tách ngang                |
| `:vne[w]`       | Tạo file mới trong khung tách dọc                  |
| `:tabedit`      | Tạo file mới trong tab mới                         |
| `:tabnew`       | Tạo file mới trong tab mới                         |
| `:tabn[ext]`    | Đi tới tab kế tiếp                                 |
| `:tabp[rev]`    | Đi tới tab trước                                   |
| `:tabc[lose]`   | Đóng tab hiện tại                                  |
| `:ls`           | Hiện mọi buffer                                    |

> **Lưu ý:** Ký tự `!` dùng để buộc lệnh chạy mà không lưu thay đổi hoặc không hỏi trước khi ghi đè file.

### Lệnh ex

Các lệnh ex này mở các bảng và cửa sổ khác nhau của Zode.

| Lệnh                | Phím tắt mặc định |
| ------------------- | ----------------- |
| Mở bảng dự án       | `:E[xplore]`      |
| Mở bảng cộng tác    | `:C[ollab]`       |
| Mở bảng git         | `:G[it]`          |
| Mở bảng debug       | `:D[ebug]`        |
| Mở bảng thông báo   | `:No[tif]`        |
| Mở cửa sổ phản hồi  | `:fe[edback]`     |
| Mở cửa sổ chẩn đoán | `:cl[ist]`        |
| Mở terminal         | `:te[rm]`         |
| Mở cửa sổ extension | `:Ext[ensions]`   |

### Điều hướng chẩn đoán

Các lệnh này điều hướng chẩn đoán.

| Lệnh                     | Mô tả                    |
| ------------------------ | ------------------------ |
| `:cn[ext]` or `:ln[ext]` | Đi tới chẩn đoán kế tiếp |
| `:cp[rev]` or `:lp[rev]` | Đi tới chẩn đoán trước   |
| `:cc` or `:ll`           | Mở trang lỗi             |

### Git

Các lệnh này tương tác với hệ thống quản lý phiên bản git.

| Lệnh            | Mô tả                                                   |
| --------------- | ------------------------------------------------------- |
| `:dif[fupdate]` | Xem diff dưới con trỏ (`d o` trong normal mode)         |
| `:rev[ert]`     | Hoàn nguyên diff dưới con trỏ (`d p` trong normal mode) |

### Nhảy

Các lệnh này nhảy tới vị trí cụ thể trong file.

| Lệnh                | Mô tả                           |
| ------------------- | ------------------------------- |
| `:<number>`         | Nhảy tới một số dòng            |
| `:$`                | Nhảy tới cuối file              |
| `:/foo` and `:?foo` | Nhảy tới dòng kế/trước khớp foo |

### Thay thế

Lệnh này thay thế văn bản. Nó giả lập lệnh substitute của vim. Lệnh substitute dùng biểu thức chính quy, và Zode dùng cú pháp hơi khác vim. Bạn có thể tìm hiểu thêm về cú pháp của Zode bên dưới, [trong mục khác biệt về regex](#regex-differences). Zode chỉ thay lần xuất hiện đầu tiên của mẫu tìm trong dòng hiện tại. Để thay mọi kết quả khớp, thêm cờ `g`.

| Lệnh                    | Mô tả                 |
| ----------------------- | --------------------- |
| `:[range]s/foo/bar/[g]` | Thay các foo bằng bar |

### Chỉnh sửa

Các lệnh này giúp bạn sửa văn bản.

| Lệnh              | Mô tả                                                          |
| ----------------- | -------------------------------------------------------------- |
| `:j[oin]`         | Nối dòng hiện tại                                              |
| `:d[elete][l][p]` | Xóa dòng hiện tại                                              |
| `:s[ort] [i]`     | Sắp xếp vùng chọn hiện tại (với i, không phân biệt hoa thường) |
| `:y[ank]`         | Yank (sao chép) vùng chọn hoặc dòng hiện tại                   |

### Set

Các lệnh này thay đổi tùy chọn trình soạn thảo cục bộ cho buffer hiện tại.

| Lệnh                            | Mô tả                                                                       |
| ------------------------------- | --------------------------------------------------------------------------- |
| `:se[t] [no]wrap`               | Dòng dài hơn chiều rộng cửa sổ sẽ xuống dòng và tiếp tục hiển thị ở dòng kế |
| `:se[t] [no]nu[mber]`           | In số dòng ở đầu mỗi dòng                                                   |
| `:se[t] [no]r[elative]nu[mber]` | Đổi số hiển thị thành tương đối so với con trỏ                              |
| `:se[t] [no]i[gnore]c[ase]`     | Điều khiển việc tìm trong buffer và dự án có phân biệt hoa thường hay không |

### Từ gợi nhớ cho lệnh

Mặc định Zode không kèm từ gợi nhớ nào cho lệnh, nhưng bạn có thể định nghĩa alias ngắn cho lệnh Zode bằng setting `command_aliases` trong file cài đặt. Khi bạn gõ một alias từ map này trong bảng lệnh, nó được chuyển thành lệnh tương ứng.

#### Cấu hình ví dụ

Để cấu hình từ gợi nhớ cho lệnh, thêm khóa `command_aliases` vào file cài đặt. Đây là cấu hình ví dụ với các từ gợi nhớ hữu ích:

```json [settings]
{
  "command_aliases": {
    "zlog": "zode::OpenLog",
    "newf": "workspace::NewFile",
    "diffs": "editor::ToggleSelectedDiffHunks",
    "crp": "workspace::CopyRelativePath",
    "cpp": "workspace::CopyPath",
    "reveal": "editor::RevealInFileManager",
    "clank": "editor::CancelLanguageServerWork"
  }
}
```

Với cấu hình này, bạn có thể dùng các lệnh như:

- `:zlog` - Mở log của Zode
- `:newf` - Tạo file mới
- `:diffs` - Bật/tắt các diff hunk đang chọn
- `:crp` - Sao chép đường dẫn tương đối của file hiện tại
- `:cpp` - Sao chép đường dẫn đầy đủ của file hiện tại
- `:reveal` - Hiện file hiện tại trong trình quản lý file
- `:clank` - Hủy công việc của language server

## Tùy chỉnh phím tắt

### Chọn đúng context

Phím tắt của Zode chỉ được đánh giá khi thuộc tính `"context"` khớp với vị trí của bạn trong trình soạn thảo. Ví dụ, nếu bạn thêm phím tắt vào context `"Editor"`, chúng chỉ hoạt động khi bạn đang sửa file. Nếu thêm vào context `"Workspace"`, chúng hoạt động ở mọi nơi trong Zode. Đây là ví dụ về phím tắt lưu khi bạn đang sửa file:

```json [keymap]
{
  "context": "Editor",
  "bindings": {
    "ctrl-s": "workspace::Save"
  }
}
```

Context lồng nhau, nên khi bạn đang sửa file, context là context `"Editor"`, nằm trong context `"Pane"`, nằm trong context `"Workspace"`. Vì vậy mọi phím tắt bạn thêm vào context `"Workspace"` đều hoạt động khi bạn đang sửa file. Ví dụ:

```json [keymap]
// This key binding will work when you're editing a file. It comes built into Zode by default as the workspace: save command.
{
  "context": "Workspace",
  "bindings": {
    "ctrl-s": "workspace::Save"
  }
}
```

Context là biểu thức. Chúng hỗ trợ toán tử boolean như `&&` (và) và `||` (hoặc). Ví dụ, bạn có thể dùng context `"Editor && vim_mode == normal"` để tạo phím tắt chỉ hoạt động khi bạn đang sửa file _và_ đang ở normal mode của vim.

Vim mode thêm vài context vào context `"Editor"`:

| Toán tử              | Mô tả                                                                                                                                                                          |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| VimControl           | Cho biết phím tắt vim nên hoạt động. Hiện là alias của `vim_mode == normal \|\| vim_mode == visual \|\| vim_mode == operator`, nhưng định nghĩa có thể thay đổi theo thời gian |
| vim_mode == normal   | Normal mode                                                                                                                                                                    |
| vim_mode == visual   | Visual mode                                                                                                                                                                    |
| vim_mode == insert   | Insert mode                                                                                                                                                                    |
| vim_mode == replace  | Replace mode                                                                                                                                                                   |
| vim_mode == waiting  | Đang chờ một phím bất kỳ (ví dụ sau khi gõ `f` hoặc `t`)                                                                                                                       |
| vim_mode == operator | Đang chờ một binding khác kích hoạt (ví dụ sau khi gõ `c` hoặc `d`)                                                                                                            |
| vim_operator         | Đặt là `none` trừ khi `vim_mode == operator`, khi đó nó đặt là phím mặc định của toán tử hiện tại (ví dụ sau khi gõ `d`, `vim_operator == d`)                                  |

> **Lưu ý**: Context chỉ được khớp ở một cấp tại một thời điểm. Nên dùng được biểu thức `"Editor && vim_mode == normal"`, nhưng `"Workspace && vim_mode == normal"` sẽ không bao giờ khớp vì ta đặt context vim ở cấp `"Editor"`.

### Context hữu ích cho phím tắt vim mode

Đây là mẫu với các context vim mode hữu ích để giúp bạn tùy chỉnh phím tắt vim mode. Bạn có thể sao chép và đưa vào keymap người dùng.

```json [keymap]
[
  {
    "context": "VimControl && !menu",
    "bindings": {
      // Put key bindings here if you want them to work in normal & visual mode.
    }
  },
  {
    "context": "vim_mode == normal && !menu",
    "bindings": {
      // "shift-y": ["workspace::SendKeystrokes", "y $"] // Use neovim's yank behavior: yank to end of line.
    }
  },
  {
    "context": "vim_mode == insert",
    "bindings": {
      // "j k": "vim::NormalBefore" // In insert mode, make jk escape to normal mode.
    }
  },
  {
    "context": "EmptyPane || SharedScreen",
    "bindings": {
      // Put key bindings here (in addition to the context above) if you want them to
      // work when no editor exists.
      // "space f": "file_finder::Toggle"
    }
  }
]
```

> **Lưu ý**: Nếu bạn muốn giả lập các lệnh `map` của Vim (`nmap`, v.v.), bạn có thể dùng action `workspace::SendKeystrokes` trong đúng context.

### Phím tắt tùy chọn

Mặc định, bạn có thể di chuyển giữa các file đang mở trong trình soạn thảo bằng phím tắt như `ctrl+w` rồi một trong `hjkl` để sang trái, xuống, lên hoặc phải.

Nhưng bạn không dùng được cùng phím tắt đó để di chuyển giữa mọi dock của trình soạn thảo (terminal, bảng dự án, bảng agent, ...). Nếu muốn dùng cùng phím tắt để tới các dock, bạn có thể thêm các phím tắt sau vào keymap người dùng.

```json [keymap]
{
  "context": "Dock",
  "bindings": {
    "ctrl-w h": "workspace::ActivatePaneLeft",
    "ctrl-w l": "workspace::ActivatePaneRight",
    "ctrl-w k": "workspace::ActivatePaneUp",
    "ctrl-w j": "workspace::ActivatePaneDown"
    // ... or other keybindings
  }
}
```

Subword motion, cho phép di chuyển và chọn từng từ riêng lẻ trong `camelCase` hoặc `snake_case`, không bật mặc định. Để bật, thêm các binding này vào keymap.

```json [keymap]
{
  "context": "VimControl && !menu && vim_mode != operator",
  "bindings": {
    "w": "vim::NextSubwordStart",
    "b": "vim::PreviousSubwordStart",
    "e": "vim::NextSubwordEnd",
    "g e": "vim::PreviousSubwordEnd"
  }
}
```

> Lưu ý: Các thao tác như `dw` không bị ảnh hưởng. Nếu muốn các thao tác
> cũng dùng subword motion, hãy bỏ `vim_mode != operator` khỏi `context`.

Vim mode có sẵn phím tắt để bao quanh vùng chọn ở normal mode (`ys`), nhưng không có phím tắt để thêm phần bao quanh ở visual mode. Mặc định, `shift-s` thay thế vùng chọn (xóa văn bản và vào insert mode). Để dùng `shift-s` thêm phần bao quanh ở visual mode, bạn có thể thêm đối tượng sau vào keymap.

```json [keymap]
{
  "context": "vim_mode == visual",
  "bindings": {
    "shift-s": "vim::PushAddSurrounds"
  }
}
```

Ở các trình soạn thảo không modal, việc di chuyển con trỏ thường xuống dòng khi vượt quá cuối dòng. Tuy nhiên Zode mặc định xử lý hành vi này y hệt Vim: con trỏ dừng ở ranh giới dòng. Nếu muốn con trỏ nhảy giữa các dòng, hãy ghi đè các phím tắt sau:

```json [keymap]
// In VimScript, this would look like this:
// set whichwrap+=<,>,[,],h,l
{
  "context": "VimControl && !menu",
  "bindings": {
    "left": "vim::WrappingLeft",
    "right": "vim::WrappingRight",
    "h": "vim::WrappingLeft",
    "l": "vim::WrappingRight"
  }
}
```

Tính năng [Sneak motion](https://github.com/justinmk/vim-sneak) cho phép nhảy nhanh tới bất kỳ chuỗi hai ký tự nào trong văn bản. Bạn có thể bật bằng cách thêm các phím tắt sau vào keymap. Mặc định, phím `s` gán cho `vim::Substitute`. Thêm các binding này sẽ ghi đè hành vi đó, nên hãy chắc thay đổi này hợp với thói quen làm việc của bạn.

```json [keymap]
{
  "context": "vim_mode == normal || vim_mode == visual",
  "bindings": {
    "s": "vim::PushSneak",
    "shift-s": "vim::PushSneakBackward"
  }
}
```

Tính năng [vim-exchange](https://github.com/tommcdo/vim-exchange) không có binding mặc định cho visual mode, vì binding `shift-x` xung đột với binding `shift-x` mặc định cho visual mode (`vim::VisualDeleteLine`). Để gán binding vim-exchange mặc định, thêm phím tắt sau vào keymap:

```json [keymap]
{
  "context": "vim_mode == visual",
  "bindings": {
    "shift-x": "vim::Exchange"
  }
}
```

### Khôi phục phím tắt soạn thảo thông dụng và phím tắt Zode

Nếu dùng vim mode trên Linux hoặc Windows, bạn có thể thấy nó ghi đè các phím tắt bạn không thể thiếu: `ctrl+v` để dán, `ctrl+f` để tìm, v.v. Bạn có thể khôi phục chúng bằng cách sao chép dữ liệu này vào keymap:

```json [keymap]
{
  "context": "Editor && !menu",
  "bindings": {
    "ctrl-f": "buffer_search::Deploy",      // vim default: page down
    "ctrl-c": "editor::Copy",               // vim default: return to normal mode
    "ctrl-x": "editor::Cut",                // vim default: decrement
    "ctrl-v": "editor::Paste",              // vim default: visual block mode
    "ctrl-a": "editor::SelectAll",          // vim default: increment
    "ctrl-y": "editor::Undo",               // vim default: line up
    "ctrl-t": "project_symbols::Toggle",    // vim default: go to older tag
    "ctrl-o": "workspace::Open",            // vim default: go back
    "ctrl-s": "workspace::Save",            // vim default: show signature
    "ctrl-b": "workspace::ToggleLeftDock"   // vim default: down
  }
},
```

## Đổi cài đặt vim mode

Bạn có thể đổi các cài đặt sau để chỉnh hành vi của vim mode:

| Thuộc tính                   | Mô tả                                                                                                                                                                             | Giá trị mặc định |
| ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| default_mode                 | Chế độ khởi động mặc định. Một trong "normal", "insert", "replace", "visual", "visual_line", "visual_block", "helix_normal".                                                      | "normal"         |
| use_system_clipboard         | Quyết định cách dùng clipboard hệ thống:<br><ul><li>"always": dùng cho mọi thao tác</li><li>"never": chỉ dùng khi chỉ định rõ</li><li>"on_yank": dùng cho thao tác yank</li></ul> | "always"         |
| use_multiline_find           | không còn dùng                                                                                                                                                                    |
| use_smartcase_find           | Nếu `true`, motion `f` và `t` không phân biệt hoa thường khi chữ đích là chữ thường.                                                                                              | false            |
| use_regex_search             | Nếu `true`, tìm kiếm vim sẽ dùng chế độ regex                                                                                                                                     | true             |
| gdefault                     | Nếu `true`, lệnh `:substitute` mặc định thay mọi kết quả khớp trong một dòng (như thể có cờ `g`). Khi đó cờ `g` đảo ngược điều này, chỉ thay kết quả khớp đầu tiên.               | false            |
| toggle_relative_line_numbers | Nếu `true`, số dòng là tương đối ở normal mode và tuyệt đối ở insert mode, cho bạn điều tốt nhất của cả hai.                                                                      | false            |
| custom_digraphs              | Đối tượng cho phép thêm digraph tùy chỉnh. Xem ví dụ bên dưới.                                                                                                                    | {}               |
| highlight_on_yank_duration   | Thời lượng của hiệu ứng làm nổi bật (tính bằng ms). Đặt `0` để tắt                                                                                                                | 200              |

Đây là ví dụ thêm digraph cho emoji zombie. Nhờ đó bạn gõ `ctrl-k f z` để chèn emoji zombie. Bạn có thể thêm bao nhiêu digraph tùy ý.

```json [settings]
{
  "vim": {
    "custom_digraphs": {
      "fz": "🧟‍♀️"
    }
  }
}
```

Đây là ví dụ các cài đặt này sau khi đổi:

```json [settings]
{
  "vim": {
    "default_mode": "insert",
    "use_system_clipboard": "never",
    "use_smartcase_find": true,
    "use_regex_search": true,
    "gdefault": true,
    "toggle_relative_line_numbers": true,
    "highlight_on_yank_duration": 50,
    "custom_digraphs": {
      "fz": "🧟‍♀️"
    }
  }
}
```

## Các cài đặt lõi Zode hữu ích cho vim mode

Đây là vài cài đặt Zode chung có thể giúp bạn tinh chỉnh trải nghiệm Vim:

| Thuộc tính              | Mô tả                                                                                                                             | Giá trị mặc định     |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| cursor_blink            | Nếu `true`, con trỏ nhấp nháy.                                                                                                    | `true`               |
| relative_line_numbers   | Nếu `"enabled"`, số dòng ở gutter bên trái tương đối so với con trỏ. Nếu `"wrapped"`, chúng cũng hiện cho các dòng bị xuống dòng. | `"disabled"`         |
| scrollbar               | Đối tượng điều khiển hiển thị thanh cuộn. Đặt `{ "show": "never" }` để ẩn thanh cuộn.                                             | `{ "show": "auto" }` |
| scroll_beyond_last_line | Nếu đặt `"one_page"`, cho phép cuộn quá dòng cuối tới một trang. Đặt `"off"` để ngăn hành vi này.                                 | `"one_page"`         |
| vertical_scroll_margin  | Số dòng giữ phía trên hoặc dưới con trỏ khi cuộn. Đặt `0` để con trỏ đi tới sát mép màn hình theo chiều dọc.                      | `3`                  |
| gutter.line_numbers     | Điều khiển hiển thị số dòng ở gutter. Đặt thuộc tính `"line_numbers"` thành `false` để ẩn số dòng.                                | `true`               |
| command_aliases         | Đối tượng định nghĩa alias cho lệnh trong bảng lệnh. Bạn có thể dùng để đặt tên tắt cho các lệnh hay dùng. Xem ví dụ bên dưới.    | `{}`                 |

Đây là ví dụ các cài đặt này sau khi đổi:

```json [settings]
{
  // Disable cursor blink
  "cursor_blink": false,
  // Use relative line numbers
  "relative_line_numbers": "enabled",
  // Hide the scroll bar
  "scrollbar": { "show": "never" },
  // Prevent the buffer from scrolling beyond the last line
  "scroll_beyond_last_line": "off",
  // Allow the cursor to reach the edges of the screen
  "vertical_scroll_margin": 0,
  "gutter": {
    // Disable line numbers completely
    "line_numbers": false
  },
  "command_aliases": {
    "W": "w",
    "Wq": "wq",
    "Q": "q"
  }
}
```

Thuộc tính `command_aliases` là một đối tượng duy nhất ánh xạ phím hoặc chuỗi phím tới lệnh vim mode. Ví dụ trên định nghĩa nhiều alias: `W` cho `w`, `Wq` cho `wq`, và `Q` cho `q`.

## Khác biệt về Regex

Zode dùng engine biểu thức chính quy khác Vim. Nghĩa là bạn sẽ phải dùng cú pháp khác trong một số trường hợp. Đây là những khác biệt phổ biến nhất:

- **Nhóm capture**: Vim dùng `\(` và `\)` để biểu diễn nhóm capture, trong Zode là `(` và `)`. Ngược lại, trong Vim `(` và `)` biểu diễn dấu ngoặc nguyên văn, nhưng trong Zode phải escape thành `\(` và `\)`.
- **Kết quả khớp**: Khi thay thế, Vim dùng ký tự backslash theo sau là một số để biểu diễn nhóm capture đã khớp. Ví dụ `\1`. Zode dùng dấu đô la. Nên khi trong Vim bạn dùng `\0` cho toàn bộ kết quả khớp, trong Zode cú pháp là `$0`. Nhóm capture đánh số cũng vậy: `\1` trong Vim là `$1` trong Zode.
- **Tùy chọn global**: Mặc định trong Vim, tìm regex chỉ khớp lần xuất hiện đầu tiên trên một dòng, và bạn thêm `/g` ở cuối truy vấn để tìm mọi kết quả. Trong Zode, tìm regex mặc định là global.
- **Phân biệt hoa thường**: Vim dùng `/i` để chỉ tìm không phân biệt hoa thường. Trong Zode bạn có thể viết `(?i)` ở đầu mẫu hoặc bật/tắt phân biệt hoa thường bằng phím tắt {#kb search::ToggleCaseSensitive}.

> **Lưu ý**: Để hỗ trợ quá trình chuyển đổi, bảng lệnh sẽ sửa dấu ngoặc và nhóm thay thế giúp bạn khi bạn viết lệnh substitute kiểu Vim, `:%s//`. Nên Zode sẽ chuyển `%s:/\(a\)(b)/\1/` thành tìm kiếm "(a)\(b\)" và thay thế bằng "$1".

Để xem đầy đủ cú pháp được engine regex của Zode hỗ trợ, [xem tài liệu của regex crate](https://docs.rs/regex/latest/regex/#syntax).
