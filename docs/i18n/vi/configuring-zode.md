---
title: Configuring Zode - Settings and Preferences
description: Configure Zode with the Settings Editor, JSON files, and project-specific overrides. Covers all settings options.
---

# Cấu hình Zode

Trang này giải thích hệ thống cài đặt của Zode hoạt động ra sao, gồm Settings Editor, các file cấu hình JSON và cài đặt riêng cho từng dự án.

Về tuỳ biến giao diện (theme, font, icon), xem [Giao diện](./appearance.md).

## Settings Editor

**Settings Editor** ({#kb zode::OpenSettings}) là cách chính để cấu hình Zode. Nó có ô tìm kiếm để bạn duyệt các cài đặt, xem giá trị hiện tại và sửa trực tiếp.

Để mở:

- Nhấn {#kb zode::OpenSettings}
- Hoặc chạy `zode: open settings` từ bảng lệnh (command palette)

Khi bạn gõ vào ô tìm kiếm, các cài đặt khớp sẽ hiện kèm mô tả và điều khiển để sửa. Thay đổi tự lưu vào file cài đặt.

> **Lưu ý:** Settings Editor chưa có đủ mọi cài đặt. Một số tuỳ chọn nâng cao, như formatter theo ngôn ngữ, phải sửa thẳng file JSON.

## File cài đặt

### Cài đặt người dùng

Cài đặt người dùng áp dụng cho mọi dự án. Mở file bằng {#kb zode::OpenSettingsFile} hoặc chạy `zode: open settings file` từ bảng lệnh.

File nằm ở:

- macOS: `~/.config/zode/settings.json`
- Linux: `~/.config/zode/settings.json` (hoặc `$XDG_CONFIG_HOME/zode/settings.json`)
- Windows: `%APPDATA%\Zode\settings.json`

Cú pháp là JSON, hỗ trợ comment `//`.

### Cài đặt mặc định

Để xem mọi cài đặt cùng giá trị mặc định, chạy {#action zode::OpenDefaultSettings} từ bảng lệnh. Lệnh này mở một bản tham chiếu chỉ đọc, tiện đối chiếu khi sửa cài đặt của bạn.

### Cài đặt dự án

Ghi đè cài đặt người dùng cho một dự án bằng cách tạo file `.zode/settings.json` ở thư mục gốc dự án. Chạy {#action zode::OpenProjectSettings} để tạo file này.

Cài đặt dự án được ưu tiên hơn cài đặt người dùng, và chỉ trong dự án đó.

```json [settings]
// .zode/settings.json
{
  "tab_size": 2,
  "formatter": "prettier",
  "format_on_save": "on"
}
```

Bạn cũng có thể đặt file cài đặt trong các thư mục con để kiểm soát chi tiết hơn.

**Giới hạn:** Không phải cài đặt nào cũng đặt được ở cấp dự án. Những cài đặt ảnh hưởng toàn trình soạn thảo (như `theme` hay `vim_mode`) chỉ có tác dụng trong cài đặt người dùng. Cài đặt dự án chỉ giới hạn ở hành vi soạn thảo và công cụ ngôn ngữ như `tab_size`, `formatter`, `format_on_save`.

## Cách các cài đặt hợp nhất

Cài đặt được áp dụng theo từng lớp:

1. **Cài đặt mặc định** — giá trị có sẵn của Zode
2. **Cài đặt người dùng** — tuỳ chọn chung của bạn
3. **Cài đặt dự án** — ghi đè riêng cho dự án

Lớp sau ghi đè lớp trước. Với cài đặt dạng object (như `terminal`), các thuộc tính được gộp chứ không thay thế toàn bộ.

## Cài đặt theo từng file

Zode hỗ trợ tương thích phần nào với [modeline](./modelines.md) của Emacs và Vim, nên bạn có thể đặt một số cài đặt riêng cho từng file.

## Ghi đè theo kênh phát hành

Dùng cài đặt khác nhau cho bản Stable, Preview hay Nightly bằng cách thêm khoá kênh ở cấp trên cùng:

```json [settings]
{
  "theme": "One Dark",
  "vim_mode": false,
  "nightly": {
    "theme": "Rosé Pine",
    "vim_mode": true
  },
  "preview": {
    "theme": "Catppuccin Mocha"
  }
}
```

Với cấu hình này:

- **Stable** dùng One Dark, tắt vim mode
- **Preview** dùng Catppuccin Mocha, tắt vim mode
- **Nightly** dùng Rosé Pine, bật vim mode

Thay đổi trong Settings Editor áp dụng cho mọi kênh.

## Deep link tới cài đặt

Zode hỗ trợ deep link để mở thẳng một cài đặt cụ thể:

```
zode://settings/theme
zode://settings/vim_mode
zode://settings/buffer_font_size
```

Hữu ích khi chia sẻ mẹo cấu hình hoặc dẫn link từ tài liệu.

## Cấu hình ví dụ

```json [settings]
{
  "theme": {
    "mode": "system",
    "light": "One Light",
    "dark": "One Dark"
  },
  "buffer_font_family": "JetBrains Mono",
  "buffer_font_size": 14,
  "tab_size": 2,
  "format_on_save": "on",
  "autosave": "on_focus_change",
  "vim_mode": false,
  "terminal": {
    "font_family": "JetBrains Mono",
    "font_size": 14
  },
  "languages": {
    "Python": {
      "tab_size": 4
    }
  }
}
```

## Tiếp theo

- [Giao diện](./appearance.md) — theme, font và tuỳ biến hiển thị
- [Phím tắt](./key-bindings.md) — tuỳ chỉnh phím tắt
- [Mọi cài đặt](./reference/all-settings.md) — tham chiếu đầy đủ các cài đặt
