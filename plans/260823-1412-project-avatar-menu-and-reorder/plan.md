---
title: 'Avatar project trên rail: menu chuột phải, chữ viết tắt và màu tự đặt, kéo thả đổi vị trí'
description: >-
  Menu chuột phải trên avatar project ở rail (6 mục, Remove và Open-in-New-Window
  đều hỏi xác nhận), state mới theo group cho chữ viết tắt và màu, kéo thả đổi thứ
  tự trong rail và kéo ra ngoài để mở cửa sổ mới, cộng một ColorPicker thật trong
  crates/ui.
status: completed
priority: P2
effort: 4-6d
branch: feat.release-v0.1.1
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
tags:
  - sidebar
  - rail
  - project
  - ui
blockedBy: []
blocks: []
---

# Avatar project: menu, tên/màu tự đặt, kéo thả

Thiết kế đã chốt trong `plans/reports/brainstorm-260823-project-avatar-menu-and-reorder.md`
— đọc file đó trước mọi phase. Mọi dữ kiện về code trong plan này đã kiểm trên cây
ngày 2026-08-23, không phải suy đoán.

**Giữ nguyên nhánh `feat.release-v0.1.1`.** Không tạo nhánh mới.

## Phases

| # | Phase | Trạng thái | Phụ thuộc | Sở hữu file |
|---|-------|-----------|-----------|-------------|
| 01 | [State theo group: chữ viết tắt + màu](phase-01-per-group-presentation-state.md) | **done** | — | `crates/workspace/src/persistence/model.rs`, `crates/workspace/src/multi_workspace.rs` |
| 02 | [Actions dùng chung + menu chuột phải trên rail](phase-02-shared-actions-and-rail-menu.md) | **done** | 01 | `crates/sidebar/src/{project_actions,rail_item,context_menu}.rs`, `rail.rs` |
| 03 | [Kéo thả: đổi thứ tự và kéo ra ngoài](phase-03-drag-to-reorder-and-drag-out.md) | **done** | 02 | `crates/sidebar/src/rail_item.rs`, `crates/workspace/src/multi_workspace.rs` |
| 04 | [ColorPicker trong crates/ui](phase-04-color-picker-component.md) | **done** | — (song song 01–03) | `crates/ui/src/components/color_picker.rs` |
| 05 | [Nối mục "Đổi màu…" vào picker](phase-05-wire-the-colour-entry.md) | **done** | 01 + 02 + 04 | `crates/sidebar/src/project_actions.rs` |

Phase 04 khác crate, không chung file nào với 01–03 → chạy song song được. 02 và 03
**phải tuần tự**: cả hai sửa cùng chỗ vẽ một avatar.

## Vì sao thứ tự này

State đi trước UI đọc nó (01 → 02), và mục "Đổi màu…" **không xuất hiện** trong menu
cho tới khi picker có thật (05). Không ship một mục menu chết rồi hứa nối sau — đó là
loại lỗi plan này tồn tại để tránh.

## Ràng buộc xuyên suốt

- **Hai menu, một bộ handler.** Người dùng chọn hai menu riêng (rail 6 mục, panel giữ
  ellipsis 2 mục). Giao diện được phép khác; **hành vi không được**. Mọi handler nằm
  trong `project_actions.rs`, hai menu chỉ khác danh sách mục.
- **Cùng một bước phá huỷ, cùng một cửa gác.** `open_project_group_in_new_window`
  (`multi_workspace.rs:1269`) gọi `remove_project_group`. Nên Remove *và* Open-in-New-Window
  *và* kéo-ra-ngoài đều đi qua cùng hộp xác nhận.
- **"Rename" không được gọi là Rename.** Phạm vi đã chốt là chỉ đổi chữ viết tắt trên
  avatar; nhãn mục menu phải nói đúng thế.
- **State cũ phải mở được.** Hai field mới vào `SerializedProjectGroup` bằng
  `#[serde(default)]`; record đã ghi từ bản hôm nay không có chúng.
- **Định danh theo `ProjectGroupKey`, không theo index.** `stable_id_for_group`
  (`context_menu.rs:11`) đã tồn tại chính vì lý do này — index đi lạc khi list reorder
  giữa lúc menu đang mở, và phase 03 làm cho reorder xảy ra thường xuyên.

## Dữ kiện đã kiểm (đừng kiểm lại)

- Thứ tự rail là `Vec<ProjectGroupState>` (`multi_workspace.rs:966`), persist thành
  `Vec<SerializedProjectGroup>` trong KVP qua `MultiWorkspace::serialize`
  (`multi_workspace.rs:2114`). Đổi thứ tự = đổi chỗ trong Vec + gọi `serialize`.
- **Activate không sắp xếp lại rail.** `ensure_project_group_state`
  (`multi_workspace.rs:796-798`) return sớm nếu group đã có; chỉ project **mới** được
  `insert(0)`. Thứ tự sắp tay sống sót qua mọi lần đổi project.
- `SerializedProjectGroup::from_group` có **đúng một** caller
  (`multi_workspace.rs:2125`) → đổi signature là an toàn.
- `project_groups.push` ở `multi_workspace.rs:2373` là `#[cfg(test)]` helper và chèn ở
  **cuối**, khác production (`insert(0)`). Test reorder không được dựa vào nó.
- Khuôn kéo thả có sẵn ở tab bar: `on_drag` / `drag_over::<T>` / `on_drop`
  (`pane.rs:2947-2989`).
- Xác nhận: `window.prompt(PromptLevel::Warning, …, &["…", "Cancel"], cx)`.
  Reveal: `cx.reveal_path(&path)`. Parse hex: `theme::try_parse_color` (`theme/src/schema.rs:17`).
- `crates/ui` **không có** ColorPicker, không slider, không component màu nào. Khuôn
  kéo con trượt gần nhất: `scrollbar.rs:1382` (`window.on_mouse_event` với `MouseMoveEvent`).

## Quan hệ với plan cũ

`plans/260805-1913-multi-project-window-switching/phase-07` đã dựng rail và menu
ellipsis 2 mục (FR5, đã tick). Plan này **mở rộng** vùng đó chứ không chờ nó: phần cần
thiết đã giao. Không đặt `blockedBy`.

## Định nghĩa xong

Chuột phải avatar ra menu; Remove và kéo-ra-ngoài đều hỏi xác nhận nêu tên project;
kéo lên/xuống đổi thứ tự và thứ tự sống qua restart; mở project mới sau khi sắp tay thì
nó vào đầu rail mà **không** xáo thứ tự tương đối của phần còn lại; chữ viết tắt và màu
tự đặt hiện ngay trên avatar và sống qua restart; record state cũ (chưa có hai field) vẫn
mở được; hành vi `Remove` ở rail và ở panel giống nhau vì cùng một handler.

## Kết quả (23/08)

Menu chuột phải trên avatar có **6 mục, không mục nào chết**. Chữ viết tắt và màu tự đặt,
persist, tương thích ngược với record cũ. Kéo đổi thứ tự trong rail, thứ tự sống qua
restart; kéo ra ngoài rail chuyển project sang cửa sổ mới sau khi hỏi. ColorPicker thật
trong `crates/ui`.

Test: `sidebar` 23 · `ui` 45 · `workspace` 239 · `icons` 1 · clippy sạch · build exit 0.

### Chỗ plan sai, đã sửa lại trong khi làm

- Hộp xác nhận + hai hành vi phá huỷ phải sống ở crate `workspace`, không phải `sidebar`:
  chỗ nhận drop "ngoài rail" là gốc cửa sổ và `workspace` không thể phụ thuộc `sidebar`.
- FR9 (ẩn) và FR15 (disable) xung đột; phân xử trong phase 02.
- FR17 thu hẹp: vạch chỉ một chiều, không nửa-trên/nửa-dưới (lý do trong phase 03).
- Nỗi lo hiệu năng của phase 04 sai hướng: gradient có thật, 8 quad thay vì 1024.

### Ba việc để **chưa tick**, không tick khống

- Test mô phỏng kéo thả ở tầng UI (logic đã test ở tầng `MultiWorkspace`).
- Test vẽ thật cho vùng picker.
- Test "hủy picker không ghi state".

### Reviewer

0 critical · **1 high đã sửa** (kéo-ra-ngoài kích hoạt cả khi thả trong rail — xem phase 03)
· 1 medium đã sửa (bỏ pre-check trùng trong `move_project_group`) · 1 medium ghi chú tại chỗ
(edit bị đánh rơi im lặng nếu project rời window lúc modal đang mở) · 1 low đã sửa (log
cảnh báo khi chữ viết tắt trên đĩa bị cắt, để đối xứng với màu) · 1 low để lại: `DraggedProject::Render`
vẽ lại ô avatar, hai chỗ phải giữ đồng bộ nếu hình dạng avatar đổi.

