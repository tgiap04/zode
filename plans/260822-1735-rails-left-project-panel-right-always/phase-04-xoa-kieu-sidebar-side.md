# Phase 04 — Xoá kiểu `SidebarSide` và mọi nhánh chết

## Context Links

- [plan.md](plan.md) · Chặn bởi: **03** · Chặn: 05
- `crates/settings_content/src/workspace.rs:1075-1093` (enum)
- `crates/workspace/src/multi_workspace.rs:14, 60-82, 108, 148, 220, 402-423, 631, 665, 2669-2909`
- `crates/workspace/src/workspace.rs:5058-5230` (`activate_pane_in_direction`)
- `crates/platform_title_bar/src/platform_title_bar.rs:238-320`

## Overview

**Priority:** P2 · **Status:** done · **Effort:** 2h

**Thay đổi hành vi: 0%.** Sau phase 03 mọi nhánh `Right` đã không tới được. Đây là dọn
nhà — nhưng là dọn nhà xuyên 6 crate và có một chỗ 9 site không test nào phủ.

**Chẩn đoán sơ kỳ sai:** plan nói viết hai test đặc tả `activate_pane_in_direction`
trong `crates/workspace`. Điều đó không thể hiện thực: một `FocusHandle` tạo bằng
`cx.focus_handle()` mà không được render không giữ được focus, nên test sẽ luôn đỏ trên
code chưa sửa và "sửa" bằng nới assertion sẽ cho test xanh vĩnh viễn mà không chứng minh gì.
Hai test sống ở `crates/sidebar/src/sidebar_tests.rs` nơi có `MultiWorkspace` + `Sidebar`
thật vẽ thật.

## Key Insights

- `SidebarSide` khai ở `settings_content` và **re-export public** qua
  `multi_workspace.rs:14` (`pub use settings::SidebarSide;`) — đó là đường `sidebar`,
  `database_ui`, `platform_title_bar` thấy nó. Xoá re-export là compiler tự liệt kê hết.
- Chỗ nguy nhất là `Workspace::activate_pane_in_direction` (`workspace.rs:5118-5230`):
  **9 site** rẽ theo `sidebar_on_right` (6 nhánh `match` + 3 giá trị dẫn xuất), và `grep` cho thấy **không test nào** gọi hàm
  này. Collapse ở đây là sửa cơ học dưới compiler đỏ, đúng loại việc dễ sai lặng lẽ.
- Countermove không phải "viết test cho cả 9 site" — mà là **bảng liệt kê đủ 9 site kèm
  nhánh sống sót** (dưới), biến việc sửa thành checklist, cộng 2 test đặc tả cho hai nhánh
  người dùng gặp hằng ngày. 80/20.
- `SidebarRenderState.side` + `occupies(side)` biến mất → `status_bar` và
  `platform_title_bar` collapse. Giữ `occupies()` mà bỏ tham số là đặt tên hai lần cho
  cùng một thứ: nó thành `open || rail`, mà `edge_width > 0` đã nói y vậy. Bỏ hẳn.
- Telemetry `side = "left"|"right"` trong `Sidebar Toggled` (`multi_workspace.rs:631, 665`):
  property của một hằng không mang thông tin gì. **Bỏ property, giữ event** — event vẫn đo
  tần suất toggle.
- **Ghi chú cho implementation:** `Workspace::OWN_COLUMN_POSITION` là một **associated const**,
  không phải một free const. Điều này có ý nghĩa vì một const không borrow gì, nên nó an toàn
  dùng từ một `PanelHandle` (`database_panel.rs::position()`) ngay cả khi không thể giữ một
  workspace handle.
- `strum::VariantArray/VariantNames` + `JsonSchema` trên enum: schema sinh runtime, **không
  có snapshot commit** trong repo (đã kiểm) nên không có cổng drift nào đỏ.

## Requirements

**Functional:** không có. Không một pixel nào đổi.

**Non-functional:** `grep -rn "SidebarSide" crates/` rỗng; `./script/clippy` 0 warning
(dead-code/unused-variable là cổng thật của phase này); mọi test đang xanh vẫn xanh.

## Architecture — cây collapse

```
settings_content: enum SidebarSide                                    ─▶ XOÁ
  └─ workspace: pub use settings::SidebarSide                         ─▶ XOÁ
       ├─ Sidebar::side() / SidebarHandle::side()                     ─▶ XOÁ (trait method)
       │    └─ sidebar::sidebar.rs::side() impl                       ─▶ XOÁ
       ├─ MultiWorkspace::sidebar_side()                              ─▶ XOÁ
       ├─ SidebarRenderState.side + occupies()                        ─▶ XOÁ
       │    ├─ status_bar::SidebarStatus.side                          ─▶ XOÁ
       │    └─ platform_title_bar (3 chỗ)                              ─▶ collapse
       ├─ multi_workspace::render sidebar_on_right (7 chỗ)            ─▶ collapse
       ├─ workspace::activate_pane_in_direction (9 chỗ)               ─▶ collapse
       ├─ sidebar::rail_side() → rail.rs, render.rs, rail_panels.rs   ─▶ XOÁ + inline
       └─ telemetry side property (2 chỗ)                             ─▶ XOÁ
```

### Bảng 9 site của `activate_pane_in_direction` — `sidebar_on_right = false`

| Dòng | Trước | Sau |
|---|---|---|
| 5118-5124 | `let sidebar_on_right = …` | xoá cả block |
| 5126-5130 | `away_from_sidebar = Left/Right` | `SplitDirection::Right` |
| 5132-5136 | `(near_dock, far_dock)` | `(&self.left_dock, &self.right_dock)` |
| 5157-5162 | Center+Left: `if on_right { dock } else { dock.or(sidebar) }` | `dock_target.or(sidebar_target)` |
| 5165-5170 | Center+Right: `if on_right { dock.or(sidebar) } else { dock }` | `dock_target` |
| 5184-5188 | LeftDock+Left: `if on_right { None } else { sidebar }` | `sidebar_target` |
| 5197-5203 | BottomDock+Left | `dock_target.or(sidebar_target)` |
| 5205-5211 | BottomDock+Right | `dock_target` |
| 5221-5226 | RightDock+Right: `if on_right { sidebar } else { None }` | `None` |

Luật cơ học: **giữ nhánh `else`, xoá nhánh `if`.** Mọi dòng trong bảng phải đối chiếu 1-1
với diff khi review — đó là phần thay cho test.

Cẩn thận `sidebar_target` bị move: sau collapse nó dùng ở 3 nhánh `match`, mà `match` chỉ
chạy một nhánh nên compiler chấp nhận; nếu không thì `.clone()`.

### `platform_title_bar` collapse

| Trước | Sau |
|---|---|
| `left_edge_covered = if side==Left { edge_width } else { px(0.) }` | `let left_edge_covered = sidebar.edge_width;` |
| `show_left_controls = !(open && side==Left)` | `!sidebar.open` |
| `!sidebar.occupies(Right)` (rounded_tr) | `true` → bỏ hẳn điều kiện, còn `!(tiling.top \|\| tiling.right)` |
| `!sidebar.occupies(Left)` (rounded_tl) | `!(sidebar.open \|\| sidebar.rail)` |
| `show_right_controls = !(open && side==Right)` | `true` → bỏ biến, luôn vẽ |

### `status_bar` collapse

| Trước | Sau |
|---|---|
| `!(open && side==Right)` (rounded_br) | bỏ điều kiện, còn `!(tiling.bottom \|\| tiling.right)` |
| `!(open && side==Left)` (rounded_bl) | `!sidebar.open` |

`SidebarStatus` chỉ còn field `open` → cân nhắc thu về `bool`, nhưng giữ struct thì diff
nhỏ hơn và tên vẫn nói đúng. Chọn giữ struct.

### `sidebar` crate collapse

| File | Trước | Sau |
|---|---|---|
| `rail.rs:12-14` | `fn rail_side(cx)` | **xoá hàm** |
| `rail.rs:114-116` | `match side { Left => border_r_1+rounded_tr, Right => … }` | `.border_r_1().rounded_tr(SURFACE_ROUNDING)` |
| `rail.rs:143, 175-177` | tham số `side` + `match` cho pill | bỏ tham số; `.left_0()` |
| `render.rs:31-33` | `match side { Left => rail rồi panel, … }` | `this.child(rail).children(panel)` |
| `rail_panels.rs:19-21` | `match rail_side { Left => left_dock, … }` | `workspace.left_dock().clone()` |
| `sidebar.rs:167-169` | `fn side()` impl | **xoá** |

### `multi_workspace::render` collapse

`sidebar_on_right = false` → resize handle dùng `.right(-SIZE/2.)`; `(left_sidebar,
right_sidebar)` → `(sidebar, None)` (bỏ tuple, dùng trực tiếp); drag `new_width =
e.event.position.x - rail_width`; `Tiling { left: enabled && open, right: false }`.

## Related Code Files

**Sửa:** `settings_content/src/workspace.rs`, `workspace/src/multi_workspace.rs`,
`workspace/src/status_bar.rs`, `workspace/src/workspace.rs`, `workspace/src/dock.rs`
(import dòng 6), `sidebar/src/{rail.rs, render.rs, sidebar.rs, rail_panels.rs,
sidebar_tests.rs}`, `platform_title_bar/src/platform_title_bar.rs`,
`database_ui/src/database_panel.rs` (import dòng 21) + `database_panel_tests.rs` (import).

**Không tạo, không xoá file.**

## Implementation Steps

1. **Trước khi sửa gì:** viết 2 test đặc tả cho `activate_pane_in_direction` (dưới), chạy
   xanh **trên code hiện tại**. Đây là baseline, không phải test mới của feature.
2. Xoá `pub use settings::SidebarSide` ở `multi_workspace.rs:14` → `cargo build -p workspace
   -p sidebar -p database_ui -p platform_title_bar` liệt kê toàn bộ site.
3. Sửa theo thứ tự lá → gốc: `sidebar` → `platform_title_bar` → `database_ui` →
   `status_bar` → `multi_workspace` → `workspace.rs`.
4. `activate_pane_in_direction`: sửa theo bảng 9 site, đối chiếu từng dòng.
5. Bỏ `side` khỏi `SidebarRenderState`, bỏ `occupies()`, bỏ `Sidebar::side` +
   `SidebarHandle::side` + `MultiWorkspace::sidebar_side`.
6. Bỏ property `side` khỏi hai `telemetry::event!`.
7. Xoá enum trong `settings_content`.
8. `cargo fmt -p <crate>` **cẩn thận**: nó format cả crate, repo này có drift sẵn — diff
   với HEAD rồi revert mọi hunk không liên quan.
9. `./script/clippy` + full suite.

## Todo List

- [x] 2 test đặc tả `activate_pane_in_direction` xanh **trước** khi sửa
- [x] Xoá re-export → thu danh sách site từ compiler
- [x] `sidebar` crate: xoá `rail_side()`, inline 5 chỗ
- [x] `platform_title_bar`: 5 chỗ theo bảng
- [x] `status_bar`: 2 chỗ + `SidebarStatus.side`
- [x] `multi_workspace`: `SidebarRenderState.side`, `occupies`, `side()` ×3, `sidebar_side()`, render ×6, telemetry ×2
- [x] `workspace.rs`: 9 site theo bảng, đối chiếu 1-1 với diff
- [x] `database_ui`: 2 import
- [x] Xoá `enum SidebarSide`
- [x] `grep -rn "SidebarSide\|sidebar_side\|sidebar_on_right\|rail_side" crates/` rỗng
- [x] `cargo fmt` diff đã lọc hunk lạ
- [x] `./script/clippy` 0 warning

## Test Matrix

| Test | Crate | Loại | Việc |
|---|---|---|---|
| `moving_focus_from_the_left_dock_reaches_the_rail` | workspace | integration | **mới, viết trước khi sửa**: workspace có sidebar mở, focus ở left dock, `ActivatePaneInDirection(Left)` → focus tới sidebar. Phủ site 5184 |
| `moving_focus_left_from_the_centre_falls_through_to_the_rail` | workspace | integration | **mới, viết trước khi sửa**: left dock đóng, focus center, Left → sidebar. Phủ site 5157 |
| 7 site còn lại của bảng | — | **review diff** | Không viết test. Chúng là nhánh không có keybinding mặc định nào chạm tới, và bảng 9 site là bằng chứng thay thế. **Ghi thẳng ra đây là chỗ plan này chấp nhận rủi ro** |
| toàn bộ suite `workspace`, `sidebar`, `database_ui`, `platform_title_bar` | | | phải xanh **và không đổi số lượng test** — phase này không được xoá test nào |
| `the_rail_stays_outside_the_panel_on_the_left_edge` | sidebar | integration | xanh; nó vẽ thật nên là chỗ bắt lỗi collapse thứ tự rail/panel |

## Success Criteria

- `grep -rn "SidebarSide\|sidebar_side\|sidebar_on_right\|rail_side" crates/` → rỗng
- Số test xanh **bằng** số test xanh cuối phase 03 (+2 test đặc tả)
- `./script/clippy` 0 warning · `cargo build --bin zode` exit 0
- Diff review: mọi dòng trong bảng 9 site được đối chiếu

## Risk Assessment

| Rủi ro | Khả năng | Tác động | Countermove |
|---|---|---|---|
| Collapse sai một trong 9 site `activate_pane_in_direction` → điều hướng focus câm ở một hướng, không crash, không test đỏ | **Trung bình** | **Cao** | Bảng 9 site + 2 test đặc tả viết **trước**; nhận rủi ro còn lại tường minh |
| Bỏ `occupies()` làm mất góc bo / che traffic light macOS | Trung bình | Trung bình | Bảng collapse `platform_title_bar` ghi từng biểu thức; nhưng chỉ **mắt người trên macOS** xác nhận được — phase 05 |
| `cargo fmt -p` kéo theo drift sẵn có của repo | **Cao** | Thấp | Diff với HEAD, revert hunk không liên quan trước khi commit |
| Đọc dock trong `Sidebar::render` → abort main thread | Thấp | **Cao** | Phase này chỉ **bớt** đường đọc, không thêm. Không đưa `workspace.read()` mới nào vào `render` |
| Xoá `MultiWorkspace::sidebar_side` bị hiểu là đã chữa re-entrancy | Trung bình | Thấp | `sidebar_render_state` vẫn borrow cho `edge_width`. Ghi rõ trong commit message: đơn giản hoá, không phải fix |
| `test_hibernate_after_ms_zero_disables_hibernation` đỏ | Cao | Thấp | Pre-existing tại f2b53d3, đỏ khi chạy riêng. Đối chiếu HEAD trước khi kết luận |

## Security Considerations

Không có. Telemetry **giảm** một property, không thêm; không dữ liệu người dùng nào mới
được gửi đi.

## Rollback

Revert độc lập được và **an toàn tuyệt đối**: phase này không đổi hành vi nào, nên revert
nó chỉ trả lại nhánh chết. Nếu hết thời gian, dừng ở cuối phase 03 là một trạng thái ship
được — đích đã đạt, chỉ còn nợ dead code.

## Next Steps

Phase 05: vẽ thật, mắt người, và cổng cuối trên toàn repo.
