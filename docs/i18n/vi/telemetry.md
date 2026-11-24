---
title: Telemetry
description: "This build collects no telemetry and sends no usage or crash data."
---

# Telemetry

**Bản build này không thu thập telemetry.** Không có gì về cách bạn dùng, dự án của bạn
hay các lần crash được ghi lại hoặc gửi đi.

Đây không phải một cài đặt bạn phải tìm rồi tắt đi — nó là thuộc tính của
bản build. Code thu thập sự kiện, code tải chúng lên và trình báo cáo crash
đều đã bị gỡ bỏ chứ không phải chỉ tắt:

- Không có sự kiện sử dụng hay số liệu nào được ghi, và không có hàng đợi sự kiện hay log nào để
  tải lên. Macro `telemetry::event!` mà phần còn lại của trình soạn thảo gọi sẽ bỏ qua
  tham số của nó.
- Không có báo cáo crash hay [minidump](https://learn.microsoft.com/en-us/windows/win32/debug/minidump-files)
  nào được thu thập hay tải lên. Dấu vết treo (hang trace) vẫn được ghi, nhưng chỉ vào một file cục bộ
  trong thư mục dữ liệu của chính bạn, và không ai đọc chúng ngoài bạn.
- Tài khoản Zode tuỳ chọn chỉ tồn tại để đồng bộ `settings.json` và
  `keymap.json` của chính bạn, và nó tắt cho tới khi bạn đăng nhập. Nó không ghi gì về cách bạn
  dùng trình soạn thảo, và những gì nó lưu được mã hoá trên máy bạn bằng một khoá
  mà máy chủ không bao giờ nhận — `legal/privacy-policy.md` trong repository có
  mô tả đầy đủ cơ chế.
- Không có dịch vụ phân tích hay báo cáo crash của bên thứ ba nào trong
  cây dependency.

## Cài đặt `telemetry`

Khoá `telemetry` vẫn được chấp nhận trong `settings.json`:

```json [settings]
"telemetry": {
    "diagnostics": false,
    "metrics": false
},
```

Nó vẫn được phân tích, nhưng **không có gì phản ứng với nó** — không có việc thu thập nào để nó
bật hay tắt. Nó chỉ còn để các file cài đặt hiện có tiếp tục tải mà không đổi.
Đặt một trong hai giá trị thành `true` cũng không khiến gì bị thu thập.

## Những gì vẫn đi ra mạng

Gỡ telemetry không có nghĩa là không bao giờ mở socket. Bản build này vẫn
thực hiện các request ra ngoài có chủ đích, và cần nói chính xác về chúng:

- **Tải extension**, khi bạn duyệt hoặc cài một extension.
- **Tải language server**, khi một ngôn ngữ bạn mở cần một server
  chưa có trên máy.
- **Tải driver cơ sở dữ liệu**, lần đầu bạn kết nối tới một engine cơ sở dữ liệu.
  Request mang tên và phiên bản driver — không mang chuỗi kết nối,
  thông tin xác thực hay câu truy vấn của bạn.
- **Kiểm tra bản cập nhật**, khi bạn chạy _Check for Updates_. Nó đọc các bản phát hành
  đã công bố của repository này và không gửi định danh nào.
- **Đồng bộ cài đặt**, nếu bạn đã đăng nhập và bật nó. Nó tải lên
  `settings.json` và `keymap.json` của bạn dưới dạng bản mã mà máy chủ không đọc được.
- **Phát triển từ xa**, khi bạn kết nối tới một host do chính bạn chọn.

Đây là các lượt tải và kết nối do bạn khởi động, và không cái nào mang dữ liệu
sử dụng. Còn một kết nối ra ngoài không khớp với mô tả đó, nên nó có
mục riêng bên dưới.

## Hạn mức đăng ký của agent

Thanh trạng thái có thể hiện mức dùng gói đăng ký Claude Code và/hoặc Codex của bạn — xem
[Chỉ báo mức dùng của agent](./agent-usage.md) để biết đầy đủ. Khác với các
request ở trên, request này không do một thao tác rõ ràng kích hoạt: nó thăm dò
tự động mỗi 60 giây khi cửa sổ đang được focus.

Nó không mang telemetry của trình soạn thảo và không bao giờ chạm tới đường ống
telemetry (đã bị gỡ) của bản build này. Thứ nó gửi: access token OAuth Claude của bạn, đọc
mới từ keychain hoặc `~/.claude/.credentials.json` cho mỗi request tới endpoint
mức dùng của chính Anthropic. Với Codex, không có gì liên quan tới thông tin xác thực rời
khỏi trình soạn thảo này — một tiến trình con `codex app-server` riêng giữ
phiên của nó.

## Tự kiểm chứng

Các khẳng định trên được thiết kế để kiểm tra được thay vì tin suông:

- `rg -n "MINIDUMP_ENDPOINT|multipart|telemetry/events" crates/` không trả về gì.
- `crates/telemetry/src/telemetry.rs` không làm gì — hãy đọc nó; chỉ vài chục dòng.
- Theo dõi tiến trình bằng công cụ giám sát mạng và xác nhận rằng sửa, lưu và
  thoát đều không tạo lưu lượng đi ra.
