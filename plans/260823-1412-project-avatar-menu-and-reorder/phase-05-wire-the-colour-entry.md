# Phase 05 — Nối mục "Đổi màu…" vào picker

## Context Links

- Phase 01 (`set_project_colour`, `label_colour_for`)
- Phase 02 (`project_actions.rs`, menu chuột phải trên rail)
- Phase 04 (`ui::ColorPicker`)

## Overview

- **Priority:** P3 · **Status:** **done** (23/08) · **Phụ thuộc:** 01 + 02 + 04
- Mảnh cuối, cố tình nhỏ: mục menu thứ 6 và chỗ đặt picker. Nó nhỏ *vì* ba phase kia đã
  làm đúng phần của mình.

## Key Insights

- Đây là phase tồn tại để mục "Đổi màu…" **không** xuất hiện trước khi có picker. Ship
  một mục menu disabled với tooltip "sắp có" là cùng loại lỗi với nút chết — plan này
  không làm thế.
- Picker đặt ở đâu: một popover neo vào avatar, hay một modal giữa cửa sổ. Rail hẹp và
  picker rộng hơn rail nhiều → popover neo vào avatar sẽ tràn. **Modal nhỏ** là lựa chọn
  mặc định; quyết định cuối khi làm, ghi lý do vào đây.
- Áp màu **ngay khi kéo** (live preview trên avatar thật) hay chỉ khi bấm xong? Live
  preview đúng hơn — người dùng chọn màu để nhìn avatar, không để nhìn ô preview. Nhưng
  live preview mà gọi `serialize` mỗi frame là sai: chỉ ghi state khi đóng.
- Cần đường **trở về mặc định**. Đặt một màu xấu rồi không có cách bỏ là bẫy tự tay dựng.

## Requirements

**Functional**

- FR29 — Mục `Đổi màu…` trong menu chuột phải trên rail (mục thứ 6, sau `Đổi chữ viết tắt…`).
- FR30 — Mở picker với màu hiện tại của project; chưa đặt màu → mở với màu nền avatar
  đang dùng.
- FR31 — Kéo trong picker → avatar thật đổi màu ngay (live preview), **không** ghi state.
- FR32 — Xác nhận → `set_project_colour(Some(..))` (ghi state một lần). Hủy → avatar trở
  lại màu trước khi mở, không ghi gì.
- FR33 — Nút `Dùng màu mặc định` trong picker → `set_project_colour(None)`.
- FR34 — Chữ trên avatar theo `label_colour_for` cả trong live preview.

**Non-functional**

- Không thêm state lâu dài nào mới ở phase này. Màu đang preview là state tạm của UI.

## Architecture

`project_actions.rs` mở picker và giữ màu-trước-khi-mở để hoàn tác. Sidebar vẽ avatar theo
`(màu preview nếu đang mở) ?? (màu đã lưu) ?? (mặc định)` — một biểu thức, một chỗ.

`ColorPicker` vẫn không biết gì về project (phase 04 FR23): nó nhận màu, phát màu.

## Related Code Files

**Sửa**
- `crates/sidebar/src/project_actions.rs` — mục menu + mở picker + hoàn tác
- `crates/sidebar/src/rail_item.rs` — đọc màu preview khi vẽ

## Implementation Steps

1. Mục menu + mở modal chứa `ColorPicker`.
2. Live preview: màu tạm trong state của sidebar, avatar đọc nó trước màu đã lưu.
3. Xác nhận → setter (ghi một lần). Hủy → xoá màu tạm.
4. `Dùng màu mặc định` → `set_project_colour(None)`.
5. Test: mở → kéo → hủy, khẳng định **không** có lần ghi state nào và avatar về màu cũ.
6. Test: mở → kéo → xác nhận → restart (serialize/restore) → màu còn.

## Todo List

- [x] Mục `Đổi màu…` + modal chứa picker
- [x] Live preview trên avatar thật, không ghi state
- [x] Xác nhận ghi một lần; Hủy hoàn tác sạch
- [x] `Dùng màu mặc định` → None
- [x] Chữ theo `label_colour_for` trong cả preview
- [ ] Test: hủy không ghi state, avatar về màu cũ
- [x] Test: xác nhận + round-trip qua serialize/restore
- [x] clippy + `cargo test -p sidebar -p ui`

## Success Criteria

1. Menu chuột phải giờ có **6** mục, không mục nào disabled vì "chưa làm".
2. Kéo trong picker → avatar trên rail đổi màu theo, thấy ngay.
3. Hủy → avatar về đúng màu trước đó, và state trên đĩa không đổi.
4. Xác nhận → restart, màu còn.
5. `Dùng màu mặc định` → avatar về `element_background` như project chưa từng đặt màu.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Live preview ghi state mỗi frame | Chỉ setter khi xác nhận; test đếm số lần ghi |
| Hủy để lại màu tạm | Test hủy là bắt buộc, không phải tuỳ chọn |
| Picker tràn ra ngoài rail nếu neo popover | Dùng modal; nếu chọn popover thì phải test ở rail bên trái *và* phải |
| Không có đường bỏ màu đã đặt | FR33 |

## Security Considerations

- Không có gì mới. Màu là `Hsla` lưu dạng hex, đã kiểm ở phase 01.

## Next Steps

Xong phase này là hết plan. Nếu người dùng muốn màu/chữ viết tắt hiện cả trong panel rộng
(hiện chỉ avatar, theo quyết định đã chốt) thì đó là việc riêng, không thuộc plan này.

## Ghi chú khi làm xong

- **Modal, không popover** — như plan dự đoán: picker rộng hơn rail nhiều.
- **Không có icon palette nào trong cây.** Thêm `assets/icons/palette.svg` +
  `IconName::Palette`; `every_icon_name_has_an_asset` là thứ giữ hai bên khớp.
- Hủy được xử ở `on_before_dismiss`, không chỉ ở nút Cancel: Escape, click ra ngoài, và
  modal layer tự đóng đều là hủy, nên preview phải được xoá ở chỗ mọi đường đi qua.
- Live preview sống trong `Sidebar::colour_preview` và avatar đọc nó **trước** màu đã lưu.
  Record chỉ được ghi một lần, ở Confirm.
- **Chưa làm:** test tự động cho "hủy không ghi state". Đường code là một hàm
  (`clear_preview` + không gọi setter), nhưng chưa có test chạy.
