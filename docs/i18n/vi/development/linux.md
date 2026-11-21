---
title: Building Zode for Linux
description: "Guide to building Zode for linux for Zode development."
---

# Build Zode cho Linux

## Repository

Clone [repository Zode](https://github.com/tgiap04/zode).

## Dependency

- Cài [rustup](https://www.rust-lang.org/tools/install)

- Cài các thư viện hệ thống cần thiết:

  ```sh
  script/linux
  ```

  Nếu muốn cài thủ công các thư viện hệ thống, bạn xem danh sách package cần thiết trong file `script/linux`.

## Build từ mã nguồn

Khi đã cài xong dependency, bạn build Zode bằng [Cargo](https://doc.rust-lang.org/cargo/).

Để build debug cho trình soạn thảo:

```sh
cargo run
```

Và để chạy test:

```sh
cargo test --workspace
```

Ở chế độ release, giao diện chính là crate `cli`. Bạn có thể chạy nó khi phát triển bằng:

```sh
cargo run -p cli
```

## Cài bản build phát triển

Bạn có thể cài bản build cục bộ lên máy bằng:

```sh
./script/install-linux
```

Lệnh này build package `zode` và `cli` ở chế độ release, cài CLI vào `~/.local/bin/zode`, và cài các file `.desktop` vào `~/.local/share`.

> **_Lưu ý_**: Nếu gặp lỗi linker tương tự như sau:
>
> ```bash
> error: linking with `cc` failed: exit status: 1 ...
> = note: /usr/bin/ld: /tmp/rustcISMaod/libaws_lc_sys-79f08eb6d32e546e.rlib(f8e4fd781484bd36-bcm.o): in function `aws_lc_0_25_0_handle_cpu_env':
>           /aws-lc/crypto/fipsmodule/cpucap/cpu_intel.c:(.text.aws_lc_0_25_0_handle_cpu_env+0x63): undefined reference to `__isoc23_sscanf'
>           /usr/bin/ld: /tmp/rustcISMaod/libaws_lc_sys-79f08eb6d32e546e.rlib(f8e4fd781484bd36-bcm.o): in function `pkey_rsa_ctrl_str':
>           /aws-lc/crypto/fipsmodule/evp/p_rsa.c:741:(.text.pkey_rsa_ctrl_str+0x20d): undefined reference to `__isoc23_strtol'
>           /usr/bin/ld: /aws-lc/crypto/fipsmodule/evp/p_rsa.c:752:(.text.pkey_rsa_ctrl_str+0x258): undefined reference to `__isoc23_strtol'
>           collect2: error: ld returned 1 exit status
>   = note: some `extern` functions couldn't be found; some native libraries may need to be installed or have their path specified
>   = note: use the `-l` flag to specify native libraries to link
>   = note: use the `cargo:rustc-link-lib` directive to specify the native libraries to link with Cargo (see https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-lib)
> error: could not compile `remote_server` (bin "remote_server") due to 1 previous error
> ```
>
> **Nguyên nhân**:
> Đây là do các bug đã biết trong aws-lc-rs (không hỗ trợ GCC >= 14): [FIPS fails to build with GCC >= 14](https://github.com/aws/aws-lc-rs/issues/569)
> & [GCC-14 - build failure for FIPS module](https://github.com/aws/aws-lc/issues/2010)
>
> Xem [linux: Linker error for remote_server when using script/install-linux](https://github.com/zed-industries/zed/issues/24880) để biết thêm.
>
> **Cách khắc phục tạm**:
> Đặt target của remote server là `x86_64-unknown-linux-gnu` như sau `export REMOTE_SERVER_TARGET=x86_64-unknown-linux-gnu; script/install-linux`

## Wayland & X11

Zode hỗ trợ cả X11 và Wayland. Mặc định, Zode chọn cái nào tìm thấy lúc chạy. Nếu bạn dùng Wayland và muốn chạy ở chế độ X11, dùng biến môi trường `WAYLAND_DISPLAY=''`.

## Ghi chú khi đóng gói Zode

Phần này dành cho người duy trì bản phân phối khi đóng gói Zode.

### Yêu cầu kỹ thuật

Zode có hai binary chính:

- Bạn cần build `crates/cli` và đặt binary của nó vào `$PATH` với tên `zode`.
- Bạn cần build `crates/zode` và đặt nó tại `$PATH/to/cli/../../libexec/zode-editor`. Ví dụ, nếu đặt cli ở `~/.local/bin/zode` thì đặt trình soạn thảo ở `~/.local/libexec/zode-editor`. Vì một số bản phân phối Linux (đáng chú ý là Arch) không khuyến khích dùng `libexec`, bạn cũng có thể đặt binary này ở `$PATH/to/cli/../../lib/zode/zode-editor` (ví dụ `~/.local/lib/zode/zode-editor`).
- Nếu định cung cấp file `.desktop`, bạn có thể lấy mẫu ở `crates/zode/resources/zode.desktop.in` và dùng `envsubst` để điền các giá trị cần thiết. File này cũng nên đổi tên thành `$APP_ID.desktop` để [tuân theo chuẩn FreeDesktop](https://github.com/zed-industries/zed/issues/12707#issuecomment-2168742761). Bạn cũng nên cho file desktop này quyền thực thi (`chmod 755`).
- Bạn cần bảo đảm đã cài các thư viện cần thiết. Có thể lấy danh sách hiện tại bằng cách [kiểm tra binary đã build](https://github.com/zed-industries/zed/blob/935cf542aebf55122ce6ed1c91d0fe8711970c82/script/bundle-linux#L65-L67) trên máy bạn.
- Xem ví dụ một script build hoàn chỉnh ở [script/bundle-linux](https://github.com/zed-industries/zed/blob/935cf542aebf55122ce6ed1c91d0fe8711970c82/script/bundle-linux).
- Bạn có thể tắt tự động cập nhật của Zode và đưa hướng dẫn cho người dùng cố cập nhật Zode thủ công, bằng cách build (hoặc chạy) Zode với biến môi trường `ZODE_UPDATE_EXPLANATION`. Ví dụ: `ZODE_UPDATE_EXPLANATION="Please use flatpak to update Zode."`.
- Nhớ cập nhật nội dung file `crates/zode/RELEASE_CHANNEL` thành 'nightly', 'preview' hoặc 'stable', không có dòng mới. Việc này khiến Zode dùng trình quản lý credential để nhớ đăng nhập của người dùng.

### Những điều khác cần lưu ý

Zode đổi nhanh, và người duy trì bản phân phối thường có ràng buộc và ưu tiên khác nhau. Các điểm dưới đây mô tả những đánh đổi hiện tại:

- Zode là dự án thay đổi nhanh. Chúng tôi thường phát hành 2-3 bản build mỗi tuần để sửa lỗi được báo và đưa ra các thay đổi lớn hơn.
- Zode tự cài các phiên bản công cụ phát triển phổ biến, tương tự rustup/rbenv/pyenv. Hành vi này được bàn [tại đây](https://github.com/zed-industries/zed/issues/12589).
- Người dùng có thể cài extension cục bộ và từ [zed-industries/extensions](https://github.com/zed-industries/extensions). Extension có thể cài thêm công cụ như language server. Các cải tiến an toàn đã lên kế hoạch được theo dõi [tại đây](https://github.com/zed-industries/zed/issues/12358).
- Mặc định Zode kết nối tới nhiều dịch vụ trực tuyến (AI, telemetry, cộng tác). Người dùng có thể tắt AI và telemetry của chúng tôi bằng setting Zode của họ, hoặc bằng cách vá [file cài đặt mặc định](https://github.com/tgiap04/zode/blob/develop/assets/settings/default.json) của chúng tôi.
- Do các điểm trên, hiện Zode chạy không tốt trong sandbox. Xem [thảo luận này](https://github.com/zed-industries/zed/pull/12006#issuecomment-2130421220).

## Flatpak

> Tích hợp Flatpak hiện tại của Zode thoát khỏi sandbox khi khởi động. Các quy trình dựa vào sandbox của Flatpak có thể không chạy như mong đợi.

Để build và cài package Flatpak cục bộ, làm theo các bước sau:

1. Cài Flatpak cho bản phân phối của bạn như hướng dẫn [tại đây](https://flathub.org/setup).
2. Chạy script `script/flatpak/deps` để cài các dependency cần thiết.
3. Chạy `script/flatpak/bundle-flatpak`.
4. Giờ package đã được cài và có một bundle tại `target/release/{app-id}.flatpak`.

## Profile bộ nhớ

[`heaptrack`](https://github.com/KDE/heaptrack) khá hữu ích để chẩn đoán rò rỉ bộ nhớ. Để cài:

```sh
$ sudo apt install heaptrack heaptrack-gui
$ cargo install cargo-heaptrack
```

Sau đó, build và chạy Zode kèm profiler:

```sh
$ cargo heaptrack -b zode
```

Khi thoát instance Zode này, output trên terminal sẽ có một lệnh để chạy `heaptrack_interpret`, chuyển profile `*.raw.zst` thành file `*.zst` để đưa vào `heaptrack_gui` xem.

## Ghi perf

Cách lấy flamegraph có symbol đã phân giải từ một instance Zode đang chạy.
Dùng khi Zode ngốn nhiều CPU. Không hữu ích cho trường hợp treo máy.

### Trong lúc sự cố xảy ra

- Tìm PID (process ID) bằng:
  `ps -eo size,pid,comm | grep zode | sort | head -n 1 | cut -d ' ' -f 2`
  Hoặc tìm PID của `zode-editor` dùng RAM nhiều nhất trong
  htop/btop/top.

- Cài perf:
  Trên Ubuntu (và bản phái sinh) chạy `sudo apt install linux-tools`.

- Chạy perf record:
  Chạy `sudo perf record -p <pid you just found>`, đợi vài giây để thu dữ liệu, rồi nhấn Ctrl+C. Giờ bạn sẽ có file `perf.data`.

- Chuyển quyền sở hữu file output cho người dùng:
  chạy `sudo chown $USER:$USER perf.data`

- Lấy thông tin build:
  Chạy lại Zode và gõ `zode: about` trong bảng lệnh (command palette) để lấy commit chính xác.

Có thể gửi file `perf.data` cho Zode kèm commit chính xác.

### Làm sau

Phần này người duy trì Zode có thể làm.

- Build Zode có symbol:
  Checkout commit tìm được trước đó và sửa `Cargo.toml`.
  Áp dụng diff sau, rồi build release.

```diff
[profile.release]
-debug = "limited"
+debug = "full"
```

- Thêm symbol vào perf database:
  `perf buildid-cache -v -a <path to release zode binary>`

- Phân giải symbol từ db:
  `perf inject -i perf.data -o perf_with_symbols.data`

- Cài flamegraph:
  `cargo install cargo-flamegraph`

- Dựng flamegraph:
  `flamegraph --perfdata perf_with_symbols.data`

## Xử lý sự cố

### Lỗi Cargo báo dependency dùng tính năng chưa ổn định

Thử `cargo clean` rồi `cargo build`.
