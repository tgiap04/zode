---
title: Zode on macOS
description: "Zode is developed primarily on macOS, making it a first-class platform with full feature support."
---

# Zode trên macOS

Zode được phát triển chủ yếu trên macOS, nên đây là nền tảng hạng nhất, hỗ trợ đầy đủ tính năng.

## Cài đặt Zode

Tải file `Zode-<arch>.dmg` từ [trang releases](https://github.com/tgiap04/zode/releases) — mở nó rồi kéo Zode vào thư mục Applications.

Sau khi cài, Zode tự kiểm tra bản cập nhật và nhắc bạn khi có phiên bản mới.

### Build từ mã nguồn

Để build Zode từ mã nguồn, xem [tài liệu phát triển trên macOS](./development/macos.md).

## Yêu cầu hệ thống

- macOS 10.15.7 (Catalina) trở lên
- Chip Apple Silicon (M1/M2/M3/M4) hoặc Intel

Zode dùng Metal để render tăng tốc bằng GPU, có sẵn trên mọi phiên bản macOS được hỗ trợ.

## Cài CLI

Zode có sẵn công cụ dòng lệnh để mở file và dự án từ Terminal. Để cài:

1. Mở Zode
2. Mở bảng lệnh (command palette) bằng `Cmd+Shift+P`
3. Chạy `cli: install`

Lệnh này tạo lệnh `zode` trong `/usr/local/bin`. Sau đó bạn mở được file và thư mục:

```sh
zode .                   # Open current folder
zode file.txt            # Open a file
zode project/ file.txt   # Open a folder and a file
```

Xem [CLI Reference](./reference/cli.md) để biết mọi tuỳ chọn.

## Gỡ cài đặt

1. Thoát Zode nếu đang chạy
2. Kéo Zode từ Applications vào Thùng rác
3. Nếu muốn, xoá cài đặt và extension của bạn:

```sh
rm -rf ~/.config/zode
rm -rf ~/Library/Application\ Support/Zode
rm -rf ~/Library/Caches/Zode
rm -rf ~/Library/Logs/Zode
rm -rf ~/Library/Saved\ Application\ State/io.github.tgiap04.zode.savedState
```

Nếu đã cài CLI, gỡ nó bằng:

```sh
rm /usr/local/bin/zode
```

Bản Zode cũ hơn tạo `/usr/local/bin/zed` thay vào đó. Chỉ xoá nó nếu `readlink /usr/local/bin/zed` trỏ vào một bundle `Zode*.app`; nếu không thì nó thuộc về Zed. Đừng chạy `/usr/local/bin/zed --uninstall` từ bản Zode cũ đó: xem cảnh báo ở trang [Gỡ cài đặt](./uninstall.md).

## Xử lý sự cố

### Zode không mở hoặc báo "damaged"

Nếu macOS báo Zode bị hỏng hoặc không mở được, nhiều khả năng là lỗi Gatekeeper. Hãy thử:

1. Bấm phải (hoặc Control-click) vào Zode trong Applications
2. Chọn "Open" từ menu ngữ cảnh
3. Bấm "Open" trong hộp thoại hiện ra

Việc này báo macOS tin tưởng ứng dụng.

Nếu không được, xoá thuộc tính quarantine:

```sh
xattr -cr /Applications/Zode.app
```

### Không tìm thấy lệnh CLI

Nếu lệnh `zode` không dùng được sau khi cài:

1. Kiểm tra `/usr/local/bin` có trong PATH của bạn
2. Thử cài lại CLI bằng `cli: install` trong bảng lệnh
3. Mở cửa sổ terminal mới để nạp lại PATH

### Sự cố GPU hoặc render

Zode dùng Metal để render. Nếu bạn gặp lỗi đồ hoạ:

1. Đảm bảo macOS đã cập nhật
2. Khởi động lại Mac để đặt lại trạng thái GPU
3. Xem Activity Monitor để biết áp lực GPU từ các ứng dụng khác

### Dùng nhiều bộ nhớ hoặc CPU

Nếu Zode dùng nhiều tài nguyên hơn dự kiến:

1. Tìm language server chạy mất kiểm soát trong output của terminal (`zode: open log`)
2. Thử tắt từng extension để tìm xung đột
3. Với dự án lớn, cân nhắc dùng [cài đặt dự án](./reference/all-settings.md#file-scan-exclusions) để loại các thư mục không cần khỏi việc đánh chỉ mục

Cần trợ giúp thêm, xem [hướng dẫn xử lý sự cố](./troubleshooting.md) hoặc tạo issue trên [GitHub](https://github.com/tgiap04/zode/issues).
