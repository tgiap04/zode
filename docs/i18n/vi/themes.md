---
title: Themes - Zode
description: Browse, install, and create themes for Zode. Includes built-in themes and community theme extensions.
---

# Giao diện (Theme)

Zode có sẵn một số giao diện (theme), và còn nhiều theme khác dưới dạng extension.

## Chọn theme

Xem theme nào đã cài và xem trước chúng qua Theme Selector, mở từ bảng lệnh (command palette) bằng action `theme selector: toggle` (phím tắt {#kb theme_selector::Toggle}).

Di chuyển lên xuống trong danh sách theme sẽ đổi theme ngay lập tức, và nhấn enter sẽ lưu theme đang chọn vào file cài đặt.

## Cài theme mới

Bạn có thể tìm hàng trăm theme trong kho extension của Zode, truy cập qua bảng lệnh bằng `zode: extensions` hoặc [website extension của Zed](https://zed.dev/extensions?filter=themes).

Nhiều theme phổ biến đã được chuyển sang Zode, và nếu khó chọn, hãy ghé [zed-themes.com](https://zed-themes.com), một thư viện của bên thứ ba có ảnh xem trước cho nhiều theme.

## Tự tạo theme

Bạn có thể dùng [Theme Builder của Zed](https://zed.dev/theme-builder) để thiết kế theme riêng dựa trên một theme có sẵn.

Công cụ này cho phép bạn tinh chỉnh và xem trước giao diện mọi bề mặt trong ứng dụng Zode.
Sau đó bạn có thể xuất JSON để [dùng cục bộ](./themes.md#local-themes) hoặc [đăng lên kho extension của Zode](./extensions/themes.md).

## Cấu hình theme

Theme bạn chọn được lưu trong file cài đặt.
Bạn có thể mở file cài đặt từ bảng lệnh bằng {#action zode::OpenSettingsFile} (phím tắt {#kb zode::OpenSettingsFile}).

Mặc định, Zode giữ hai theme: một cho chế độ sáng và một cho chế độ tối.
Bạn có thể đặt mode là `"dark"` hoặc `"light"` để bỏ qua chế độ hiện tại của hệ thống.

```json [settings]
{
  "theme": {
    "mode": "system",
    "light": "One Light",
    "dark": "One Dark"
  }
}
```

### Đổi chế độ theme bằng bàn phím

Dùng {#kb theme::ToggleMode} để chuyển chế độ theme hiện tại giữa sáng và tối.

Nếu cài đặt của bạn đang dùng giá trị theme tĩnh, như:

```json [settings]
{
  "theme": "Any Theme"
}
```

lần chuyển đầu tiên sẽ đổi nó thành chọn theme động với các theme mặc định:

```json [settings]
{
  "theme": {
    "mode": "system",
    "light": "One Light",
    "dark": "One Dark"
  }
}
```

Bạn cần tự đặt cả theme `light` và `dark` sau lần chuyển đầu tiên.

Sau đó, mỗi lần chuyển chỉ cập nhật `theme.mode`.
Nếu `light` và `dark` là cùng một theme, lần chuyển đầu có thể không làm giao diện đổi cho tới khi bạn đặt giá trị khác nhau cho `light` và `dark`.

## Ghi đè theme

Để ghi đè các thuộc tính cụ thể của theme, dùng setting `theme_overrides`.
Setting này dùng để cấu hình ghi đè riêng cho từng theme.

Ví dụ, thêm đoạn sau vào `settings.json` nếu bạn muốn ghi đè màu nền của trình soạn thảo và hiển thị comment lẫn doc comment bằng chữ nghiêng:

```json [settings]
{
  "theme_overrides": {
    "One Dark": {
      "editor.background": "#333",
      "syntax": {
        "comment": {
          "font_style": "italic"
        },
        "comment.doc": {
          "font_style": "italic"
        }
      },
      "accents": [
        "#ff0000",
        "#ff7f00",
        "#ffff00",
        "#00ff00",
        "#0000ff",
        "#8b00ff"
      ]
    }
  }
}
```

Zode phủ màu cam thương hiệu lên mọi theme khi tải, trước khi áp dụng phần ghi đè của bạn.
Nó đặt `text.accent`, `icon.accent`, `border.focused`, `element.selection_background` cùng con trỏ, vùng chọn và nền của người chơi cục bộ.
Màu cú pháp, trạng thái và cộng tác viên vẫn theo theme.
Để lấy lại một trong các token đó, đặt nó dưới tên theme của bạn trong `theme_overrides`:

```json [settings]
{
  "theme_overrides": {
    "One Dark": {
      "border.focused": "#4a90d9"
    }
  }
}
```

Để xem danh sách đầy đủ các capture (như `comment` và `comment.doc`), xem [Language Extensions: Syntax highlighting](./extensions/languages.md#syntax-highlighting).

Để xem danh sách thuộc tính theme có sẵn, hãy mở file JSON của theme.
Ví dụ, `assets/themes/vscode-2026/vscode-2026.json` cho Dark 2026 và Light 2026 mặc định,
hoặc `assets/themes/one/one.json` cho One Dark và One Light.

## Theme cục bộ {#local-themes}

Lưu theme mới cục bộ bằng cách đặt chúng trong thư mục `~/.config/zode/themes` (macOS và Linux) hoặc `%USERPROFILE%\AppData\Roaming\Zode\themes\` (Windows).

Ví dụ, để tạo theme mới tên `my-cool-theme`, tạo file `my-cool-theme.json` trong thư mục đó.
Nó sẽ có trong bộ chọn theme vào lần Zode tải kế tiếp.
