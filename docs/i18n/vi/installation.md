---
title: Install Zode - macOS, Linux, Windows
description: Download and install Zode on macOS, Linux, or Windows. Includes direct download and the Linux installation script.
---

# Cài đặt Zode

## Tải Zode

### macOS

Tải `Zode-<arch>.dmg` mới nhất từ [trang releases](https://github.com/tgiap04/zode/releases), mở file và kéo Zode vào thư mục Applications. Sau lần cài thủ công đầu tiên, Zode sẽ định kỳ kiểm tra bản cập nhật.

### Windows

Tải trình cài đặt `Zode-<arch>.exe` mới nhất từ [trang releases](https://github.com/tgiap04/zode/releases) rồi chạy. Sau lần cài thủ công đầu tiên, Zode sẽ định kỳ kiểm tra bản cập nhật.

### Linux

Với đa số người dùng Linux, cách cài Zode dễ nhất là dùng script cài đặt của chúng tôi:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh
```

Bạn có thể chọn **phiên bản** Zode cần cài bằng biến môi trường `ZODE_VERSION`:

```sh
# Install the latest stable version (default)
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh

# Install a specific version
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | ZODE_VERSION=0.1.6 sh
```

Để cài bản nightly:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | ZODE_CHANNEL=nightly sh
```

Bản preview không được phát hành. Tên biến cũ `ZODE_VERSION` và `ZODE_CHANNEL` vẫn được chấp nhận.

Nếu bạn đã cài Zode 0.1.5 trên Linux, bản này không tự cập nhật lên 0.1.6 được: hãy chạy lại script cài đặt một lần. Xem [Nâng cấp từ Zode 0.1.5](./linux.md#upgrading-from-zode-015).

Script này hỗ trợ `x86_64` và `AArch64`, cùng các bản phân phối Linux phổ biến: Ubuntu, Arch, Debian, RedHat, CentOS, Fedora và nhiều bản khác.

Nếu Zode được cài bằng script này, bạn gỡ được bất cứ lúc nào bằng lệnh shell `zode --uninstall`. Shell sẽ hỏi bạn muốn giữ hay xóa cấu hình cá nhân. Sau khi chọn, bạn sẽ thấy thông báo Zode đã được gỡ thành công.

Nếu script này chưa đáp ứng nhu cầu, bạn gặp sự cố khi chạy Zode, hoặc có lỗi khi gỡ Zode, xem [tài liệu riêng cho Linux](./linux.md).

## Yêu cầu hệ thống

### macOS

Zode hỗ trợ các bản macOS sau:

| Phiên bản     | Tên mã   | Trạng thái Apple | Trạng thái Zode     |
| ------------- | -------- | ---------------- | ------------------- |
| macOS 26.x    | Tahoe    | Supported        | Supported           |
| macOS 15.x    | Sequoia  | Supported        | Supported           |
| macOS 14.x    | Sonoma   | Supported        | Supported           |
| macOS 13.x    | Ventura  | Supported        | Supported           |
| macOS 12.x    | Monterey | EOL 2024-09-16   | Supported           |
| macOS 11.x    | Big Sur  | EOL 2023-09-26   | Partially Supported |
| macOS 10.15.x | Catalina | EOL 2022-09-12   | Partially Supported |

Các bản macOS ghi "Partially Supported" (Big Sur và Catalina) không hỗ trợ chia sẻ màn hình qua Zode Collaboration. Các tính năng này dùng [LiveKit SDK](https://livekit.io), vốn dựa vào [ScreenCaptureKit.framework](https://developer.apple.com/documentation/screencapturekit/) chỉ có trên macOS 12 (Monterey) trở lên.

#### Phần cứng Mac

Zode hỗ trợ máy dùng bộ xử lý Intel (x86_64) hoặc Apple (aarch64) đáp ứng yêu cầu macOS ở trên:

- MacBook Pro (đầu 2015 trở lên)
- MacBook Air (đầu 2015 trở lên)
- MacBook (đầu 2016 trở lên)
- Mac Mini (cuối 2014 trở lên)
- Mac Pro (cuối 2013 trở lên)
- iMac (cuối 2015 trở lên)
- iMac Pro (mọi model)
- Mac Studio (mọi model)

### Linux

Zode hỗ trợ bộ xử lý Intel/AMD 64-bit (x86_64) và Arm 64-bit (aarch64).

Zode cần driver Vulkan 1.3 và các desktop portal sau:

- `org.freedesktop.portal.FileChooser`
- `org.freedesktop.portal.OpenURI`
- `org.freedesktop.portal.Secret` hoặc `org.freedesktop.Secrets`

### Windows

Zode hỗ trợ các bản Windows sau:

| Phiên bản                          | Trạng thái Zode |
| ---------------------------------- | --------------- |
| Windows 11, version 22H2 and later | Supported       |
| Windows 10, version 1903 and later | Supported       |

Cần hệ điều hành 64-bit để chạy Zode.

#### Phần cứng Windows

Zode hỗ trợ máy dùng bộ xử lý x64 (Intel, AMD) hoặc Arm64 (Qualcomm) đáp ứng các yêu cầu sau:

- Đồ họa: GPU hỗ trợ DirectX 11 (đa số PC từ 2012 trở lên).
- Driver: driver NVIDIA/AMD/Intel/Qualcomm mới (không phải Microsoft Basic Display Adapter).

### FreeBSD

Chưa có bản tải chính thức. Có thể build [từ mã nguồn](./development/freebsd.md).

### Web

Hiện chưa hỗ trợ. Xem [issue Platform Support](https://github.com/zed-industries/zed/issues/5391).
