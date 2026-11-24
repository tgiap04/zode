---
title: Diagnostics - Errors and Warnings in Zode
description: View and navigate errors, warnings, and code diagnostics from language servers in Zode.
---

# Chẩn đoán

Zode lấy chẩn đoán từ các language server và hỗ trợ cả biến thể push lẫn pull của LSP, nên tương thích với mọi language server hiện có.

# Chẩn đoán thông thường

Mặc định, Zode hiển thị mọi chẩn đoán dưới dạng chữ gạch chân trong trình soạn thảo và trên thanh cuộn.

Có thể lọc chẩn đoán trong trình soạn thảo bằng

```json [settings]
"diagnostics_max_severity": null
```

cài đặt (giá trị có thể dùng: `"off"`, `"error"`, `"warning"`, `"info"`, `"hint"`, `null` (mặc định, hiện mọi chẩn đoán)).

Phần hiển thị trên thanh cuộn được cấu hình bằng

```json [settings]
"scrollbar": {
  "diagnostics": "all",
}
```

cấu hình (giá trị có thể dùng: `"none"`, `"error"`, `"warning"`, `"information"`, `"all"` (mặc định))

Di chuột lên chẩn đoán để hiện tooltip chứa thông báo chẩn đoán đầy đủ, đã được hiển thị đẹp.
Hoặc dùng `editor::GoToDiagnostic` và `editor::GoToPreviousDiagnostic` để di chuyển giữa các chẩn đoán trong trình soạn thảo, kèm popover cho chẩn đoán đang chọn.

# Chẩn đoán nội tuyến (Error lens)

Zode hỗ trợ hiển thị chẩn đoán dạng lens ở bên phải mã.
Mặc định tính năng này tắt, nhưng có thể bật (hoặc tắt) tạm thời bằng menu của trình soạn thảo, hoặc bật vĩnh viễn bằng

```json [settings]
"diagnostics": {
  "inline": {
    "enabled": true,
    "max_severity": null, // same values as the `diagnostics_max_severity` from the editor settings
  }
}
```

# Các vị trí giao diện khác

## Bảng Project

Bảng Project có thể tô màu các mục theo mức độ nghiêm trọng của chẩn đoán trong file.

Để cấu hình, dùng

```json [settings]
"project_panel": {
  "show_diagnostics": "all",
}
```

cấu hình (giá trị có thể dùng: `"off"`, `"errors"`, `"all"` (mặc định))

## Tab của trình soạn thảo

Tương tự bảng Project, tab của trình soạn thảo có thể được tô màu bằng

```json [settings]
"tabs": {
  "show_diagnostics": "off",
}
```

cấu hình (giá trị có thể dùng: `"off"` (mặc định), `"errors"`, `"all"`)
