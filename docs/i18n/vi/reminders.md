---
title: Nhắc nhở và đồng hồ tập trung
description: Nhắc uống nước, nghỉ mắt và đứng dậy, cùng đồng hồ tập trung, từ bảng điều khiển dưới icon khay.
---

# Nhắc nhở và đồng hồ tập trung

Zode có thể nhắc bạn uống nước, nghỉ mắt và đứng dậy, và chạy một
đồng hồ tập trung (Pomodoro) xen kẽ làm việc với nghỉ ngơi. Mọi chu kỳ đều
do bạn đặt. Tính năng này tắt theo mặc định.

## Bật tính năng

Bấm icon Zode trên thanh menu (macOS) hoặc khay hệ thống (Windows,
Linux) rồi bật **Zode Reminders**, hoặc đặt trong file settings:

```json
{
  "reminders": { "enabled": true }
}
```

Bảng mở ra gồm đồng hồ tập trung và mỗi loại nhắc một thẻ, thẻ nào
cũng có công tắc và chu kỳ riêng. Bảng không nhận bàn phím, nên
chu kỳ được chỉnh bằng nút **−** và **+**, mỗi lần năm phút;
**Settings…** mở danh sách đầy đủ, nơi bạn gõ được bất kỳ giá trị nào từ 1 đến 480
phút.

## Các loại nhắc

| Nhắc      | Mặc định    | Nội dung                                             |
| --------- | ----------- | ---------------------------------------------------- |
| Uống nước | mỗi 45 phút | Uống một cốc nước.                                   |
| Nghỉ mắt  | mỗi 20 phút | Quy tắc 20-20-20: nhìn xa 6 m (20 ft) trong 20 giây. |
| Đứng dậy  | mỗi 60 phút | Đứng dậy, vươn vai và đi lại một phút.               |

Bật một loại nhắc chưa làm nó chạy: bấm nút bắt đầu của nó, vòng tròn sẽ
đếm ngược tới lần nhắc kế tiếp. Nút dừng giữ nó lại cho tới khi bạn bắt đầu lại.

Nhắc đến hạn lúc bạn đang rời bàn sẽ chờ bạn quay lại chứ không
bắn vào một chỗ ngồi trống: sau `idle_pause_minutes` (mặc định 5) phút máy không có
thao tác nào, các nhắc đến hạn được giữ lại. Linux không báo được khoảng
thời gian này, nên ở đó nhắc nhở không bao giờ bị giữ vì lý do ấy. Máy ngủ thì không
bắn bù các nhắc đã lỡ; chu kỳ của chúng tính lại từ lúc máy thức.

## Đồng hồ tập trung

Bắt đầu từ bảng. Sau một vòng tập trung (25 phút) là một lần nghỉ
ngắn (5 phút), cứ bốn vòng thì có một lần nghỉ dài (15 phút). Đồng hồ
đếm ngược hiện trên thanh trạng thái, và mỗi lần chuyển giai đoạn được báo như
một lời nhắc. Các nút Pause, Skip và Stop nằm trong bảng.

## Cách hiện nhắc nhở

Với `"style": "bubble"` (mặc định), nhắc nhở hiện thành một bong bóng nhỏ dưới
icon khay, có nút **Done** và **Snooze**. Bong bóng tự ẩn sau 15
giây; icon nhắc nhở trên thanh trạng thái vẫn đổi màu cho tới khi bạn trả lời.

Bong bóng cần icon khay đang nhìn thấy. Khi không thấy, nhắc nhở đến dưới dạng
thông báo hệ thống:

- macOS: khi có app toàn màn hình, hoặc thanh menu đặt chế độ tự ẩn.
- Windows: khi icon nằm trong khu tràn của taskbar (nút **^**);
  ghim icon ra taskbar để nhận bong bóng.
- Linux: Zode không đặt được vị trí cửa sổ trên Wayland, nên ở đó nhắc nhở luôn là
  thông báo. GNOME gốc không có khay nếu chưa cài extension như
  AppIndicator; thanh trạng thái và thông báo vẫn chạy.

Đặt `"style": "system"` để luôn dùng thông báo hệ thống. Bong bóng không theo
chế độ Focus hay Không làm phiền của hệ thống; thông báo thì có.

Bảng và bong bóng dùng chính nền cửa sổ của app: `window_opacity`
và `window_vibrancy` làm chúng trong suốt như các cửa sổ của trình soạn thảo.

## Settings

```json
{
  "reminders": {
    "enabled": false,
    "style": "bubble",
    "idle_pause_minutes": 5,
    "snooze_minutes": 5,
    "water": { "enabled": true, "interval_minutes": 45 },
    "eye_break": { "enabled": true, "interval_minutes": 20 },
    "stand_up": { "enabled": true, "interval_minutes": 60 },
    "focus": {
      "focus_minutes": 25,
      "short_break_minutes": 5,
      "long_break_minutes": 15,
      "rounds_before_long_break": 4
    }
  },
  "tray_icon": true
}
```

Đặt `"tray_icon": false` để bỏ icon khỏi thanh menu hoặc khay.
