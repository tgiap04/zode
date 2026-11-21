---
title: Uninstall
description: "This guide covers how to uninstall Zode on different operating systems."
---

# Gỡ cài đặt

Hướng dẫn này nói cách gỡ cài đặt Zode trên các hệ điều hành khác nhau.

> **Cảnh báo:** Đừng chạy `--uninstall` từ một file thực thi Zode 0.1.5 còn sót lại, chẳng hạn `zed --uninstall`, `~/.local/bin/zed --uninstall` hoặc `/usr/local/bin/zed --uninstall`. Zode 0.1.5 nhúng sẵn script gỡ cài đặt cũ, script này xoá `~/.local/share/zed`, `~/.config/zed` và `~/.zode_server`, và trên macOS còn xoá cả `~/Library/Application Support/Zed` và `~/Library/Logs/Zed`. Đó là các thư mục mà Zed gốc dùng để giữ dữ liệu của chính nó. Hãy gỡ Zode thủ công theo các bước bên dưới. Script đi kèm từ bản 0.1.6 chỉ đụng tới đường dẫn của Zode.

## macOS

### Cài đặt thông thường

Nếu bạn cài Zode bằng cách tải từ trang web:

1. Thoát Zode nếu nó đang chạy
2. Mở Finder và vào thư mục Applications
3. Kéo Zode vào Trash (hoặc bấm chuột phải và chọn "Move to Trash")
4. Làm trống Trash

### Xoá dữ liệu người dùng (tuỳ chọn)

Để xoá hoàn toàn mọi file cấu hình và dữ liệu của Zode:

1. Mở Finder
2. Nhấn `Cmd + Shift + G` để mở "Go to Folder"
3. Xoá các thư mục sau nếu chúng tồn tại:
   - `~/Library/Application Support/Zode`
   - `~/Library/Saved Application State/io.github.tgiap04.zode.savedState`
   - `~/Library/Logs/Zode`
   - `~/Library/Caches/io.github.tgiap04.zode`
   - `~/Library/Caches/Zode`
   - `~/.config/zode`
   - `~/.local/state/Zode`

Nếu bạn đã cài CLI bằng `cli: install`, hãy xoá thêm `/usr/local/bin/zode`.

## Linux

### Gỡ cài đặt thông thường

Nếu Zode được cài bằng script cài đặt mặc định, hãy chạy:

```sh
zode --uninstall
```

Bạn sẽ được hỏi giữ hay xoá các tuỳ chọn cá nhân. Sau khi chọn, bạn sẽ thấy thông báo Zode đã được gỡ cài đặt thành công.

Nếu không tìm thấy lệnh `zode` trong PATH, hãy thử:

```sh
$HOME/.local/bin/zode --uninstall
```

hoặc:

```sh
$HOME/.local/zode.app/bin/zode --uninstall
```

### Trình quản lý gói

Nếu bạn cài Zode bằng trình quản lý gói (như Flatpak, Snap hoặc trình quản lý gói riêng của bản phân phối), hãy xem tài liệu của trình quản lý gói đó để biết cách gỡ cài đặt.

### Xoá thủ công

Nếu lệnh gỡ cài đặt thất bại hoặc Zode được cài vào vị trí tuỳ chọn, bạn có thể xoá thủ công:

- Thư mục cài đặt: `~/.local/zode.app` (hoặc đường dẫn cài đặt tuỳ chọn của bạn)
- Symlink của file thực thi: `~/.local/bin/zode`
- Cấu hình và dữ liệu: `~/.config/zode`, `~/.local/share/zode`

Các đường dẫn tên `zed` (`~/.local/zed.app`, `~/.local/bin/zed`, `/usr/local/bin/zed`) thuộc về Zed, trừ khi chúng trỏ vào một bundle Zode. Bản cài Zode 0.1.5 trên Linux dùng `~/.local/zed.app` và `~/.local/bin/zed`; xem [Nâng cấp từ Zode 0.1.5](./linux.md#upgrading-from-zode-015) để biết cách phân biệt hai bên. Chỉ Zode 0.1.5 trên macOS mới tạo `/usr/local/bin/zed`; chỉ xoá nó nếu `readlink /usr/local/bin/zed` trỏ vào một bundle `Zode*.app`.

## Windows

### Cài đặt thông thường

1. Thoát Zode nếu nó đang chạy
2. Mở Settings (phím Windows + I)
3. Vào "Apps" > "Installed apps" (hoặc "Apps & features" trên Windows 10)
4. Tìm "Zode"
5. Bấm menu ba chấm cạnh Zode và chọn "Uninstall"
6. Làm theo hướng dẫn để hoàn tất việc gỡ cài đặt

Hoặc bạn có thể:

1. Mở menu Start
2. Bấm chuột phải vào Zode
3. Chọn "Uninstall"

### Xoá dữ liệu người dùng (tuỳ chọn)

Để xoá hoàn toàn mọi file cấu hình và dữ liệu của Zode:

1. Nhấn `Windows key + R` để mở Run
2. Gõ `%APPDATA%` rồi nhấn Enter
3. Xoá thư mục `Zode` nếu nó tồn tại
4. Nhấn `Windows key + R` lần nữa, gõ `%LOCALAPPDATA%` rồi nhấn Enter
5. Xoá thư mục `Zode` nếu nó tồn tại

## Xử lý sự cố

Nếu gặp sự cố khi gỡ cài đặt:

- **macOS/Windows**: Đảm bảo Zode đã thoát hoàn toàn trước khi gỡ. Kiểm tra Activity Manager (macOS) hoặc Task Manager (Windows) xem còn tiến trình Zode nào đang chạy không.
- **Linux**: Nếu script gỡ cài đặt thất bại, hãy xem thông báo lỗi và cân nhắc xoá thủ công các thư mục liệt kê ở trên.
- **Mọi nền tảng**: Nếu muốn bắt đầu lại từ đầu mà vẫn giữ Zode, bạn có thể xoá các thư mục cấu hình thay vì gỡ cài đặt cả ứng dụng.

Cần thêm trợ giúp, xem [tài liệu riêng cho Linux](./linux.md) hoặc tạo issue trên [GitHub](https://github.com/tgiap04/zode/issues).
