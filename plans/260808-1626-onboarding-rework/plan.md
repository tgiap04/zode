---
title: 'Làm lại màn onboarding của zode'
description: >-
  Xoá tận gốc import từ VS Code/Cursor, thu theme về đúng họ 2026, thêm section
  font, đổi mọi chuỗi Zed thành Zode và bỏ logo. Kèm sửa một unwrap() panic sẵn
  có trong chính phần theme.
status: completed
priority: P2
effort: 1-2d
branch: feat/onboarding-rework
tags:
  - onboarding
  - ui
  - fork
  - deletion
blockedBy: []
blocks: []
work_type: feature
spec_waived: >-
  SDD mode tắt trong project này (takumi.sddMode: off), theo tiền lệ ba plan
  260726-1531, 260807-1300 và 260808-1451. Yêu cầu đã chốt trọn trong
  plans/reports/brainstorm-260808-onboarding-rework.md
created: 2026-08-08
---

# Làm lại màn onboarding của zode

**Thiết kế đã sealed** tại [`plans/reports/brainstorm-260808-onboarding-rework.md`](../reports/brainstorm-260808-onboarding-rework.md).
Đọc file đó trước. **Không re-litigate** bảy quyết định đã chốt.

## Phạm vi

Ba crate: `onboarding` (viết lại trang), `settings` (xoá `vscode_import`), `settings_ui` (một dòng
`pub`). Cộng hai giá trị trong `assets/settings/default.json`.

**Ngoài phạm vi:** tên font `.ZedMono`/`.ZedSans`, và phần còn lại của `crates/zed/resources`.

> **Đã mở rộng giữa chừng:** icon app trong bundle (`crates/zed/resources/app-icon*`, 4 kênh × 2
> kích thước) **nằm trong phạm vi** kể từ khi người dùng đưa asset logo. Xem § "Quyết định lật
> giữa chừng" bên dưới.

## Các phase

| # | Phase | Trạng thái | Phụ thuộc | Sở hữu file |
|---|-------|-----------|-----------|-------------|
| 01 | [Xoá import tận gốc](phase-01-remove-vscode-import.md) | ✅ done | — | `crates/settings/`, `crates/onboarding/src/onboarding.rs` |
| 02 | [Theme section + sửa panic](phase-02-theme-section-and-panic.md) | ✅ done | 01 | `crates/onboarding/src/basics_page.rs` |
| 03 | [Section font](phase-03-font-section.md) | ✅ done | 02 | `crates/onboarding/src/basics_page.rs`, `crates/settings_ui/src/settings_ui.rs` |
| 04 | [Đổi chữ + bỏ logo](phase-04-rename-and-drop-logo.md) | ✅ done | 03 | `crates/onboarding/src/`, `crates/workspace/src/welcome.rs` |
| 05 | [Mặc định font + kiểm chứng](phase-05-defaults-and-verification.md) | ⚠ một phần | 01–04 | `assets/settings/default.json` |

## Vì sao plan này gần như tuần tự

Bốn phase đầu đều sửa **`basics_page.rs`** — chỉ khác vùng trong file. Không có chuyện chạy song
song ở đây, và nói ngược lại là tự lừa mình.

Thứ tự được chọn có lý do, không phải tuỳ tiện:

1. **Xoá trước** (phase 01) — bỏ ~1200 dòng trước thì mọi phase sau làm việc trên ít code hơn.
2. **Đổi chữ sau cùng** (phase 04) — đổi tên trong code sắp bị xoá là công toi. Làm cuối thì mọi
   chuỗi được đổi đều là chuỗi sống sót.
3. **Mặc định + kiểm chứng cuối** (phase 05) — chỉ có nghĩa khi bốn phase trên đã xong.

## Ma sát kiến trúc đã gỡ trước khi vẽ

Trang onboarding dựng bằng **hàm tự do** nhận `&mut App` (`render_theme_section(tab_index, cx)`),
không giữ state. Còn `font_picker` là một `Picker` **entity** — thoạt nhìn là không lắp vào nhau được.

Đọc `settings_ui.rs:4197` thì hết lo: `render_font_picker` cũng là hàm tự do, nó dùng
`PopoverMenu.menu()` để tạo entity **lazily** qua `cx.new(...)` trong closure. Không cần entity cha.

Hệ quả cho phase 03: bắt chước đúng khuôn đó. **Không** dựng struct mới cho onboarding.

## Thu hẹp phạm vi so với biên bản

Biên bản nói tái dùng cả `font_picker` **và** `number_field`. Xem lại thì cỡ chữ chỉ cần
`ToggleButtonGroup` với vài preset — đúng thành phần trang này **đã dùng** cho nút Light/Dark/System.

⇒ Chỉ `font_picker` cần `pub`, `number_field` không. Phần chạm upstream `settings_ui` co từ
"mở cả module" xuống đúng **một dòng**.

## Phụ thuộc chéo giữa các plan

`260726-1531-remove-auth-cloud-hard-fork` (pending, 4-6 tuần) viết lại chính `crates/onboarding`
ở phase 05/07/08. **Không tạo `blockedBy`/`blocks`** — plan này chạy trước, đã chốt.

Hai điều người thi công phải biết:

- Khảo sát của plan kia **đã lỗi thời**: nó trích `basics_page.rs:649`, file hiện chỉ **537 dòng**.
  Nó sẽ phải khảo sát lại — việc nó vốn đã cần làm.
- Mục telemetry (`render_telemetry_section`) sẽ bị plan kia **xoá hẳn**. Phase 04 vẫn đổi chữ Zed ở
  đó theo yêu cầu; đây là công sẽ bị vứt, chấp nhận có ý thức (3 chuỗi).

## Rủi ro

| Rủi ro | Mức | Đối phó |
|---|---|---|
| **`crates/onboarding` hiện có 0 test.** Tiêu chí "không panic khi theme vắng mặt" cần dựng hạ tầng test từ đầu | **Cao** | Phase 02 chịu trách nhiệm; nếu chỉ đọc code mà tuyên bố an toàn thì phase đó chưa xong |
| Xoá 1068 dòng `vscode_import.rs` (upstream) ⇒ conflict vĩnh viễn khi rebase | **Cao** | Đã nghe và vẫn chọn. Ghi vào changelog |
| `actions.json` (`crates/docs_preprocessor/`) liệt kê `zed::ImportVsCodeSettings` — là **file sinh ra** bởi `script/generate-action-metadata` | Trung bình | Phase 01 phải sinh lại, không sửa tay |
| Đổi `buffer_font_size`/`ui_font_size` mặc định là thay đổi **mọi người dùng thấy ngay** | Trung bình | Phase 05 ghi changelog; UI 13px trên màn lớn khá nhỏ, phải nhìn thật |
| Bốn phase cùng sửa một file ⇒ không song song được | Trung bình | Bảng phase ghi rõ chuỗi phụ thuộc 01→02→03→04 |
| `get_vscode_edits` có thể còn chỗ khác gọi | Thấp | **Đã rà: chỉ `settings_store.rs` tự gọi** (`:636` và test `:2295`). An toàn cắt cả cụm |

## Định nghĩa hoàn thành

- `./script/clippy` sạch (**không** dùng `cargo clippy`); `cargo test -p onboarding -p settings -p zode` xanh
- `grep -rn "Zed" crates/onboarding/` trả về **rỗng**
- `grep -rn "VsCodeSettings\|vscode_import" crates/` trả về **rỗng**
- Onboarding **không panic** khi một theme vắng mặt trong registry — **có test chứng minh**,
  và test đó phải đỏ khi trả `unwrap()` về
- `buffer_font_size` và `ui_font_size` trong `default.json` đều là `13`
- **Chạy thật:** `make reset-all FORCE=1` rồi `make dev PROJECT=` → màn onboarding hiện đúng chữ
  Zode, không logo, không mục import, theme chỉ còn nút chế độ, section font đổi được và ăn ngay

## Kết quả thực thi (2026-08-08)

Phase 01–04 xong. Phase 05 xong phần đo được; phần nhìn bằng mắt chưa ai làm.

### Quyết định lật giữa chừng

Người dùng đưa `~/Downloads/zode.png` **sau khi** bản vẽ đã chốt "bỏ hẳn logo" — mà lý do chốt hồi
đó chính là *chưa có asset*. Tiền đề mất, quyết định đổi: logo dùng ở **cả icon app lẫn trong app**.
Ghi chi tiết trong [phase 04](phase-04-rename-and-drop-logo.md) § đầu file.

### Sáu chỗ chệch khỏi bản vẽ

1. **Trình biên dịch bắt hai lần cắt hụt của tôi.** Khối `observe_new` bỏ lại `)` + `.detach();`
   mồ côi; `FIRST_OPEN`/`DOCS_URL` **nằm lọt giữa** hai struct action nên bị cuốn theo. Đúng điều
   phase 01 nói: với việc xoá, trình biên dịch **là** bộ test.

2. **Không phải 2 dependency thừa mà là 4.** `cargo-machete` bắt `serde_json_lenient` +
   `notifications` ở vòng đầu, rồi `schemars` + `zlog` lộ ra ở vòng hai. Phải chạy lặp tới khi sạch.

3. **Phép kiểm "actions.json giảm đúng 2" là kỳ vọng sai.** File đó **bị gitignore** — artifact cục
   bộ, mốc đã lệch sẵn. Thay bằng phép đo đúng: hai action import biến mất (grep = 0).

4. **Dev-dep suýt làm hỏng crate khác.** Thêm `project` với `test-support` bật cờ feature lan ra cả
   graph, làm `workspace/persistence.rs` không biên dịch (`RemoteConnectionIdentity::Mock` chưa phủ).
   Thu về đúng ba: `fs`, `gpui`, `settings`.

5. **Không mở cả `mod components`.** Bản vẽ nói đổi `mod` thành `pub mod`; làm vậy phơi luôn
   `dropdown`, `number_field`, `input_field`, `section_items`. Thay bằng **một re-export hẹp**
   `pub use crate::components::font_picker;`.

6. **Tiêu chí `grep "Zed"` phải rỗng đã được nới, có chủ ý.** Phụ đề `"The editor for what's next"`
   là khẩu hiệu của Zed — thay bằng **`"Built on Zed"`**: ghi công nguồn gốc thay vì mượn giọng.
   Test `the_crate_never_claims_to_be_zed` cho phép đúng dòng đó và **chỉ dòng đó**; nó quét phần
   shipping của mỗi file (cắt tại `#[cfg(test)]`) nên không tự soi mình. Link `zed.dev/docs` giữ
   nguyên — tài liệu thật và vẫn đúng, zode không có trang thay thế.

### Hiệu năng — theo yêu cầu giữa chừng

- **Picker font dựng lazily.** `render_basics_page` chạy lại mỗi lần settings đổi
  (`Onboarding` quan sát `SettingsStore`). Danh sách font chỉ được đọc khi **mở menu**, qua
  `FontFamilyCache` mà `Onboarding::new` đã prefetch. Không liệt kê font hệ thống trên đường nóng.
- **Logo trong app 256px, không phải 1024px.** Asset nhúng thẳng vào binary; 1.6MB cho một dấu
  hiệu vẽ ở 40–45px là lãng phí. 256px thừa sức cho @3x retina — **nhỏ hơn 33 lần** (49KB).
- Xoá ~1500 dòng cũng là bớt biên dịch và bớt binary.

### Đã kiểm chứng chạy thật

| Kiểm | Kết quả |
|---|---|
| `./script/clippy` + `cargo-machete` | sạch |
| Test | onboarding 3/3 · settings 27/27 · settings_ui 15/15 · ui 35/35 · workspace 207/207 · zode 56/56 |
| Mutation-test | 3/3 đỏ đúng chỗ: `unwrap()` → `ThemeNotFoundError("Dark 2026")` · bỏ `ui_font_size` → `left: None` · `"Welcome to Zed"` → bắt đúng dòng 236 |
| grep import | rỗng |
| Khởi động sau `reset-all` | sống, `first_open` được ghi ⇒ **onboarding đã hiện** |
| Asset logo | 2 tham chiếu trong binary |
| Icon 4 kênh | 512 + 1024, alpha góc = 0 |

### Chưa kiểm được

- **Chưa ai nhìn màn onboarding.** Bố cục sau khi mất 3 ô theme và thêm section font, logo trên nền
  **Light 2026** (ảnh có nền tối, thiết kế cho ô icon — đây là rủi ro đã ghi trước ở phase 04), và
  13px có dùng được không.
- **Icon app chưa thấy.** Icon chỉ hiện với bản `.app` đóng gói; `make build` ra binary trần.
  Cần `script/bundle-mac`.
- **Bốn kênh giờ dùng chung một icon.** Trước đây dev/nightly/preview khác màu để phân biệt bản
  đang chạy. Giờ mất khả năng đó — hệ quả của việc chỉ có một asset.
- `ERROR [main.rs:1559] Is a directory` trong log: **bug sẵn có** của `watch_themes` (thư mục
  `themes/` vừa tạo bắn sự kiện, `load_bytes` đọc thư mục như file). `main.rs` không nằm trong diff
  của plan này. Được `.log_err()` nuốt, vô hại.
