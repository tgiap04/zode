---
title: Debugging Crashes
description: "Guide to debugging crashes for Zode development."
---

# Gỡ lỗi crash

Khi Zode panic hoặc crash, nó gửi một thông điệp tới tiến trình phụ (sidecar). Tiến trình này đọc bộ nhớ của trình soạn thảo và tạo một [minidump](https://chromium.googlesource.com/breakpad/breakpad/+/master/docs/getting_started_with_breakpad.md#the-minidump-file-format) trong `~/Library/Logs/Zode` hoặc `$XDG_DATA_HOME/zode/logs`. Bạn có thể dùng minidump này để tạo backtrace cho mọi stack của thread.

Bản dựng này không tải báo cáo crash lên bất kỳ đâu; xem [Telemetry](../telemetry.md).

Các báo cáo crash này chứa dữ liệu hữu ích, nhưng khó đọc nếu thiếu span hoặc thông tin symbol. Bạn vẫn có thể phân tích cục bộ bằng cách tải mã nguồn và binary chưa strip (hoặc file symbol riêng) của bản Zode đang dùng, rồi chạy:

```sh
zstd -d ~/.local/share/zode/logs/<uuid>.dmp -o minidump.dmp
minidump-stackwalk minidump.dmp
```

Cạnh file minidump trong thư mục log, bạn cũng sẽ thấy file `<uuid>.json` chứa metadata như thông báo panic, span và cấu hình hệ thống.

## Dùng trình gỡ lỗi (debugger)

Nếu tái hiện được crash đều đặn, hãy dùng trình gỡ lỗi để xem trạng thái chương trình tại điểm crash.

Chi tiết cài đặt xem [Dùng trình gỡ lỗi](./debuggers.md#debugging-panics-and-crashes).
