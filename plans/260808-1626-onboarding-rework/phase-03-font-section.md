# Phase 03 — Section font: font UI, font editor, cỡ chữ

## Context Links

- Biên bản: [`brainstorm-260808-onboarding-rework.md`](../reports/brainstorm-260808-onboarding-rework.md) § "Font"
- Khuôn để bắt chước: `crates/settings_ui/src/settings_ui.rs:4197` (`render_font_picker`)
- **Depends:** phase 02 (cùng file `basics_page.rs`)

## Overview

**Priority:** P2 · **Status:** pending · **Depends:** 02 · **Effort:** ~0.5d

Thêm một section cho phép chọn font giao diện, font editor và cỡ chữ ngay ở màn đầu.

## Key Insights

- **Ma sát tưởng là chặn, hoá ra không.** Trang onboarding dựng bằng **hàm tự do** nhận `&mut App`,
  không giữ state; còn `font_picker` là một `Picker` **entity**. Nhưng `settings_ui.rs:4197` cho
  thấy khuôn đúng: `PopoverMenu.menu()` tạo entity **lazily** trong closure qua `cx.new(...)`.
  ⇒ **Không dựng struct mới cho onboarding.** Bắt chước đúng khuôn đó.
- **Thu hẹp so với biên bản.** Biên bản nói tái dùng cả `font_picker` *và* `number_field`. Cỡ chữ
  chỉ cần `ToggleButtonGroup` với vài preset — đúng thành phần trang này **đã dùng** cho nút
  Light/Dark/System. ⇒ chỉ `font_picker` cần `pub`, phần chạm upstream co còn **một dòng**.
- `mod components;` (`settings_ui.rs:1`) là **private**. Đổi thành `pub mod components;` là chạm
  code upstream — một dòng, chấp nhận được, nhưng phải ghi vào changelog để nhớ khi rebase.
- Trang này ghi settings bằng `update_settings_file(fs, cx, |settings, cx| ...)` từ crate `settings`
  (`basics_page.rs:198, 210, 263, ...`). **Dùng đúng helper đó**, không dùng biến thể của `settings_ui`.
- `cargo tree` đã xác nhận `settings_ui` **không** kéo theo `onboarding` ⇒ thêm dependency không tạo
  vòng lặp.

## Requirements

**Chức năng:** Chọn được font UI, font editor và cỡ chữ; đổi là **ăn ngay**, và ghi vào file
settings người dùng.
**Phi chức năng:** Không dựng lại UI đã có ở `settings_ui`. Không thêm state vào trang onboarding.

## Architecture

```
render_font_section(tab_index, cx) -> impl IntoElement
├── PopoverMenu "ui-font"      → font_picker(...)  → ui_font_family
├── PopoverMenu "buffer-font"  → font_picker(...)  → buffer_font_family
└── ToggleButtonGroup [12|13|14|16] → buffer_font_size + ui_font_size
```

Đặt sau section theme trong `render_basics_page` — cùng nhóm "cái nhìn thấy được".

## Related Code Files

**Sửa:** `crates/onboarding/src/basics_page.rs`, `crates/onboarding/Cargo.toml` (thêm `settings_ui`),
`crates/settings_ui/src/settings_ui.rs` (một dòng `pub`)
**Đọc để bắt chước:** `crates/settings_ui/src/settings_ui.rs:4197-4245`,
`crates/settings_ui/src/components/font_picker.rs:169`
**Không sửa:** `components/number_field.rs` — không dùng tới

## Implementation Steps

1. **Mở `font_picker` ra ngoài crate** — `settings_ui.rs:1`: `mod components;` → `pub mod components;`.
   Kiểm xem `font_picker` đã `pub` trong `components.rs` chưa; chưa thì `pub use` nó.
   **Chỉ mở đúng thứ cần** — mở cả module rồi để đó là mở rộng bề mặt API không ai xin.

2. **Thêm dependency** `settings_ui.workspace = true` vào `crates/onboarding/Cargo.toml`.
   Build ngay để chắc không có vòng lặp bất ngờ — `cargo tree` dự đoán là không, nhưng dự đoán
   không phải bằng chứng.

3. **Dựng `render_font_section`** theo khuôn `settings_ui.rs:4197`: `PopoverMenu` + `.menu()` tạo
   picker lazily. Ghi settings bằng `update_settings_file` như các section khác trong file.

4. **Cỡ chữ** — `ToggleButtonGroup` với preset. Một lần bấm ghi **cả** `buffer_font_size` và
   `ui_font_size` (quyết định 7 của biên bản: cả hai cùng giá trị).

5. **Cắm vào trang** — thêm `.child(render_font_section(&mut tab_index, cx))` vào
   `render_basics_page`, ngay sau section theme. **Giữ `tab_index` chạy đúng thứ tự** — nó là
   thứ tự tab bằng bàn phím, sai là trang không dùng được bằng keyboard.

## Todo List

- [ ] `pub mod components` + kiểm `font_picker` đã `pub`
- [ ] Thêm dep `settings_ui`, build kiểm vòng lặp
- [ ] `render_font_section` — 2 popover font
- [ ] Cỡ chữ — `ToggleButtonGroup`, ghi cả hai khoá
- [ ] Cắm vào `render_basics_page`, kiểm `tab_index` liên tục
- [ ] Test: chọn font ghi đúng khoá vào settings
- [ ] Mutation-test test đó
- [ ] Nhìn thật: đổi font và cỡ chữ, xác nhận ăn ngay

## Success Criteria

- Chọn font UI ⇒ `ui_font_family` trong settings người dùng đổi; giao diện đổi **ngay**, không cần
  khởi động lại
- Chọn cỡ chữ ⇒ **cả** `buffer_font_size` và `ui_font_size` đổi
- `./script/clippy` sạch; `cargo test -p onboarding -p settings_ui` xanh
- Tab bằng bàn phím đi qua các control mới **đúng thứ tự**, không nhảy cóc

## Test Design

Cái đáng khoá là **ghi đúng khoá settings**, không phải hình dạng element.

Dùng hạ tầng test phase 02 vừa dựng. Test gọi hàm ghi rồi đọc lại `SettingsStore` — không cần
click thật.

```rust
// Cỡ chữ ghi HAI khoá, không phải một. Khoá thứ hai là chỗ dễ quên nhất
// khi ai đó sửa lại sau này.
#[gpui::test]
fn choosing_a_font_size_writes_both_editor_and_ui(cx: &mut TestAppContext) { ... }
```

**Không** test được `PopoverMenu` mở ra đúng danh sách font — nó phụ thuộc font cài trên máy, nên
test sẽ khác nhau giữa máy dev và CI. Phần đó kiểm bằng mắt ở todo cuối, và nói rõ như vậy.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Mở cả `components` ra public ⇒ bề mặt API phình | Bước 1 nói rõ chỉ mở thứ cần |
| Thêm `settings_ui` gây vòng phụ thuộc | Bước 2 build ngay, không tin `cargo tree` một mình |
| Quên ghi `ui_font_size` khi đổi cỡ chữ | Có test riêng cho đúng chuyện này |
| `tab_index` đứt quãng ⇒ trang không dùng được bằng bàn phím | Bước 5 + todo riêng |
| Dựng struct state cho onboarding vì tưởng Picker cần | Key Insights giải sẵn: `PopoverMenu` tạo lazily |

## Security Considerations

`font_picker` liệt kê font hệ thống qua `all_font_names()` — chỉ đọc, không ghi.

## Next Steps

Mở khoá phase 04.
