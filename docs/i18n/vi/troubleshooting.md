---
title: Troubleshooting
description: "Common issues and solutions for Zode on all platforms."
---

# Xử lý sự cố

Hướng dẫn này nêu các kỹ thuật xử lý sự cố thông dụng cho Zode.
Có lúc bạn tự nhận ra và giải quyết được vấn đề nhờ thông tin này.
Có lúc xử lý sự cố nghĩa là thu thập đúng thông tin (log, profile hay các bước tái hiện) để giúp chúng tôi chẩn đoán và sửa lỗi.

> **Lưu ý**: Để mở bảng lệnh (command palette), dùng `cmd-shift-p` trên macOS hoặc `ctrl-shift-p` trên Windows / Linux.

## Lấy thông tin Zode và hệ thống

Khi báo lỗi hoặc nhờ trợ giúp, nên biết phiên bản Zode và cấu hình hệ thống của bạn. Bạn có thể lấy thông tin này bằng các action sau từ bảng lệnh:

- {#action zode::About}: Xem số phiên bản Zode
- {#action zode::CopySystemSpecsIntoClipboard}: Chép vào clipboard số phiên bản Zode, phiên bản hệ điều hành và cấu hình phần cứng
- {#action zode::CopyInstalledExtensionsIntoClipboard}: Chép vào clipboard danh sách extension đã cài cùng phiên bản

## Log của Zode

Thường thì nơi đầu tiên nên xem khi xử lý bất kỳ sự cố nào trong Zode là log của Zode, có thể chứa manh mối về điều đang sai.
Bạn có thể xem 1000 dòng gần nhất của log bằng cách chạy action {#action zode::OpenLog} từ bảng lệnh.
Nếu muốn xem cả file, bạn có thể mở nó trong trình quản lý file của hệ điều hành qua {#action zode::RevealLogInFileManager} từ bảng lệnh.

Log của Zode nằm ở các vị trí tương ứng trên từng hệ điều hành:

- macOS: `~/Library/Logs/Zode/Zode.log`
- Windows: `C:\Users\YOU\AppData\Local\Zode\logs\Zode.log`
- Linux: `~/.local/share/zode/logs/Zode.log`, hoặc `$XDG_DATA_HOME/zode/logs/Zode.log` khi đặt `XDG_DATA_HOME`

> **Lưu ý:** Trong một số trường hợp, theo dõi log trực tiếp sẽ hữu ích, chẳng hạn khi [phát triển extension Zode](https://zodekit.com/docs/extensions/developing-extensions).
> Ví dụ: `tail -f ~/Library/Logs/Zode/Zode.log`

Log có thể chứa đủ ngữ cảnh để bạn tự gỡ lỗi, hoặc bạn có thể thấy các lỗi cụ thể hữu ích khi tạo [GitHub issue](https://github.com/tgiap04/zode/issues/new/choose) hay trao đổi với người duy trì Zode trên [issue tracker](https://github.com/tgiap04/zode/issues).

## Vấn đề hiệu năng (profiling)

Nếu bạn gặp vấn đề hiệu năng trong Zode (giật, treo hoặc không phản hồi), đính kèm một performance profile vào issue sẽ giúp chúng tôi xác định đúng chỗ bị kẹt.

### macOS

Xcode Instruments (đi kèm bản tải [Xcode](https://apps.apple.com/us/app/xcode/id497799835) của bạn) là công cụ chuẩn để profiling trên macOS.

1. Khi Zode đang chạy, mở Instruments
1. Chọn `Time Profiler` làm mẫu profiling
   ![Bộ chọn mẫu của Instruments với Time Profiler được chọn](https://images.zed.dev/docs/troubleshooting/instruments-template-picker.webp)
1. Trong cấu hình `Time Profiler`, đặt target là tiến trình Zode đang chạy
1. Bắt đầu ghi
   ![Cấu hình Time Profiler hiện danh sách chọn target và nút ghi](https://images.zed.dev/docs/troubleshooting/instruments-target-and-record.webp)
1. Thực hiện thao tác trong Zode gây ra vấn đề hiệu năng
1. Dừng ghi
   ![Một bản ghi Time Profiler đã hoàn tất trong Instruments](https://images.zed.dev/docs/troubleshooting/instruments-recording.webp)
1. Lưu file trace
1. Nén file trace thành file zip
1. Tạo [GitHub issue](https://github.com/tgiap04/zode/issues/new/choose) kèm file trace zip

<!--### Windows-->

<!--### Linux-->

## Sự cố khi khởi động và workspace

Zode tạo các cơ sở dữ liệu SQLite cục bộ để lưu dữ liệu liên quan tới workspace và các dự án của bạn. Các cơ sở dữ liệu này lưu, chẳng hạn, các tab và khung bạn đang mở trong một dự án, vị trí cuộn của từng file đang mở, danh sách mọi dự án bạn đã mở (cho bộ chọn dự án gần đây), v.v. Bạn có thể tìm và xem các cơ sở dữ liệu này ở các vị trí sau:

- macOS: `~/Library/Application Support/Zode/db`
- Linux và FreeBSD: `~/.local/share/zode/db` (hoặc `zode/db` trong `XDG_DATA_HOME` hay `FLATPAK_XDG_DATA_HOME`)
- Windows: `%LOCALAPPDATA%\Zode\db`

Tên các cơ sở dữ liệu này có dạng `0-<zed_channel>`:

- Stable: `0-stable`
- Preview: `0-preview`
- Nightly: `0-nightly`
- Dev: `0-dev`

Tuy hiếm, chúng tôi đã gặp vài trường hợp cơ sở dữ liệu workspace bị hỏng, khiến Zode không khởi động được.
Nếu gặp sự cố khởi động, bạn có thể kiểm tra xem có liên quan tới workspace không bằng cách tạm di chuyển cơ sở dữ liệu khỏi vị trí của nó, rồi thử khởi động Zode lại.

> **Lưu ý**: Di chuyển cơ sở dữ liệu workspace sẽ khiến Zode tạo một cái mới.
> Các dự án gần đây, tab đang mở, v.v. của bạn sẽ được đặt lại về trạng thái ban đầu.

Nếu sự cố vẫn còn sau khi tạo lại cơ sở dữ liệu, hãy [tạo issue](https://github.com/tgiap04/zode/issues/new/choose).

## Sự cố language server

Nếu bạn gặp sự cố liên quan language server, như chẩn đoán lỗi cũ hay không nhảy được tới định nghĩa, khởi động lại language server qua {#action editor::RestartLanguageServer} từ bảng lệnh thường giải quyết được vấn đề.
