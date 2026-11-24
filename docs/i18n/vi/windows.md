---
title: Zode on Windows
description: "Download the Zode installer for Windows from the GitHub releases page, or build Zode from source."
---

# Zode trên Windows

## Cài đặt Zode

Tải bộ cài `Zode-<arch>.exe` mới nhất từ [trang releases](https://github.com/tgiap04/zode/releases) và chạy nó. Sau lần cài thủ công đầu tiên, Zode sẽ định kỳ kiểm tra bản cập nhật.

Bạn cũng có thể build Zode từ mã nguồn, xem hướng dẫn trong [tài liệu này](./development/windows.md).

## Gỡ cài đặt

- Cài bằng bộ cài: dùng `Settings` → `Apps` → `Installed apps`, tìm Zode rồi bấm Uninstall.
- Build từ mã nguồn: xoá thư mục kết quả build bạn đã tạo (ví dụ thư mục target/install).

Cài đặt và extension của bạn nằm trong hồ sơ người dùng. Khi gỡ cài đặt, bạn có thể chọn giữ hoặc xoá chúng.

## Phát triển từ xa (SSH)

Zode hỗ trợ phát triển từ xa trên Windows qua cả SSH và WSL. Bạn có thể kết nối tới máy chủ từ xa qua SSH hoặc làm việc trực tiếp với file trong các bản phân phối WSL từ Zode.

Để biết hướng dẫn chi tiết về thiết lập và dùng tính năng phát triển từ xa, gồm cấu hình SSH, thiết lập WSL và xử lý sự cố, xem [tài liệu Phát triển từ xa](./remote-development.md).

## Xử lý sự cố

### Zode không khởi động được hoặc hiện cửa sổ trống

- Kiểm tra phần cứng và phiên bản hệ điều hành có tương thích với Zode không. Xem [hướng dẫn cài đặt](./installation.md) để biết thêm.
- Cập nhật driver GPU từ nhà sản xuất GPU (Intel/AMD/NVIDIA/Qualcomm).
- Đảm bảo tăng tốc phần cứng được bật trong Windows và không bị phần mềm bên thứ ba chặn.
- Thử mở Zode không có extension hay cài đặt tuỳ chỉnh để tìm ra xung đột.

### Lỗi terminal

Nếu các script kích hoạt không chạy, hãy cập nhật lên phiên bản mới nhất và kiểm tra file profile của shell không thoát sớm. Với thao tác Git, hãy xác nhận Git Bash hoặc PowerShell có sẵn và nằm trong PATH.

### Sự cố SSH từ xa

Khi được yêu cầu nhập thông tin đăng nhập, hãy dùng hộp thoại askpass dạng đồ hoạ. Nếu nó không hiện, hãy kiểm tra xung đột với trình quản lý thông tin xác thực và xem terminal của bạn có chặn hộp thoại GUI không.

### Sự cố đồ hoạ

#### Zode không mở được / hiệu năng giảm

Zode cần GPU tương thích DirectX 11 để chạy. Nếu Zode không mở được, GPU của bạn có thể chưa đạt yêu cầu tối thiểu.

Để kiểm tra GPU có hỗ trợ DirectX 11 không, chạy lệnh sau:

```
dxdiag
```

Lệnh này mở DirectX Diagnostic Tool, công cụ cho biết phiên bản DirectX mà GPU hỗ trợ tại `System` → `System Information` → `DirectX Version`.

Nếu bạn chạy Zode trong máy ảo, nó sẽ dùng bộ điều hợp giả lập do máy ảo cung cấp. Zode vẫn chạy được trong môi trường này, nhưng hiệu năng có thể giảm.
