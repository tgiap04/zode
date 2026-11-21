---
title: Appearance and Visual Customization - Zode
description: Customize Zode's themes, fonts, icons, UI density, and other visual settings to match your preferences.
---

# Giao diện

Tuỳ chỉnh giao diện của Zode cho hợp ý bạn. Hướng dẫn này nói về giao diện (theme), font, biểu tượng và các cài đặt hiển thị khác.

Về cách hệ thống cài đặt hoạt động, xem [All Settings](./reference/all-settings.md).

## Tuỳ biến Zode trong 5 phút

Cách biến Zode thành nơi quen thuộc:

1. **Chọn giao diện (theme)**: Nhấn {#kb theme_selector::Toggle} để mở Theme Selector. Dùng phím mũi tên duyệt danh sách để xem trước giao diện ngay lập tức, rồi nhấn Enter để áp dụng.

2. **Đổi nhanh chế độ sáng/tối**: Nhấn {#kb theme::ToggleMode}. Nếu bạn đang dùng giá trị tĩnh `"theme": "..."`, lần đổi đầu tiên sẽ chuyển nó sang cài đặt chế độ động với các giao diện mặc định.

3. **Đặt font**: Mở Settings Editor bằng {#kb zode::OpenSettings} và tìm `buffer_font_family`. Đặt thành font lập trình bạn thích.

4. **Chỉnh cỡ chữ**: Trong cùng Settings Editor, tìm `buffer_font_size` và `ui_font_size` để chỉnh cỡ chữ của trình soạn thảo và giao diện.

Xong rồi. Giờ bạn đã có một Zode mang dấu ấn riêng.

## Giao diện (Themes)

Cài giao diện từ trang Extensions ({#action zode::Extensions}), rồi chuyển qua lại bằng Theme Selector ({#kb theme_selector::Toggle}).

Zode hỗ trợ giao diện riêng cho chế độ sáng và tối, tự chuyển theo thiết lập hệ thống của bạn:

```json [settings]
{
  "theme": {
    "mode": "system",
    "light": "One Light",
    "dark": "One Dark"
  }
}
```

Bạn cũng có thể ghi đè từng thuộc tính của giao diện để điều khiển chi tiết.

→ [Tài liệu về Themes](./themes.md)

## Icon Themes

Biểu tượng file và thư mục trong Project Panel và các tab lấy từ Material Icon Theme, có màu, và không đổi được — không có Icon Theme Selector, và icon theme nào khác cũng không có hiệu lực.

→ [Tài liệu về Icon Themes](./icon-themes.md)

## Font

Zode dùng ba setting font cho các ngữ cảnh khác nhau:

| Setting                | Dùng cho                  |
| ---------------------- | ------------------------- |
| `buffer_font_family`   | Chữ trong trình soạn thảo |
| `ui_font_family`       | Thành phần giao diện      |
| `terminal.font_family` | [Terminal](./terminal.md) |

Ví dụ cấu hình:

```json [settings]
{
  "buffer_font_family": "JetBrains Mono",
  "buffer_font_size": 14,
  "ui_font_family": "Inter",
  "ui_font_size": 16,
  "terminal": {
    "font_family": "JetBrains Mono",
    "font_size": 14
  }
}
```

### Ligature của font

Để tắt ligature của font:

```json [settings]
{
  "buffer_font_features": {
    "calt": false
  }
}
```

### Chiều cao dòng

Chỉnh khoảng cách dòng bằng `buffer_line_height`:

- `"comfortable"` — tỉ lệ 1.618 (mặc định)
- `"standard"` — tỉ lệ 1.3
- `{ "custom": 1.5 }` — tỉ lệ tuỳ chọn

## Thành phần giao diện

Zode cho điều khiển rộng rãi các thành phần giao diện, gồm:

- **Thanh tab** — Hiện/ẩn, nút điều hướng, biểu tượng file, trạng thái git
- **Thanh trạng thái** — Bộ chọn ngôn ngữ, vị trí con trỏ, kiểu xuống dòng, [mức dùng agent](./agent-usage.md)
- **Thanh cuộn** — Hiển thị, chỉ báo git diff, kết quả tìm kiếm
- **Minimap** — Hiển thị tổng quan mã
- **Gutter** — Số dòng, chỉ báo gập, breakpoint
- **Các bảng** — Kích thước Project Panel; kích thước và neo của Terminal (Project Panel luôn neo bên phải, đối diện project rail, và không di chuyển được)

→ [Tài liệu Visual Customization](./visual-customization.md) cho mọi cài đặt thành phần giao diện

## Project Rail

Mỗi dự án đang mở có một ô vuông trên thanh dọc bên trái. Bấm một ô
để chuyển sang dự án đó; bấm phải để mở menu riêng của nó:

- **Open Project in New Window** — chuyển dự án ra khỏi cửa sổ này. Chỉ
  hiện khi có nhiều hơn một dự án đang mở, vì nếu không thì cửa sổ
  nó chuyển tới chính là cửa sổ này.
- **Reveal in Finder** / **Copy Project Path** — dự án mở từ nhiều
  thư mục sẽ chép tất cả, mỗi thư mục một dòng.
- **Change Initials…** — hai chữ cái vẽ trên ô vuông. Hữu ích khi hai
  dự án rút gọn ra cùng chữ cái đầu. Việc này chỉ đổi ô vuông: tên
  dự án, đường dẫn và tiêu đề cửa sổ không bị đụng tới.
- **Change Colour…** — chọn màu cho ô vuông, xem trước ngay trên ô
  trong lúc bạn chọn. **Use Default** trả về màu của giao diện. Các
  chữ tự đổi giữa sáng và tối để vẫn đọc rõ trên
  màu bạn chọn.
- **Remove Project** — gỡ dự án khỏi cửa sổ này. Không có gì trên đĩa
  bị đụng tới và dự án vẫn ở trong danh sách gần đây của bạn.

Có thể **kéo các ô để sắp xếp lại**, và thứ tự được nhớ giữa các
phiên. Dự án mới mở vẫn xuất hiện ở trên cùng. Kéo một ô ra khỏi
thanh hẳn sẽ chuyển dự án đó sang cửa sổ mới — cả việc đó lẫn **Remove**
đều hỏi xác nhận trước, vì cả hai đều gỡ dự án khỏi cửa sổ này.

Con số ở góc trên bên phải của một ô đếm các tab agent Claude của dự án
đó đang chờ bạn: hộp thoại cấp quyền đang hiện, hoặc một câu trả lời đã xong
mà bạn chưa xem. Focus vào tab sẽ xoá loại thứ hai. Con số
hiện **99+** khi trên chín mươi chín, và rê chuột lên ô sẽ nêu số chính xác. Các agent khác (Codex, OpenCode, Copilot) và các tab Zode
không theo dõi thì không được đếm. Chấm báo dự án đang đánh chỉ mục lại sau
khi thức dậy nằm ở góc dưới bên phải.

## Tiếp theo

- [All Settings](./reference/all-settings.md) — Tài liệu tham khảo đầy đủ về cài đặt
- [Key bindings](./key-bindings.md) — Tuỳ chỉnh phím tắt
- [Vim Mode](./vim.md) — Bật soạn thảo theo chế độ
