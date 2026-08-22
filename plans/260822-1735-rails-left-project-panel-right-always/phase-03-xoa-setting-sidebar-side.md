# Phase 03 — Xoá `multi_project.sidebar_side`, rail chốt bên trái

## Context Links

- [plan.md](plan.md) · Chặn bởi: **01, 02** · Chặn: 04
- `crates/settings_content/src/workspace.rs:1163-1168`
- `crates/workspace/src/workspace_settings.rs:71-72, 197`
- `crates/settings/src/settings.rs:249-280` (test invariant)
- `assets/settings/default.json:208, 904, 956`

## Overview

**Priority:** P1 · **Status:** done · **Effort:** 1.5h

Seam thật của cả plan. Sau phase này **hành vi đã đúng đích** và không còn key nào bị bỏ
qua. Phase 04 chỉ còn dọn kiểu và nhánh chết, không đổi hành vi một chút nào.

## Key Insights

- Đây là chỗ ba mặc định đảo cùng lúc: `sidebar_side` mất key, `outline_panel.dock` và
  `git_panel.dock` `"right"` → `"left"`. Đảo lệch nhịp = nút của git/outline **biến mất
  khỏi rail**, không lỗi, không empty state, chỉ là một dải rail trống.
- Test `the_panel_docks_line_up_with_the_rails_side` (`settings.rs:254`) đọc
  `defaults["multi_project"]["sidebar_side"]` — key đang bị xoá. **Viết lại, không xoá**:
  nó là thứ duy nhất chốt "panel cưỡi rail phải dock đúng bên rail".
- `crates/settings` không thấy được `workspace`, nên test đó không gọi được
  `own_column_position`. Nó phải mang một literal `"left"` cùng comment trỏ về nơi hằng
  thật sống. Đó là giới hạn của hướng phụ thuộc, không phải lười.
- Vì thế **bằng chứng hành vi phải nằm ở crate khác**: 4 test đang parametrize theo hai
  bên sẽ collapse thành khẳng định "trái", và chúng là thứ chứng minh bố cục, không phải
  cái literal trong `settings`.
- `MultiWorkspace::sidebar_side()` borrow entity sidebar (`s.side(cx)`) — chính cái
  `multi_workspace.rs:163` cảnh báo. Xoá nó **bớt** một borrow trên đường render, nhưng
  `sidebar_render_state` vẫn còn borrow cho `edge_width`, nên đây là **đơn giản hoá, không
  phải sửa bug**. Đừng ghi vào changelog như đã chữa re-entrancy.

## Requirements

**Functional:** key `multi_project.sidebar_side` không còn tồn tại ở bất kỳ tầng nào; rail
đứng bên trái; cột own (agent, database) đứng bên trái; nút git + outline nằm trong rail.

**Non-functional:** không panic khởi động; suite `settings` xanh.

## Architecture

Điểm hằng duy nhất — **không tạo const mới xuyên crate**:

```
Workspace::own_column_position()  ─▶  DockPosition::Left      ← chỗ được đặt tên
dock::rail_draws_panel(name, position, rail_drawn)
        └─ matches!(position, DockPosition::Left)
sidebar::rail.rs / render.rs / rail_panels.rs  ─▶ inline Left
settings::…test…                                ─▶ literal "left" + comment trỏ lên trên
```

**Phương án bị loại:** đặt `pub const RAIL_SIDE: DockSide = DockSide::Left` trong
`settings_content` cho cả test và code đọc. DRY hơn về hình thức, nhưng nó bắt mọi chỗ
layout phải `match RAIL_SIDE { Left => …, Right => … }` — tức là **giữ lại đúng những
nhánh chết** mà cả plan này đang đi xoá. Hằng chỉ đáng nếu bên rail còn có thể đổi, và đề
bài nói nó không đổi nữa. YAGNI.

`own_column_position` giữ lại **dưới dạng method** (bỏ tham số `cx`), vì nó là chỗ duy nhất
được đặt tên cho câu "cột đứng cạnh rail"; 6 call site bỏ `cx` là sửa cơ học.

## Related Code Files

**Sửa**

| File | Việc |
|---|---|
| `crates/settings_content/src/workspace.rs` | xoá `MultiProjectContent::sidebar_side` (1163-1168). **Giữ enum `SidebarSide`** — phase 04 mới xoá |
| `crates/workspace/src/workspace_settings.rs` | xoá field resolved (71-72) và dòng `unwrap()` (197) |
| `assets/settings/default.json` | xoá dòng 208; dòng 904 `"right"`→`"left"`; dòng 956 `"right"`→`"left"` |
| `crates/workspace/src/workspace.rs` | 1710-1714 database dock → `DockPosition::Left`; 7767-7772 `own_column_position` → hằng, bỏ `cx`; 6 call site (1727, 7827, 7854, 8212, 8236) bỏ `cx`; test 13512, 13679 bỏ setter; test 13752-13762 collapse |
| `crates/workspace/src/dock.rs` | `rail_draws` (1808-1815) bỏ đọc setting; `rail_draws_panel` bỏ tham số `rail_side`; test 2302-2355 collapse |
| `crates/workspace/src/multi_workspace.rs` | dòng 2377 `sidebar_side: None` bỏ |
| `crates/workspace/src/multi_workspace_tests.rs` | 42, 71, 99 bỏ `sidebar_side: None` |
| `crates/sidebar/src/rail.rs` | `rail_side()` (12-14) trả `SidebarSide::Left` (tạm, 04 xoá) |
| `crates/sidebar/src/rail_panels.rs` | test 239-256 collapse |
| `crates/sidebar/src/sidebar_tests.rs` | 248-280 collapse, đổi tên test |
| `crates/database_ui/src/database_panel.rs` | `position()` (333-341) → `DockPosition::Left` |
| `crates/database_ui/src/database_panel_tests.rs` | 117-140 collapse |
| `crates/settings/src/settings.rs` | viết lại test 249-280 |

## Implementation Steps

1. `default.json` trước: xoá 208, đảo 904 và 956.
2. `settings_content`: xoá field. `workspace_settings.rs`: xoá field resolved + `unwrap()`.
3. `cargo build -p workspace` → compiler liệt kê mọi reader trực tiếp. Sửa lần lượt:
   `workspace.rs:1710`, `workspace.rs:7768`, `dock.rs:1812`.
4. `rail_draws_panel`: bỏ tham số `rail_side`, thân thành
   `rail_drawn && panel_name != PANEL_ALWAYS_IN_STATUS_BAR && position == DockPosition::Left`.
   Sửa 2 call site (`dock.rs:1809`, `rail_panels.rs:42`).
5. `sidebar/rail.rs::rail_side()` trả `SidebarSide::Left` — **có TODO trỏ phase 04**.
6. `database_panel.rs::position()` trả `DockPosition::Left`.
7. Bỏ mọi setter `sidebar_side` trong test + struct literal (5 file).
8. Collapse 4 test parametrize + viết lại test invariant trong `settings`.
9. `cargo test -p settings -p workspace -p sidebar -p database_ui`.

### Test invariant viết lại

```rust
/// Rail đứng bên TRÁI và bên đó không còn là setting — `Workspace::
/// own_column_position` là chỗ code nói ra điều đó. Crate này không thấy được
/// `workspace` (hướng phụ thuộc ngược lại), nên bên rail ở đây là literal;
/// đổi nó ở kia mà quên ở đây thì test này đỏ, đúng ý.
const RAIL_SIDE: &str = "left";

#[test]
fn the_rail_riding_panels_dock_where_the_rail_stands() {
    // outline + git cưỡi rail: dock lệch bên rail là nút của chúng vắng mặt
    // khỏi rail, không lỗi, không empty state.
    for panel in ["outline_panel", "git_panel"] { assert_eq!(defaults[panel]["dock"], RAIL_SIDE); }

    // project panel không còn key `dock` nào để mà lệch — nó cứng bên phải
    // trong code. Một default sót lại ở đây là default chết.
    assert!(defaults["project_panel"].get("dock").is_none());

    // Và bên rail không còn là setting nữa.
    assert!(defaults["multi_project"].get("sidebar_side").is_none());
}
```

## Todo List

- [x] `default.json`: xoá `sidebar_side`, đảo outline + git sang `"left"`
- [x] `settings_content` + `workspace_settings` bỏ field
- [x] `workspace.rs`: database dock, `own_column_position`, 6 call site
- [x] `rail_draws_panel` bỏ tham số `rail_side`, 2 call site
- [x] `sidebar/rail.rs::rail_side()` trả hằng + TODO(phase-04)
- [x] `database_panel::position()` trả hằng
- [x] Bỏ 8 setter/literal `sidebar_side` trong test
- [x] Collapse 4 test parametrize
- [x] Viết lại test invariant trong `settings`
- [x] `cargo build --bin zode` exit 0

## Test Matrix

| Test | Crate | Việc |
|---|---|---|
| `the_panel_docks_line_up_with_the_rails_side` | settings | **viết lại** → `the_rail_riding_panels_dock_where_the_rail_stands` (xem trên) |
| `an_own_column_follows_the_rail` (13738+) | workspace | **collapse** → `the_own_columns_stand_left_beside_the_rail`: bỏ vòng hai bên, khẳng định `own_column_position() == Left` |
| `the_rail_draws_the_panels_on_its_own_edge_and_nothing_else` | workspace | **collapse**: bỏ vòng, khẳng định Left→true, Right→false, Bottom→false |
| `re_docking_the_project_panel_never_lifts_it_into_the_rail` | workspace | **collapse**: bỏ vòng `for side in [...]` |
| `the_rail_stays_outside_the_panel_on_either_edge` | sidebar | **collapse + đổi tên** → `..._on_the_left_edge` |
| test `rail_panels` (239) | sidebar | **collapse**: bỏ `(Right, 1)`, giữ khẳng định 2 panel bên trái |
| `the_column_follows_the_rail` | database_ui | **collapse** → `the_column_stands_left_beside_the_rail` |
| `dragging_an_own_columns_handle_resizes_that_column` | workspace | bỏ setter, còn lại giữ nguyên |
| `a_column_cannot_be_dragged_past_the_room_beside_it` | workspace | bỏ setter, còn lại giữ nguyên |

Collapse **không phải** xoá: mỗi test giữ đúng nửa "trái" của vòng cũ và trở thành lời
khẳng định về bố cục đích. Xoá chúng là mất đúng phần chứng minh mà yêu cầu đòi.

## Success Criteria

- `grep -rn "sidebar_side" crates/` → **rỗng**
- `cargo test -p settings -p workspace -p sidebar -p database_ui` exit 0
- `cargo build --bin zode` exit 0
- Mở app: rail trái, project panel phải, nút git + outline trong rail

## Risk Assessment

| Rủi ro | Khả năng | Tác động | Countermove |
|---|---|---|---|
| Đảo `sidebar_side` mà quên đảo outline/git → rail trống trơn, không lỗi nào | Trung bình | **Cao** (người dùng thấy ngay) | Ba dòng `default.json` là bước 1, cùng một commit; test `settings` viết lại chốt đúng cặp này |
| Xoá field content mà quên field resolved → panic khởi động | Trung bình | Cao | `cargo build --bin zode` là cổng phase; hai dòng nằm cạnh nhau trong checklist |
| Collapse test thành tautology ("giá trị hôm nay đúng") | Trung bình | Trung bình | Mỗi test collapse phải giữ **nhánh phủ định**: Right→false, Bottom→false. Chỉ khẳng định Left→true là không chứng minh gì |
| `rail.rs::rail_side()` trả hằng bị bỏ quên, sống mãi | Thấp | Thấp | TODO(phase-04) trong code + item trong phase 04 |
| Đọc dock từ trong `Sidebar::render` → abort | Thấp | **Cao** | Phase này **không thêm** đường đọc nào; `rail_draws` sau sửa đọc **ít hơn** trước |

## Security Considerations

Không có. Không setting nào ở đây gác quyền hay dữ liệu.

## Rollback

Revert một commit. Người dùng đã chạy bản này: `sidebar_side` trong settings.json của họ
thành key lạ **bị bỏ qua** (`SettingsContent` không `deny_unknown_fields` — đã kiểm), nên
revert đưa họ về đúng chỗ cũ, không mất gì.

## Next Steps

Phase 04 xoá kiểu `SidebarSide` và mọi nhánh chết mà phase này để lại. Sau 03, hành vi đã
là đích — nếu hết thời gian thì **dừng được ở đây** và 04 lùi sang lần sau.
