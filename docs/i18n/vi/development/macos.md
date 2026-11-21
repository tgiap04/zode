---
title: Building Zode for macOS
description: "Guide to building zed for macos for Zode development."
---

# Build Zode trên macOS

## Repository

Clone [repository của Zode](https://github.com/tgiap04/zode).

## Dependency

- Cài [rustup](https://www.rust-lang.org/tools/install)

- Cài [Xcode](https://apps.apple.com/us/app/xcode/id497799835?mt=12) từ macOS App Store hoặc từ trang [Apple Developer](https://developer.apple.com/download/all/). Tải từ Apple Developer cần có tài khoản developer.

> Sau khi cài, hãy mở Xcode và cài các thành phần macOS (tùy chọn mặc định).

- Cài [Xcode command line tools](https://developer.apple.com/xcode/resources/)

  ```sh
  xcode-select --install
  ```

- Đảm bảo Xcode command line tools đang dùng bản Xcode bạn vừa cài:

  ```sh
  sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -license accept
  ```

- Cài `cmake` (cần cho [một dependency](https://docs.rs/wasmtime-c-api-impl/latest/wasmtime_c_api/))

  ```sh
  brew install cmake
  ```

## Build Zode từ mã nguồn

Khi đã cài xong dependency, bạn có thể build Zode bằng [Cargo](https://doc.rust-lang.org/cargo/).

Build bản debug:

```sh
cargo run
```

Build bản release:

```sh
cargo run --release
```

Và để chạy test:

```sh
cargo test --workspace
```

## Xử lý sự cố

### Lỗi biên dịch metal shader

```sh
error: failed to run custom build command for gpui v0.1.0 (/Users/path/to/zed)`**

xcrun: error: unable to find utility "metal", not a developer tool or in PATH
```

Thử `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`

Nếu bạn dùng macOS 26, thử `xcodebuild -downloadComponent MetalToolchain`.
Nếu lệnh đó lỗi, chạy `xcodebuild -runFirstLaunch` rồi tải lại toolchain.

### Cargo báo dependency dùng tính năng chưa ổn định (unstable)

Thử `cargo clean` rồi `cargo build`.

### Lỗi: 'dispatch/dispatch.h' file not found

Nếu bạn gặp lỗi tương tự:

```sh
src/platform/mac/dispatch.h:1:10: fatal error: 'dispatch/dispatch.h' file not found

Caused by:
  process didn't exit successfully

  --- stdout
  cargo:rustc-link-lib=framework=System
  cargo:rerun-if-changed=src/platform/mac/dispatch.h
  cargo:rerun-if-env-changed=TARGET
  cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS_aarch64-apple-darwin
  cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_darwin
  cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS
```

File này thuộc về Xcode. Hãy chắc là Xcode command line tools đã được cài và đường dẫn đặt đúng:

```sh
xcode-select --install
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
```

Ngoài ra, đặt biến môi trường `BINDGEN_EXTRA_CLANG_ARGS`:

```sh
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$(xcrun --show-sdk-path)"
```

Sau đó clean và build lại dự án:

```sh
cargo clean
cargo run
```

### Test lỗi `Too many open files (os error 24)`

Lỗi này có vẻ do giới hạn tài nguyên của hệ điều hành. Cài và chạy test bằng `cargo-nextest` sẽ giải quyết được.

- `cargo install cargo-nextest --locked`
- `cargo nextest run --workspace --no-fail-fast`

## Mẹo

### Tránh build lại liên tục

Nếu Zode cứ build lại các root crate, có thể bạn đang mở chính mã nguồn Zode trong bản build phát triển.

Điều này gây vấn đề vì `cargo run` export một loạt biến môi trường
mà `rust-analyzer` chạy trong bản build phát triển
của Zode nhặt lấy. Các biến này lại được truyền cho `cargo check`, làm
mất hiệu lực cache build của một số crate mà chúng ta phụ thuộc vào.

Để tránh, hãy chạy binary đã build với một dự án khác, ví dụ `cargo run ~/path/to/other/project`.

### Tăng tốc bước xác minh

Nếu bạn build Zode thường xuyên, macOS có thể cứ xác minh các bản build mới, mỗi lần lặp tốn thêm vài giây.

Để khắc phục, bạn có thể:

- Chạy `sudo spctl developer-mode enable-terminal` để bật bảng Developer Tools trong System Settings.
- Trong System Settings, tìm "Developer Tools" và thêm terminal của bạn (ví dụ iTerm hoặc Ghostty) vào danh sách "Allow applications to use developer tools"
- Khởi động lại terminal.

Cảm ơn các nhà phát triển nextest đã đăng [hướng dẫn này](https://nexte.st/docs/installation/macos/#gatekeeper).
