# Phase 01 — Xoá `project_panel.dock`, panel đóng cứng bên phải

## Context Links

- [plan.md](plan.md) · Chặn bởi: **không** · Chặn: 03 · Song song được với: **02**
- `crates/project_panel/src/project_panel.rs:7330-7384`
- `crates/settings_content/src/workspace.rs:746-749`
- `crates/settings_ui/src/page_data.rs:4278-4290`
- `assets/settings/default.json:779-790`

## Overview

**Priority:** P1 (mọi phase sau đứng trên nó) · **Status:** done · **Effort:** 1h

Cắt **đường ghi**. Sau phase này không còn code nào trong app ghi `sidebar_side` hay
`project_panel.dock`, và `project_panel.dock` không còn tồn tại như một key.

## Key Insights

- `write_dock_and_opposite_rail` ghi **bốn** key trong một lần move. Xoá cả hàm, không tỉa.
- `assets/settings/default.json` là authoritative và `from_settings` `unwrap()` từng field:
  **xoá key mà không xoá field resolved là panic lúc khởi động, không phải fallback.** Lần
  trước trong session này việc đó làm 332 test đỏ. Bốn chỗ phải đi cùng nhịp:
  `settings_content` → struct resolved (`ProjectPanelSettings`) → `default.json` →
  `settings_ui/page_data.rs`.
- `page_data.rs` trả **mảng cỡ cố định** `[SettingsPageItem; 29]` → phải giảm còn **28**.
  Bỏ sót chỗ này không làm test nào đỏ, nhưng panel biến mất khỏi Settings UI sai cách.
- `Panel::set_position` **không có** default body trong trait (`dock.rs:51`), nên
  `ProjectPanel` vẫn phải khai — thành no-op có comment, không xoá.
- `ProjectPanel` **không** implement `supports_flexible_size` → mặc định `false`. Cộng với
  `position_is_valid` chỉ còn một giá trị, menu right-click của nó sẽ **rỗng hoàn toàn**.
  Đó là việc của phase 02, ghi ở đây để hai phase không tưởng là chuyện của nhau.

## Requirements

**Functional:** `ProjectPanel::position()` luôn `Right`; `position_is_valid` chỉ nhận
`Right`; `set_position` không ghi gì; key `project_panel.dock` không còn trong schema, trong
default.json, trong Settings UI.

**Non-functional:** không panic lúc khởi động; `cargo test -p settings` xanh.

## Architecture

```
TRƯỚC:  right-click ─▶ set_position ─▶ write_dock_and_opposite_rail ─▶ 4 key
        project_panel.dock ─▶ ProjectPanelSettings.dock ─▶ position()

SAU:    right-click ─▶ (phase 02 chặn ở menu)
        position() ═ DockPosition::Right          (hằng, không đọc setting)
```

## Related Code Files

**Sửa**

| File | Việc |
|---|---|
| `crates/project_panel/src/project_panel.rs` | xoá `write_dock_and_opposite_rail` (7330-7366); `position` → `DockPosition::Right`; `position_is_valid` → `matches!(position, DockPosition::Right)`; `set_position` → thân rỗng + comment |
| `crates/project_panel/src/project_panel_settings.rs` | bỏ field `dock: DockSide` (19) và dòng `unwrap()` (104); bỏ import `DockSide` nếu thành mồ côi |
| `crates/project_panel/src/project_panel_tests.rs` | **xoá** hai test 10343-10406; thêm test mới (dưới) |
| `crates/settings_content/src/workspace.rs` | xoá `ProjectPanelSettingsContent::dock` (746-749) |
| `crates/settings_ui/src/page_data.rs` | xoá item "Project Panel Dock" (4283-4287), `29` → `28` |
| `assets/settings/default.json` | xoá dòng 787 `"dock": "left",` trong khối `project_panel` |

**Không tạo, không xoá file nào.**

## Implementation Steps

1. `settings_content/src/workspace.rs`: xoá field `dock` khỏi `ProjectPanelSettingsContent`.
2. `project_panel_settings.rs`: xoá field và dòng `from_settings`.
3. `default.json`: xoá dòng `"dock": "left",` trong `"project_panel"`.
4. `page_data.rs`: xoá item, sửa `[SettingsPageItem; 29]` → `28`.
5. `project_panel.rs`: xoá hàm `write_dock_and_opposite_rail` cùng toàn bộ doc-comment của
   nó (7300-7366 — doc nói về hành vi đã không còn); ghim ba method của `impl Panel`.
6. Xoá hai test cũ, viết hai test mới.
7. `cargo build -p project_panel -p settings_ui -p settings` rồi chạy test.

### `set_position` viết thế nào

```rust
/// Bên của panel không còn là một preference. Trait đòi method này nên nó ở
/// đây, nhưng không có gì để ghi — và `position_is_valid` đã chặn mọi giá trị
/// khác `Right` trước khi tới được đây.
fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
```

## Todo List

- [x] `ProjectPanelSettingsContent::dock` xoá
- [x] `ProjectPanelSettings::dock` + `unwrap()` xoá
- [x] `default.json` bỏ `project_panel.dock`
- [x] `page_data.rs` bỏ item, mảng `29 → 28`
- [x] `write_dock_and_opposite_rail` xoá cả hàm cả doc
- [x] `position` / `position_is_valid` / `set_position` ghim
- [x] Xoá `re_docking_the_panel_parks_the_rail_on_the_far_side`
- [x] Xoá `re_docking_the_panel_leaves_a_bottom_docked_git_panel_alone`
- [x] Thêm `the_project_panel_only_ever_lives_on_the_right`
- [x] Thêm `asking_the_project_panel_to_move_changes_nothing`
- [x] `cargo test -p project_panel -p settings -p settings_ui` xanh

## Test Matrix

| Test | Crate | Loại | Việc |
|---|---|---|---|
| `re_docking_the_panel_parks_the_rail_on_the_far_side` | project_panel | unit | **xoá** — nó test đúng cái swap đang bị bỏ |
| `re_docking_the_panel_leaves_a_bottom_docked_git_panel_alone` | project_panel | unit | **xoá** — cùng lý do |
| `the_project_panel_only_ever_lives_on_the_right` | project_panel | unit | **mới**: `position()==Right`; `position_is_valid(Left)==false`; `position_is_valid(Bottom)==false`; `position_is_valid(Right)==true` |
| `asking_the_project_panel_to_move_changes_nothing` | project_panel | unit | **mới**: gọi `set_position(Left)`, khẳng định `position()` vẫn `Right` **và** file settings không thay đổi một byte |
| toàn bộ suite `settings` | settings | unit | phải xanh — đây là cổng bắt "xoá key mà quên field" |
| toàn bộ suite `settings_ui` | settings_ui | unit | phải xanh — bắt lệch cỡ mảng |

Test thứ hai phải kiểm **cả** file settings, không chỉ `position()`: nếu chỉ kiểm
`position()` thì một `set_position` vẫn ghi bừa vào settings vẫn qua được.

## Success Criteria

- `grep -rn "project_panel.*dock\|\.dock" crates/project_panel/src/` không còn field dock
- `cargo test -p project_panel -p settings -p settings_ui` exit 0
- App khởi động được (`cargo build --bin zode` exit 0) — cổng bắt panic thiếu field

## Risk Assessment

| Rủi ro | Khả năng | Tác động | Countermove |
|---|---|---|---|
| Xoá key mà quên field resolved → panic khởi động, 332 test đỏ | Trung bình | Cao | Bốn chỗ nằm trong checklist trên; `cargo build --bin zode` là cổng cuối của phase |
| Quên giảm cỡ mảng `page_data.rs` | Trung bình | Thấp | Compile error trực tiếp — nhưng chỉ khi build `settings_ui`, nên build crate đó riêng |
| Menu right-click của panel thành rỗng giữa 01 và 02 | Cao | Thấp | Trạng thái trong-branch, không ship; 02 dọn. Nếu 02 chưa xong mà cần demo thì để nguyên `position_is_valid` cũ tới sau |

## Security Considerations

Không có mặt bảo mật. Không có ranh giới auth/dữ liệu nào bị chạm.

## Rollback

`git revert` một commit. Không có state ngoài repo: không migration DB, không key nào bị
ghi vào settings người dùng bởi phase này. Người dùng đã lỡ chạy bản này rồi revert vẫn
đọc lại được `project_panel.dock` cũ nếu họ còn giữ trong settings.json.

## Next Steps

Phase 03 cần `default.json` đã sạch key `project_panel.dock` trước khi đảo
`outline_panel.dock` / `git_panel.dock` — nếu 03 chạy trước, panel và rail sẽ chung một
bên trong một khoảng.
