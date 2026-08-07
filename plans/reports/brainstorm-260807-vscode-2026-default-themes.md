# Brainstorm — Port VSCode "Dark 2026" / "Light 2026" thành theme mặc định của zode

**Ngày:** 2026-08-07 · **Lens:** CTO · **Trạng thái:** thiết kế đã chốt, chưa triển khai

## Commission

Đổi theme dark/light mặc định của zode cho giống theme **Light 2026** và **Dark 2026** của VSCode,
lấy trực tiếp từ bản VSCode đang cài trên máy (`/Applications/Visual Studio Code.app`).

## Khảo sát — sự thật đo được

### Phía VSCode
- `theme-defaults/themes/2026-dark.json`, `2026-light.json` tồn tại, đọc được.
- Cả hai dùng `"include"`: `2026-dark → dark_modern → dark_plus → dark_vs`.
  Sau khi flatten: **dark 328 màu / 118 tokenColors**, **light 342 màu / 113 tokenColors**.
- `tokenColors` của 2026 là bảng màu kiểu GitHub (comment `#8b949e`, keyword `#ff7b72`,
  string `#a5d6ff`, function `#d2a8ff`, constant `#79c0ff`) — khác hẳn One Dark.
- Accent hệ thống: `#3994BC` (dark) / `#0069CC` (light).
- **`terminal.ansi*` = 0 key** trong cả chuỗi include. VSCode rơi về bảng ANSI hardcode
  trong `workbench.desktop.main.js`; đã trích được đủ 16 màu kèm biến thể light/dark.

### Phía zode
- Theme JSON của Zed = **màu thuần**: 141 style key + syntax + players. Không có spacing,
  không có font size, **không có border radius**.
- `crates/theme_importer` có sẵn nhưng **không xử lý `include`** (`VsCodeTheme` chỉ đọc
  `colors` + `tokenColors` của một file).
- Bundled theme nạp qua `crates/assets` (`#[include = "themes/**/*"]`) → thêm thư mục là đủ.
- Mặc định hiện tại: `assets/settings/default.json:11-12` và
  `crates/theme/src/theme.rs:45` (`DEFAULT_DARK_THEME`).

### Border radius — câu hỏi ban đầu, và câu trả lời
Không nằm trong theme. 227 chỗ `.rounded_*()` rải khắp `crates/`, nhưng **tất cả trỏ về một thang
duy nhất** ở `crates/gpui_macros/src/styles.rs:1285`: `xs=2px, sm=4px, md=6px, lg=8px, xl=12px`
(thang Tailwind). Thang này **đã trùng với VSCode 2026**.
→ **Không đụng tới radius.** Khác biệt thị giác giữa hai IDE gần như 100% là màu.

### Kiểm chứng thực tế: chạy thử importer
Đã flatten + chạy `cargo run -p theme_importer` trên `2026-dark`. Kết quả:

| Hạng mục | Importer điền được | Cần |
|---|---|---|
| Style key | **58** | 141 (thiếu **92**) |
| Syntax key | 34 | 45 |
| `players` | 0 (mảng rỗng) | 8 |

**Lỗi nhìn thấy được trong output tự động:**
- `border.transparent = "#2A2B2CFF"` — đục, phải là `#00000000`.
- `border.variant`, `border.selected` bị gộp về cùng giá trị với `border` → mất phân tầng viền.

→ Chạy importer rồi commit thẳng là hỏng. Ước lượng ban đầu (70-80%) sai; con số thật là 41%.

### Truy nguồn 92 key thiếu
Gần như toàn bộ đều dẫn xuất được từ chính palette 2026, không phải bịa:

| Nhóm | Nguồn |
|---|---|
| `error/warning/info/success` + `.background/.border` | `errorForeground #f48771`, `notificationsWarningIcon #CCA700`, `notificationsInfoIcon #3a94bc`, `gitDecoration.added #73c991` |
| `version_control.*`, `created/deleted/modified/renamed/conflict/ignored/hidden` | `gitDecoration.*` + `diffEditor.*` |
| `icon.*`, `text.accent/disabled/placeholder` | `icon.foreground #8C8C8C`, `textLink #48A0C7`, `disabledForeground #555555`, `input.placeholderForeground #555555` |
| `element.active/disabled`, `ghost_element.*` | `list.hoverBackground #FFFFFF14`, `toolbar.activeBackground #FFFFFF33` |
| `editor.document_highlight.*`, `editor.invisible`, `editor.highlighted_line` | `editor.wordHighlight* #27678250/80`, `editorWhitespace #8C8C8C4D`, `editor.rangeHighlight` |
| `terminal.ansi.*` (24 key) | Bảng mặc định trích từ `workbench.desktop.main.js` |
| `players` (8) | **Phải tự sáng tác** — dẫn xuất từ accent `#3994BC` / `#0069CC` |

`players` là hạng mục duy nhất không có nguồn thật: VSCode không có khái niệm con trỏ collab.

## Các đường đã cân nhắc

| Đường | Nội dung | Vì sao chọn / loại |
|---|---|---|
| **A. Commit JSON, không giữ script** ✅ | Chạy pipeline một lần, commit duy nhất `assets/themes/vscode-2026/vscode-2026.json` | **Đã chọn.** Bảng suy 92 key là phán đoán thẩm mỹ một lần, không phải quy tắc máy móc — đóng gói thành generator là build công cụ cho lần sync thứ hai chưa chắc xảy ra (YAGNI). Chỉ thêm file mới → bề mặt conflict với upstream Zed bằng 0. |
| B. Giữ script sinh theme trong repo | `script/generate-vscode-2026-theme.py` tái chạy được | Loại. Phụ thuộc đường dẫn `VSCode.app` cục bộ → CI không chạy được; thêm công cụ phải bảo trì cho lợi ích giả định. |
| C. Sửa `crates/theme_importer` | Thêm xử lý `include` + suy luận vào `converter.rs` | Loại. `converter.rs` là code upstream Zed — mỗi dòng sửa là điểm conflict vĩnh viễn khi rebase; luật suy luận đặc thù 2026 không thuộc về converter tổng quát. |

## Hướng đã chốt

1. **Đóng gói:** thêm `assets/themes/vscode-2026/vscode-2026.json` chứa hai theme
   `"Dark 2026"` + `"Light 2026"`. Đổi mặc định tại `assets/settings/default.json:11-12`
   và `crates/theme/src/theme.rs:45`. **Giữ nguyên One Dark / One Light** — người dùng
   đổi lại được qua theme selector.
2. **Phạm vi:** đầy đủ — UI chrome + syntax highlighting + terminal ANSI 16 màu + player colors.
3. **92 key thiếu:** suy thủ công theo bảng truy nguồn ở trên, không để rơi về fallback.
4. **Viền:** bê nguyên `#2A2B2C` của VSCode cho `border`, nhưng **khôi phục phân tầng mà
   importer làm mất** — `border.variant` nhạt hơn, `border.selected` theo accent,
   `border.transparent` về `#00000000`.
5. **Border radius:** không đụng. Thang hiện tại đã khớp.

## Rủi ro cần canh

- **Sửa `DEFAULT_DARK_THEME` chạm code upstream Zed** → điểm conflict duy nhất khi rebase.
  Chỉ một hằng số, chấp nhận được, nhưng phải ghi nhớ.
- **`players` là phần sáng tác** — cần kiểm tra tương phản giữa 8 màu con trỏ, không chỉ
  lấy accent nhân sắc độ.
- **Syntax 34/45**: 11 key Zed không có scope TextMate tương ứng trong 2026
  (importer đã cảnh báo `primary`, `variant`). Phải suy từ token gần nhất chứ không bỏ trống.
- **Light 2026 dùng nhiều màu có alpha** (`list.activeSelectionBackground #00000025`,
  `input.border #D8D8D866`). Zed hợp nhất alpha khác VSCode ở một số chỗ — cần soi lại
  các trạng thái selected/hover trên nền sáng.
- **Theme mặc định đổi = thay đổi nhìn thấy ngay với mọi người dùng zode.** Không phải
  thay đổi nội bộ.

## Đo thành công

- Build `./script/clippy` sạch; zode khởi động, cả hai theme xuất hiện trong theme selector.
- Không còn key nào rơi về fallback: 141/141 style key + 45/45 syntax + 8 players có giá trị.
- `border.transparent = #00000000`; `border`, `border.variant`, `border.selected` ba giá trị khác nhau.
- Đối chiếu ảnh chụp zode vs VSCode 2026 trên: editor, sidebar, tab bar, status bar, terminal,
  command palette, popover/menu.

## Bước tiếp theo

Chuyển sang lập kế hoạch triển khai (`tkm:create-plan`) hoặc thực thi trực tiếp — công việc
gói gọn trong một file JSON mới + hai điểm sửa cấu hình.
