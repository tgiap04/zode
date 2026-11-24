---
title: Using a debugger
description: "Guide to using a debugger for Zode development."
---

# Dùng trình gỡ lỗi (debugger)

> Trang này không nói về [cấu hình trình gỡ lỗi của Zode](../debugger.md).
> Nó nói về cách dùng trình gỡ lỗi khi phát triển chính Zode.

## Dùng trình gỡ lỗi tích hợp sẵn của Zode

Khi dự án Zode đang mở, bạn mở `New Process Modal` và chọn tab `Debug`. Ở đó có hai cấu hình gỡ lỗi để debug Zode, một cho GDB và một cho LLDB. Chọn cấu hình bạn muốn và Zode sẽ build rồi chạy binary.

GDB không được hỗ trợ trên máy Mac Apple Silicon.

## Lưu ý về profile build release

Mặc định, các bản build dùng profile release (profile dùng cho nightly, preview và stable) chỉ chứa thông tin debug hạn chế.

Điều này đạt được bằng cách đặt trường `profile.(release).debug` trong `Cargo.toml` gốc thành `"limited"`.

Tài liệu chính thức về trường `debug` nằm [ở đây](https://doc.rust-lang.org/cargo/reference/profiles.html#debug).
Tóm lại, `"limited"` lược bỏ thông tin debug ở mức kiểu và mức biến.

Ở bản release, điều này giảm kích thước binary. Thông tin debug mức kiểu và mức biến không cần để có stack trace hữu ích.

Tuy nhiên, dữ liệu này quan trọng khi bạn đang debug thật sự. Thiếu nó, trình gỡ lỗi không phân giải được biến cục bộ, không xem được giá trị, và không định dạng được output bằng pretty-printer.

Để có trải nghiệm debug đầy đủ trên bản release, hãy biên dịch binary Zode với đầy đủ thông tin debug.

Cách đơn giản nhất là dùng `--config` để ghi đè trường `debug` trong `Cargo.toml` gốc khi chạy `cargo run` hoặc `cargo build`:

```sh
cargo run --config 'profile.release.debug="full"'
cargo build --config 'profile.release.debug="full"'
```

> Nếu không muốn truyền `--config` ở mọi lệnh `cargo`, bạn cũng có thể sửa mục trong [`Cargo.toml` gốc](https://github.com/tgiap04/zode/blob/develop/Cargo.toml)
>
> từ
>
> ```toml
> [profile.release]
> debug = "limited"
> ```
>
> thành
>
> ```toml
> [profile.release]
> debug = "full"
> ```
>
> Khi đó mọi lần gọi `cargo run --release` hoặc `cargo build --release` đều biên dịch với đầy đủ thông tin debug.
>
> **Cảnh báo:** Đừng commit những thay đổi này.

## Chạy Zode với shell debugger GDB/LLDB

### Bối cảnh

Khi bạn cài Rust qua rustup (cách khuyến nghị để phát triển Zode; xem hướng dẫn cho nền tảng của bạn [ở đây](../development.md)), rustup cũng cài các script hỗ trợ debug binary Rust.

Các script này là `rust-gdb` và `rust-lldb`.

Bạn có thể đọc thêm về các script này [ở đây](https://michaelwoerister.github.io/2015/03/27/rust-xxdb.html).

Chúng là script bọc quanh `gdb` và `lldb`, chèn thêm lệnh và cờ cho các tính năng riêng của Rust như pretty-printer và thông tin kiểu.

Để dùng `rust-gdb` hoặc `rust-lldb`, hãy cài `gdb` hoặc `lldb` trên hệ thống.

[Bài viết được liên kết](https://michaelwoerister.github.io/2015/03/27/rust-xxdb.html) ghi rằng phiên bản tối thiểu được hỗ trợ là GDB 7.7 và LLDB 310. Thực tế, bản mới hơn thường tốt hơn.

> **Lưu ý**: `rust-gdb` không được cài mặc định trên Windows vì hỗ trợ `gdb` ở đó chưa ổn định. Hãy dùng `rust-lldb`.

Nếu bạn mới làm quen với các công cụ này, xem tài liệu `gdb` [ở đây](https://www.gnu.org/software/gdb/) và tài liệu `lldb` [ở đây](https://lldb.llvm.org/).

### Dùng với Zode

Sau khi bật đầy đủ thông tin debug và build bằng `cargo build`, chạy `rust-gdb` hoặc `rust-lldb` với binary Zode đã biên dịch:

```
rust-gdb target/debug/zode
rust-lldb target/debug/zode
```

Bạn cũng có thể attach vào một tiến trình Zode đang chạy (ví dụ tiến trình khởi động bằng `cargo run`):

```
rust-gdb -p <pid>
rust-lldb -p <pid>
```

`<pid>` là ID tiến trình của bản Zode bạn muốn attach.

Để tìm PID, dùng công cụ quản lý tiến trình của hệ thống, như Task Manager trên Windows hoặc Activity Monitor trên macOS.

Bạn cũng có thể chạy `ps aux | grep -i zode` trên macOS và Linux, hoặc `Get-Process | Select-Object Id, ProcessName` trong PowerShell trên Windows.

#### Debug panic và crash

Trình gỡ lỗi hữu ích để tìm nguyên nhân panic và crash, kể cả trong Zode.

Mặc định, khi một tiến trình gắn với `gdb` hoặc `lldb` gặp một exception như panic, trình gỡ lỗi dừng tại đó để bạn xem trạng thái chương trình.

Điểm dừng đầu tiên thường nằm trong mã xử lý panic hoặc exception của thư viện chuẩn Rust, nên bạn thường phải đi ngược lên stack để tìm nguyên nhân gốc.

Trong `lldb`, dùng `backtrace` với `frame select`. `gdb` có các lệnh tương đương.

Sau khi chương trình dừng ở exception, bạn thường không chạy tiếp bình thường được. Bạn vẫn di chuyển giữa các stack frame và xem biến, biểu thức, thường đủ để xác định nguyên nhân crash.

Bạn có thể tìm thêm thông tin về debug crash của Zode [ở đây](./debugging-crashes.md).
