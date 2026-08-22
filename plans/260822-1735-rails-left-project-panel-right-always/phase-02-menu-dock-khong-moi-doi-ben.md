# Phase 02 — Menu dock chỉ mời khi có ≥2 bên (fix chung)

## Context Links

- [plan.md](plan.md) · Chặn bởi: **không** · Chặn: 03 · Song song được với: **01**
- `crates/workspace/src/dock.rs:148-166` (`move_to_next_position`)
- `crates/workspace/src/dock.rs:1909-1990` (vòng `POSITIONS` trong menu)
- `crates/workspace/src/dock.rs:2100+` (`pub mod test` — `TestPanel`)

## Overview

**Priority:** P1 · **Status:** done · **Effort:** 1.5h

Chặn hai đường người dùng move panel, và chặn ở **chỗ chung** chứ không đặc-cách project
panel. Với đúng một position hợp lệ, vòng `POSITIONS` hiện tại vẫn vẽ một entry "Dock
Right" đã tick sẵn và click vào không làm gì — **một control chết**.

## Key Insights

- Fix chung thắng fix riêng: cùng cái vòng đó phục vụ mọi panel. Đặc-cách "Project Panel"
  theo tên là chép lại `PANEL_ALWAYS_IN_STATUS_BAR` lần thứ hai, mà lần này không cần.
- **Menu có thể rỗng hoàn toàn.** `ProjectPanel` không implement `supports_flexible_size`
  → mặc định `false`. Bỏ hết entry position mà vẫn gắn `right_click_menu` = một popover
  trắng. Nên điều kiện phải ở mức **có gắn menu hay không**, không chỉ mức entry.
- `move_to_next_position` kết thúc bằng `.unwrap_or(DockPosition::Left)` (`dock.rs:165`).
  **Chẩn đoán sơ kỳ sai:** đây không phải bug thuần — với thứ tự duyệt `[Left, Bottom, Right]`
  và panel ở `Right`, `nth(1)` trả `None`, và fallback `Left` **chính là wrap-around lại phần
  tử đầu**. Chỉ sai với panel một-cạnh. Sửa đúng: wrap **trong danh sách hợp lệ**:
  `valid[(i + 1) % valid.len()]` với `current_position` là fallback nếu danh sách rỗng.
- `TestPanel::position_is_valid` hiện trả `true` cho mọi giá trị → không test được luật
  mới. Phải thêm field. `TestPanel` nằm trong `dock.rs` nên vẫn trong quyền sở hữu phase.
- Tách quyết định ra thành **hàm thuần** để test không cần window — đúng khuôn
  `rail_draws_panel` đã dùng, và comment của nó đã nói vì sao (`dock.rs:1830-1832`).

## Requirements

**Functional**
1. Panel có ≥2 position hợp lệ: menu **không đổi gì** so với hôm nay.
2. Panel có ≤1 position hợp lệ: không entry position nào.
3. Không entry position **và** không flex → không gắn `right_click_menu`, nút vẫn click
   được như thường.
4. `move_to_next_position` trên panel một-position: giữ nguyên vị trí.

**Non-functional:** không đọc workspace/dock entity nào mới trong `render` — đây là đường
đã từng abort process (`dock.rs:1796-1798`).

## Architecture

```
PanelButtons::render
  │
  ├─ dockable_positions(panel, cx) ─▶ Vec<DockPosition>   ← hàm thuần, ≥2 mới trả
  │
  ├─ nếu rỗng && !supports_flexible ─▶ panel_button(...)              (không menu)
  └─ ngược lại                      ─▶ right_click_menu(panel_button) (có menu)
```

`panel_button(...)` phải được tách thành **một** closure/hàm dùng cho cả hai nhánh — nếu
copy hai bản thì tooltip, count badge và element-id sẽ lệch nhau lúc nào không biết.
Hai nhánh khác type nên cùng `.into_any_element()`, và `buttons` thành `Vec<AnyElement>`.

Nhánh không-menu gọi `panel_button(false, ...)`: tham số đó là "menu đang mở", mà không có
menu thì nó không bao giờ đúng.

## Related Code Files

**Sửa** — chỉ một file: `crates/workspace/src/dock.rs`

| Vùng | Việc |
|---|---|
| gần `rail_draws_panel` (~1833) | thêm `pub fn dockable_positions(panel: &Arc<dyn PanelHandle>, cx: &App) -> Vec<DockPosition>` |
| `PanelHandle::move_to_next_position` (148-166) | `.unwrap_or(DockPosition::Left)` → `.unwrap_or(current_position)` |
| `PanelButtons::render` (1909-1990) | tách `panel_button`, dùng `dockable_positions`, hai nhánh có/không menu |
| `pub mod test` → `TestPanel` | thêm `pub valid_positions: Vec<DockPosition>` (mặc định cả ba, giữ y hành vi test cũ); `position_is_valid` đọc nó |
| `mod tests` | 3 test mới (dưới) |

**Không tạo file. Không xoá file.**

`workspace.rs::move_focused_panel_to_next_position` (3328-3350) **không sửa** — nó chỉ
tìm dock đang focus rồi gọi xuống; luật nằm ở `move_to_next_position`.

## Implementation Steps

1. Viết `dockable_positions`:
   ```rust
   /// Những bên menu được phép mời. Một bên duy nhất KHÔNG phải một lựa chọn:
   /// vẽ nó ra chỉ cho một entry đã tick sẵn mà click vào không làm gì.
   ///
   /// Tách khỏi `render` để test được mà không cần window — cùng lý do
   /// `rail_draws_panel` được tách.
   pub fn dockable_positions(panel: &Arc<dyn PanelHandle>, cx: &App) -> Vec<DockPosition> {
       let valid: Vec<_> = [DockPosition::Left, DockPosition::Right, DockPosition::Bottom]
           .into_iter()
           .filter(|position| panel.position_is_valid(*position, cx))
           .collect();
       if valid.len() >= 2 { valid } else { Vec::new() }
   }
   ```
2. Sửa `move_to_next_position` → `.unwrap_or(current_position)`, thêm comment nói vì sao
   `Left` là câu trả lời sai.
3. Thêm `valid_positions` vào `TestPanel`, mặc định `vec![Left, Right, Bottom]`; cập nhật
   mọi chỗ khởi tạo `TestPanel` trong repo (`grep -rn "TestPanel {" crates/`).
4. Tách `panel_button` trong `render`, rồi rẽ hai nhánh.
5. `cargo test -p workspace` — chú ý flake đã biết ở phần Risk.

## Todo List

- [x] `dockable_positions` + doc-comment
- [x] `move_to_next_position` fallback về `current_position`
- [x] `TestPanel.valid_positions` + cập nhật mọi call site khởi tạo
- [x] Tách `panel_button` thành một chỗ dùng chung
- [x] Nhánh không-menu khi rỗng entry **và** không flex
- [x] 3 test mới
- [x] `cargo test -p workspace` xanh (đối chiếu baseline HEAD)

## Test Matrix

| Test | Loại | Nội dung |
|---|---|---|
| `a_panel_with_one_valid_position_is_offered_none` | unit (hàm thuần) | `dockable_positions` trả rỗng khi 1 hợp lệ; trả đủ 2 khi 2 hợp lệ; trả đủ 3 khi 3 — **nhánh ≥2 là phần chống hồi quy cho panel bình thường**, không phải phần thêm |
| `moving_a_one_position_panel_leaves_it_alone` | unit + window | `TestPanel` với `valid_positions=vec![Right]`, `position=Right` → `move_to_next_position` → vẫn `Right`. Hôm nay test này **đỏ** (thành `Left`) |
| `moving_a_three_position_panel_still_cycles` | unit + window | Left → Bottom → Right → Left, chống hồi quy cho đường cũ |
| `the_rail_draws_the_panels_on_its_own_edge_and_nothing_else` | unit (có) | phải xanh, chưa sửa ở phase này |
| `re_docking_the_project_panel_never_lifts_it_into_the_rail` | unit (có) | phải xanh, chưa sửa ở phase này |

Không viết test "menu rỗng thì không gắn `right_click_menu`" — `ContextMenu` sau khi build
không phơi ra số entry, nên test đó sẽ phải vẽ và đọc DOM. Luật đã được `dockable_positions`
+ `supports_flexible_size` quyết định trước khi tới UI; phần còn lại là **mắt người**, và
phase 05 nhận nó.

## Success Criteria

- `cargo test -p workspace` exit 0, so với baseline HEAD (xem Risk)
- `./script/clippy` không warning mới trong `workspace`
- Đọc diff: mọi panel 2–3 position không có một dòng nào đổi hành vi

## Risk Assessment

| Rủi ro | Khả năng | Tác động | Countermove |
|---|---|---|---|
| Tách `panel_button` làm lệch element-id → tooltip/badge cache sai | Trung bình | Trung bình | Một bản duy nhất dùng cho cả hai nhánh; giữ nguyên `(name, is_active_button as u64)` |
| Đổi `unwrap_or` làm hỏng panel 3-position nào đó dựa vào fallback `Left` | Thấp | Trung bình | `moving_a_three_position_panel_still_cycles`; và `nth(1)` chỉ trả `None` khi <2 hợp lệ |
| `test_hibernate_after_ms_zero_disables_hibernation` đỏ | Cao | Thấp | **Đã biết là pre-existing tại f2b53d3**, đỏ 100% khi chạy riêng. `git stash` + chạy đúng lệnh đó ở HEAD trước khi gọi nó là hồi quy |
| Test worktree/terminal treo trong sandbox | Cao | Thấp | Đã biết. Skip tường minh, **không** pipe cargo dài qua `tail` |

## Security Considerations

Không có. Menu này không gác quyền gì.

## Rollback

`git revert` một commit, một file. `dockable_positions` là hàm mới nên không ai ngoài
`dock.rs` gọi; `TestPanel` chỉ dùng trong test.

## Next Steps

Phase 03 sở hữu `dock.rs` tiếp (bỏ tham số `rail_side` của `rail_draws_panel`) nên phải
đợi 02 xong. Phase 05 kiểm bằng mắt: right-click nút project panel không ra popover trắng.
