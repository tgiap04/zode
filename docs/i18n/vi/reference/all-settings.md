---
title: All Settings
description: "Complete reference for all Zode settings."
---

# Tất cả cài đặt

Đây là tài liệu tham chiếu đầy đủ về mọi cài đặt của Zode.

Có thể bạn cũng muốn đổi [giao diện (theme)](../themes.md), cấu hình [phím tắt](../key-bindings.md), thiết lập [task](../tasks.md) hoặc cài [extension](../extensions.md).

# Cài đặt

Các phần dưới đây mô tả những cài đặt Zode hỗ trợ.

## Bộ chỉnh khung đang hoạt động

- Mô tả: Cài đặt kiểu dáng áp dụng cho khung đang hoạt động.
- Cài đặt: `active_pane_modifiers`
- Mặc định:

```json [settings]
{
  "active_pane_modifiers": {
    "border_size": 0.0,
    "inactive_opacity": 1.0
  }
}
```

### Độ dày viền

- Mô tả: Độ dày viền quanh khung đang hoạt động. Đặt bằng 0 thì khung đang hoạt động không có viền. Viền được vẽ lùi vào trong.
- Cài đặt: `border_size`
- Mặc định: `0.0`

**Tùy chọn**

Giá trị `float` không âm

### Độ mờ khi không hoạt động

- Mô tả: Độ mờ của các khung không hoạt động. Đặt bằng 1.0 thì khung không hoạt động có cùng độ mờ với khung đang hoạt động. Đặt bằng 0 thì nội dung khung không hoạt động không hiện ra. Giá trị được giới hạn trong khoảng [0.0, 1.0].
- Cài đặt: `inactive_opacity`
- Mặc định: `1.0`

**Tùy chọn**

Giá trị `float`

## Bố cục dock dưới

- Mô tả: Điều khiển bố cục của dock dưới so với dock trái và dock phải.
- Cài đặt: `bottom_dock_layout`
- Mặc định: `"contained"`

**Tùy chọn**

1. Gói dock dưới lại, dành toàn bộ chiều cao cửa sổ cho dock trái và dock phải.

```json [settings]
{
  "bottom_dock_layout": "contained"
}
```

2. Cho dock dưới chiếm toàn bộ chiều rộng cửa sổ, cắt bớt dock trái và dock phải.

```json [settings]
{
  "bottom_dock_layout": "full"
}
```

3. Căn dock dưới sang trái, cắt bớt dock trái và dành toàn bộ chiều cao cửa sổ cho dock phải.

```json [settings]
{
  "bottom_dock_layout": "left_aligned"
}
```

4. Căn dock dưới sang phải, dành toàn bộ chiều cao cửa sổ cho dock trái và cắt bớt dock phải.

```json [settings]
{
  "bottom_dock_layout": "right_aligned"
}
```

## Cho phép rewrap

- Mô tả: Điều khiển nơi action {#action editor::Rewrap} được phép chạy trong phạm vi ngôn ngữ hiện tại
- Cài đặt: `allow_rewrap`
- Mặc định: `"in_comments"`

**Tùy chọn**

1. Chỉ cho phép rewrap trong comment:

```json [settings]
{
  "allow_rewrap": "in_comments"
}
```

2. Chỉ cho phép rewrap trong vùng chọn:

```json [settings]
{
  "allow_rewrap": "in_selections"
}
```

3. Cho phép rewrap ở mọi nơi:

```json [settings]
{
  "allow_rewrap": "anywhere"
}
```

Lưu ý: Cài đặt này không có tác dụng trong chế độ Vim vì rewrap vốn đã được phép ở mọi nơi.

## Tự động thụt lề

- Mô tả: Có tự điều chỉnh thụt lề theo ngữ cảnh khi gõ hay không. Có thể chỉ định riêng cho từng ngôn ngữ.
- Cài đặt: `auto_indent`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Tự động thụt lề khi dán

- Mô tả: Có tự điều chỉnh thụt lề của nội dung dán theo ngữ cảnh hay không
- Cài đặt: `auto_indent_on_paste`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Tự động cài extension

- Mô tả: Khai báo những extension sẽ tự động cài hoặc không bao giờ cài.
- Cài đặt: `auto_install_extensions`
- Mặc định: `{ "html": true }`

**Tùy chọn**

Bạn có thể xem tên các extension đã cài bằng cách liệt kê các thư mục con trong [vị trí cài đặt extension](../extensions/installing-extensions.md#installation-location):

Trên macOS:

```sh
ls ~/Library/Application\ Support/Zode/extensions/installed/
```

Trên Linux:

```sh
ls ~/.local/share/zode/extensions/installed
```

Khai báo những extension cần cài (`true`) hoặc không bao giờ cài (`false`).

```json [settings]
{
  "auto_install_extensions": {
    "html": true,
    "dockerfile": true,
    "docker-compose": false
  }
}
```

## Tự động lưu

- Mô tả: Khi nào tự động lưu các buffer đã sửa.
- Cài đặt: `autosave`
- Mặc định: `off`

**Tùy chọn**

1. Để tắt tự động lưu, đặt thành `off`:

```json [settings]
{
  "autosave": "off"
}
```

2. Để tự lưu khi đổi tiêu điểm, dùng `on_focus_change`:

```json [settings]
{
  "autosave": "on_focus_change"
}
```

3. Để tự lưu khi cửa sổ đang hoạt động thay đổi, dùng `on_window_change`:

```json [settings]
{
  "autosave": "on_window_change"
}
```

4. Để tự lưu sau một khoảng thời gian không thao tác, dùng `after_delay`:

```json [settings]
{
  "autosave": {
    "after_delay": {
      "milliseconds": 1000
    }
  }
}
```

Lưu ý: tab chưa lưu sẽ được lưu khi đóng, kể cả khi sớm hơn khoảng thời gian không thao tác đã cấu hình.

## Tự cuộn khi bấm chuột

- Mô tả: Có cuộn khi bấm gần mép vùng văn bản đang hiển thị hay không.
- Cài đặt: `autoscroll_on_clicks`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Tự động gợi ý chữ ký

- Mô tả: Hiện chữ ký phương thức trong trình soạn thảo khi con trỏ nằm trong dấu ngoặc đơn.
- Cài đặt: `auto_signature_help`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

### Hiện gợi ý chữ ký sau khi sửa

- Mô tả: Có hiện gợi ý chữ ký sau khi hoàn thành mã hoặc sau khi chèn một cặp ngoặc hay không. Nếu `auto_signature_help` đang bật thì cài đặt này cũng được coi là bật.
- Cài đặt: `show_signature_help_after_edits`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Keymap cơ sở

- Mô tả: Bộ phím tắt cơ sở. Keymap cơ sở có thể bị keymap của người dùng ghi đè.
- Cài đặt: `base_keymap`
- Mặc định: `VSCode`

**Tùy chọn**

1. VS Code

```json [settings]
{
  "base_keymap": "VSCode"
}
```

2. Atom

```json [settings]
{
  "base_keymap": "Atom"
}
```

3. JetBrains

```json [settings]
{
  "base_keymap": "JetBrains"
}
```

4. Không dùng

```json [settings]
{
  "base_keymap": "None"
}
```

5. Sublime Text

```json [settings]
{
  "base_keymap": "SublimeText"
}
```

6. TextMate

```json [settings]
{
  "base_keymap": "TextMate"
}
```

## Font của buffer

- Mô tả: Tên font dùng để hiển thị văn bản trong trình soạn thảo.
- Cài đặt: `buffer_font_family`
- Mặc định: `SF Mono` trên macOS, có sẵn trong hệ thống; `JetBrains Mono`, đi kèm Zode, trên các hệ điều hành khác.

**Tùy chọn**

Tên của bất kỳ họ font nào đã cài trên máy người dùng, hoặc `".ZodeMono"`.

## Tính năng font của buffer

- Mô tả: Các tính năng OpenType được bật cho văn bản trong trình soạn thảo.
- Cài đặt: `buffer_font_features`
- Mặc định: `null`
- Nền tảng: macOS và Windows.

**Tùy chọn**

Zode hỗ trợ mọi tính năng OpenType có thể bật hoặc tắt cho font của buffer hoặc terminal, cũng như đặt giá trị cho các tính năng font.

Ví dụ, để tắt ligature của font, thêm đoạn sau vào cài đặt:

```json [settings]
{
  "buffer_font_features": {
    "calt": false
  }
}
```

Bạn cũng có thể đặt các tính năng OpenType khác, chẳng hạn đặt `cv01` thành `7`:

```json [settings]
{
  "buffer_font_features": {
    "cv01": 7
  }
}
```

## Font dự phòng của buffer

- Mô tả: Đặt font dự phòng cho văn bản trong buffer, sẽ được gộp với font dự phòng mặc định của nền tảng.
- Cài đặt: `buffer_font_fallbacks`
- Mặc định: `null`
- Nền tảng: macOS và Windows.

**Tùy chọn**

Ví dụ, để dùng `Nerd Font` làm font dự phòng, thêm đoạn sau vào cài đặt:

```json [settings]
{
  "buffer_font_fallbacks": ["Nerd Font"]
}
```

## Cỡ chữ của buffer

- Mô tả: Cỡ chữ mặc định cho văn bản trong trình soạn thảo.
- Cài đặt: `buffer_font_size`
- Mặc định: `15`

**Tùy chọn**

Cỡ chữ từ `6` đến `100` pixel (gồm cả hai đầu)

## Độ đậm font của buffer

- Mô tả: Độ đậm font mặc định cho văn bản trong trình soạn thảo.
- Cài đặt: `buffer_font_weight`
- Mặc định: `400`

**Tùy chọn**

Giá trị `integer` từ `100` đến `900`

## Chiều cao dòng của buffer

- Mô tả: Chiều cao dòng mặc định cho văn bản trong trình soạn thảo.
- Cài đặt: `buffer_line_height`
- Mặc định: `"comfortable"`

**Tùy chọn**

`"standard"`, `"comfortable"` hoặc `{ "custom": float }` (`1` là sát nhau, `2` là thưa)

## Bố cục căn giữa

- Mô tả: Cấu hình cho chế độ bố cục căn giữa.
- Cài đặt: `centered_layout`
- Mặc định:

```json [settings]
{
  "centered_layout": {
    "left_padding": 0.2,
    "right_padding": 0.2
  }
}
```

**Tùy chọn**

Hai tùy chọn `left_padding` và `right_padding` xác định chiều rộng tương đối của phần đệm trái và phải
quanh khung trung tâm trong workspace khi bật chế độ bố cục căn giữa. Giá trị hợp lệ nằm trong khoảng từ `0` đến `0.4`.

## Đóng khi file bị xóa

- Mô tả: Có tự động đóng tab trình soạn thảo khi file tương ứng bị xóa khỏi đĩa hay không.
- Cài đặt: `close_on_file_delete`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

Khi bật, cài đặt này tự động đóng các tab của file đã bị xóa khỏi hệ thống file. Rất hữu ích với quy trình dùng file tạm hoặc file nháp được tạo và xóa liên tục. Khi tắt (mặc định), file đã xóa vẫn mở và tiêu đề tab bị gạch ngang.

Lưu ý: File có thay đổi chưa lưu sẽ không bị tự đóng, kể cả khi bật cài đặt này, để bạn không mất phần đang làm dở.

## Xác nhận khi thoát

- Mô tả: Có hỏi người dùng xác nhận trước khi đóng ứng dụng hay không.
- Cài đặt: `confirm_quit`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Mức độ nghiêm trọng tối đa của chẩn đoán

- Mô tả: Mức dùng để lọc các chẩn đoán hiển thị trong trình soạn thảo
- Cài đặt: `diagnostics_max_severity`
- Mặc định: `null`

**Tùy chọn**

1. Cho phép mọi chẩn đoán (mặc định):

```json [settings]
{
  "diagnostics_max_severity": "all"
}
```

2. Chỉ hiện lỗi:

```json [settings]
{
  "diagnostics_max_severity": "error"
}
```

3. Hiện lỗi và cảnh báo:

```json [settings]
{
  "diagnostics_max_severity": "warning"
}
```

4. Hiện lỗi, cảnh báo và thông tin:

```json [settings]
{
  "diagnostics_max_severity": "info"
}
```

5. Hiện tất cả, kể cả gợi ý:

```json [settings]
{
  "diagnostics_max_severity": "hint"
}
```

## Kiểu xem diff

- Mô tả: Cách hiển thị diff trong trình soạn thảo.
- Cài đặt: `diff_view_style`
- Mặc định: `"split"`

**Tùy chọn**

- `"unified"`: Hiện thay đổi liền mạch, dòng thêm và dòng xóa xếp chồng theo chiều dọc
- `"split"`: Hiện bản cũ và bản mới cạnh nhau trong hai khung riêng (mặc định)

Xem [tài liệu Git](../git.md#diff-view-styles) để biết thêm chi tiết.

## Tắt AI

- Mô tả: Có tắt toàn bộ tính năng AI trong Zode hay không
- Cài đặt: `disable_ai`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Tích hợp direnv

- Mô tả: Cài đặt cho tích hợp [direnv](https://direnv.net/). Yêu cầu đã cài `direnv`.
  Tích hợp `direnv` cho phép dùng các biến môi trường do cấu hình `direnv` đặt để tìm một số language server trong `$PATH` thay vì cài chúng.
  Nó cũng cho phép dùng các biến môi trường đó trong task.
- Cài đặt: `load_direnv`
- Mặc định: `"direct"`

**Tùy chọn**

Có ba lựa chọn:

1. `shell_hook`: Dùng shell hook để nạp direnv. Cách này dựa vào direnv tự kích hoạt khi vào thư mục. Hỗ trợ shell POSIX và fish.
2. `direct`: Dùng `direnv export json` để nạp direnv. Cách này nạp direnv trực tiếp, không dựa vào shell hook và có thể gây vài điểm không nhất quán. Nhờ vậy direnv chạy được với mọi shell.
3. `disabled`: Không tự động nạp môi trường shell nào; phải gọi direnv thủ công (ví dụ `direnv exec`) để dùng.

## Bấm đúp trong multibuffer

- Mô tả: Việc cần làm khi bấm đúp vào multibuffer ở một trong các đoạn trích của nó (phần của các buffer đơn)
- Cài đặt: `double_click_in_multibuffer`
- Mặc định: `"select"`

**Tùy chọn**

1. Hành xử như buffer thường và chọn cả từ (mặc định):

```json [settings]
{
  "double_click_in_multibuffer": "select"
}
```

2. Mở đoạn trích được bấm thành một buffer mới trong tab mới:

```json [settings]
{
  "double_click_in_multibuffer": "open"
}
```

Với trường hợp "open", muốn có hành vi chọn thông thường thì giữ `alt` khi bấm đúp.

## Kích thước vùng thả

- Mô tả: Kích thước tương đối của vùng thả trong trình soạn thảo, nơi file được thả vào sẽ mở thành khung chia (0-0.5). Ví dụ, 0.25 nghĩa là thả vào một phần tư phía trên/dưới của khung thì chia dọc, thả vào một phần tư bên trái/phải của khung thì chia ngang.
- Cài đặt: `drop_target_size`
- Mặc định: `0.2`

**Tùy chọn**

Giá trị `float` từ `0` đến `0.5`

## Dự đoán chỉnh sửa

- Mô tả: Cài đặt cho dự đoán chỉnh sửa.
- Cài đặt: `edit_predictions`
- Mặc định:

```json [settings]
  "edit_predictions": {
    "disabled_globs": [
      "**/.env*",
      "**/*.pem",
      "**/*.key",
      "**/*.cert",
      "**/*.crt",
      "**/.dev.vars",
      "**/secrets.yml"
    ]
  }
```

**Tùy chọn**

### Glob bị tắt

- Mô tả: Danh sách glob mà dự đoán chỉnh sửa bị tắt. Danh sách này được cộng thêm vào bộ glob mặc định có sẵn; những glob bạn thêm sẽ được gộp với chúng.
- Cài đặt: `disabled_globs`
- Mặc định: `["**/.env*", "**/*.pem", "**/*.key", "**/*.cert", "**/*.crt", "**/.dev.vars", "**/secrets.yml"]`

**Tùy chọn**

Danh sách giá trị `string`.

## Tắt dự đoán chỉnh sửa trong

- Mô tả: Danh sách phạm vi ngôn ngữ mà dự đoán chỉnh sửa bị tắt.
- Cài đặt: `edit_predictions_disabled_in`
- Mặc định: `[]`

**Tùy chọn**

Danh sách giá trị `string`

1. Không hiện dự đoán chỉnh sửa trong comment:

```json [settings]
{
  "edit_predictions_disabled_in": ["comment"]
}
```

2. Không hiện dự đoán chỉnh sửa trong chuỗi và comment:

```json [settings]
{
  "edit_predictions_disabled_in": ["comment", "string"]
}
```

3. Chỉ với Go, không hiện dự đoán chỉnh sửa trong chuỗi và comment:

```json [settings]
{
  "languages": {
    "Go": {
      "edit_predictions_disabled_in": ["comment", "string"]
    }
  }
}
```

## Tô sáng dòng hiện tại

- Mô tả: Cách tô sáng dòng hiện tại trong trình soạn thảo.
- Cài đặt: `current_line_highlight`
- Mặc định: `all`

**Tùy chọn**

1. Không tô sáng dòng hiện tại:

```json [settings]
{
  "current_line_highlight": "none"
}
```

2. Tô sáng vùng gutter:

```json [settings]
{
  "current_line_highlight": "gutter"
}
```

3. Tô sáng vùng soạn thảo:

```json [settings]
{
  "current_line_highlight": "line"
}
```

4. Tô sáng cả dòng:

```json [settings]
{
  "current_line_highlight": "all"
}
```

## Tô sáng vùng chọn

- Mô tả: Có tô sáng mọi chỗ xuất hiện của văn bản đang chọn trong trình soạn thảo hay không.
- Cài đặt: `selection_highlight`
- Mặc định: `true`

## Vùng chọn bo góc

- Mô tả: Vùng chọn văn bản có bo tròn góc hay không.
- Cài đặt: `rounded_selection`
- Mặc định: `true`

## Con trỏ nhấp nháy

- Mô tả: Con trỏ có nhấp nháy hay không.
- Cài đặt: `cursor_blink`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hình dạng con trỏ

- Mô tả: Hình dạng con trỏ cho trình soạn thảo mặc định.
- Cài đặt: `cursor_shape`
- Mặc định: `bar`

**Tùy chọn**

1. Thanh dọc:

```json [settings]
{
  "cursor_shape": "bar"
}
```

2. Khối bao quanh ký tự phía sau:

```json [settings]
{
  "cursor_shape": "block"
}
```

3. Gạch chân chạy dọc theo ký tự phía sau:

```json [settings]
{
  "cursor_shape": "underline"
}
```

4. Khung vẽ quanh ký tự phía sau:

```json [settings]
{
  "cursor_shape": "hollow"
}
```

## Gutter

- Mô tả: Cài đặt cho gutter của trình soạn thảo
- Cài đặt: `gutter`
- Mặc định:

```json [settings]
{
  "gutter": {
    "line_numbers": true,
    "runnables": true,
    "breakpoints": true,
    "folds": true,
    "min_line_number_digits": 4
  }
}
```

**Tùy chọn**

- `line_numbers`: Có hiện số dòng trong gutter hay không
- `runnables`: Có hiện nút chạy trong gutter hay không
- `breakpoints`: Có hiện breakpoint trong gutter hay không
- `folds`: Có hiện nút thu gọn trong gutter hay không
- `min_line_number_digits`: Số ký tự tối thiểu dành chỗ trong gutter

## Ẩn con trỏ chuột

- Mô tả: Xác định khi nào ẩn con trỏ chuột trong trình soạn thảo hoặc ô nhập liệu.
- Cài đặt: `hide_mouse`
- Mặc định: `on_typing_and_movement`

**Tùy chọn**

1. Không bao giờ ẩn con trỏ chuột:

```json [settings]
{
  "hide_mouse": "never"
}
```

2. Chỉ ẩn khi đang gõ:

```json [settings]
{
  "hide_mouse": "on_typing"
}
```

3. Ẩn khi gõ và khi di chuyển con trỏ:

```json [settings]
{
  "hide_mouse": "on_typing_and_movement"
}
```

## Thứ tự sắp xếp snippet

- Mô tả: Xác định cách sắp xếp snippet so với các mục hoàn thành mã khác.
- Cài đặt: `snippet_sort_order`
- Mặc định: `inline`

**Tùy chọn**

1. Đặt snippet ở đầu danh sách hoàn thành mã:

```json [settings]
{
  "snippet_sort_order": "top"
}
```

2. Đặt snippet bình thường, không ưu tiên:

```json [settings]
{
  "snippet_sort_order": "inline"
}
```

3. Đặt snippet ở cuối danh sách hoàn thành mã:

```json [settings]
{
  "snippet_sort_order": "bottom"
}
```

4. Không hiện snippet trong danh sách hoàn thành mã:

```json [settings]
{
  "snippet_sort_order": "none"
}
```

## Thanh cuộn của trình soạn thảo

- Mô tả: Có hiện thanh cuộn của trình soạn thảo và các thành phần trên nó hay không.
- Cài đặt: `scrollbar`
- Mặc định:

```json [settings]
{
  "scrollbar": {
    "show": "auto",
    "cursors": true,
    "git_diff": true,
    "search_results": true,
    "selected_text": true,
    "selected_symbol": true,
    "diagnostics": "all",
    "axes": {
      "horizontal": true,
      "vertical": true
    }
  }
}
```

### Chế độ hiển thị

- Mô tả: Khi nào hiện thanh cuộn của trình soạn thảo.
- Cài đặt: `show`
- Mặc định: `auto`

**Tùy chọn**

1. Hiện thanh cuộn nếu có thông tin quan trọng hoặc theo hành vi hệ thống đã cấu hình:

```json [settings]
{
  "scrollbar": {
    "show": "auto"
  }
}
```

2. Theo hành vi hệ thống đã cấu hình:

```json [settings]
{
  "scrollbar": {
    "show": "system"
  }
}
```

3. Luôn hiện thanh cuộn:

```json [settings]
{
  "scrollbar": {
    "show": "always"
  }
}
```

4. Không bao giờ hiện thanh cuộn:

```json [settings]
{
  "scrollbar": {
    "show": "never"
  }
}
```

### Chỉ báo con trỏ

- Mô tả: Có hiện vị trí con trỏ trên thanh cuộn hay không.
- Cài đặt: `cursors`
- Mặc định: `true`

Chỉ báo con trỏ hiện dưới dạng các dấu nhỏ trên thanh cuộn, cho biết con trỏ của các cộng tác viên khác đang ở đâu trong file.

**Tùy chọn**

Giá trị `boolean`

### Chỉ báo git diff

- Mô tả: Có hiện chỉ báo git diff trên thanh cuộn hay không.
- Cài đặt: `git_diff`
- Mặc định: `true`

Chỉ báo git diff hiện dưới dạng các dấu màu cho biết dòng được thêm, sửa hoặc xóa so với git HEAD.

**Tùy chọn**

Giá trị `boolean`

### Chỉ báo kết quả tìm kiếm

- Mô tả: Có hiện kết quả tìm kiếm trong buffer trên thanh cuộn hay không.
- Cài đặt: `search_results`
- Mặc định: `true`

Chỉ báo kết quả tìm kiếm hiện dưới dạng các dấu cho biết mọi vị trí trong file khớp với truy vấn tìm kiếm hiện tại.

**Tùy chọn**

Giá trị `boolean`

### Chỉ báo văn bản đang chọn

- Mô tả: Có hiện các chỗ xuất hiện của văn bản đang chọn trên thanh cuộn hay không.
- Cài đặt: `selected_text`
- Mặc định: `true`

Chỉ báo văn bản đang chọn hiện dưới dạng các dấu cho biết mọi chỗ xuất hiện của văn bản đang chọn trong file.

**Tùy chọn**

Giá trị `boolean`

### Chỉ báo symbol đang chọn

- Mô tả: Có hiện các chỗ xuất hiện của symbol đang chọn trên thanh cuộn hay không.
- Cài đặt: `selected_symbol`
- Mặc định: `true`

Chỉ báo symbol đang chọn hiện dưới dạng các dấu cho biết mọi chỗ xuất hiện của symbol đang chọn (như tên hàm hoặc biến) trong file.

**Tùy chọn**

Giá trị `boolean`

### Chẩn đoán

- Mô tả: Hiện những chỉ báo chẩn đoán nào trên thanh cuộn.
- Cài đặt: `diagnostics`
- Mặc định: `all`

Chỉ báo chẩn đoán hiện dưới dạng các dấu màu cho biết lỗi, cảnh báo và các chẩn đoán khác của language server tại dòng tương ứng trong file.

**Tùy chọn**

1. Hiện mọi chẩn đoán:

```json [settings]
{
  "scrollbar": {
    "diagnostics": "all"
  }
}
```

2. Không hiện chẩn đoán nào:

```json [settings]
{
  "scrollbar": {
    "diagnostics": "none"
  }
}
```

3. Chỉ hiện lỗi:

```json [settings]
{
  "scrollbar": {
    "diagnostics": "error"
  }
}
```

4. Chỉ hiện lỗi và cảnh báo:

```json [settings]
{
  "scrollbar": {
    "diagnostics": "warning"
  }
}
```

5. Chỉ hiện lỗi, cảnh báo và thông tin:

```json [settings]
{
  "scrollbar": {
    "diagnostics": "information"
  }
}
```

### Trục

- Mô tả: Buộc bật hoặc tắt thanh cuộn cho từng trục
- Cài đặt: `axes`
- Mặc định:

```json [settings]
{
  "scrollbar": {
    "axes": {
      "horizontal": true,
      "vertical": true
    }
  }
}
```

#### Ngang

- Mô tả: Khi là false, buộc tắt thanh cuộn ngang. Nếu không, theo các cài đặt khác.
- Cài đặt: `horizontal`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

#### Dọc

- Mô tả: Khi là false, buộc tắt thanh cuộn dọc. Nếu không, theo các cài đặt khác.
- Cài đặt: `vertical`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Minimap

- Mô tả: Cài đặt liên quan đến minimap của trình soạn thảo, cho bạn cái nhìn tổng quan về tài liệu.
- Cài đặt: `minimap`
- Mặc định:

```json [settings]
{
  "minimap": {
    "show": "never",
    "thumb": "always",
    "thumb_border": "left_open",
    "current_line_highlight": null
  }
}
```

### Chế độ hiển thị

- Mô tả: Khi nào hiện minimap trong trình soạn thảo.
- Cài đặt: `show`
- Mặc định: `never`

**Tùy chọn**

1. Luôn hiện minimap:

```json [settings]
{
  "minimap": {
    "show": "always"
  }
}
```

2. Hiện minimap nếu thanh cuộn của trình soạn thảo đang hiện:

```json [settings]
{
  "minimap": {
    "show": "auto"
  }
}
```

3. Không bao giờ hiện minimap:

```json [settings]
{
  "minimap": {
    "show": "never"
  }
}
```

### Hiển thị thumb

- Mô tả: Khi nào hiện thumb của minimap (vùng soạn thảo đang nhìn thấy) trong minimap.
- Cài đặt: `thumb`
- Mặc định: `always`

**Tùy chọn**

1. Hiện thumb khi rê chuột lên minimap:

```json [settings]
{
  "minimap": {
    "thumb": "hover"
  }
}
```

2. Luôn hiện thumb:

```json [settings]
{
  "minimap": {
    "thumb": "always"
  }
}
```

### Viền thumb

- Mô tả: Viền của thumb minimap trông như thế nào.
- Cài đặt: `thumb_border`
- Mặc định: `left_open`

**Tùy chọn**

1. Hiện viền cả bốn phía của thumb:

```json [settings]
{
  "minimap": {
    "thumb_border": "full"
  }
}
```

2. Hiện viền ở mọi phía trừ phía trái:

```json [settings]
{
  "minimap": {
    "thumb_border": "left_open"
  }
}
```

3. Hiện viền ở mọi phía trừ phía phải:

```json [settings]
{
  "minimap": {
    "thumb_border": "right_open"
  }
}
```

4. Chỉ hiện viền ở phía trái:

```json [settings]
{
  "minimap": {
    "thumb_border": "left_only"
  }
}
```

5. Hiện thumb không có viền:

```json [settings]
{
  "minimap": {
    "thumb_border": "none"
  }
}
```

### Tô sáng dòng hiện tại

- Mô tả: Cách tô sáng dòng hiện tại trong minimap.
- Cài đặt: `current_line_highlight`
- Mặc định: `null`

**Tùy chọn**

1. Kế thừa cài đặt tô sáng dòng hiện tại của trình soạn thảo:

```json [settings]
{
  "minimap": {
    "current_line_highlight": null
  }
}
```

2. Tô sáng dòng hiện tại trong minimap:

```json [settings]
{
  "minimap": {
    "current_line_highlight": "line"
  }
}
```

hoặc

```json [settings]
{
  "minimap": {
    "current_line_highlight": "all"
  }
}
```

3. Không tô sáng dòng hiện tại trong minimap:

```json [settings]
{
  "minimap": {
    "current_line_highlight": "gutter"
  }
}
```

hoặc

```json [settings]
{
  "minimap": {
    "current_line_highlight": "none"
  }
}
```

## Thanh tab của trình soạn thảo

- Mô tả: Cài đặt liên quan đến thanh tab của trình soạn thảo.
- Cài đặt: `tab_bar`
- Mặc định:

```json [settings]
{
  "tab_bar": {
    "show": true,
    "show_nav_history_buttons": true,
    "show_tab_bar_buttons": true
  }
}
```

### Hiển thị

- Mô tả: Có hiện thanh tab trong trình soạn thảo hay không.
- Cài đặt: `show`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Nút lịch sử điều hướng

- Mô tả: Có hiện các nút lịch sử điều hướng hay không.
- Cài đặt: `show_nav_history_buttons`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Nút trên thanh tab

- Mô tả: Có hiện các nút trên thanh tab hay không.
- Cài đặt: `show_tab_bar_buttons`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Tab của trình soạn thảo

- Mô tả: Cấu hình cho các tab của trình soạn thảo.
- Cài đặt: `tabs`
- Mặc định:

```json [settings]
{
  "tabs": {
    "close_position": "right",
    "file_icons": false,
    "git_status": false,
    "activate_on_close": "history",
    "show_close_button": "hover",
    "show_diagnostics": "off"
  }
}
```

### Vị trí nút đóng

- Mô tả: Vị trí hiện nút đóng trong một tab.
- Cài đặt: `close_position`
- Mặc định: `right`

**Tùy chọn**

1. Hiện nút đóng ở bên phải:

```json [settings]
{
  "tabs": {
    "close_position": "right"
  }
}
```

2. Hiện nút đóng ở bên trái:

```json [settings]
{
  "tabs": {
    "close_position": "left"
  }
}
```

### Icon file

- Mô tả: Có hiện icon file cho tab hay không.
- Cài đặt: `file_icons`
- Mặc định: `false`

### Trạng thái Git

- Mô tả: Có hiện trạng thái Git của file trong tab hay không.
- Cài đặt: `git_status`
- Mặc định: `false`

### Kích hoạt khi đóng

- Mô tả: Việc cần làm sau khi đóng tab hiện tại.
- Cài đặt: `activate_on_close`
- Mặc định: `history`

**Tùy chọn**

1.  Kích hoạt tab đã mở trước đó:

```json [settings]
{
  "tabs": {
    "activate_on_close": "history"
  }
}
```

2. Kích hoạt tab lân cận bên phải nếu có:

```json [settings]
{
  "tabs": {
    "activate_on_close": "neighbour"
  }
}
```

3. Kích hoạt tab lân cận bên trái nếu có:

```json [settings]
{
  "tabs": {
    "activate_on_close": "left_neighbour"
  }
}
```

### Hiện nút đóng

- Mô tả: Điều khiển cách hiện nút đóng của tab.
- Cài đặt: `show_close_button`
- Mặc định: `hover`

**Tùy chọn**

1.  Chỉ hiện khi rê chuột lên tab:

```json [settings]
{
  "tabs": {
    "show_close_button": "hover"
  }
}
```

2. Luôn hiện:

```json [settings]
{
  "tabs": {
    "show_close_button": "always"
  }
}
```

3. Không bao giờ hiện, kể cả khi rê chuột lên nó:

```json [settings]
{
  "tabs": {
    "show_close_button": "hidden"
  }
}
```

### Hiện chẩn đoán

- Mô tả: Có hiện chỉ báo chẩn đoán trong tab hay không. Cài đặt này chỉ hoạt động khi icon file đang bật, và điều khiển việc đánh dấu những file có vấn đề chẩn đoán nào.
- Cài đặt: `show_diagnostics`
- Mặc định: `off`

**Tùy chọn**

1. Không đánh dấu file nào:

```json [settings]
{
  "tabs": {
    "show_diagnostics": "off"
  }
}
```

2. Chỉ đánh dấu file có lỗi:

```json [settings]
{
  "tabs": {
    "show_diagnostics": "errors"
  }
}
```

3. Đánh dấu file có lỗi và cảnh báo:

```json [settings]
{
  "tabs": {
    "show_diagnostics": "all"
  }
}
```

### Hiện code action ngay trong dòng

- Mô tả: Có hiện nút code action ở đầu dòng của buffer hay không.
- Cài đặt: `inline_code_actions`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Phiên làm việc

- Mô tả: Điều khiển hành vi liên quan đến vòng đời của Zode.
- Cài đặt: `session`
- Mặc định:

```json
{
  "session": {
    "restore_unsaved_buffers": true,
    "trust_all_worktrees": false
  }
}
```

**Tùy chọn**

1.  Có khôi phục các buffer chưa lưu khi khởi động lại hay không:

```json [settings]
{
  "session": {
    "restore_unsaved_buffers": true
  }
}
```

Nếu là true, người dùng sẽ không bị hỏi lưu hay bỏ các file đã sửa khi đóng ứng dụng.

2. Có bỏ qua kiểm tra tin cậy worktree và workspace hay không:

```json [settings]
{
  "session": {
    "trust_all_worktrees": false
  }
}
```

Khi được tin cậy, cài đặt dự án được đồng bộ tự động, còn language server và máy chủ MCP được tải về và khởi chạy tự động.

### Kéo thả vùng chọn

- Mô tả: Có cho phép kéo thả vùng văn bản đã chọn trong buffer hay không. `delay` là số mili giây phải trôi qua trước khi cho phép kéo thả. Nếu chưa đủ, một vùng chọn mới sẽ được tạo.
- Cài đặt: `drag_and_drop_selection`
- Mặc định:

```json [settings]
{
  "drag_and_drop_selection": {
    "enabled": true,
    "delay": 300
  }
}
```

## Thanh công cụ của trình soạn thảo

- Mô tả: Có hiện các thành phần khác nhau trên thanh công cụ của trình soạn thảo hay không.
- Cài đặt: `toolbar`
- Mặc định:

```json [settings]
{
  "toolbar": {
    "breadcrumbs": true,
    "quick_actions": true,
    "selections_menu": true,
    "agent_review": true,
    "code_actions": false
  }
}
```

**Tùy chọn**

Mỗi tùy chọn điều khiển việc hiện một thành phần của thanh công cụ. Nếu ẩn hết các thành phần, thanh công cụ của trình soạn thảo sẽ không hiện.

## Dùng tab hệ thống

- Mô tả: Có cho phép các cửa sổ gộp thành tab theo tùy chọn tab của người dùng hay không (chỉ macOS).
- Cài đặt: `use_system_window_tabs`
- Mặc định: `false`

**Tùy chọn**

Cài đặt này bật tích hợp với tính năng tab cửa sổ gốc của macOS. Khi đặt là `true`, các cửa sổ Zode có thể được gộp thành tab trong một cửa sổ macOS, theo tùy chọn tab toàn hệ thống do người dùng đặt (như "Always", "In Full Screen" hoặc "Never"). Cài đặt này chỉ có trên macOS.

## Bật language server

- Mô tả: Có dùng language server để cung cấp code intelligence hay không.
- Cài đặt: `enable_language_server`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Đảm bảo dòng mới cuối file khi lưu

- Mô tả: Xóa các dòng chỉ chứa khoảng trắng ở cuối file và đảm bảo chỉ còn đúng một dòng mới ở cuối.
- Cài đặt: `ensure_final_newline_on_save`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Số dòng mở rộng đoạn trích

- Mô tả: Số dòng mặc định dùng để mở rộng các đoạn trích trong multibuffer
- Cài đặt: `expand_excerpt_lines`
- Mặc định: `5`

**Tùy chọn**

Giá trị `integer` dương

## Số dòng ngữ cảnh của đoạn trích

- Mô tả: Số dòng ngữ cảnh cung cấp khi hiện các đoạn trích trong multibuffer.
- Cài đặt: `excerpt_context_lines`
- Mặc định: `2`

**Tùy chọn**

Giá trị `integer` dương từ 1 đến 32. Giá trị nằm ngoài khoảng này sẽ bị ép về khoảng đó.

## Nối tiếp comment khi xuống dòng

- Mô tả: Có bắt đầu dòng mới bằng comment khi dòng trước cũng là comment hay không.
- Cài đặt: `extend_comment_on_newline`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Thanh trạng thái

- Mô tả: Điều khiển các thành phần trên thanh trạng thái. Lưu ý một số mục trên thanh trạng thái có cài đặt riêng ở chỗ khác: nút tìm kiếm, language server và chẩn đoán nằm ở `editor.search.button`, `global_lsp_settings.button` và `diagnostics.button`; huy hiệu CPU/RAM nằm ở `project_footprint_indicator` cấp cao nhất; còn chỉ báo giữ màn hình sáng nằm ở `keep_display_awake` và `keep_display_awake_always` cấp cao nhất, và nó hiện khi một trong hai đang bật.
- Cài đặt: `status_bar`
- Mặc định:

```json [settings]
{
  "status_bar": {
    "show_active_file": false,
    "active_language_button": true,
    "cursor_position_button": true,
    "line_endings_button": false,
    "active_encoding_button": "non_utf8",
    "claude_usage_button": true,
    "codex_usage_button": true,
    "agent_usage_display": "detailed",
    "activity_indicator": true,
    "active_toolchain_button": true,
    "vim_mode_button": true,
    "image_info_button": true,
    "app_version_button": true
  }
}
```

Bấm chuột phải vào chỗ trống trên thanh trạng thái sẽ mở menu chứa các công tắc này, nhóm theo
phía của thanh mà mục đó nằm. Ẩn một mục qua menu đó ghi cùng cài đặt như khi bạn sửa tay,
và không chỉ ngừng vẽ nó: mục bị gỡ khỏi thanh và entity của nó
bị hủy, nên mọi việc thăm dò hay tác vụ nền nó đang làm cũng dừng theo. Keep Display Awake do hai cài đặt điều khiển, và ẩn nó sẽ tắt cả `keep_display_awake` lẫn `keep_display_awake_always`, nhờ đó mọi giữ màn hình sáng đều được nhả.

`active_encoding_button` nhận `always`, `non_utf8` hoặc `never` chứ không phải boolean; menu
chuột phải coi `never` là tắt và bật lại thành `non_utf8`, nên giá trị `always` không
được giữ nguyên sau một lần ẩn rồi hiện.

`app_version_button` hiện phiên bản của Zode, chẳng hạn `v0.1.5`, ở đầu bên phải của thanh. Bản
tiền phát hành giữ hậu tố (`v0.1.5-beta.1`); metadata của bản build không bao giờ được hiện. Rê chuột lên sẽ hiện
`Zode 0.1.5` (kênh Preview và Nightly tự gọi tên mình, như `Zode Preview 0.1.5-beta.1`)
và commit rút gọn nếu bản build biết, còn bấm vào sẽ mở cửa sổ About. Chỉ các nút
dock, và chỉ báo remote control khi remote control đang bật, nằm xa hơn về bên phải.
Menu chuột phải liệt kê nó là **App Version**, còn cửa sổ Cài đặt gọi là **App Version Button**
trong mục Window & Layout.

Có một cài đặt thử nghiệm ẩn hoàn toàn thanh trạng thái. Việc này gây vấn đề lớn về khả năng sử dụng (bạn sẽ không dùng được nhiều tính năng của Zode), nhưng được cung cấp cho những ai đặt không gian màn hình lên trên hết.

```json
"status_bar": {
  "experimental.show": false
}
```

## LSP

- Mô tả: Cấu hình cho language server.
- Cài đặt: `lsp`
- Mặc định: `null`

**Tùy chọn**

Các cài đặt sau có thể được ghi đè cho từng language server:

- `initialization_options`
- `settings`

Để ghi đè cấu hình cho một language server, thêm một mục mang tên language server đó vào giá trị `lsp`.

Một số tùy chọn được truyền cho language server qua `initialization_options`. Đó là các tùy chọn phải chỉ định lúc language server khởi động, và khi đổi thì phải khởi động lại language server.

Ví dụ, để truyền tùy chọn `check` cho `rust-analyzer`, dùng cấu hình sau:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "check": {
          "command": "clippy" // rust-analyzer.check.command (default: "check")
        }
      }
    }
  }
}
```

Các tùy chọn khác có thể đổi lúc đang chạy và nên đặt trong `settings`:

```json [settings]
{
  "lsp": {
    "yaml-language-server": {
      "settings": {
        "yaml": {
          "keyOrdering": true // Enforces alphabetical ordering of keys in maps
        }
      }
    }
  }
}
```

## Cài đặt LSP toàn cục

- Mô tả: Cấu hình cho cài đặt LSP toàn cục, áp dụng cho mọi language server
- Cài đặt: `global_lsp_settings`
- Mặc định:

```json [settings]
{
  "global_lsp_settings": {
    "button": true,
    "request_timeout": 120,
    "notifications": {
      // Timeout in milliseconds for automatically dismissing language server notifications.
      // Set to 0 to disable auto-dismiss.
      "dismiss_timeout_ms": 5000
    }
  }
}
```

**Tùy chọn**

- `button`: Có hiện nút trạng thái LSP trên thanh trạng thái hay không
- `request_timeout`: Thời gian tối đa chờ phản hồi từ language server, tính bằng giây. Giá trị `0` nghĩa là không áp dụng timeout (mọi phản hồi LSP chờ vô thời hạn đến khi xong). Mặc định: `120`
- `notifications`: Cài đặt liên quan đến thông báo.
  - `dismiss_timeout_ms`: Thời gian chờ tính bằng mili giây để tự đóng thông báo của language server. Đặt 0 để tắt tự đóng.

## Debounce tô sáng LSP

- Mô tả: Độ trễ debounce tính bằng mili giây trước khi truy vấn phần tô sáng từ language server theo vị trí con trỏ hiện tại.
- Cài đặt: `lsp_highlight_debounce`
- Mặc định: `75`

**Tùy chọn**

Giá trị `integer` tính bằng mili giây

## Tính năng

- Mô tả: Các tính năng có thể bật hoặc tắt toàn cục
- Cài đặt: `features`
- Mặc định:

```json [settings]
{
  "edit_predictions": {
    "provider": "zed"
  }
}
```

### Nhà cung cấp dự đoán chỉnh sửa

- Mô tả: Dùng nhà cung cấp dự đoán chỉnh sửa nào
- Cài đặt: `edit_prediction_provider`
- Mặc định: `"zed"`

**Tùy chọn**

1. Dùng Zeta làm nhà cung cấp dự đoán chỉnh sửa:

```json [settings]
{
  "edit_predictions": {
    "provider": "zed"
  }
}
```

2. Dùng Copilot làm nhà cung cấp dự đoán chỉnh sửa:

```json [settings]
{
  "edit_predictions": {
    "provider": "copilot"
  }
}
```

3. Tắt dự đoán chỉnh sửa cho mọi nhà cung cấp

```json [settings]
{
  "edit_predictions": {
    "provider": "none"
  }
}
```

## Định dạng khi lưu

- Mô tả: Có định dạng buffer trước khi lưu hay không.
- Cài đặt: `format_on_save`
- Mặc định: `on`

**Tùy chọn**

1. `on`, bật định dạng khi lưu theo cài đặt `formatter`:

```json [settings]
{
  "format_on_save": "on"
}
```

2. `off`, tắt định dạng khi lưu:

```json [settings]
{
  "format_on_save": "off"
}
```

## Formatter

- Mô tả: Cách định dạng buffer.
- Cài đặt: `formatter`
- Mặc định: `auto`

**Tùy chọn**

1. Để dùng language server hiện tại, dùng `"language_server"`:

```json [settings]
{
  "formatter": "language_server"
}
```

2. Hoặc để dùng lệnh bên ngoài, dùng `"external"`. Chỉ định tên chương trình định dạng cần chạy và mảng đối số truyền cho chương trình. Văn bản của buffer sẽ được truyền vào stdin của chương trình, và kết quả đã định dạng phải được ghi ra stdout. Ví dụ, lệnh sau sẽ xóa khoảng trắng ở cuối dòng bằng [`sed(1)`](https://linux.die.net/man/1/sed):

```json [settings]
{
  "formatter": {
    "external": {
      "command": "sed",
      "arguments": ["-e", "s/ *$//"]
    }
  }
}
```

3. Formatter bên ngoài có thể kèm placeholder `{buffer_path}`, lúc chạy sẽ chứa đường dẫn của buffer đang được định dạng. Formatter hoạt động bằng cách nhận nội dung file qua đầu vào chuẩn, định dạng lại rồi xuất ra đầu ra chuẩn, nên thường không biết tên file của thứ chúng đang định dạng. Các công cụ như Prettier hỗ trợ nhận đường dẫn file qua đối số dòng lệnh, từ đó ảnh hưởng đến quyết định định dạng.

CẢNH BÁO: Không dùng `{buffer_path}` để bảo formatter đọc từ một tên file. Formatter chỉ nên đọc từ đầu vào chuẩn và không nên đọc hay ghi file trực tiếp.

```json [settings]
  "formatter": {
    "external": {
      "command": "prettier",
      "arguments": ["--stdin-filepath", "{buffer_path}"]
    }
  }
```

4. Hoặc để dùng code action do các language server đang kết nối cung cấp, dùng `"code_actions"`:

```json [settings]
{
  "formatter": [
    // Use ESLint's --fix:
    { "code_action": "source.fixAll.eslint" },
    // Organize imports on save:
    { "code_action": "source.organizeImports" }
  ]
}
```

5. Hoặc để dùng nhiều formatter nối tiếp nhau, dùng một mảng formatter:

```json [settings]
{
  "formatter": [
    { "language_server": { "name": "rust-analyzer" } },
    {
      "external": {
        "command": "sed",
        "arguments": ["-e", "s/ *$//"]
      }
    }
  ]
}
```

Ở đây `rust-analyzer` được dùng trước để định dạng code, sau đó gọi sed.
Nếu một formatter nào thất bại, các formatter sau vẫn được chạy.

6. Để tắt formatter, dùng `"none"`. Cài đặt này tắt formatter đã cấu hình, nhưng mọi action trong `code_actions_on_format` vẫn được thực thi:

```json [settings]
{
  "formatter": "none"
}
```

## Tự đóng ngoặc

- Mô tả: Có tự thêm ký tự đóng tương ứng khi gõ dấu mở ngoặc đơn, ngoặc vuông, ngoặc nhọn, nháy đơn hoặc nháy kép hay không.
- Cài đặt: `use_autoclose`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Luôn coi ngoặc là đã tự đóng

- Mô tả: Điều khiển cách trình soạn thảo xử lý các ký tự tự đóng.
- Cài đặt: `always_treat_brackets_as_autoclosed`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

**Ví dụ**

Nếu đặt cài đặt này là `true`:

1. Nhập vào trình soạn thảo: `)))`
2. Di con trỏ về đầu: `^)))`
3. Nhập lại: `)))`

Kết quả vẫn là `)))` chứ không phải `))))))` như mặc định.

## Loại trừ file khi quét

- Cài đặt: `file_scan_exclusions`
- Mô tả: File hoặc glob file mà Zode loại trừ hoàn toàn. Chúng bị bỏ qua khi quét file, tìm file, và không hiện trong cây file của dự án. Ghi đè `file_scan_inclusions`.
- Mặc định:

```json [settings]
{
  "file_scan_exclusions": [
    "**/.git",
    "**/.svn",
    "**/.hg",
    "**/.jj",
    "**/CVS",
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/.classpath",
    "**/.settings"
  ]
}
```

Lưu ý: chỉ định `file_scan_exclusions` trong settings.json sẽ ghi đè các giá trị mặc định (ở trên). Nếu muốn loại trừ thêm mục khác, bạn phải liệt kê lại toàn bộ giá trị mặc định trong cài đặt.

## Bao gồm file khi quét

- Cài đặt: `file_scan_inclusions`
- Mô tả: File hoặc glob file mà Zode đưa vào, kể cả khi git bỏ qua. Hữu ích với file không được git theo dõi nhưng vẫn quan trọng với dự án. Lưu ý glob quá rộng có thể làm chậm việc quét file của Zode. `file_scan_exclusions` được ưu tiên hơn các mục bao gồm này.
- Mặc định:

```json [settings]
{
  "file_scan_inclusions": [".env*"]
}
```

## Loại file

- Cài đặt: `file_types`
- Mô tả: Cấu hình cách Zode chọn ngôn ngữ cho file dựa trên tên hoặc đuôi file. Hỗ trợ mục glob.
- Mặc định:

```json [settings]
{
  "file_types": {
    "JSONC": [
      "**/.zode/**/*.json",
      "**/zed/**/*.json",
      "**/Zode/**/*.json",
      "**/.vscode/**/*.json"
    ],
    "Shell Script": [".env.*"]
  }
}
```

**Ví dụ**

Để coi mọi file `.c` là C++, file tên `MyLockFile` là TOML và file bắt đầu bằng `Dockerfile` là Dockerfile:

```json [settings]
{
  "file_types": {
    "C++": ["c"],
    "TOML": ["MyLockFile"],
    "Dockerfile": ["Dockerfile*"]
  }
}
```

## Chẩn đoán

- Mô tả: Cấu hình cho các tính năng liên quan đến chẩn đoán.
- Cài đặt: `diagnostics`
- Mặc định:

```json [settings]
{
  "diagnostics": {
    "include_warnings": true,
    "inline": {
      "enabled": false
    }
  }
}
```

### Chẩn đoán nội tuyến

- Mô tả: Có hiện thông tin chẩn đoán ngay trong dòng hay không.
- Cài đặt: `inline`
- Mặc định:

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": false,
      "update_debounce_ms": 150,
      "padding": 4,
      "min_column": 0,
      "max_severity": null
    }
  }
}
```

**Tùy chọn**

1. Bật chẩn đoán nội tuyến.

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": true
    }
  }
}
```

2. Trì hoãn cập nhật chẩn đoán đến một lúc sau lần cập nhật chẩn đoán cuối.

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": true,
      "update_debounce_ms": 150
    }
  }
}
```

3. Đặt khoảng đệm giữa cuối dòng mã nguồn và đầu chẩn đoán.

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": true,
      "padding": 4
    }
  }
}
```

4. Căn chẩn đoán nội tuyến theo chiều ngang tại cột đã cho.

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": true,
      "min_column": 80
    }
  }
}
```

5. Chỉ hiện chẩn đoán mức cảnh báo và lỗi.

```json [settings]
{
  "diagnostics": {
    "inline": {
      "enabled": true,
      "max_severity": "warning"
    }
  }
}
```

## Git

- Mô tả: Cấu hình cho các tính năng liên quan đến git.
- Cài đặt: `git`
- Mặc định:

```json [settings]
{
  "git": {
    "git_gutter": "tracked_files",
    "inline_blame": {
      "enabled": true
    },
    "branch_picker": {
      "show_author_name": true
    },
    "hunk_style": "staged_hollow"
  }
}
```

### Git gutter

- Mô tả: Có hiện git gutter hay không.
- Cài đặt: `git_gutter`
- Mặc định: `tracked_files`

**Tùy chọn**

1. Hiện git gutter trong file đang được theo dõi

```json [settings]
{
  "git": {
    "git_gutter": "tracked_files"
  }
}
```

2. Ẩn git gutter

```json [settings]
{
  "git": {
    "git_gutter": "hide"
  }
}
```

### Debounce của gutter

- Mô tả: Đặt ngưỡng debounce (tính bằng mili giây), sau đó thay đổi mới được phản ánh lên git gutter.
- Cài đặt: `gutter_debounce`
- Mặc định: `null`

**Tùy chọn**

Giá trị `integer` tính bằng mili giây

Ví dụ:

```json [settings]
{
  "git": {
    "gutter_debounce": 100
  }
}
```

### Git blame nội tuyến

- Mô tả: Có hiện thông tin git blame ngay trong dòng, trên dòng đang được tiêu điểm hay không.
- Cài đặt: `inline_blame`
- Mặc định:

```json [settings]
{
  "git": {
    "inline_blame": {
      "enabled": true
    }
  }
}
```

**Tùy chọn**

1. Tắt git blame nội tuyến:

```json [settings]
{
  "git": {
    "inline_blame": {
      "enabled": false
    }
  }
}
```

2. Chỉ hiện git blame nội tuyến sau một độ trễ (bắt đầu tính khi con trỏ ngừng di chuyển):

```json [settings]
{
  "git": {
    "inline_blame": {
      "delay_ms": 500
    }
  }
}
```

3. Hiện tóm tắt commit bên cạnh ngày commit và tác giả:

```json [settings]
{
  "git": {
    "inline_blame": {
      "show_commit_summary": true
    }
  }
}
```

4. Dùng giá trị này làm cột tối thiểu để hiện thông tin blame nội tuyến:

```json [settings]
{
  "git": {
    "inline_blame": {
      "min_column": 80
    }
  }
}
```

5. Đặt khoảng đệm giữa cuối dòng và gợi ý blame nội tuyến, tính bằng em:

```json [settings]
{
  "git": {
    "inline_blame": {
      "padding": 10
    }
  }
}
```

### Bộ chọn nhánh

- Mô tả: Cấu hình liên quan đến bộ chọn nhánh.
- Cài đặt: `branch_picker`
- Mặc định:

```json [settings]
{
  "git": {
    "branch_picker": {
      "show_author_name": false
    }
  }
}
```

**Tùy chọn**

1. Hiện tên tác giả trong bộ chọn nhánh:

```json [settings]
{
  "git": {
    "branch_picker": {
      "show_author_name": true
    }
  }
}
```

### Kiểu hunk

- Mô tả: Kiểu hiển thị dùng cho các hunk của diff.
- Cài đặt: `hunk_style`
- Mặc định:

```json [settings]
{
  "git": {
    "hunk_style": "staged_hollow"
  }
}
```

**Tùy chọn**

1. Hiện các hunk đã stage mờ đi và có viền:

```json [settings]
{
  "git": {
    "hunk_style": "staged_hollow"
  }
}
```

2. Hiện các hunk chưa stage mờ đi và có viền:

```json [settings]
{
  "git": {
    "hunk_style": "unstaged_hollow"
  }
}
```

## Phương án dự phòng khi đi tới định nghĩa

- Mô tả: Việc cần làm khi action {#action editor::GoToDefinition} không tìm thấy định nghĩa
- Cài đặt: `go_to_definition_fallback`
- Mặc định: `"find_all_references"`

**Tùy chọn**

1. Không làm gì:

```json [settings]
{
  "go_to_definition_fallback": "none"
}
```

2. Tìm các tham chiếu của cùng symbol (mặc định):

```json [settings]
{
  "go_to_definition_fallback": "find_all_references"
}
```

## Hard tab

- Mô tả: Có thụt lề dòng bằng ký tự tab hay nhiều khoảng trắng.
- Cài đặt: `hard_tabs`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Chế độ Helix

- Mô tả: Có bật chế độ Helix hay không. Bật `helix_mode` cũng bật `vim_mode`. Xem [tài liệu Helix](../helix.md) để biết thêm chi tiết.
- Cài đặt: `helix_mode`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Giữ màn hình sáng

- Mô tả: Có giữ màn hình sáng khi một terminal (agent, task hoặc shell) đang liên tục xuất dữ liệu hay không. Tự nhả khi chạy bằng pin và khi không có gì xuất dữ liệu. Xem [Giữ màn hình sáng](../keep-display-awake.md) để biết nền tảng được hỗ trợ và các đánh đổi.
- Cài đặt: `keep_display_awake`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Luôn giữ màn hình sáng

- Mô tả: Có giữ màn hình sáng suốt thời gian còn mở một cửa sổ Zode, cho đến khi tắt cài đặt này hay không. Độc lập với `keep_display_awake`, và vẫn giữ khi dùng pin. Không có giới hạn thời gian, và màn hình sẽ không khóa khi rảnh chừng nào cài đặt còn bật. Bật tắt từ menu của icon con mắt trên thanh trạng thái (**Always Keep Display Awake**) hoặc bằng action `keep_awake::ToggleAlwaysOn` trong bảng lệnh (command palette); action này không có phím tắt mặc định và, như icon, chỉ có ở nơi nền tảng giữ được màn hình. Xem [Giữ màn hình sáng](../keep-display-awake.md) để biết nền tảng được hỗ trợ và các đánh đổi.
- Cài đặt: `keep_display_awake_always`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Thông báo khi agent xong việc

- Mô tả: Có gửi thông báo hệ điều hành khi agent trả lời xong, đang chờ bạn duyệt, hoặc CLI của nó thoát hay không -- gửi bất kể cửa sổ có tiêu điểm hay không. Với tab Claude có phiên được theo dõi, "trả lời xong" lấy từ cuối lượt trong transcript và thông báo chờ duyệt lấy từ hộp thoại cấp quyền của nó. Với mọi agent khác, `quiet_period_ms` là thời gian nó phải im lặng sau lần xuất dữ liệu cuối trước khi "trả lời xong" được kích hoạt; đây là debounce, không phải bảo đảm, vì agent có thể dừng lâu hơn mức này khi đang chạy công cụ hoặc chờ model. Sàn là 2000ms. Xem [Thông báo khi agent xong việc](../agent-finished-notifications.md) để biết đánh đổi, bảng nền tảng và cách tắt.
- Cài đặt: `agent_finished_notification`
- Mặc định:

```json [settings]
{
  "agent_finished_notification": {
    "enabled": true,
    "quiet_period_ms": 12000
  }
}
```

**Tùy chọn**

- `enabled`: `boolean`
- `quiet_period_ms`: `integer` dương, tính bằng mili giây, sàn là `2000`

## Remote control

- Mô tả: Có cho thiết bị khác của bạn, đăng nhập cùng tài khoản, xem và gõ vào các agent và terminal đang chạy trong Zode này hay không. Tắt nghĩa là Zode không mở kết nối nào cho việc đó và không theo dõi terminal nào. Bật cũng cần bạn đã đăng nhập, và mỗi thiết bị được phép kết nối phải được duyệt một lần bằng cách so khớp mã sáu chữ số. Khi có thiết bị đang kết nối, thanh trạng thái báo như vậy và có cách ngắt kết nối nó, và thanh trạng thái được hiện suốt thời gian còn một thiết bị kết nối dù `status_bar.show` ẩn nó; một toast thông báo mỗi thiết bị tiếp quản (trong cửa sổ bạn đang nhìn, hoặc cửa sổ dùng gần nhất nếu Zode chạy nền); tối đa bốn thiết bị kết nối cùng lúc; các action `remote_control::DisconnectAll`, `remote_control::Disable` và `remote_control::ManageDevices` có trong bảng lệnh. Khi có thiết bị kết nối, Zode giữ màn hình sáng theo cùng điều kiện như `keep_display_awake`. Mọi thứ gửi giữa các thiết bị đều được mã hóa đầu cuối; định dạng truyền được mô tả trong [Giao thức remote control](../remote-control-protocol.md).
- Cài đặt: `remote_control`
- Mặc định:

```json [settings]
{
  "remote_control": {
    "enabled": false,
    "idle_timeout_minutes": 30,
    "push_notifications": {
      "approval": true,
      "answered": true,
      "ended": true
    }
  }
}
```

**Tùy chọn**

- `enabled`: `boolean`
- `idle_timeout_minutes`: số phút kiểu `integer` mà thiết bị đã kết nối được phép không dùng trước khi bị ngắt: gõ trong terminal, duyệt file, liệt kê hoặc tiếp tục phiên, và yêu cầu từ dự án mở từ một Zode khác đều tính là có dùng, còn ping và heartbeat của dự án đó thì không; `0` tắt giới hạn. Xem [Remote control](../remote-control.md#turn-it-on)
- `push_notifications`: `object` gồm `approval`, `answered` và `ended`, mỗi khoá là `boolean`: sự kiện agent nào được đẩy tới điện thoại và máy tính bảng đã ghép nối và đã cho phép thông báo. Cần gói có thông báo đẩy. Xem [Remote control](../remote-control.md#push-notifications)

## Chỉ báo mức tiêu thụ của dự án

- Mô tả: Có hiện tổng mức CPU và RAM của mọi tiến trình con của các dự án đang được theo dõi (language server và terminal) trên thanh trạng thái hay không. Bấm vào huy hiệu để xem chi tiết theo từng dự án. Zode chạy mọi dự án trong một tiến trình hệ điều hành, nên heap và CPU của chính nó không thể quy cho từng dự án và không được tính. Xem [Chỉ báo mức tiêu thụ của dự án](../project-footprint-indicator.md) để biết cái gì được đo, chi phí thăm dò và quyền riêng tư.
- Cài đặt: `project_footprint_indicator`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hướng dẫn thụt lề

- Mô tả: Cấu hình liên quan đến hướng dẫn thụt lề. Có thể cấu hình riêng cho từng ngôn ngữ.
- Cài đặt: `indent_guides`
- Mặc định:

```json [settings]
{
  "indent_guides": {
    "enabled": true,
    "line_width": 1,
    "active_line_width": 1,
    "coloring": "fixed",
    "background_coloring": "disabled"
  }
}
```

**Tùy chọn**

1. Tắt hướng dẫn thụt lề

```json [settings]
{
  "indent_guides": {
    "enabled": false
  }
}
```

2. Bật hướng dẫn thụt lề cho một ngôn ngữ cụ thể.

```json [settings]
{
  "languages": {
    "Python": {
      "indent_guides": {
        "enabled": true
      }
    }
  }
}
```

3. Bật tô màu theo mức thụt lề ("rainbow indentation").
   Màu dùng cho từng mức thụt lề được định nghĩa trong giao diện (theme) (khóa theme: `accents`). Có thể tùy biến bằng ghi đè theme.

```json [settings]
{
  "indent_guides": {
    "enabled": true,
    "coloring": "indent_aware"
  }
}
```

4. Bật tô màu nền theo mức thụt lề ("rainbow indentation").
   Màu dùng cho từng mức thụt lề được định nghĩa trong giao diện (theme) (khóa theme: `accents`). Có thể tùy biến bằng ghi đè theme.

```json [settings]
{
  "indent_guides": {
    "enabled": true,
    "coloring": "indent_aware",
    "background_coloring": "indent_aware"
  }
}
```

## Hộp thông tin khi rê chuột

- Mô tả: Có hiện hộp thông tin khi rê chuột lên symbol trong trình soạn thảo hay không.
- Cài đặt: `hover_popover_enabled`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Độ trễ hiện hộp thông tin khi rê chuột

- Mô tả: Thời gian chờ (mili giây) trước khi hiện hộp thông tin khi rê chuột. Độ trễ này cũng áp dụng cho tự động gợi ý chữ ký khi `auto_signature_help` được bật.
- Cài đặt: `hover_popover_delay`
- Mặc định: `300`

**Tùy chọn**

Giá trị `integer` tính bằng mili giây

## Hộp thông tin khi rê chuột có dính

- Mô tả: Hộp thông tin có giữ lại khi chuột di chuyển về phía nó hay không, để bạn thao tác với nội dung của nó trước khi nó biến mất.
- Cài đặt: `hover_popover_sticky`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Độ trễ ẩn hộp thông tin khi rê chuột

- Mô tả: Thời gian chờ (mili giây) trước khi ẩn hộp thông tin sau khi chuột rời khỏi đối tượng đang rê. Chỉ áp dụng khi `hover_popover_sticky` được bật.
- Cài đặt: `hover_popover_hiding_delay`
- Mặc định: `300`

**Tùy chọn**

Giá trị `integer` tính bằng mili giây

## Bộ icon

- Mô tả: Cài đặt bộ icon có thể viết dưới hai dạng: tên của một bộ icon, hoặc một object chứa các bộ icon `mode`, `dark` và `light` cho file/thư mục bên trong Zode. **Bản build này bỏ qua giá trị đó**: icon file và thư mục được cố định là Material Icon Theme, có màu, và không tên nào ghi ở đây thay đổi được chúng. Khóa này vẫn nằm trong schema vì nó luôn được phân giải khi khởi động — xem [Bộ icon](../icon-themes.md).
- Cài đặt: `icon_theme`
- Mặc định: `Material Icon Theme`

### Object bộ icon

- Mô tả: Chỉ định bộ icon bằng một object gồm `mode`, `dark` và `light`. Được chấp nhận và phân tích, nhưng, như dạng chuỗi ở trên, không ảnh hưởng đến icon hiển thị.
- Cài đặt: `icon_theme`
- Mặc định:

```json [settings]
{
  "icon_theme": {
    "mode": "system",
    "dark": "Material Icon Theme",
    "light": "Material Icon Theme"
  }
}
```

### Chế độ

- Mô tả: Chỉ định chế độ của bộ icon. Không có tác dụng trong bản build này (xem ở trên).
- Cài đặt: `mode`
- Mặc định: `system`

### Tối

- Mô tả: Tên bộ icon tối. Không có tác dụng trong bản build này (xem ở trên).
- Cài đặt: `dark`
- Mặc định: `Material Icon Theme`

### Sáng

- Mô tả: Tên bộ icon sáng. Không có tác dụng trong bản build này (xem ở trên).
- Cài đặt: `light`
- Mặc định: `Material Icon Theme`

## Trình xem ảnh

- Mô tả: Cài đặt cho chức năng xem ảnh
- Cài đặt: `image_viewer`
- Mặc định:

```json [settings]
{
  "image_viewer": {
    "unit": "binary"
  }
}
```

**Tùy chọn**

### Đơn vị

- Mô tả: Đơn vị cho kích thước file ảnh
- Cài đặt: `unit`
- Mặc định: `"binary"`

**Tùy chọn**

1. Dùng đơn vị nhị phân (KiB, MiB):

```json [settings]
{
  "image_viewer": {
    "unit": "binary"
  }
}
```

2. Dùng đơn vị thập phân (KB, MB):

```json [settings]
{
  "image_viewer": {
    "unit": "decimal"
  }
}
```

## Inlay hint

- Mô tả: Cấu hình hiển thị văn bản phụ kèm gợi ý trong trình soạn thảo.
- Cài đặt: `inlay_hints`
- Mặc định:

```json [settings]
{
  "inlay_hints": {
    "enabled": false,
    "show_type_hints": true,
    "show_parameter_hints": true,
    "show_other_hints": true,
    "show_background": false,
    "edit_debounce_ms": 700,
    "scroll_debounce_ms": 50,
    "toggle_on_modifiers_press": null
  }
}
```

**Tùy chọn**

Việc truy vấn inlay hint gồm hai phần: trình soạn thảo (client) và LSP server.
Khi các cài đặt inlay ở trên được đổi để bật gợi ý, trình soạn thảo sẽ bắt đầu truy vấn một số loại gợi ý và phản ứng với yêu cầu làm mới gợi ý LSP từ server.
Lúc đó server có thể trả hoặc không trả gợi ý tùy cách nó triển khai; có thể cần cấu hình thêm, hãy xem tài liệu của LSP server tương ứng.

Các ngôn ngữ sau được Zode cấu hình sẵn inlay hint:

- [Go](https://zodekit.com/docs/languages/go)
- [Rust](https://zodekit.com/docs/languages/rust)
- [Svelte](https://zodekit.com/docs/languages/svelte)
- [TypeScript](https://zodekit.com/docs/languages/typescript)

Dùng mục `lsp` để cấu hình server. Ví dụ có trong tài liệu của ngôn ngữ tương ứng.

Zode không truy vấn gợi ý ngay lập tức mà dùng hai loại debounce, đặt cái nào về 0 là tắt cái đó.
Việc cập nhật gợi ý do đổi cài đặt không bị debounce.

Mọi giá trị cấu hình có thể có của `toggle_on_modifiers_press` là:

```json [settings]
{
  "inlay_hints": {
    "toggle_on_modifiers_press": {
      "control": true,
      "shift": true,
      "alt": true,
      "platform": true,
      "function": true
    }
  }
}
```

Giá trị không chỉ định mặc định là `false`; gợi ý sẽ không được bật tắt nếu mọi phím bổ trợ đều là `false` hoặc chưa nhấn đủ các phím bổ trợ.

## Nhật ký

- Mô tả: Cấu hình cho nhật ký.
- Cài đặt: `journal`
- Mặc định:

```json [settings]
{
  "journal": {
    "path": "~",
    "hour_format": "hour12"
  }
}
```

### Đường dẫn

- Mô tả: Đường dẫn của thư mục lưu các mục nhật ký. Nếu chỉ định đường dẫn không hợp lệ, nhật ký sẽ quay về dùng `~` (thư mục home).
- Cài đặt: `path`
- Mặc định: `~`

**Tùy chọn**

Giá trị `string`

### Định dạng giờ

- Mô tả: Định dạng dùng để hiện giờ trong nhật ký.
- Cài đặt: `hour_format`
- Mặc định: `hour12`

**Tùy chọn**

1. Định dạng 12 giờ:

```json [settings]
{
  "journal": {
    "hour_format": "hour12"
  }
}
```

2. Định dạng 24 giờ:

```json [settings]
{
  "journal": {
    "hour_format": "hour24"
  }
}
```

## Tự đóng thẻ JSX

- Mô tả: Có tự động đóng thẻ JSX hay không
- Cài đặt: `jsx_tag_auto_close`
- Mặc định:

```json [settings]
{
  "jsx_tag_auto_close": {
    "enabled": true
  }
}
```

**Tùy chọn**

- `enabled`: Có bật tự động đóng thẻ JSX hay không

## Ngôn ngữ

- Mô tả: Cấu hình cho từng ngôn ngữ cụ thể.
- Cài đặt: `languages`
- Mặc định: `null`

**Tùy chọn**

Để ghi đè cài đặt cho một ngôn ngữ, thêm một mục mang tên ngôn ngữ đó vào giá trị `languages`. Ví dụ:

```json [settings]
{
  "languages": {
    "C": {
      "format_on_save": "off",
      "preferred_line_length": 64,
      "soft_wrap": "bounded"
    },
    "JSON": {
      "tab_size": 4
    }
  }
}
```

Các cài đặt sau có thể được ghi đè cho từng ngôn ngữ cụ thể:

- [`enable_language_server`](#enable-language-server)
- [`ensure_final_newline_on_save`](#ensure-final-newline-on-save)
- [`format_on_save`](#format-on-save)
- [`formatter`](#formatter)
- [`hard_tabs`](#hard-tabs)
- [`preferred_line_length`](#preferred-line-length)
- [`remove_trailing_whitespace_on_save`](#remove-trailing-whitespace-on-save)
- [`semantic_tokens`](#semantic-tokens)
- [`show_edit_predictions`](#show-edit-predictions)
- [`show_whitespaces`](#show-whitespaces)
- [`whitespace_map`](#whitespace-map)
- [`soft_wrap`](#soft-wrap)
- [`tab_size`](#tab-size)
- [`use_autoclose`](#use-autoclose)
- [`always_treat_brackets_as_autoclosed`](#always-treat-brackets-as-autoclosed)

Các giá trị này nhận cùng tùy chọn với cài đặt cấp gốc có cùng tên.

### Document symbol

- Mô tả: Điều khiển nguồn document symbol dùng cho outline và breadcrumb.
- Cài đặt: `document_symbols`
- Mặc định: `off`

**Tùy chọn**

- `"off"`: Dùng truy vấn tree-sitter để tính document symbol (mặc định)
- `"on"`: Dùng phản hồi LSP `textDocument/documentSymbol` của language server. Khi bật, tree-sitter không được dùng cho document symbol

Document symbol từ LSP có thể chính xác hơn với các tính năng ngôn ngữ phức tạp (ví dụ kiểu generic, macro, decorator) mà tree-sitter có thể xử lý chưa tốt. Dùng khi language server của bạn cho thông tin symbol tốt hơn grammar tree-sitter.

Ví dụ:

```json [settings]
{
  "languages": {
    "TypeScript": {
      "document_symbols": "on"
    }
  }
}
```

## Định dạng chỉ báo dòng

- Mô tả: Định dạng của chỉ báo dòng trên thanh trạng thái
- Cài đặt: `line_indicator_format`
- Mặc định: `"short"`

**Tùy chọn**

1. Định dạng ngắn:

```json [settings]
{
  "line_indicator_format": "short"
}
```

2. Định dạng dài:

```json [settings]
{
  "line_indicator_format": "long"
}
```

## Sửa liên kết

- Mô tả: Có sửa liên kết các vùng liên quan hay không, nếu language server hỗ trợ. Ví dụ, khi sửa thẻ mở `<html>`, nội dung của thẻ đóng `</html>` cũng được sửa theo.
- Cài đặt: `linked_edits`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Màu tài liệu LSP

- Mô tả: Cách hiển thị màu `textDocument/documentColor` của LSP trong trình soạn thảo
- Cài đặt: `lsp_document_colors`
- Mặc định: `inlay`

**Tùy chọn**

1. `inlay`: Hiện màu của tài liệu dưới dạng inlay hint cạnh văn bản màu.
2. `background`: Vẽ nền phía sau văn bản màu.
3. `border`: Vẽ viền quanh văn bản màu.
4. `none`: Không truy vấn và không hiển thị màu của tài liệu.

## Số tab tối đa

- Mô tả: Số tab tối đa hiện trên thanh tab
- Cài đặt: `max_tabs`
- Mặc định: `null`

**Tùy chọn**

Giá trị `integer` dương hoặc `null` cho số tab không giới hạn

## Dán bằng chuột giữa (chỉ Linux)

- Mô tả: Bật dán bằng chuột giữa trên Linux
- Cài đặt: `middle_click_paste`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Phím bổ trợ đa con trỏ

- Mô tả: Xác định phím bổ trợ dùng để thêm nhiều con trỏ bằng chuột. Các thao tác chuột mở liên kết khi rê sẽ tự điều chỉnh để không xung đột với phím bổ trợ đa con trỏ.
- Cài đặt: `multi_cursor_modifier`
- Mặc định: `alt`

**Tùy chọn**

1. Ứng với `Alt` trên Linux và Windows, và `Option` trên macOS:

```json [settings]
{
  "multi_cursor_modifier": "alt"
}
```

2. Ứng với `Control` trên Linux và Windows, và `Command` trên macOS:

```json [settings]
{
  "multi_cursor_modifier": "cmd_or_ctrl" // alias: "cmd", "ctrl"
}
```

## Node

- Mô tả: Cấu hình cho tích hợp Node.js
- Cài đặt: `node`
- Mặc định:

```json [settings]
{
  "node": {
    "ignore_system_version": false,
    "path": null,
    "npm_path": null
  }
}
```

**Tùy chọn**

- `ignore_system_version`: Có bỏ qua phiên bản Node.js của hệ thống hay không
- `path`: Đường dẫn tùy chỉnh đến binary Node.js
- `npm_path`: Đường dẫn tùy chỉnh đến binary npm

## Proxy mạng

- Mô tả: Cấu hình proxy mạng cho Zode.
- Cài đặt: `proxy`
- Mặc định: `null`

**Tùy chọn**

Cài đặt proxy phải chứa URL của proxy.

Hỗ trợ các URI scheme sau:

- `http`
- `https`
- `socks4` - proxy SOCKS4 với DNS cục bộ
- `socks4a` - proxy SOCKS4 với DNS từ xa
- `socks5` - proxy SOCKS5 với DNS cục bộ
- `socks5h` - proxy SOCKS5 với DNS từ xa

Nếu không chỉ định scheme thì dùng `http`.

Mặc định không dùng proxy, hoặc Zode sẽ thử lấy cài đặt proxy từ biến môi trường, như `http_proxy`, `HTTP_PROXY`, `https_proxy`, `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, `no_proxy` và `NO_PROXY`.

Ví dụ, để đặt proxy `http`, thêm đoạn sau vào cài đặt:

```json [settings]
{
  "proxy": "http://127.0.0.1:10809"
}
```

Hoặc để đặt proxy `socks5`:

```json [settings]
{
  "proxy": "socks5h://localhost:10808"
}
```

Nếu muốn loại trừ một số host khỏi proxy, đặt biến môi trường `NO_PROXY`. Biến này nhận danh sách phân tách bằng dấu phẩy gồm hostname, hậu tố host, địa chỉ IPv4/IPv6 hoặc dải không dùng proxy. Ví dụ nếu môi trường có `NO_PROXY="google.com, 192.168.1.0/24"` thì mọi host trong `192.168.1.*`, `google.com` và `*.google.com` sẽ bỏ qua proxy. Xem thêm [tài liệu reqwest NoProxy](https://docs.rs/reqwest/latest/reqwest/struct.NoProxy.html#method.from_string).

## Khi đóng cửa sổ cuối cùng

- Mô tả: Việc cần làm khi đóng cửa sổ cuối cùng
- Cài đặt: `on_last_window_closed`
- Mặc định: `"platform_default"`

**Tùy chọn**

1. Dùng hành vi mặc định của nền tảng:

```json [settings]
{
  "on_last_window_closed": "platform_default"
}
```

2. Luôn thoát ứng dụng:

```json [settings]
{
  "on_last_window_closed": "quit_app"
}
```

## Profile

- Mô tả: Các profile cấu hình có thể áp dụng tạm thời lên trên cài đặt hiện có hoặc cài đặt mặc định của Zode.
- Cài đặt: `profiles`
- Mặc định: `{}`

**Tùy chọn**

Mỗi profile là một object với các trường tùy chọn sau:

- `base`: Cài đặt xuất phát trước khi áp dụng phần ghi đè của profile.
  - `"user"` (mặc định): Áp dụng lên trên cài đặt người dùng hiện tại.
  - `"default"`: Áp dụng lên trên cài đặt mặc định của Zode, bỏ qua tùy biến của người dùng.
- `settings`: Các cài đặt ghi đè cho profile này.

Ví dụ:

```json [settings]
{
  "profiles": {
    "Presentation": {
      "settings": {
        "buffer_font_size": 20,
        "ui_font_size": 18,
        "theme": "One Light"
      }
    },
    "Clean Slate": {
      "base": "default",
      "settings": {
        "theme": "Ayu Dark"
      }
    }
  }
}
```

## Tab xem trước

- Mô tả:
  Tab xem trước cho phép mở file ở chế độ xem trước: tab tự đóng khi bạn chuyển sang file khác trừ khi bạn ghim nó. Hữu ích để xem nhanh file mà không làm workspace rối. Tab xem trước hiện tên file in nghiêng. \
  Có nhiều cách chuyển tab xem trước thành tab thường:

  - Bấm đúp vào file
  - Bấm đúp vào tiêu đề tab
  - Dùng action {#action project_panel::OpenPermanent}
  - Sửa file
  - Kéo file sang khung khác

- Cài đặt: `preview_tabs`
- Mặc định:

```json [settings]
{
  "preview_tabs": {
    "enabled": true,
    "enable_preview_from_project_panel": true,
    "enable_preview_from_file_finder": false,
    "enable_preview_from_multibuffer": true,
    "enable_preview_multibuffer_from_code_navigation": false,
    "enable_preview_file_from_code_navigation": true,
    "enable_keep_preview_on_code_navigation": false
  }
}
```

### Bật xem trước từ bảng dự án

- Mô tả: Xác định có mở file ở chế độ xem trước khi mở từ bảng dự án bằng một lần bấm hay không.
- Cài đặt: `enable_preview_from_project_panel`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Bật xem trước từ bộ tìm file

- Mô tả: Xác định có mở file ở chế độ xem trước khi chọn từ bộ tìm file hay không.
- Cài đặt: `enable_preview_from_file_finder`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

### Bật xem trước từ multibuffer

- Mô tả: Xác định có mở file ở chế độ xem trước khi mở từ multibuffer hay không.
- Cài đặt: `enable_preview_from_multibuffer`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Bật xem trước multibuffer từ điều hướng code

- Mô tả: Xác định có mở tab ở chế độ xem trước khi dùng điều hướng code để mở một multibuffer hay không.
- Cài đặt: `enable_preview_multibuffer_from_code_navigation`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

### Bật xem trước file từ điều hướng code

- Mô tả: Xác định có mở tab ở chế độ xem trước khi dùng điều hướng code để mở một file đơn hay không.
- Cài đặt: `enable_preview_file_from_code_navigation`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Bật giữ xem trước khi điều hướng code

- Mô tả: Xác định có giữ tab ở chế độ xem trước khi dùng điều hướng code để rời khỏi chúng hay không. Nếu `enable_preview_file_from_code_navigation` hoặc `enable_preview_multibuffer_from_code_navigation` cũng là true, tab mới có thể thay thế tab hiện có.
- Cài đặt: `enable_keep_preview_on_code_navigation`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Bộ tìm file

### Icon file

- Mô tả: Có hiện icon file trong bộ tìm file hay không.
- Cài đặt: `file_icons`
- Mặc định: `true`

### Chiều rộng tối đa của hộp thoại

- Mô tả: Chiều rộng tối đa của hộp thoại bộ tìm file. Nhận một trong các giá trị: `small`, `medium`, `large`, `xlarge` và `full`.
- Cài đặt: `modal_max_width`
- Mặc định: `small`

### Bỏ qua tiêu điểm cho file đang hoạt động trong kết quả tìm

- Mô tả: Xác định bộ tìm file có bỏ qua tiêu điểm cho file đang hoạt động trong kết quả tìm kiếm hay không.
- Cài đặt: `skip_focus_for_active_in_search`
- Mặc định: `true`

## Hướng chia khung theo chiều ngang

- Mô tả: Hướng bạn muốn chia khung theo chiều ngang
- Cài đặt: `pane_split_direction_horizontal`
- Mặc định: `"up"`

**Tùy chọn**

1. Chia lên trên:

```json [settings]
{
  "pane_split_direction_horizontal": "up"
}
```

2. Chia xuống dưới:

```json [settings]
{
  "pane_split_direction_horizontal": "down"
}
```

## Hướng chia khung theo chiều dọc

- Mô tả: Hướng bạn muốn chia khung theo chiều dọc
- Cài đặt: `pane_split_direction_vertical`
- Mặc định: `"left"`

**Tùy chọn**

1. Chia sang trái:

```json [settings]
{
  "pane_split_direction_vertical": "left"
}
```

2. Chia sang phải:

```json [settings]
{
  "pane_split_direction_vertical": "right"
}
```

## Độ dài dòng ưa dùng

- Mô tả: Cột mà tại đó dòng được ngắt mềm, cho các buffer bật ngắt dòng mềm.
- Cài đặt: `preferred_line_length`
- Mặc định: `80`

**Tùy chọn**

Giá trị `integer`

## File riêng tư

- Mô tả: Glob khớp với đường dẫn file để xác định file có riêng tư hay không
- Cài đặt: `private_files`
- Mặc định: `["**/.env*", "**/*.pem", "**/*.key", "**/*.cert", "**/*.crt", "**/secrets.yml"]`

**Tùy chọn**

Danh sách mẫu glob kiểu `string`

## Dự án trực tuyến theo mặc định

- Mô tả: Có hiện chế độ xem dự án trực tuyến theo mặc định hay không.
- Cài đặt: `projects_online_by_default`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Đọc cấu hình SSH

- Mô tả: Có đọc các file cấu hình SSH hay không
- Cài đặt: `read_ssh_config`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Ẩn giá trị riêng tư

- Mô tả: Ẩn giá trị của biến khỏi phần hiển thị trong file riêng tư
- Cài đặt: `redact_private_values`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

## Số dòng tương đối

- Mô tả: Có hiện số dòng tương đối trong gutter hay không
- Cài đặt: `relative_line_numbers`
- Mặc định: `"disabled"`

**Tùy chọn**

1. Hiện số dòng tương đối trong gutter, tính dòng bị ngắt mềm là một dòng:

```json [settings]
{
  "relative_line_numbers": "enabled"
}
```

2. Hiện số dòng tương đối trong gutter, tính cả các dòng bị ngắt mềm:

```json [settings]
{
  "relative_line_numbers": "wrapped"
}
```

2. Không dùng số dòng tương đối:

```json [settings]
{
  "relative_line_numbers": "disabled"
}
```

## Xóa khoảng trắng cuối dòng khi lưu

- Mô tả: Có xóa khoảng trắng ở cuối các dòng của buffer trước khi lưu hay không.
- Cài đặt: `remove_trailing_whitespace_on_save`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Đổi kích thước mọi bảng trong dock

- Mô tả: Có đổi kích thước mọi bảng trong dock khi đổi kích thước dock hay không. Có thể là tổ hợp của "left", "right" và "bottom".
- Cài đặt: `resize_all_panels_in_dock`
- Mặc định: `["left"]`

**Tùy chọn**

Danh sách chuỗi chứa tổ hợp bất kỳ của:

- `"left"`: Đổi kích thước các bảng của dock trái cùng nhau
- `"right"`: Đổi kích thước các bảng của dock phải cùng nhau
- `"bottom"`: Đổi kích thước các bảng của dock dưới cùng nhau

## Khôi phục khi mở lại file

- Mô tả: Có thử khôi phục trạng thái của file trước đó khi mở lại không. Trạng thái được lưu theo từng khung.
- Cài đặt: `restore_on_file_reopen`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Khôi phục khi khởi động

- Mô tả: Điều khiển việc khôi phục phiên khi khởi động.
- Cài đặt: `restore_on_startup`
- Mặc định: `last_session`

**Tùy chọn**

1. Khôi phục mọi workspace đang mở khi thoát Zode:

```json [settings]
{
  "restore_on_startup": "last_session"
}
```

2. Khôi phục workspace đã đóng gần nhất:

```json [settings]
{
  "restore_on_startup": "last_workspace"
}
```

3. Luôn bắt đầu với trình soạn thảo trống:

```json [settings]
{
  "restore_on_startup": "empty_tab"
}
```

4. Luôn bắt đầu với launchpad chào mừng:

```json [settings]
{
  "restore_on_startup": "launchpad"
}
```

## Cuộn quá dòng cuối

- Mô tả: Trình soạn thảo có cuộn quá dòng cuối hay không
- Cài đặt: `scroll_beyond_last_line`
- Mặc định: `"one_page"`

**Tùy chọn**

1. Cuộn quá dòng cuối một trang:

```json [settings]
{
  "scroll_beyond_last_line": "one_page"
}
```

2. Trình soạn thảo cuộn quá dòng cuối bằng đúng số dòng của `vertical_scroll_margin`:

```json [settings]
{
  "scroll_beyond_last_line": "vertical_scroll_margin"
}
```

3. Trình soạn thảo không cuộn quá dòng cuối:

```json [settings]
{
  "scroll_beyond_last_line": "off"
}
```

**Tùy chọn**

Giá trị `boolean`

## Độ nhạy cuộn

- Mô tả: Hệ số nhân độ nhạy cuộn. Hệ số này áp dụng cho cả giá trị delta ngang và dọc khi cuộn.
- Cài đặt: `scroll_sensitivity`
- Mặc định: `1.0`

**Tùy chọn**

Giá trị `float` dương

### Zoom bằng con lăn chuột

- Mô tả: Có phóng to thu nhỏ cỡ chữ trình soạn thảo bằng con lăn chuột khi giữ phím bổ trợ chính (Cmd trên macOS, Ctrl trên nền tảng khác) hay không.
- Cài đặt: `mouse_wheel_zoom`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

### Độ nhạy cuộn nhanh

- Mô tả: Hệ số nhân độ nhạy khi cuộn nhanh. Hệ số này áp dụng cho cả giá trị delta ngang và dọc khi cuộn. Cuộn nhanh xảy ra khi người dùng giữ phím alt hoặc option lúc cuộn.
- Cài đặt: `fast_scroll_sensitivity`
- Mặc định: `4.0`

**Tùy chọn**

Giá trị `float` dương

### Lề cuộn ngang

- Mô tả: Số ký tự giữ lại ở mỗi bên khi cuộn bằng chuột
- Cài đặt: `horizontal_scroll_margin`
- Mặc định: `5`

**Tùy chọn**

Giá trị `integer` không âm

### Lề cuộn dọc

- Mô tả: Số dòng giữ lại phía trên/dưới con trỏ khi cuộn bằng bàn phím
- Cài đặt: `vertical_scroll_margin`
- Mặc định: `3`

**Tùy chọn**

Giá trị `integer` không âm

## Tìm kiếm

- Mô tả: Các tùy chọn tìm kiếm bật sẵn khi mở tìm kiếm dự án và tìm kiếm trong buffer mới.
- Cài đặt: `search`
- Mặc định:

```json [settings]
{
  "search": {
    "button": true,
    "whole_word": false,
    "case_sensitive": false,
    "include_ignored": false,
    "regex": false,
    "center_on_match": false
  }
}
```

### Nút

- Mô tả: Có hiện nút tìm kiếm dự án trên thanh trạng thái hay không.
- Cài đặt: `button`
- Mặc định: `true`

### Nguyên từ

- Mô tả: Có chỉ khớp nguyên từ hay không.
- Cài đặt: `whole_word`
- Mặc định: `false`

### Phân biệt hoa thường

- Mô tả: Có khớp phân biệt hoa thường hay không. Cài đặt này ảnh hưởng đến cả
  tìm kiếm lẫn các action của trình soạn thảo như "Select Next Occurrence", "Select Previous
  Occurrence" và "Select All Occurrences".
- Cài đặt: `case_sensitive`
- Mặc định: `false`

### Bao gồm file bị ignore

- Mô tả: Có đưa các file bị gitignore vào kết quả tìm kiếm hay không.
- Cài đặt: `include_ignored`
- Mặc định: `false`

### Regex

- Mô tả: Có diễn giải truy vấn tìm kiếm như biểu thức chính quy hay không.
- Cài đặt: `regex`
- Mặc định: `false`

### Căn giữa theo kết quả khớp

- Mô tả: Có căn giữa con trỏ vào từng kết quả khớp khi điều hướng hay không.
- Cài đặt: `center_on_match`
- Mặc định: `false`

## Tìm kiếm vòng

- Mô tả: Nếu `search_wrap` bị tắt, kết quả tìm kiếm không quay vòng qua cuối file
- Cài đặt: `search_wrap`
- Mặc định: `true`

## Lấy truy vấn tìm kiếm từ con trỏ

- Mô tả: Khi nào điền sẵn truy vấn của một lần tìm kiếm mới dựa trên văn bản dưới con trỏ.
- Cài đặt: `seed_search_query_from_cursor`
- Mặc định: `always`

**Tùy chọn**

1. `always` luôn điền sẵn truy vấn bằng từ dưới con trỏ
2. `selection` chỉ điền sẵn truy vấn khi có văn bản đang được chọn
3. `never` không bao giờ điền sẵn truy vấn

## Semantic token

- Mô tả: Điều khiển cách dùng semantic token từ language server để tô màu cú pháp.
- Cài đặt: `semantic_tokens`
- Mặc định: `off`

**Tùy chọn**

1. `off`: Không yêu cầu semantic token từ language server.
2. `combined`: Dùng semantic token của LSP cùng với tô màu của tree-sitter.
3. `full`: Chỉ dùng semantic token của LSP, thay thế hoàn toàn tô màu của tree-sitter.

Để bật semantic token toàn cục:

```json [settings]
{
  "semantic_tokens": "combined"
}
```

Để bật semantic token cho một ngôn ngữ cụ thể:

```json [settings]
{
  "languages": {
    "Rust": {
      "semantic_tokens": "full"
    }
  }
}
```

Có thể cần khởi động lại language server để áp dụng đúng.

## Vùng gấp từ LSP

- Mô tả: Điều khiển việc dùng vùng gấp từ language server thay cho cách gấp theo tree-sitter và theo thụt lề. Mặc định là gấp theo tree-sitter và thụt lề; chúng được dùng làm phương án dự phòng khi LSP không trả dữ liệu gấp hoặc cài đặt này bị tắt.
- Cài đặt: `document_folding_ranges`
- Mặc định: `off`

**Tùy chọn**

1. `off`: Dùng cách gấp theo tree-sitter và thụt lề.
2. `on`: Dùng cách gấp của LSP bất cứ khi nào có thể, quay về tree-sitter và thụt lề khi server không trả kết quả.

Để bật vùng gấp từ LSP toàn cục:

```json [settings]
{
  "document_folding_ranges": "on"
}
```

Để bật vùng gấp từ LSP cho một ngôn ngữ cụ thể:

```json [settings]
{
  "languages": {
    "Rust": {
      "document_folding_ranges": "on"
    }
  }
}
```

## Document symbol từ LSP

- Mô tả: Điều khiển nguồn document symbol dùng cho outline và breadcrumb. Đây là tính năng LSP — khi bật, tree-sitter không được dùng cho document symbol mà dùng phản hồi `textDocument/documentSymbol` của language server.
- Cài đặt: `document_symbols`
- Mặc định: `off`

**Tùy chọn**

1. `off`: Dùng truy vấn tree-sitter để tính document symbol.
2. `on`: Dùng phản hồi LSP `textDocument/documentSymbol` của language server. Khi bật, tree-sitter không được dùng cho document symbol.

Để bật document symbol từ LSP toàn cục:

```json [settings]
{
  "document_symbols": "on"
}
```

Để bật document symbol từ LSP cho một ngôn ngữ cụ thể:

```json [settings]
{
  "languages": {
    "Rust": {
      "document_symbols": "on"
    }
  }
}
```

## Dùng tìm kiếm smartcase

- Mô tả: Khi bật, tự điều chỉnh độ phân biệt hoa thường theo truy vấn của bạn. Nếu truy vấn có chữ hoa, tìm kiếm phân biệt hoa thường; nếu chỉ có chữ thường, tìm kiếm không phân biệt hoa thường. \
  Điều này áp dụng cho cả tìm trong file và tìm trên toàn dự án.
- Cài đặt: `use_smartcase_search`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

Ví dụ:

- Tìm "function" sẽ khớp "function", "Function", "FUNCTION", v.v.
- Tìm "Function" chỉ khớp "Function", không khớp "function" hay "FUNCTION"

## Hoàn thành mã

- Mô tả: Điều khiển cách xử lý hoàn thành mã cho ngôn ngữ này.
- Cài đặt: `completions`
- Mặc định:

```json [settings]
{
  "completions": {
    "words": "fallback",
    "words_min_length": 3,
    "lsp": true,
    "lsp_fetch_timeout_ms": 0,
    "lsp_insert_mode": "replace_suffix"
  }
}
```

### Từ

- Mô tả: Điều khiển cách hoàn thành từ. Với tài liệu lớn, không phải mọi từ đều được lấy để hoàn thành.
- Cài đặt: `words`
- Mặc định: `fallback`

**Tùy chọn**

1. `enabled` - Luôn lấy các từ của tài liệu cho hoàn thành mã cùng với hoàn thành mã của LSP
2. `fallback` - Chỉ khi phản hồi LSP lỗi hoặc hết thời gian, mới dùng các từ của tài liệu để hiện gợi ý hoàn thành
3. `disabled` - Không bao giờ lấy hay hoàn thành các từ của tài liệu (vẫn có thể truy vấn hoàn thành theo từ qua một action riêng)

### Độ dài tối thiểu của truy vấn từ

- Mô tả: Số ký tự tối thiểu cần có để tự động kích hoạt hoàn thành theo từ.
  Dưới giá trị đó vẫn có thể kích hoạt hoàn thành theo từ thủ công bằng lệnh tương ứng của trình soạn thảo.
- Cài đặt: `words_min_length`
- Mặc định: `3`

**Tùy chọn**

Giá trị số nguyên dương

### LSP

- Mô tả: Có lấy hoàn thành mã từ LSP hay không.
- Cài đặt: `lsp`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Thời gian chờ lấy từ LSP (ms)

- Mô tả: Khi lấy hoàn thành mã từ LSP, xác định thời gian chờ phản hồi của một server cụ thể. Đặt 0 thì chờ vô thời hạn.
- Cài đặt: `lsp_fetch_timeout_ms`
- Mặc định: `0`

**Tùy chọn**

Giá trị `integer` tính bằng mili giây

### Chế độ chèn của LSP

- Mô tả: Điều khiển phạm vi được thay thế khi chấp nhận hoàn thành mã từ LSP.
- Cài đặt: `lsp_insert_mode`
- Mặc định: `replace_suffix`

**Tùy chọn**

1. `insert` - Thay văn bản trước con trỏ, dùng phạm vi `insert` trong đặc tả LSP
2. `replace` - Thay văn bản trước và sau con trỏ, dùng phạm vi `replace` trong đặc tả LSP
3. `replace_subsequence` - Hành xử như `"replace"` nếu văn bản sẽ bị thay là dãy con của văn bản hoàn thành, còn lại hành xử như `"insert"`
4. `replace_suffix` - Hành xử như `"replace"` nếu văn bản sau con trỏ là hậu tố của văn bản hoàn thành, còn lại hành xử như `"insert"`

## Hiện hoàn thành mã khi nhập

- Mô tả: Có hiện gợi ý hoàn thành mã khi bạn gõ hay không.
- Cài đặt: `show_completions_on_input`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hiện xem trước hoàn thành mã

- Mô tả: Có xem trước mục hoàn thành đang chọn ngay trong dòng, dưới dạng chữ mờ tại con trỏ, cho biết chấp nhận nó sẽ chèn gì. Sẽ không xem trước gì khi văn bản đã gõ không phải tiền tố nguyên văn của mục hoàn thành, điều thường gặp với khớp mờ.
- Cài đặt: `show_completion_preview`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hiện tài liệu của hoàn thành mã

- Mô tả: Có hiện tài liệu ngay trong dòng và bên cạnh cho các mục trong menu hoàn thành mã hay không.
- Cài đặt: `show_completion_documentation`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hiện dự đoán chỉnh sửa

- Mô tả: Có hiện dự đoán chỉnh sửa khi bạn gõ hay chỉ khi kích hoạt thủ công bằng `editor::ShowEditPrediction`.
- Cài đặt: `show_edit_predictions`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Hiện khoảng trắng

- Mô tả: Có hiển thị các ký tự khoảng trắng trong trình soạn thảo hay không.
- Cài đặt: `show_whitespaces`
- Mặc định: `selection`

**Tùy chọn**

1. `all`
2. `selection`
3. `none`
4. `boundary`

## Bản đồ khoảng trắng

- Mô tả: Chỉ định các ký tự dùng để hiển thị khoảng trắng khi show_whitespaces được bật.
- Cài đặt: `whitespace_map`
- Mặc định:

```json [settings]
{
  "whitespace_map": {
    "space": "•",
    "tab": "→"
  }
}
```

## Ngắt dòng mềm

- Mô tả: Có tự động ngắt dòng văn bản cho vừa trình soạn thảo / độ rộng ưa dùng hay không.
- Cài đặt: `soft_wrap`
- Mặc định: `none`

**Tùy chọn**

1. `none` nói chung không ngắt dòng, trừ khi dòng quá dài
2. `prefer_line` (đã lỗi thời, giống `none`)
3. `editor_width` ngắt các dòng tràn khỏi chiều rộng trình soạn thảo
4. `preferred_line_length` ngắt các dòng tràn quá giá trị cấu hình `preferred_line_length`
5. `bounded` ngắt dòng tại giá trị nhỏ hơn giữa `editor_width` và `preferred_line_length`

## Hiện đường dẫn ngắt dòng

- Mô tả: Có hiện đường dẫn ngắt dòng (thước dọc) trong trình soạn thảo hay không. Đặt true sẽ hiện một đường tại giá trị 'preferred_line_length' nếu 'soft_wrap' là 'preferred_line_length', và hiện thêm các đường khác theo cài đặt 'wrap_guides'.
- Cài đặt: `show_wrap_guides`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Dùng định dạng khi gõ

- Mô tả: Có dùng thêm truy vấn LSP để định dạng (và chỉnh) code sau mỗi ký hiệu "kích hoạt" được nhập, do khả năng của LSP server xác định
- Cài đặt: `use_on_type_format`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Dùng tự bao quanh

- Mô tả: Có tự bao quanh văn bản đang chọn khi gõ dấu mở ngoặc đơn, ngoặc vuông, ngoặc nhọn, nháy đơn hoặc nháy kép hay không. Ví dụ, khi bạn chọn văn bản rồi gõ '(', Zode sẽ bao văn bản bằng ().
- Cài đặt: `use_auto_surround`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Dùng hộp thoại đường dẫn của hệ thống

- Mô tả: Có dùng hộp thoại do hệ thống cung cấp cho Open và Save As hay không. Đặt false thì Zode dùng bộ chọn tích hợp ưu tiên bàn phím.
- Cài đặt: `use_system_path_prompts`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Dùng hộp thoại của hệ thống

- Mô tả: Có dùng hộp thoại do hệ thống cung cấp cho các lời nhắc, như hộp xác nhận, hay không. Đặt false thì Zode dùng lời nhắc tích hợp. Lưu ý trên Linux tùy chọn này bị bỏ qua và Zode luôn dùng lời nhắc tích hợp.
- Cài đặt: `use_system_prompts`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Đường dẫn ngắt dòng (thước dọc)

- Mô tả: Vị trí hiện thước dọc làm đường dẫn ngắt dòng. Tắt bằng cách đặt `show_wrap_guides` thành `false`.
- Cài đặt: `wrap_guides`
- Mặc định: []

**Tùy chọn**

Danh sách số cột kiểu `integer`

## Kích thước tab

- Mô tả: Số khoảng trắng dùng cho mỗi ký tự tab.
- Cài đặt: `tab_size`
- Mặc định: `4`

**Tùy chọn**

Giá trị `integer`

## Task

- Mô tả: Cấu hình cho các task có thể chạy trong Zode
- Cài đặt: `tasks`
- Mặc định:

```json [settings]
{
  "tasks": {
    "variables": {},
    "enabled": true,
    "prefer_lsp": false
  }
}
```

**Tùy chọn**

- `variables`: Biến tùy chỉnh cho cấu hình task
- `enabled`: Có bật task hay không
- `prefer_lsp`: Có ưu tiên task do LSP cung cấp hơn task của extension ngôn ngữ Zode hay không

## Telemetry

- Mô tả: Điều khiển thông tin Zode thu thập.
- Cài đặt: `telemetry`
- Mặc định:

```json [settings]
{
  "telemetry": {
    "diagnostics": true,
    "metrics": true
  }
}
```

**Tùy chọn**

### Chẩn đoán

- Mô tả: Cài đặt gửi dữ liệu liên quan đến gỡ lỗi, như báo cáo sự cố.
- Cài đặt: `diagnostics`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

### Số liệu

- Mô tả: Cài đặt gửi dữ liệu sử dụng đã ẩn danh, như bạn dùng Zode với những ngôn ngữ nào.
- Cài đặt: `metrics`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

## Terminal

- Mô tả: Cấu hình cho terminal.
- Cài đặt: `terminal`
- Lưu ý: khi gắn ở dưới, `default_height` đặt chiều cao của cả dock dưới, không chỉ terminal. Dock giữ một chiều cao chung cho mọi bảng gắn ở đó, nên chuyển sang bảng trình gỡ lỗi (debugger) không làm cạnh của nó dịch chuyển — và kéo cạnh đó khi bảng debugger đang hiện cũng đổi kích thước terminal.
- Mặc định:

```json [settings]
{
  "terminal": {
    "alternate_scroll": "off",
    "blinking": "terminal_controlled",
    "copy_on_select": false,
    "keep_selection_on_copy": true,
    "dock": "bottom",
    "default_width": 640,
    "default_height": 320,
    "detect_venv": {
      "on": {
        "directories": [".env", "env", ".venv", "venv"],
        "activate_script": "default"
      }
    },
    "env": {},
    "font_family": null,
    "font_features": null,
    "font_size": null,
    "line_height": "comfortable",
    "minimum_contrast": 45,
    "option_as_meta": false,
    "button": true,
    "shell": "system",
    "scroll_multiplier": 3.0,
    "toolbar": {
      "breadcrumbs": false
    },
    "working_directory": "current_project_directory",
    "scrollbar": {
      "show": null
    }
  }
}
```

### Terminal: Dock

- Mô tả: Điều khiển vị trí của dock
- Cài đặt: `dock`
- Mặc định: `bottom`

**Tùy chọn**

`"bottom"`, `"left"` hoặc `"right"`

### Terminal: Alternate Scroll

- Mô tả: Đặt chế độ Alternate Scroll (mã DECSET: `?1007`) có bật mặc định hay không. Chế độ Alternate Scroll đổi sự kiện cuộn chuột thành phím lên / xuống khi ở màn hình phụ (ví dụ khi chạy ứng dụng như vim hoặc less). Terminal vẫn có thể bật và tắt chế độ này bằng mã escape ANSI.
- Cài đặt: `alternate_scroll`
- Mặc định: `off`

**Tùy chọn**

1. Mặc định tắt chế độ alternate scroll

```json [settings]
{
  "terminal": {
    "alternate_scroll": "off"
  }
}
```

2. Mặc định bật chế độ alternate scroll

```json [settings]
{
  "terminal": {
    "alternate_scroll": "on"
  }
}
```

### Terminal: Nhấp nháy

- Mô tả: Đặt hành vi nhấp nháy của con trỏ trong terminal
- Cài đặt: `blinking`
- Mặc định: `terminal_controlled`

**Tùy chọn**

1. Không bao giờ nhấp nháy con trỏ, bỏ qua chế độ của terminal

```json [settings]
{
  "terminal": {
    "blinking": "off"
  }
}
```

2. Mặc định tắt nhấp nháy con trỏ, nhưng cho phép terminal bật nhấp nháy

```json [settings]
{
  "terminal": {
    "blinking": "terminal_controlled"
  }
}
```

3. Luôn nhấp nháy con trỏ, bỏ qua chế độ của terminal

```json [settings]
{
  "terminal": {
    "blinking": "on"
  }
}
```

### Terminal: Copy khi chọn

- Mô tả: Chọn văn bản trong terminal có tự động sao chép vào clipboard hệ thống hay không.
- Cài đặt: `copy_on_select`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

**Ví dụ**

```json [settings]
{
  "terminal": {
    "copy_on_select": true
  }
}
```

### Terminal: Hình dạng con trỏ

- Mô tả: Điều khiển hình dạng hiển thị của con trỏ trong terminal. Khi không đặt rõ ràng, mặc định là dạng khối.
- Cài đặt: `cursor_shape`
- Mặc định: `null` (mặc định là dạng khối)

**Tùy chọn**

1. Khối bao quanh ký tự phía sau

```json [settings]
{
  "terminal": {
    "cursor_shape": "block"
  }
}
```

2. Thanh dọc

```json [settings]
{
  "terminal": {
    "cursor_shape": "bar"
  }
}
```

3. Gạch chân chạy dọc theo ký tự phía sau

```json [settings]
{
  "terminal": {
    "cursor_shape": "underline"
  }
}
```

4. Khung vẽ quanh ký tự phía sau

```json [settings]
{
  "terminal": {
    "cursor_shape": "hollow"
  }
}
```

### Terminal: Giữ vùng chọn sau khi copy

- Mô tả: Có giữ vùng chọn trong terminal sau khi sao chép văn bản hay không.
- Cài đặt: `keep_selection_on_copy`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

**Ví dụ**

```json [settings]
{
  "terminal": {
    "keep_selection_on_copy": false
  }
}
```

### Terminal: Biến môi trường

- Mô tả: Mọi cặp khóa-giá trị thêm vào object này sẽ được thêm vào môi trường của terminal. Khóa phải là duy nhất, dùng `:` để phân tách nhiều giá trị trong một biến
- Cài đặt: `env`
- Mặc định: `{}`

**Ví dụ**

```json [settings]
{
  "terminal": {
    "env": {
      "ZED": "1",
      "KEY": "value1:value2"
    }
  }
}
```

### Terminal: Cỡ chữ

- Mô tả: Cỡ chữ dùng cho terminal. Khi không đặt, mặc định khớp với cỡ chữ của trình soạn thảo
- Cài đặt: `font_size`
- Mặc định: `null`

**Tùy chọn**

Giá trị `integer`

```json [settings]
{
  "terminal": {
    "font_size": 15
  }
}
```

### Terminal: Font

- Mô tả: Font dùng cho terminal. Khi không đặt, mặc định khớp với font của trình soạn thảo.
- Cài đặt: `font_family`
- Mặc định: `SF Mono` trên macOS, `JetBrains Mono NL` ở nơi khác

**Tùy chọn**

Tên của bất kỳ họ font nào đã cài trên máy người dùng

```json [settings]
{
  "terminal": {
    "font_family": "Berkeley Mono"
  }
}
```

### Terminal: Tính năng font

- Mô tả: Các tính năng font dùng cho terminal. Khi không đặt, mặc định khớp với tính năng font của trình soạn thảo.
- Cài đặt: `font_features`
- Mặc định: `null`
- Nền tảng: macOS và Windows.

**Tùy chọn**

Xem Tính năng font của buffer

```json [settings]
{
  "terminal": {
    "font_features": {
      "calt": false
      // See Buffer Font Features for more features
    }
  }
}
```

### Terminal: Chiều cao dòng

- Mô tả: Đặt chiều cao dòng của terminal.
- Cài đặt: `line_height`
- Mặc định: `standard`

**Tùy chọn**

1. Dùng chiều cao dòng `comfortable` dễ đọc, 1.618.

```json [settings]
{
  "terminal": {
    "line_height": "comfortable"
  }
}
```

2. Dùng chiều cao dòng `standard`, 1.3. Tùy chọn này hữu ích cho TUI, nhất là khi chúng dùng ký tự vẽ khung. (mặc định)

```json [settings]
{
  "terminal": {
    "line_height": "standard"
  }
}
```

3.  Dùng chiều cao dòng tùy chỉnh.

```json [settings]
{
  "terminal": {
    "line_height": {
      "custom": 2
    }
  }
}
```

### Terminal: Độ tương phản tối thiểu

- Mô tả: Điều khiển độ tương phản tối thiểu giữa màu chữ và màu nền trong terminal. Dùng APCA (Accessible Perceptual Contrast Algorithm) để điều chỉnh màu. Đặt 0 để tắt tính năng này.
- Cài đặt: `minimum_contrast`
- Mặc định: `45`

**Tùy chọn**

Giá trị `integer` từ 0 đến 106. Các giá trị khuyến nghị thường gặp:

- `0`: Không điều chỉnh độ tương phản
- `45`: Tối thiểu cho chữ lớn, mượt (mặc định)
- `60`: Tối thiểu cho nội dung văn bản khác
- `75`: Tối thiểu cho văn bản nội dung chính
- `90`: Nên dùng cho văn bản nội dung chính

```json [settings]
{
  "terminal": {
    "minimum_contrast": 45
  }
}
```

### Terminal: Option làm Meta

- Mô tả: Diễn giải lại phím option để hoạt động như phím 'meta', như trong Emacs.
- Cài đặt: `option_as_meta`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

```json [settings]
{
  "terminal": {
    "option_as_meta": true
  }
}
```

### Terminal: Shell

- Mô tả: Shell dùng khi khởi chạy terminal.
- Cài đặt: `shell`
- Mặc định: `system`

**Tùy chọn**

1. Dùng cấu hình terminal mặc định của hệ thống (thường là file `/etc/passwd`).

```json [settings]
{
  "terminal": {
    "shell": "system"
  }
}
```

2. Một chương trình để khởi chạy:

```json [settings]
{
  "terminal": {
    "shell": {
      "program": "sh"
    }
  }
}
```

3. Một chương trình kèm đối số:

```json [settings]
{
  "terminal": {
    "shell": {
      "with_arguments": {
        "program": "/bin/bash",
        "args": ["--login"]
      }
    }
  }
}
```

## Terminal: Phát hiện môi trường ảo {#terminal-detect_venv}

- Mô tả: Kích hoạt [Python Virtual Environment](https://docs.python.org/3/library/venv.html), nếu tìm thấy, trong thư mục làm việc của terminal (do working_directory xác định) và tự động kích hoạt môi trường ảo.
- Cài đặt: `detect_venv`
- Mặc định:

```json [settings]
{
  "terminal": {
    "detect_venv": {
      "on": {
        // Default directories to search for virtual environments, relative
        // to the current working directory. We recommend overriding this
        // in your project's settings, rather than globally.
        "directories": [".env", "env", ".venv", "venv"],
        // Can also be `csh`, `fish`, and `nushell`
        "activate_script": "default"
      }
    }
  }
}
```

Tắt bằng:

```json [settings]
{
  "terminal": {
    "detect_venv": "off"
  }
}
```

### Terminal: Hệ số cuộn

- Mô tả: Hệ số nhân tốc độ cuộn trong terminal khi dùng con lăn chuột hoặc trackpad.
- Cài đặt: `scroll_multiplier`
- Mặc định: `1.0`

**Tùy chọn**

Số thực dương. Giá trị nhỏ hơn hoặc bằng 0 sẽ bị ép về tối thiểu 0.01.

**Ví dụ**

```json
{
  "terminal": {
    "scroll_multiplier": 5.0
  }
}
```

## Terminal: Thanh công cụ

- Mô tả: Có hiện các thành phần khác nhau trên thanh công cụ của terminal hay không.
- Cài đặt: `toolbar`
- Mặc định:

```json [settings]
{
  "terminal": {
    "toolbar": {
      "breadcrumbs": false
    }
  }
}
```

**Tùy chọn**

Hiện tại chỉ có tùy chọn `breadcrumbs`, điều khiển việc hiện tiêu đề terminal, có thể đổi qua `PROMPT_COMMAND`.

Nếu tiêu đề terminal trống, breadcrumb sẽ không hiện.

Shell chạy trong terminal cần được cấu hình để phát ra tiêu đề.

Lệnh mẫu để đặt tiêu đề: `echo -e "\e]2;New Title\007";`

### Terminal: Nút

- Mô tả: Điều khiển hiện hay ẩn nút terminal trên thanh trạng thái
- Cài đặt: `button`
- Mặc định: `true`

**Tùy chọn**

Giá trị `boolean`

```json [settings]
{
  "terminal": {
    "button": false
  }
}
```

### Terminal: Thư mục làm việc

- Mô tả: Thư mục làm việc dùng khi khởi chạy terminal.
- Cài đặt: `working_directory`
- Mặc định: `"current_project_directory"`

**Tùy chọn**

1. Dùng thư mục của file hiện tại, nếu không được thì dùng thư mục dự án, rồi đến dự án đầu tiên trong workspace.

```json [settings]
{
  "terminal": {
    "working_directory": "current_file_directory"
  }
}
```

2. Dùng thư mục dự án của file hiện tại. Nếu không được thì dùng chiến lược thư mục dự án đầu tiên.

```json [settings]
{
  "terminal": {
    "working_directory": "current_project_directory"
  }
}
```

3. Dùng thư mục của dự án đầu tiên trong workspace này. Nếu không được thì dùng thư mục home của nền tảng này.

```json [settings]
{
  "terminal": {
    "working_directory": "first_project_directory"
  }
}
```

4. Luôn dùng thư mục home của nền tảng này nếu tìm thấy.

```json [settings]
{
  "terminal": {
    "working_directory": "always_home"
  }
}
```

5. Luôn dùng một thư mục cụ thể. Giá trị này sẽ được shell mở rộng. Nếu đường dẫn không phải thư mục hợp lệ, terminal sẽ mặc định dùng thư mục home của nền tảng này.

```json [settings]
{
  "terminal": {
    "working_directory": {
      "always": {
        "directory": "~/zed/projects/"
      }
    }
  }
}
```

### Terminal: Regex đường dẫn siêu liên kết

- Mô tả: Các regex dùng để nhận diện siêu liên kết đường dẫn. Regex có thể viết dưới hai dạng: một chuỗi regex duy nhất, hoặc một mảng chuỗi (sẽ được gộp thành một chuỗi regex nhiều dòng).
- Cài đặt: `path_hyperlink_regexes`
- Mặc định:

```json [settings]
{
  "terminal": {
    "path_hyperlink_regexes": [
      // Python-style diagnostics
      "File \"(?<path>[^\"]+)\", line (?<line>[0-9]+)",
      // Common path syntax with optional line, column, description, trailing punctuation, or
      // surrounding symbols or quotes
      [
        "(?x)",
        "# optionally starts with 0-2 opening prefix symbols",
        "[({\\[<]{0,2}",
        "# which may be followed by an opening quote",
        "(?<quote>[\"'`])?",
        "# `path` is the shortest sequence of any non-space character",
        "(?<link>(?<path>[^ ]+?",
        "    # which may end with a line and optionally a column,",
        "    (?<line_column>:+[0-9]+(:[0-9]+)?|:?\\([0-9]+([,:][0-9]+)?\\))?",
        "))",
        "# which must be followed by a matching quote",
        "(?(<quote>)\\k<quote>)",
        "# and optionally a single closing symbol",
        "[)}\\]>]?",
        "# if line/column matched, may be followed by a description",
        "(?(<line_column>):[^ 0-9][^ ]*)?",
        "# which may be followed by trailing punctuation",
        "[.,:)}\\]>]*",
        "# and always includes trailing whitespace or end of line",
        "([ ]+|$)"
      ]
    ]
  }
}
```

### Terminal: Thời gian chờ siêu liên kết đường dẫn (ms)

- Mô tả: Thời gian tối đa để tìm một siêu liên kết đường dẫn. Đặt 0 thì tắt siêu liên kết đường dẫn.
- Cài đặt: `path_hyperlink_timeout_ms`
- Mặc định: `1`

## REPL

- Mô tả: Cài đặt Repl.
- Cài đặt: `repl`
- Mặc định:

```json [settings]
{
  "repl": {
    // Maximum number of columns to keep in REPL's scrollback buffer.
    // Clamped with [20, 512] range.
    "max_columns": 128,
    // Maximum number of lines to keep in REPL's scrollback buffer.
    // Clamped with [4, 256] range.
    "max_lines": 32
  }
}
```

## Giao diện (theme)

- Mô tả: Cài đặt giao diện (theme) có thể viết dưới hai dạng: tên của một giao diện, hoặc một object chứa các giao diện `mode`, `dark` và `light` cho UI của Zode.
- Cài đặt: `theme`
- Mặc định: `Dark 2026`

### Object giao diện

- Mô tả: Chỉ định giao diện bằng một object gồm các giao diện `mode`, `dark` và `light`.
- Cài đặt: `theme`
- Mặc định:

```json [settings]
{
  "theme": {
    "mode": "system",
    "dark": "Dark 2026",
    "light": "Light 2026"
  }
}
```

### Chế độ

- Mô tả: Chỉ định chế độ của giao diện.
- Cài đặt: `mode`
- Mặc định: `system`

**Tùy chọn**

1. Đặt giao diện ở chế độ tối

```json [settings]
{
  "theme": {
    "mode": "dark",
    "dark": "One Dark",
    "light": "One Light"
  }
}
```

2. Đặt giao diện ở chế độ sáng

```json [settings]
{
  "theme": {
    "mode": "light",
    "dark": "One Dark",
    "light": "One Light"
  }
}
```

3. Đặt giao diện theo chế độ của hệ thống

```json [settings]
{
  "theme": {
    "mode": "system",
    "dark": "One Dark",
    "light": "One Light"
  }
}
```

### Tối

- Mô tả: Tên giao diện Zode tối dùng cho UI.
- Cài đặt: `dark`
- Mặc định: `One Dark`

**Tùy chọn**

Chạy action {#action theme_selector::Toggle} trong bảng lệnh (command palette) để xem danh sách tên giao diện hợp lệ hiện có.

### Sáng

- Mô tả: Tên giao diện Zode sáng dùng cho UI.
- Cài đặt: `light`
- Mặc định: `One Light`

**Tùy chọn**

Chạy action {#action theme_selector::Toggle} trong bảng lệnh (command palette) để xem danh sách tên giao diện hợp lệ hiện có.

## Thanh tiêu đề

- Mô tả: Có hiện các thành phần khác nhau trên thanh tiêu đề hay không
- Cài đặt: `title_bar`
- Mặc định:

```json [settings]
{
  "title_bar": {
    "show_branch_status_icon": false,
    "show_branch_name": true,
    "show_project_items": true,
    "show_onboarding_banner": true,
    "show_menus": false,
    "button_layout": "platform_default"
  }
}
```

**Tùy chọn**

- `show_branch_status_icon`: Có hiện chỉ báo trạng thái git trên icon nhánh ở thanh tiêu đề hay không
- `show_branch_name`: Có hiện nút tên nhánh ở thanh tiêu đề hay không
- `show_project_items`: Có hiện host và tên dự án ở thanh tiêu đề hay không
- `show_onboarding_banner`: Có hiện banner làm quen ở thanh tiêu đề hay không
- `show_user_picture`: Có hiện ảnh người dùng ở thanh tiêu đề hay không
- `show_user_menu`: Có hiện nút menu người dùng ở thanh tiêu đề hay không (nút mặc định hiện avatar của bạn và chứa các mục như Settings, Keymap, Themes, v.v.)
- `show_sign_in`: Có hiện nút đăng nhập ở thanh tiêu đề hay không
- `show_menus`: Có hiện các menu trên một hàng riêng phía trên thanh tiêu đề hay không. Trên Windows và Linux, menu luôn hiện trong thanh tiêu đề
- `button_layout`: Bố cục các nút điều khiển cửa sổ trên thanh tiêu đề (chỉ Linux). Có thể đặt `"platform_default"` để theo cài đặt hệ thống, `"standard"` để dùng bố cục tích hợp của Zode, hoặc định dạng tùy chỉnh như `"close:minimize,maximize"`

## Vim

- Mô tả: Có bật chế độ vim hay không.
- Cài đặt: `vim_mode`
- Mặc định: `false`

## Khi đóng mà không còn tab

- Mô tả: Cửa sổ có nên đóng khi dùng 'close active item' trên cửa sổ không còn tab hay không
- Cài đặt: `when_closing_with_no_tabs`
- Mặc định: `"platform_default"`

**Tùy chọn**

1. Dùng hành vi mặc định của nền tảng:

```json [settings]
{
  "when_closing_with_no_tabs": "platform_default"
}
```

2. Luôn đóng cửa sổ:

```json [settings]
{
  "when_closing_with_no_tabs": "close_window"
}
```

3. Không bao giờ đóng cửa sổ:

```json [settings]
{
  "when_closing_with_no_tabs": "keep_window_open"
}
```

## Bảng dự án

- Mô tả: Tùy biến bảng dự án
- Cài đặt: `project_panel`
- Lưu ý: bảng dự án luôn gắn ở bên phải cửa sổ, đối diện thanh dự án; không có cài đặt `dock` cho nó.
- Lưu ý: `default_width` đặt chiều rộng của cả cột công cụ, không chỉ bảng này. Cột hiện từng bảng một và giữ một chiều rộng duy nhất, nên chuyển sang lịch sử agent không làm cạnh của nó dịch chuyển — và kéo cạnh đó khi lịch sử agent đang hiện cũng đổi kích thước cả cột.
- Mặc định:

```json [settings]
{
  "project_panel": {
    "button": true,
    "default_width": 240,
    "entry_spacing": "comfortable",
    "file_icons": true,
    "folder_icons": true,
    "git_status": true,
    "indent_size": 20,
    "auto_reveal_entries": true,
    "auto_fold_dirs": true,
    "bold_folder_labels": false,
    "drag_and_drop": true,
    "scrollbar": {
      "show": null,
      "horizontal_scroll": true
    },
    "sticky_scroll": true,
    "show_diagnostics": "all",
    "indent_guides": {
      "show": "on_hover"
    },
    "sort_mode": "directories_first",
    "hide_root": false,
    "hide_hidden": false,
    "starts_open": true,
    "auto_open": {
      "on_create": true,
      "on_paste": true,
      "on_drop": true
    }
  }
}
```

### Khoảng cách giữa các mục

- Mô tả: Khoảng cách giữa các mục worktree
- Cài đặt: `entry_spacing`
- Mặc định: `comfortable`

**Tùy chọn**

1. Khoảng cách mục thoải mái

```json [settings]
{
  "project_panel": {
    "entry_spacing": "comfortable"
  }
}
```

2. Khoảng cách mục tiêu chuẩn

```json [settings]
{
  "project_panel": {
    "entry_spacing": "standard"
  }
}
```

### Trạng thái Git

- Mô tả: Đánh dấu các file mới tạo và mới cập nhật
- Cài đặt: `git_status`
- Mặc định: `true`

**Tùy chọn**

1. Mặc định bật trạng thái git

```json [settings]
{
  "project_panel": {
    "git_status": true
  }
}
```

2. Mặc định tắt trạng thái git

```json [settings]
{
  "project_panel": {
    "git_status": false
  }
}
```

### Chiều rộng mặc định

- Mô tả: Tùy biến chiều rộng mặc định của bảng dự án
- Cài đặt: `default_width`
- Mặc định: `240`

**Tùy chọn**

Giá trị `float`

### Tự động hiện mục

- Mô tả: Có tự động hiện mục trong bảng dự án khi mục dự án tương ứng trở thành đang hoạt động hay không. Mục bị gitignore không bao giờ được tự động hiện.
- Cài đặt: `auto_reveal_entries`
- Mặc định: `true`

**Tùy chọn**

1. Bật tự động hiện mục

```json [settings]
{
  "project_panel": {
    "auto_reveal_entries": true
  }
}
```

2. Tắt tự động hiện mục

```json [settings]
{
  "project_panel": {
    "auto_reveal_entries": false
  }
}
```

### Tự động gộp thư mục

- Mô tả: Có tự động gộp thư mục khi thư mục chỉ chứa đúng một thư mục con hay không.
- Cài đặt: `auto_fold_dirs`
- Mặc định: `true`

**Tùy chọn**

1. Bật tự động gộp thư mục

```json [settings]
{
  "project_panel": {
    "auto_fold_dirs": true
  }
}
```

2. Tắt tự động gộp thư mục

```json [settings]
{
  "project_panel": {
    "auto_fold_dirs": false
  }
}
```

### Nhãn thư mục in đậm

- Mô tả: Có hiện tên thư mục bằng chữ đậm trong bảng dự án hay không.
- Cài đặt: `bold_folder_labels`
- Mặc định: `false`

**Tùy chọn**

1. Bật nhãn thư mục in đậm

```json [settings]
{
  "project_panel": {
    "bold_folder_labels": true
  }
}
```

2. Tắt nhãn thư mục in đậm

```json [settings]
{
  "project_panel": {
    "bold_folder_labels": false
  }
}
```

### Kích thước thụt lề

- Mô tả: Mức thụt lề (tính bằng pixel) cho các mục lồng nhau.
- Cài đặt: `indent_size`
- Mặc định: `20`

### Hướng dẫn thụt lề: Hiển thị

- Mô tả: Khi nào hiện hướng dẫn thụt lề trong bảng dự án.
- Cài đặt: `indent_guides`
- Mặc định:

```json [settings]
{
  "project_panel": {
    "indent_guides": {
      "show": "on_hover"
    }
  }
}
```

**Tùy chọn**

1. Luôn hiện hướng dẫn thụt lề trong bảng dự án

```json [settings]
{
  "project_panel": {
    "indent_guides": {
      "show": "always"
    }
  }
}
```

2. Chỉ hiện khi con trỏ chuột nằm trong bảng dự án

```json [settings]
{
  "project_panel": {
    "indent_guides": {
      "show": "on_hover"
    }
  }
}
```

3. Ẩn hướng dẫn thụt lề trong bảng dự án

```json [settings]
{
  "project_panel": {
    "indent_guides": {
      "show": "never"
    }
  }
}
```

### Thanh cuộn

- Mô tả: Cài đặt liên quan đến thanh cuộn của bảng dự án.
- Cài đặt: `scrollbar`
- Mặc định:

```json [settings]
{
  "project_panel": {
    "scrollbar": {
      "show": null,
      "horizontal_scroll": true
    }
  }
}
```

**Tùy chọn**

- `show`: Có hiện thanh cuộn trong bảng dự án hay không. Giá trị có thể: null, "auto", "system", "always", "never". Kế thừa cài đặt của trình soạn thảo khi không có, xem mô tả của nó để biết thêm chi tiết.
- `horizontal_scroll`: Có cho phép cuộn ngang trong bảng dự án hay không. Khi là `false`, khung nhìn bị khóa ở vị trí ngoài cùng bên trái và tên file dài bị cắt.

### Chế độ sắp xếp

- Mô tả: Thứ tự sắp xếp các mục trong bảng dự án
- Cài đặt: `sort_mode`
- Mặc định: `directories_first`

**Tùy chọn**

1. Hiện thư mục trước, rồi đến file

```json [settings]
{
  "project_panel": {
    "sort_mode": "directories_first"
  }
}
```

2. Trộn lẫn thư mục và file

```json [settings]
{
  "project_panel": {
    "sort_mode": "mixed"
  }
}
```

3. Hiện file trước, rồi đến thư mục

```json [settings]
{
  "project_panel": {
    "sort_mode": "files_first"
  }
}
```

### Thứ tự sắp xếp

- Mô tả: Có sắp xếp tên file và thư mục phân biệt hoa thường trong bảng dự án hay không. Cài đặt này kết hợp với `sort_mode`. `sort_mode` điều khiển cách nhóm file và thư mục (ví dụ thư mục trước), còn cài đặt này điều khiển cách so sánh tên trong các nhóm đó.
- Cài đặt: `sort_order`
- Mặc định: `default`

**Tùy chọn**

1. Sắp xếp tự nhiên không phân biệt hoa thường, ưu tiên chữ thường khi bằng nhau. Số trong tên file được so theo giá trị số (ví dụ `file2` đứng trước `file10`). Tên chỉ khác nhau ở kiểu chữ được xếp chữ thường trước (ví dụ `apple` trước `Apple`).

```json [settings]
{
  "project_panel": {
    "sort_order": "default"
  }
}
```

2. Tên chữ hoa được nhóm trước tên chữ thường, với sắp xếp tự nhiên không phân biệt hoa thường trong mỗi nhóm. Tên bắt đầu bằng dấu chấm (ví dụ `.gitignore`) đứng trước cả hai nhóm. Chữ hoa có dấu như `É` được coi là chữ hoa.

```json [settings]
{
  "project_panel": {
    "sort_order": "upper"
  }
}
```

3. Tên chữ thường được nhóm trước tên chữ hoa, với sắp xếp tự nhiên không phân biệt hoa thường trong mỗi nhóm. Tên bắt đầu bằng dấu chấm đứng trước cả hai nhóm.

```json [settings]
{
  "project_panel": {
    "sort_order": "lower"
  }
}
```

4. So sánh thuần theo codepoint Unicode. Không gộp hoa thường và không sắp xếp số tự nhiên. Chữ ASCII hoa (`A`–`Z`) đứng trước chữ thường (`a`–`z`) là hệ quả tự nhiên của giá trị codepoint. Ký tự có dấu như `É` (U+00C9) đứng sau mọi chữ cái ASCII. Số được so sánh theo thứ tự từ điển (`file10` đứng trước `file2`).

```json [settings]
{
  "project_panel": {
    "sort_order": "unicode"
  }
}
```

### Tự động mở

- Mô tả: Điều khiển việc file có được mở tự động sau các luồng tạo khác nhau trong bảng dự án hay không.
- Cài đặt: `auto_open`
- Mặc định:

```json [settings]
{
  "project_panel": {
    "auto_open": {
      "on_create": true,
      "on_paste": true,
      "on_drop": true
    }
  }
}
```

**Tùy chọn**

- `on_create`: Có tự động mở file mới tạo trong trình soạn thảo hay không.
- `on_paste`: Có tự động mở file sau khi dán hoặc nhân đôi hay không.
- `on_drop`: Có tự động mở file được thả từ nguồn bên ngoài hay không.

## Trình gỡ lỗi (debugger)

- Mô tả: Cấu hình cho bảng debugger và các cài đặt của nó
- Cài đặt: `debugger`
- Mặc định:

```json [settings]
{
  "debugger": {
    "stepping_granularity": "line",
    "save_breakpoints": true,
    "dock": "bottom",
    "button": true
  }
}
```

Xem [trang trình gỡ lỗi](../debugger.md) để biết thêm về hỗ trợ gỡ lỗi trong Zode.

## Bảng Git

- Mô tả: Cài đặt để tùy biến hành vi của bảng git.
- Cài đặt: `git_panel`
- Lưu ý: `default_width` đặt chiều rộng của cả dock trái, không chỉ bảng này. Dock giữ một chiều rộng chung cho mọi bảng gắn ở đó — hiện do `default_width` của bảng nhánh quyết định, vì nó giữ `activation_priority` thấp nhất ở cạnh đó. Chuyển sang bảng outline hay bảng nhánh không làm cạnh dock dịch chuyển, và kéo cạnh đó khi một trong hai đang hiện sẽ đổi `default_width` của bảng nhánh chứ không phải của bảng này.
- Mặc định:

```json [settings]
{
  "git_panel": {
    "button": true,
    "dock": "left",
    "default_width": 360,
    "status_style": "icon",
    "fallback_branch_name": "main",
    "sort_by_path": false,
    "collapse_untracked_diff": false,
    "scrollbar": {
      "show": null
    },
    "starts_open": false
  }
}
```

**Tùy chọn**

- `button`: Có hiện nút bảng git trên thanh trạng thái hay không
- `dock`: Nơi gắn bảng git. Có thể là `left` hoặc `right`
- `default_width`: Chiều rộng mặc định của bảng git
- `status_style`: Cách hiện trạng thái git. Có thể là `label_color` hoặc `icon`
- `fallback_branch_name`: Tên nhánh dùng nếu `init.defaultBranch` chưa được đặt
- `sort_by_path`: Có sắp xếp các mục trong bảng theo đường dẫn hay theo trạng thái (mặc định)
- `collapse_untracked_diff`: Có thu gọn các file chưa được theo dõi trong bảng diff hay không
- `scrollbar`: Khi nào hiện thanh cuộn trong bảng git
- `starts_open`: Bảng git có mở khi khởi động hay không, khi workspace chưa có trạng thái dock riêng đã lưu. Khi một dock đã khôi phục một bản ghi — kể cả bản ghi có bảng đang đóng — bản ghi đó được ưu tiên và `starts_open` không còn áp dụng.

## Thư mục Git Worktree

- Mô tả: Thư mục nơi git worktree được tạo, tính tương đối so với thư mục làm việc của repository.
- Cài đặt: `git.worktree_directory`
- Mặc định: `"../worktrees"`

Khi thư mục đã phân giải nằm ngoài gốc dự án, tên thư mục của dự án được tự động nối vào để các repo cùng cấp không đụng nhau. Ví dụ, với giá trị mặc định `"../worktrees"` và dự án ở `~/code/zed`, worktree được tạo dưới `~/code/worktrees/zed/`.

Khi thư mục đã phân giải nằm trong gốc dự án thì không thêm thành phần nào (nó đã gắn với dự án rồi).

**Ví dụ**:

- `"../worktrees"` — `~/code/worktrees/<project>/` (mặc định)
- `".git/zed-worktrees"` — `<project>/.git/zed-worktrees/`
- `"my-worktrees"` — `<project>/my-worktrees/`

Dấu gạch chéo ở cuối bị bỏ qua.

```json [settings]
{
  "git": {
    "worktree_directory": "../worktrees"
  }
}
```

## Nhà cung cấp lưu trữ Git

- Mô tả: Đăng ký các instance GitHub, GitLab hoặc Bitbucket tự lưu trữ để hash commit, tham chiếu issue và permalink trỏ đúng host.
- Cài đặt: `git_hosting_providers`
- Mặc định: `[]`

**Tùy chọn**

Mỗi mục nhận:

- `provider`: Một trong `github`, `gitlab` hoặc `bitbucket`
- `name`: Tên hiển thị của instance
- `base_url`: URL gốc, ví dụ `https://git.example.corp`

Bạn có thể định nghĩa chúng trong cài đặt người dùng hoặc cài đặt dự án; cài đặt dự án được gộp lên trên cài đặt người dùng.

```json [settings]
{
  "git_hosting_providers": [
    {
      "provider": "github",
      "name": "BigCorp GitHub",
      "base_url": "https://git.example.corp"
    }
  ]
}
```

## Bảng Outline

- Mô tả: Tùy biến bảng Outline
- Cài đặt: `outline_panel`
- Mặc định:

```json [settings]
{
  "outline_panel": {
    "button": true,
    "default_width": 300,
    "dock": "left",
    "file_icons": true,
    "folder_icons": true,
    "git_status": true,
    "indent_size": 20,
    "auto_reveal_entries": true,
    "auto_fold_dirs": true,
    "indent_guides": {
      "show": "always"
    },
    "scrollbar": {
      "show": null
    }
  }
}
```

## Bảng Devices

- Mô tả: Tùy biến bảng Devices, nơi liệt kê simulator iOS, emulator Android và thiết bị thật. Xem [Thiết bị](../devices.md).
- Cài đặt: `devices_panel`
- Lưu ý: bảng luôn nằm ở cạnh phải cửa sổ; không có cài đặt `dock` cho nó.
- Mặc định:

```json [settings]
{
  "devices_panel": {
    "button": true,
    "default_width": 320
  }
}
```

## Tô màu ngoặc

- Mô tả: Có dùng truy vấn ngoặc của tree-sitter để phát hiện và tô màu các dấu ngoặc trong trình soạn thảo hay không (còn gọi là "rainbow brackets").
- Cài đặt: `colorize_brackets`
- Mặc định: `false`

**Tùy chọn**

Giá trị `boolean`

Màu dùng cho từng mức thụt lề được định nghĩa trong giao diện (theme) (khóa theme: `accents`). Có thể tùy biến bằng ghi đè theme.

## Làm mờ code không cần thiết

- Mô tả: Mức làm mờ code không dùng.
- Cài đặt: `unnecessary_code_fade`
- Mặc định: `0.3`

**Tùy chọn**

Giá trị float từ `0.0` đến `0.9`, trong đó:

- `0.0` nghĩa là không làm mờ (code không dùng trông như code đang dùng)
- `0.9` nghĩa là làm mờ tối đa (code không dùng rất mờ nhưng vẫn thấy được)

**Ví dụ**

```json [settings]
{
  "unnecessary_code_fade": 0.5
}
```

## Độ mờ của cửa sổ

- Mô tả: Mức đục của nền cửa sổ. Mọi nền mờ đều nhau, nên toàn bộ cửa sổ để lộ thứ phía sau — màn hình nền hoặc ảnh nền — ở cùng mức. Trên macOS chữ và icon vẫn đặc (vibrancy); trên Windows và Linux hiện tại cả cửa sổ đều mờ, kể cả chữ.
- Cài đặt: `window_opacity`
- Mặc định: `1.0`

**Tùy chọn**

Giá trị float từ `0.0` (trong suốt) đến `1.0` (đặc). Khi bật vibrancy hoặc ảnh nền, các nền giữ [`window_vibrancy_opacity`](#window-vibrancy-opacity) của giá trị này, nên hiệu ứng kính vẫn hiện kể cả ở `1.0`.

## Vibrancy của cửa sổ

- Mô tả: Cho thấy thứ phía sau cửa sổ qua các nền của nó, đã làm nhòe, trong khi chữ và icon vẫn đặc. Khi không có ảnh nền, màn hình nền hiện qua vật liệu vibrancy của hệ thống (trên macOS là cùng vật liệu Tabby dùng); khi có ảnh nền thì ảnh hiện qua, đã làm nhòe.
- Cài đặt: `window_vibrancy`
- Mặc định: `false`

**Tùy chọn**

`true` hoặc `false`.

## Độ mờ vibrancy của cửa sổ

- Mô tả: Phần `window_opacity` mà các nền cửa sổ giữ lại khi vibrancy hoặc ảnh nền đang bật. Càng thấp thì càng lộ nhiều thứ phía sau cửa sổ. Không có tác dụng khi cả hai đều tắt.
- Cài đặt: `window_vibrancy_opacity`
- Mặc định: `0.8`

**Tùy chọn**

Giá trị float từ `0.0` (nền hoàn toàn trong suốt) đến `1.0` (nền đục như `window_opacity`).

## Ảnh nền cửa sổ

- Mô tả: Ảnh hiện phía sau các nền của cửa sổ, phủ toàn bộ cửa sổ.
- Cài đặt: `window_background_image`
- Mặc định: `""` (không có ảnh)

**Tùy chọn**

Đường dẫn của một file ảnh trên máy này; `~/` được mở rộng. Ảnh được đọc từ đường dẫn đó và không bao giờ được sao chép, nên di chuyển hoặc xóa file sẽ gỡ ảnh khỏi cửa sổ, còn thay file sẽ hiện ảnh mới.

**Ví dụ**

```json [settings]
{
  "window_opacity": 1.0,
  "window_vibrancy": true,
  "window_vibrancy_opacity": 0.7,
  "window_background_image": "~/Pictures/wallpaper.jpg"
}
```

## Font UI

- Mô tả: Tên font dùng cho văn bản trong UI.
- Cài đặt: `ui_font_family`
- Mặc định: `SF Pro` trên macOS, có sẵn trong hệ thống; nơi khác là [Inter](https://rsms.me/inter/), đi kèm Zode, nên mặc định chạy được trên máy không cài font nào.

**Tùy chọn**

Tên của bất kỳ họ font nào đã cài trên hệ thống, `".ZodeSans"` để theo bất kỳ font sans nào Zode đi kèm mà không cần gọi tên, hoặc `".SystemUIFont"` để dùng font UI mặc định của hệ thống (trên macOS và Windows).

## Tính năng font UI

- Mô tả: Các tính năng OpenType được bật cho văn bản trong UI.
- Cài đặt: `ui_font_features`
- Mặc định:

```json [settings]
{
  "ui_font_features": {
    "calt": false
  }
}
```

- Nền tảng: macOS và Windows.

**Tùy chọn**

Zode hỗ trợ mọi tính năng OpenType có thể bật hoặc tắt cho một font UI, cũng như đặt giá trị cho các tính năng font.

Ví dụ, để tắt ligature của font, thêm đoạn sau vào cài đặt:

```json [settings]
{
  "ui_font_features": {
    "calt": false
  }
}
```

Bạn cũng có thể đặt các tính năng OpenType khác, chẳng hạn đặt `cv01` thành `7`:

```json [settings]
{
  "ui_font_features": {
    "cv01": 7
  }
}
```

## Font dự phòng của UI

- Mô tả: Font dự phòng dùng cho văn bản trong UI.
- Cài đặt: `ui_font_fallbacks`
- Mặc định: `null`
- Nền tảng: macOS và Windows.

**Tùy chọn**

Ví dụ, để dùng `Nerd Font` làm font dự phòng, thêm đoạn sau vào cài đặt:

```json [settings]
{
  "ui_font_fallbacks": ["Nerd Font"]
}
```

## Cỡ chữ UI

- Mô tả: Cỡ chữ mặc định cho văn bản trong UI.
- Cài đặt: `ui_font_size`
- Mặc định: `16`

**Tùy chọn**

Giá trị `integer` từ `6` đến `100` pixel (gồm cả hai đầu)

## Độ đậm font UI

- Mô tả: Độ đậm font mặc định cho văn bản trong UI.
- Cài đặt: `ui_font_weight`
- Mặc định: `400`

**Tùy chọn**

Giá trị `integer` từ `100` đến `900`

## Profile cài đặt

- Mô tả: Cấu hình số lượng tùy ý profile cài đặt được áp dụng tạm thời khi chọn từ `settings profile selector: toggle`.
- Cài đặt: `profiles`
- Mặc định: `{}`

Trong file `settings.json`, thêm object `profiles`.
Mỗi khóa trong object này là tên của một profile cài đặt. Mỗi profile có trường `base` tùy chọn (`"user"` hoặc `"default"`) và một object `settings` chứa bất kỳ cài đặt nào của Zode.

Ví dụ:

```json [settings]
{
  "profiles": {
    "Presenting (Dark)": {
      "settings": {
        "buffer_font_size": 18.0,
        "theme": "One Dark",
        "ui_font_size": 18.0
      }
    },
    "Presenting (Light)": {
      "settings": {
        "buffer_font_size": 18.0,
        "theme": "One Light",
        "ui_font_size": 18.0
      }
    },
    "Writing": {
      "settings": {
        "buffer_font_size": 15.0,
        "theme": "Catppuccin Frappé - No Italics",
        "ui_font_size": 15.0,
        "tab_bar": { "show": false },
        "toolbar": { "breadcrumbs": false }
      }
    }
  }
}
```

Để xem trước và bật một profile cài đặt, mở bảng lệnh (command palette) bằng {#kb command_palette::Toggle} và tìm `settings profile selector: toggle`.

## Một cấu hình mẫu:

```json [settings]
// ~/.config/zode/settings.json
{
  "theme": "cave-light",
  "tab_size": 2,
  "preferred_line_length": 80,
  "soft_wrap": "none",

  "buffer_font_size": 18,
  "buffer_font_family": ".ZodeMono",

  "autosave": "on_focus_change",
  "format_on_save": "off",
  "vim_mode": false,
  "terminal": {
    "font_family": "FiraCode Nerd Font Mono",
    "blinking": "off"
  },
  "languages": {
    "C": {
      "format_on_save": "on",
      "formatter": "language_server",
      "preferred_line_length": 64,
      "soft_wrap": "bounded"
    }
  }
}
```
