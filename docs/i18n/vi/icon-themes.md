---
title: Icon Themes
description: "Zode's file and folder icons come from the Material Icon Theme, in colour, and cannot be changed."
---

# Bộ biểu tượng (Icon Theme)

Zode đi kèm một bộ biểu tượng duy nhất — [Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme), hiển thị đúng màu gốc — và không cho chọn bộ biểu tượng khác. Không có Icon Theme Selector, và không bộ biểu tượng nào khác có hiệu lực, dù được ghi trong cài đặt hay do extension cung cấp.

Việc khớp tên file và thư mục với biểu tượng không phân biệt hoa thường, nên các tên như `Dockerfile`, `README.MD` hay `.PNG` vẫn ra đúng biểu tượng.

## Cài đặt `icon_theme`

Cài đặt `icon_theme` vẫn còn trong schema cài đặt — kể cả dạng object cho biến thể sáng/tối — vì cơ chế cài đặt luôn phân giải nó và thiếu khoá sẽ làm ứng dụng panic lúc khởi động. Tên nào bạn ghi ở đây cũng không ảnh hưởng đến biểu tượng được vẽ:

```json [settings]
{
  "icon_theme": "Material Icon Theme"
}
```

Đây là giá trị mặc định đi kèm. Ghi tên khác, hoặc dạng object `{ "mode", "light", "dark" }`, không làm đổi biểu tượng hiển thị.

## Phát triển bộ biểu tượng

Extension vẫn có thể khai báo thư mục `icon_themes` (xem [Phát triển bộ biểu tượng cho Zode](./extensions/icon-themes.md)), nhưng bộ biểu tượng do extension cung cấp không bao giờ được chọn — biểu tượng file là một phần của bản build này, không phải tuỳ chọn của người dùng.
