# Phase 02 — Theme section: bỏ 3 ô, và gỡ quả mìn `unwrap()`

## Context Links

- Biên bản: [`brainstorm-260808-onboarding-rework.md`](../reports/brainstorm-260808-onboarding-rework.md) § "Quả mìn trong chính phần cần sửa"
- **Depends:** phase 01 (cùng file `basics_page.rs`)

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** 01 · **Effort:** ~0.5d

Thu phần theme về đúng nút Light/Dark/System, và sửa chỗ panic sẵn có mà thay đổi này sẽ kích hoạt.

## Key Insights

- **Đây là phase rủi ro nhất trong plan, dù nghe như đơn giản nhất.**
  ```rust
  // basics_page.rs:122
  let themes = theme_names.map(|theme| theme_registry.get(theme).unwrap());
  [0, 1, 2].map(|index| { ... })   // :124 — cứng số 3
  ```
  `unwrap()` **panic ngay màn onboarding** nếu một tên theme vắng mặt trong registry. Đây là lỗi
  **sẵn có**, không do thay đổi này sinh ra — nhưng đổi danh sách theme là đúng thứ kích hoạt nó.
- **Một họ theme thì ô không còn là lựa chọn.** Nút Light/Dark/System vẫn là lựa chọn thật
  (System = tự đổi theo hệ thống), nên giữ. Ba ô kia chỉ còn một — đó là trang trí, không phải UI.
- **`crates/onboarding` có 0 test.** Tiêu chí "không panic" của plan này buộc phải dựng hạ tầng test
  từ số không. Không có đường tắt: đọc code rồi tuyên bố an toàn thì phase này chưa xong.
- Xoá 3 ô làm biến `LIGHT_THEMES`/`DARK_THEMES`/`FAMILY_NAMES` (`:22-28`),
  `get_theme_family_themes` (`:30`) và `render_theme_previews` (`:95`) thành **code chết** — kéo
  theo cả `theme_preview.rs` (381 dòng) nếu không còn ai gọi. Phải kiểm, không đoán.

## Requirements

**Chức năng:** Phần theme chỉ còn nút Light/Dark/System, ghi settings như cũ.
**Phi chức năng:** Onboarding **không được panic** dù registry thiếu bất kỳ theme nào.

## Architecture

`render_theme_section` giữ nguyên chữ ký `(tab_index: &mut isize, cx: &mut App)`. Bỏ nhánh
`render_theme_previews` và mọi thứ chỉ nó dùng.

## Related Code Files

**Sửa:** `crates/onboarding/src/basics_page.rs`
**Có thể xoá:** `crates/onboarding/src/theme_preview.rs` (381 dòng) — **chỉ khi** rà xong không còn
ai gọi
**Đọc để đối chiếu:** `crates/theme/src/theme.rs` (`ThemeRegistry::get` trả `Result`)

## Implementation Steps

1. **Xoá phần xem trước** — lời gọi `render_theme_previews` (`:92`) và chính hàm đó (`:95-...`),
   kèm `LIGHT_THEMES`, `DARK_THEMES`, `FAMILY_NAMES`, `get_theme_family_themes`.

2. **Quả mìn đi cùng.** Xoá `render_theme_previews` là `unwrap()` ở `:122` biến mất theo. **Nhưng
   đừng dừng ở đó** — rà `unwrap()`/`expect()` còn lại trong crate:
   ```
   grep -rn "unwrap()\|expect(" crates/onboarding/src/
   ```
   Mỗi chỗ còn lại: hoặc chứng minh không panic được, hoặc sửa. `CLAUDE.md` cấm `unwrap()`.

3. **Kiểm `theme_preview.rs` còn ai dùng không:**
   ```
   grep -rn "theme_preview\|ThemePreviewTile\|ThemePreviewStyle" crates/
   ```
   Không còn ⇒ xoá cả file và khai báo mod. Còn ⇒ **giữ**, ghi rõ ai dùng vào phase file này.

4. **Dựng hạ tầng test cho crate** — chưa từng có. Xem Test Design.

5. **Rà sót** — `cargo build -p onboarding` không cảnh báo dead code.

## Todo List

- [ ] Xoá `render_theme_previews` + 4 hằng/hàm phụ trợ
- [ ] Rà `unwrap()`/`expect()` còn lại trong crate, xử từng chỗ
- [ ] Quyết số phận `theme_preview.rs`, ghi lý do vào file này
- [ ] Dựng `#[cfg(test)] mod tests` đầu tiên cho crate
- [ ] Test: theme section dựng được khi registry **thiếu** theme
- [ ] Mutation-test: trả `unwrap()` về → test phải đỏ
- [ ] `cargo build -p onboarding` không có cảnh báo dead code

## Success Criteria

- Trang onboarding dựng được với một registry **không có** "Dark 2026"/"Light 2026"
- **Test có răng:** đưa lại `theme_registry.get(name).unwrap()` vào ⇒ test đỏ. Chưa mutation-test
  thì chưa xong.
- `grep -rn "unwrap()" crates/onboarding/src/` — rỗng, hoặc mỗi chỗ còn lại có comment giải thích
  vì sao không panic được
- `./script/clippy` sạch

## Test Design

Crate chưa có test nào, nên đây là lúc đặt khuôn cho crate.

Cái cần khoá không phải "hàm trả về đúng element" — mà là **không panic khi dữ liệu thiếu**. Nên
test phải dựng một `App` với `ThemeRegistry` **rỗng hoặc thiếu**, rồi gọi `render_theme_section`.

Mẫu để bắt chước: `crates/sidebar/src/sidebar_tests.rs` — nó dựng `SettingsStore::test(cx)` +
`theme_settings::init(theme::LoadThemes::JustBase, cx)`. **`JustBase` chính là thứ cần**: nó nạp
theme nền mà không nạp bộ đầy đủ, tức đúng cảnh "theme mong đợi vắng mặt".

```rust
// Không khẳng định hình dạng element — khẳng định là nó KHÔNG panic.
// Cũ hơn: unwrap() ở đây làm hỏng màn đầu tiên người dùng thấy.
#[gpui::test]
fn theme_section_survives_a_registry_without_the_expected_themes(cx: &mut TestAppContext) { ... }
```

**Không** viết test khẳng định "có đúng 0 ô xem trước" — nó chỉ chép lại dòng code vừa xoá, và sẽ
đỏ vì lý do vô nghĩa khi ai đó thêm lại một ô hợp lệ.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Xoá `unwrap()` mà quên chỗ khác trong crate | Bước 2 rà toàn crate, không chỉ dòng 122 |
| Xoá `theme_preview.rs` khi còn nơi khác dùng | Bước 3 bắt grep trước, và ghi kết luận lại |
| Test "không panic" xanh vì nó không thực sự chạm code | Mutation-test bắt buộc ở Success Criteria |
| Dựng test đầu tiên cho crate tốn hơn dự kiến | Có mẫu sẵn ở `crates/sidebar/src/sidebar_tests.rs` |
| Bỏ nút chế độ theo đà ⇒ mất System | Requirements ghi rõ: **giữ** nút chế độ |

## Security Considerations

Không có.

## Next Steps

Mở khoá phase 03. Sau phase này trang còn: theme (gọn), base keymap, telemetry.
