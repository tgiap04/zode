---
title: Developing Extensions
description: "Create Zode extensions: languages, themes, debuggers, and more."
---

# Phát triển extension {#developing-extensions}

Extension của Zode là các repository Git chứa file manifest `extension.toml`. Chúng có thể cung cấp ngôn ngữ, giao diện (theme), trình gỡ lỗi (debugger), snippet và MCP server.

## Tính năng của extension {#extension-features}

Extension có thể cung cấp:

- [Ngôn ngữ](./languages.md)
- [Trình gỡ lỗi](./debugger-extensions.md)
- [Giao diện](./themes.md)
- [Giao diện biểu tượng](./icon-themes.md)
- [Snippet](./snippets.md)

## Phát triển extension ở máy cục bộ

Trước khi bắt đầu phát triển extension cho Zode, hãy [cài Rust qua rustup](https://www.rust-lang.org/tools/install).

> Rust phải được cài qua rustup. Nếu bạn cài Rust bằng homebrew hay cách khác, việc cài dev extension sẽ không hoạt động.

Khi phát triển extension, bạn có thể dùng nó trong Zode mà không cần xuất bản, bằng cách cài như một _dev extension_.

Từ trang extension, bấm nút `Install Dev Extension` (hoặc action {#action zode::InstallDevExtension}) rồi chọn thư mục chứa extension của bạn.

Nếu cần xử lý sự cố, xem thêm đầu ra trong Zode.log ({#action zode::OpenLog}). Để có đầu ra debug, hãy đóng rồi mở lại Zode từ dòng lệnh bằng `zode --foreground`, lệnh này hiện log mức INFO chi tiết hơn.

Nếu bạn đã cài bản đã xuất bản của extension, bản đó sẽ bị gỡ trước khi cài dev extension. Sau khi cài thành công, trang `Extensions` sẽ ghi extension gốc là "Overridden by dev extension".

## Cấu trúc thư mục của một extension Zode

Extension Zode là một repository Git chứa file `extension.toml`. File này phải có một số
thông tin cơ bản về extension:

```toml
id = "my-extension"
name = "My extension"
version = "0.0.1"
schema_version = 1
authors = ["Your Name <you@example.com>"]
description = "Example extension"
repository = "https://github.com/your-name/my-zed-extension"
```

Ngoài ra còn nhiều file và thư mục tùy chọn khác để thêm chức năng cho extension Zode. Ví dụ cấu trúc thư mục của một extension cung cấp đủ mọi khả năng như sau:

```
my-extension/
  extension.toml
  Cargo.toml
  src/
    lib.rs
  languages/
    my-language/
      config.toml
      highlights.scm
  themes/
    my-theme.json
  snippets/
    snippets.json
    rust.json
```

## WebAssembly

Phần xử lý theo thủ tục của extension được viết bằng Rust và biên dịch sang WebAssembly. Để phát triển extension có mã tùy chỉnh, hãy thêm một `Cargo.toml` như sau:

```toml
[package]
name = "my-extension"
version = "0.0.1"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
zed_extension_api = "0.1.0"
```

Hãy dùng phiên bản mới nhất của [`zed_extension_api`](https://crates.io/crates/zed_extension_api) trên crates.io. Đảm bảo nó vẫn [tương thích với các phiên bản Zode](https://github.com/tgiap04/zode/blob/develop/crates/extension_api#compatible-zed-versions) bạn muốn hỗ trợ.

Trong file `src/lib.rs` của crate Rust, bạn cần định nghĩa một struct cho extension và triển khai trait `Extension`, đồng thời dùng macro `register_extension!` để đăng ký extension:

```rs
use zed_extension_api as zed;

struct MyExtension {
    // ... state
}

impl zode::Extension for MyExtension {
    // ...
}

zed::register_extension!(MyExtension);
```

> `stdout`/`stderr` được chuyển thẳng tới tiến trình Zode. Để xem đầu ra `println!`/`dbg!` từ extension, bạn có thể chạy Zode trong terminal với cờ `--foreground`.

## Fork và clone repo

1. Fork repo

> **Lưu ý:** Nên fork repo `zed-industries/extensions` về tài khoản GitHub cá nhân thay vì tổ chức GitHub, vì như vậy nhân viên Zed có thể đẩy các thay đổi cần thiết vào PR của bạn để đẩy nhanh việc xuất bản.

2. Clone repo về máy cục bộ

```sh
# Substitute the url of your fork here:
# git clone https://github.com/zed-industries/extensions
cd extensions
git submodule init
git submodule update
```

## Yêu cầu giấy phép của extension

Từ ngày 1 tháng 10 năm 2025, repository extension phải có giấy phép.
Các giấy phép được chấp nhận:

- [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [BSD 2-Clause](https://opensource.org/license/bsd-2-clause)
- [BSD 3-Clause](https://opensource.org/license/bsd-3-clause)
- [CC BY 4.0](https://creativecommons.org/licenses/by/4.0)
- [GNU GPLv3](https://www.gnu.org/licenses/gpl-3.0.en.html)
- [GNU LGPLv3](https://www.gnu.org/licenses/lgpl-3.0.en.html)
- [MIT](https://opensource.org/license/mit)
- [Unlicense](https://unlicense.org)
- [zlib](https://opensource.org/license/zlib)

Điều này cho phép chúng tôi phân phối bản nhị phân tạo từ mã extension của bạn tới người dùng.
Nếu không có giấy phép hợp lệ, pull request thêm hoặc cập nhật extension ở các bước sau sẽ không qua CI.

File giấy phép nên nằm ở thư mục gốc của repository extension. Mọi tên file có tiền tố `LICENCE` hoặc `LICENSE` (không phân biệt hoa thường) đều được kiểm tra xem có khớp một trong các giấy phép được chấp nhận không. Xem [mã nguồn kiểm tra giấy phép](https://github.com/zed-industries/extensions/blob/main/src/lib/license.js).

> Yêu cầu giấy phép này chỉ áp dụng cho chính mã extension của bạn (phần mã được biên dịch thành binary của extension).
> Nó không áp dụng cho các công cụ mà extension có thể tải về hay tương tác, như language server hoặc các thành phần phụ thuộc bên ngoài khác.
> Nếu repository của bạn chứa cả mã extension lẫn các dự án khác (như language server), bạn không bắt buộc đổi giấy phép cho các dự án đó — chỉ mã extension cần thuộc một trong các giấy phép được chấp nhận ở trên.

## Điều kiện trước khi xuất bản extension

Trước khi xuất bản, hãy chắc chắn bạn đã chọn một ID extension duy nhất trong [manifest của extension](#directory-structure-of-a-zed-extension).
Đây sẽ là định danh chính của extension và không thể đổi sau khi đã xuất bản.
Đồng thời, hãy điền đủ mọi trường bắt buộc trong manifest.

Ngoài ra, hãy đảm bảo extension đáp ứng các điều kiện sau trước khi chuyển sang bước xuất bản:

- ID và tên extension không được chứa các từ `zed`, `Zed` hay `extension`, vì tất cả đều là extension của Zode.
- ID extension nên cho biết extension định làm gì. Ví dụ với giao diện thì thêm hậu tố `-theme`, extension snippet thêm hậu tố `-snippets`, v.v. Ngoại lệ là extension hỗ trợ ngôn ngữ hoặc công cụ phổ biến mà người ta mong tìm thấy dưới đúng ID đó. Bạn có thể xem danh sách [extension hiện có](https://github.com/zed-industries/extensions/blob/main/extensions.toml) để hình dung quy tắc này thường được áp dụng ra sao.
- Extension nên cung cấp thứ chưa có trên marketplace, thay vì sửa một lỗi có thể giải quyết ngay trong extension hiện có. Ví dụ, nếu thấy extension hiện có hỗ trợ language server chưa chạy đúng, hãy thử đóng góp bản sửa cho extension đó trước, thay vì gửi ngay extension mới.
  - Nếu bạn không nhận được phản hồi nào từ repository gốc trong một khoảng thời gian hợp lý, cứ gửi pull request để sửa vấn đề đó. Hãy nêu những nỗ lực trước đó của bạn trong pull request gửi tới repository extensions để thêm extension. Người duy trì Zode sẽ quyết định cách xử lý theo từng trường hợp.
- Extension định cung cấp ngôn ngữ, trình gỡ lỗi hoặc MCP server không được đóng gói sẵn language server. Thay vào đó, extension nên tải language server về hoặc kiểm tra xem language server có sẵn trong môi trường của người dùng không, bằng các API của [Zode Rust Extension API](https://docs.rs/zed_extension_api/latest/zed_extension_api/).
- Giao diện và giao diện biểu tượng không nên xuất bản chung với extension cung cấp tính năng khác, ví dụ hỗ trợ ngôn ngữ. Thay vào đó, hãy xuất bản thành extension riêng. Điều này cũng áp dụng khi giao diện và giao diện biểu tượng nằm cùng một repository.

Lưu ý: người duyệt sẽ nêu các điểm không tuân thủ trong quá trình xuất bản và việc này làm chậm việc phát hành extension của bạn.

## Xuất bản extension

Để xuất bản extension, hãy mở PR tới [repo `zed-industries/extensions`](https://github.com/zed-industries/extensions).

Trong PR, hãy làm như sau:

1. Thêm extension như một Git submodule trong thư mục `extensions/` theo đường dẫn `extensions/{extension-id}`

```sh
git submodule add https://github.com/your-username/foobar-zed.git extensions/my-extension
git add extensions/my-extension
```

> Mọi submodule extension phải dùng URL HTTPS, không dùng URL SSH (`git@github.com`).

2. Thêm một mục mới vào file `extensions.toml` cấp cao nhất, chứa extension của bạn:

```toml
[my-extension]
submodule = "extensions/my-extension"
version = "0.0.1"
```

Nếu extension nằm trong thư mục con của submodule, bạn có thể dùng trường `path` để trỏ tới nơi extension nằm:

```toml
[my-extension]
submodule = "extensions-my-extension"
path = "packages/zed"
version = "0.0.1"
```

> Lưu ý [giấy phép extension bắt buộc](#extension-license-requirements) phải nằm ở đường dẫn đã chỉ định; giấy phép ở thư mục gốc repository sẽ không có tác dụng. Tuy nhiên, bạn có thể tạo symlink tới giấy phép có sẵn trong repository hoặc chọn giấy phép khác trong danh sách được chấp nhận cho mã extension.

3. Chạy `pnpm sort-extensions` để đảm bảo `extensions.toml` và `.gitmodules` được sắp xếp

Khi PR được merge, extension sẽ được đóng gói và xuất bản lên registry extension của Zode.

## Cập nhật extension

Để cập nhật extension, hãy mở PR tới [repo `zed-industries/extensions`](https://github.com/zed-industries/extensions).

Trong PR, hãy làm như sau:

1. Cập nhật submodule của extension lên commit của phiên bản mới. Bạn có thể chạy

```sh
# From the root of the repository:
git submodule update --remote extensions/your-extension-name
```

để cập nhật extension lên commit mới nhất có trong repository từ xa của bạn.

2. Cập nhật trường `version` của extension trong `extensions.toml`
   - Đảm bảo `version` khớp với giá trị đặt trong `extension.toml` tại commit đó.

Nếu muốn tự động hóa quy trình này, có một [GitHub Action của cộng đồng](https://github.com/huacnlee/zed-extension-action) bạn có thể dùng.

> **Lưu ý:** Nếu repository extension của bạn có giấy phép khác, bạn cần đổi sang một trong các [giấy phép extension được chấp nhận](#extension-license-requirements) trước khi xuất bản bản cập nhật.
