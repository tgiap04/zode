# Brainstorm — Làm lại màn onboarding của zode

**Ngày:** 2026-08-08 · **Lens:** CTO · **Trạng thái:** thiết kế đã chốt

## Commission

Màn onboarding vẫn là của Zed. Đổi chữ, bỏ logo, bỏ import từ IDE khác, thu theme về đúng
Dark/Light 2026, và thêm phần chỉnh font.

## Khảo sát — sự thật đo được

### Onboarding là MỘT trang, không phải nhiều tab

`Onboarding::render_page` (`onboarding.rs:241`) gọi đúng một hàm: `render_basics_page`.
Trang đó có bốn section (`basics_page.rs:530-536`):

| Section | Dòng | Nội dung |
|---|---|---|
| `render_theme_section` | 39 | Nút Light/Dark/System + 3 ô họ theme |
| `render_base_keymap_section` | 325 | Chọn base keymap + vim mode |
| `render_import_settings_section` | 487 | Nhập từ VS Code / Cursor |
| `render_telemetry_section` | 237 | Telemetry + crash report |

### Quả mìn trong chính phần cần sửa

```rust
// basics_page.rs:122
let themes = theme_names.map(|theme| theme_registry.get(theme).unwrap());
[0, 1, 2].map(|index| { ... })   // :124 — cứng số 3
```

`unwrap()` **panic ngay màn đầu** nếu một tên theme vắng mặt trong registry, và danh sách theme là
`[&str; 3]` cố định (`:22-23`). Đổi danh sách mà không đụng hai chỗ này là app chết.
`CLAUDE.md` của project cấm `unwrap()` — đây là vi phạm sẵn có, không phải do thay đổi này sinh ra.

### Logo

`VectorName::ZedLogo` → `assets/images/zed_logo.svg`, dùng ở **hai** chỗ:
`onboarding.rs:290` và `workspace/src/welcome.rs:426`. Icon app trong bundle
(`crates/zed/resources/app-icon*.png`, `.icns`) là **bộ riêng**, không liên quan.

### Font

`settings_ui` đã có sẵn `components/font_picker.rs` và `components/number_field.rs` — nhưng
`mod components;` (`settings_ui.rs:1`) là **private**, nên `onboarding` chưa gọi tới được.
`cargo tree` xác nhận `settings_ui` **không** kéo theo `onboarding`, nên không có vòng phụ thuộc.

Mặc định hiện tại: `buffer_font_size: 15`, `ui_font_size: 16`,
`buffer_font_family: ".ZedMono"`, `ui_font_family: ".ZedSans"`.

### Bán kính xoá import

| Nơi | Quy mô |
|---|---|
| `crates/onboarding/` | 24 tham chiếu — section UI, 2 action, handler, `SettingsImportState` |
| `crates/settings/src/vscode_import.rs` | **1068 dòng** |
| `crates/settings/src/settings_store.rs` | 8 tham chiếu (`import_vscode_settings`, `get_vscode_edits`) |

Hai cái sau là **code upstream Zed**.

## Quyết định đã chốt

| # | Quyết định | Ghi chú |
|---|---|---|
| 1 | Đổi chữ Zed → Zode toàn màn, **kể cả mục telemetry** | |
| 2 | **Bỏ hẳn dấu hiệu logo**, chỉ để chữ "Welcome to Zode" | Không dựng logo tạm |
| 3 | **Xoá tận gốc** import — cả `vscode_import.rs` và method trong `settings_store` | Ngược khuyến nghị, xem dưới |
| 4 | Theme: **giữ nút Light/Dark/System, bỏ 3 ô họ theme** | Một họ thì ô không còn là lựa chọn |
| 5 | **Sửa `.unwrap()` + `[0,1,2]`** | Bắt buộc, không phụ thuộc quyết định khác |
| 6 | Thêm section font: font UI + font editor + cỡ chữ | Tái dùng `settings_ui`, đổi `mod components` thành `pub` |
| 7 | Mặc định **cả `buffer_font_size` lẫn `ui_font_size` = 13** | Thay đổi nhìn thấy ngay với mọi người dùng |

Bỏ logo là lựa chọn duy nhất không vướng nhãn hiệu: mã nguồn Zed theo GPL-3.0 nhưng **logo là
nhãn hiệu, không nằm trong phạm vi đó** — fork mang tên khác mà vẫn dùng dấu hiệu của Zed là vấn
đề pháp lý chứ không phải thẩm mỹ.

## Chỗ tôi phản đối, và người dùng vẫn giữ nguyên

**Xoá import config từ IDE khác.** Đã nêu: zode là fork **hướng VSCode** — theme VSCode 2026,
title bar VSCode, activity bar VSCode. Nhập settings từ VSCode hợp với zode **hơn** cả với Zed, và
nó là đúng thứ khiến người dùng VSCode chuyển sang được.

Người dùng chốt xoá tận gốc. Cái giá được ghi lại ở đây để sau này không ai phải đoán:

- **1068 dòng upstream biến mất** ⇒ điểm conflict vĩnh viễn mỗi lần rebase, không phải một lần.
- Mất đường di cư từ VSCode — đúng nhóm người dùng fork này nhắm tới.
- `get_vscode_edits` trong `settings_store.rs` có thể còn chỗ khác gọi; phải rà trước khi cắt.

Ba tầng đã cân nhắc: chỉ giấu UI (~45 dòng) · bỏ khỏi app, giữ code nền (~150 dòng) ·
**xoá tận gốc (~1200 dòng)** ← đã chọn.

## Rủi ro cần canh

| Rủi ro | Mức | Ghi chú |
|---|---|---|
| **Plan `260726-1531` viết lại chính crate này** — phase 05/07/08 sửa `onboarding` sau khi xoá client/auth. Và khảo sát của nó **đã lỗi thời**: nó trích `basics_page.rs:649`, mà file hiện chỉ **537 dòng** | **Cao** | Hai plan cùng sửa `basics_page.rs`. Phải quyết thứ tự chạy |
| Mục telemetry sẽ bị plan kia **xoá hẳn** — đổi chữ Zed ở đó là làm việc sẽ bị vứt | Trung bình | Người dùng đã chọn làm; 3 chuỗi, chi phí nhỏ |
| Đổi `buffer_font_size`/`ui_font_size` mặc định là thay đổi **mọi người dùng thấy ngay** | Trung bình | Ghi vào changelog. UI 13px trên màn lớn khá nhỏ |
| Sửa `mod components` thành `pub` là chạm code upstream | Thấp | Đúng một dòng |
| `.ZedMono` / `.ZedSans` vẫn mang tên Zed | Thấp | **Ngoài phạm vi** — đổi tên font kéo theo cả file font và mọi tham chiếu |
| Bỏ 3 ô theme làm trang trống hơn hẳn | Thấp | Section font mới lấp vào |

## Đo thành công

- `./script/clippy` sạch; test `-p onboarding -p settings -p zode` xanh
- **Không chuỗi "Zed" nào còn trong `crates/onboarding/`**
- Onboarding **không panic** khi một theme vắng mặt — có test chứng minh, không chỉ đọc code
- `grep -rn "VsCodeSettings\|vscode_import" crates/` trả về rỗng
- Chạy thật: reset sạch → `make dev PROJECT=` → màn onboarding hiện đúng chữ, không logo,
  không mục import, phần theme chỉ còn nút chế độ, có section font đổi được và ăn ngay
- `buffer_font_size` và `ui_font_size` mặc định = 13

## Thứ tự với plan `260726-1531` — đã chốt

**Onboarding chạy trước.** Plan kia đang `pending`, ước lượng 4-6 tuần, và khảo sát của nó đã lệch
thực tế. Chờ nó nghĩa là màn onboarding giữ nguyên tên Zed thêm nhiều tuần nữa.

Hệ quả phải chấp nhận: plan `260726-1531` sẽ phải khảo sát lại `onboarding` — nhưng nó **đã cần
làm việc đó** rồi, vì số dòng nó trích không còn đúng. Không tạo `blockedBy`/`blocks`; chỉ là rủi
ro merge cùng file, xử ở mức git.

## Bước tiếp theo

Chuyển sang `tkm:create-plan`.
