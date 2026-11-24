---
title: Building Zode for Windows
description: "Guide to building Zode for windows for Zode development."
---

# Build Zode trên Windows

> Các lệnh sau có thể chạy trong bất kỳ shell nào.

## Repository

Clone [repository của Zode](https://github.com/tgiap04/zode).

## Phụ thuộc

- Cài [rustup](https://www.rust-lang.org/tools/install)

- Cài [Visual Studio](https://visualstudio.microsoft.com/downloads/) kèm các thành phần tùy chọn `MSVC v*** - VS YYYY C++ x64/x86 build tools` và `MSVC v*** - VS YYYY C++ x64/x86 Spectre-mitigated libs (latest)` (`v***` là phiên bản VS của bạn và `YYYY` là năm phát hành. Đổi kiến trúc nếu cần).
- Hoặc nếu muốn cài gọn hơn, chỉ cài [Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (cùng các thư viện ở trên) và workload "Desktop development with C++".
  Cấu hình này rustup không tự nhận ra. Trước khi biên dịch, hãy khởi tạo biến môi trường bằng cách mở developer shell (cmd/PowerShell) được cài trong menu Start hoặc Windows Terminal.
- Cài Windows 11 hoặc 10 SDK cho hệ thống của bạn, và đảm bảo ít nhất đã cài `Windows 10 SDK version 2104 (10.0.20348.0)`. Bạn có thể tải ở [Windows SDK Archive](https://developer.microsoft.com/windows/downloads/windows-sdk/).
- Cài [CMake](https://cmake.org/download) (được [một thư viện phụ thuộc](https://docs.rs/wasmtime-c-api-impl/latest/wasmtime_c_api/) yêu cầu). Hoặc cài qua Visual Studio Installer, rồi thêm thủ công thư mục `bin` vào `PATH`, ví dụ: `C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.

Nếu không biên dịch được Zode, hãy đảm bảo bản cài Visual Studio có ít nhất các thành phần sau:

```json
{
  "version": "1.0",
  "components": [
    "Microsoft.VisualStudio.Component.CoreEditor",
    "Microsoft.VisualStudio.Workload.CoreEditor",
    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
    "Microsoft.VisualStudio.ComponentGroup.WebToolsExtensions.CMake",
    "Microsoft.VisualStudio.Component.VC.CMake.Project",
    "Microsoft.VisualStudio.Component.Windows11SDK.26100",
    "Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre"
  ],
  "extensions": []
}
```

Nếu chỉ dùng Build Tools, hãy đảm bảo đã cài các thành phần sau:

```json
{
  "version": "1.0",
  "components": [
    "Microsoft.VisualStudio.Component.Roslyn.Compiler",
    "Microsoft.Component.MSBuild",
    "Microsoft.VisualStudio.Component.CoreBuildTools",
    "Microsoft.VisualStudio.Workload.MSBuildTools",
    "Microsoft.VisualStudio.Component.Windows10SDK",
    "Microsoft.VisualStudio.Component.VC.CoreBuildTools",
    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
    "Microsoft.VisualStudio.Component.VC.Redist.14.Latest",
    "Microsoft.VisualStudio.Component.Windows11SDK.26100",
    "Microsoft.VisualStudio.Component.VC.CMake.Project",
    "Microsoft.VisualStudio.Component.TextTemplating",
    "Microsoft.VisualStudio.Component.VC.CoreIde",
    "Microsoft.VisualStudio.ComponentGroup.NativeDesktop.Core",
    "Microsoft.VisualStudio.Workload.VCTools",
    "Microsoft.VisualStudio.Component.VC.Runtimes.x86.x64.Spectre"
  ],
  "extensions": []
}
```

Bạn có thể xuất danh sách thành phần này như sau:

- Mở Visual Studio Installer
- Bấm `More` ở tab `Installed`
- Bấm `Export configuration`

### Ghi chú

Sửa `pg_hba.conf` trong thư mục `data` để dùng `trust` thay cho `scram-sha-256` ở phương thức `host`. Nếu không, kết nối sẽ lỗi `password authentication failed`. File này thường nằm ở `C:\Program Files\PostgreSQL\17\data\pg_hba.conf`. Sau khi sửa, nó trông như sau:

```conf
# IPv4 local connections:
host    all             all             127.0.0.1/32            trust
# IPv6 local connections:
host    all             all             ::1/128                 trust
```

Nếu bạn dùng locale Windows không phải Latin, hãy đặt tham số `lc_messages` trong `postgresql.conf` (ở thư mục `data`) thành `English_United States.1252` (hoặc bảng mã khác tương thích UTF-8 có trên hệ thống). Nếu không, cơ sở dữ liệu có thể panic. File sẽ trông như sau:

```conf
# lc_messages = 'Chinese (Simplified)_China.936' # locale for system error message strings
lc_messages = 'English_United States.1252'
```

Sau đó, khởi động lại service `postgresql`. Nhấn `Win`+`R` để mở hộp thoại Run, nhập `services.msc` rồi chọn **OK**. Trong Services Manager, tìm `postgresql-x64-XX`, bấm chuột phải và chọn **Restart**.

## Build từ mã nguồn

Khi đã cài xong các thành phần phụ thuộc, bạn có thể build Zode bằng [Cargo](https://doc.rust-lang.org/cargo/).

Build debug:

```sh
cargo run
```

Build release:

```sh
cargo run --release
```

Và để chạy test:

```sh
cargo test --workspace
```

> **Lưu ý:** Test hồi quy giao diện hiện chỉ chạy trên macOS và cần quyền Screen Recording. Xem chi tiết ở [Build Zode trên macOS](./macos.md#visual-regression-tests).

## Cài từ msys2

Zode không hỗ trợ các gói Zode MSYS2 không chính thức build cho Mingw-w64. Hãy báo mọi sự cố với [mingw-w64-zed](https://packages.msys2.org/base/mingw-w64-zed) tại [msys2/MINGW-packages/issues](https://github.com/msys2/MINGW-packages/issues?q=is%3Aissue+is%3Aopen+zed).

Hãy xem trước [tài liệu MSYS2](https://www.msys2.org/docs/ides-editors/#zed).

## Xử lý sự cố

### Đặt biến môi trường `RUSTFLAGS` làm hỏng build

Nếu bạn đặt biến môi trường `RUSTFLAGS`, nó sẽ ghi đè cài đặt `rustflags` trong `.cargo/config.toml`, vốn cần thiết để build Zode đúng cách.

Vì các cài đặt này thay đổi theo thời gian, lỗi build có thể từ lỗi linker đến những lỗi khó chẩn đoán khác.

Nếu cần thêm cờ Rust, hãy dùng một trong các cách sau trong `.cargo/config.toml`:

Thêm cờ vào mục build

```toml
[build]
rustflags = ["-C", "symbol-mangling-version=v0", "--cfg", "tokio_unstable"]
```

Thêm cờ vào mục target windows

```toml
[target.'cfg(target_os = "windows")']
rustflags = [
    "--cfg",
    "windows_slim_errors",
    "-C",
    "target-feature=+crt-static",
]
```

Hoặc tạo `.cargo/config.toml` mới ở thư mục cha của repo Zode (xem bên dưới). Cách này hữu ích trong CI vì không cần sửa `.cargo/config.toml` gốc của repo.

```
upper_dir
├── .cargo          // <-- Make this folder
│   └── config.toml // <-- Make this file
└── zed
    ├── .cargo
    │   └── config.toml
    └── crates
        ├── assistant
        └── ...
```

Trong `.cargo/config.toml` mới ở trên, nếu muốn thêm `--cfg gles` vào rustflags, nó sẽ như sau

```toml
[target.'cfg(all())']
rustflags = ["--cfg", "gles"]
```

### Cargo báo lỗi một thư viện phụ thuộc đang dùng tính năng chưa ổn định

Thử `cargo clean` rồi `cargo build`.

### `STATUS_ACCESS_VIOLATION`

Lỗi này có thể xảy ra nếu bạn dùng linker "rust-lld.exe". Hãy thử linker khác.

Nếu bạn dùng cấu hình toàn cục, hãy cân nhắc chuyển repo Zode vào một thư mục lồng bên trong và thêm `.cargo/config.toml` với cấu hình linker tùy chỉnh ở thư mục cha.

Xem issue này để biết thêm thông tin [#12041](https://github.com/zed-industries/zed/issues/12041)

### Đường dẫn RC được chọn không hợp lệ

Đôi khi, tùy theo quy tắc bảo mật áp dụng trên laptop của bạn, bạn có thể gặp lỗi sau khi biên dịch Zode:

```
error: failed to run custom build command for `zed(C:\Users\USER\src\zed\crates\zed)`

Caused by:
  process didn't exit successfully: `C:\Users\USER\src\zed\target\debug\build\zed-b24f1e9300107efc\build-script-build` (exit code: 1)
  --- stdout
  cargo:rerun-if-changed=../../.git/logs/HEAD
  cargo:rustc-env=ZODE_COMMIT_SHA=25e2e9c6727ba9b77415588cfa11fd969612adb7
  cargo:rustc-link-arg=/stack:8388608
  cargo:rerun-if-changed=resources/windows/app-icon.ico
  package.metadata.winresource does not exist
  Selected RC path: 'bin\x64\rc.exe'

  --- stderr
  The system cannot find the path specified. (os error 3)
warning: build failed, waiting for other jobs to finish...
```

Để khắc phục, hãy đặt thủ công biến môi trường `ZODE_RC_TOOLKIT_PATH` thành đường dẫn RC toolkit. Thường là:
`C:\Program Files (x86)\Windows Kits\10\bin\<SDK_version>\x64`.

Xem [issue](https://github.com/zed-industries/zed/issues/18393) này để biết thêm thông tin.

### Build lỗi: đường dẫn quá dài

Bạn có thể gặp lỗi như sau khi build

```
error: failed to get `pet` as a dependency of package `languages v0.1.0 (D:\a\zed-windows-builds\zed-windows-builds\crates\languages)`

Caused by:
  failed to load source for dependency `pet`

Caused by:
  Unable to update https://github.com/microsoft/python-environment-tools.git?rev=ffcbf3f28c46633abd5448a52b1f396c322e0d6c#ffcbf3f2

Caused by:
  path too long: 'C:/Users/runneradmin/.cargo/git/checkouts/python-environment-tools-903993894b37a7d2/ffcbf3f/crates/pet-conda/tests/unix/conda_env_without_manager_but_found_in_history/some_other_location/conda_install/conda-meta/python-fastjsonschema-2.16.2-py310hca03da5_0.json'; class=Filesystem (30)
```

Để khắc phục, hãy bật hỗ trợ đường dẫn dài cho cả Git lẫn Windows.

Với git: `git config --system core.longpaths true`

Và với Windows, dùng lệnh PowerShell này:

```powershell
New-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" -Name "LongPathsEnabled" -Value 1 -PropertyType DWORD -Force
```

Xem thêm [tài liệu win32](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation?tabs=powershell)

(Bạn cần khởi động lại hệ thống sau khi bật hỗ trợ đường dẫn dài.)

### Sự cố đồ họa

#### Zode không khởi động được

Zode hiện dùng Vulkan làm API đồ họa trên Windows. Nếu Zode không khởi động được, Vulkan là nguyên nhân thường gặp.

Bạn có thể xem log của Zode tại:
`C:\Users\YOU\AppData\Local\Zode\logs\Zode.log`

Nếu thấy các thông báo như:

- `Zode failed to open a window: NoSupportedDeviceFound`
- `ERROR_INITIALIZATION_FAILED`
- `GPU Crashed`
- `ERROR_SURFACE_LOST_KHR`

Vulkan có thể đang không hoạt động đúng trên hệ thống của bạn. Cập nhật driver GPU thường giải quyết được.

Nếu trong log không có gì liên quan đến Vulkan và máy bạn có cài Bandicam, hãy thử gỡ nó. Hiện Zode không tương thích với Bandicam.
