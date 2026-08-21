# Phase 02 — Agent mở ra thành một tab của editor

**Status:** ✅ done (2026-08-21) · **Priority:** P1 · **Người dùng thấy:** agent thành tab của editor

## Mục tiêu

`AgentView` vào `workspace.panes()` thay vì vào pane của `AgentPanel`. Từ đây một thanh tab
duy nhất giữ cả code và agent, và mở file khi đang ở tab agent thì file thành tab kế bên.

## Việc

1. **`AgentView::open_inner`** — thay cái hop qua `AgentPanel`:
   - Tìm `AgentView` của agent này trong mọi pane của `workspace.panes()`. Có → activate
     item đó trong pane của nó, rồi `view.update(… view.show(mode, …))`.
   - Không có (hoặc `always_new`) → `workspace.add_item_to_active_pane(Box::new(view), None,
     true, window, cx)`.
   - **Giữ nguyên hình dạng `cx.spawn_in(window, …)` rồi `update_in`.** Đó không phải kiểu
     viết, đó là hàng rào chống re-entrancy mà crate này đã trả giá ba lần. Đọc mode đã ghi
     nhớ vẫn là một hop background như hiện tại.
   - Bỏ `workspace.focus_panel::<AgentPanel>` — `add_item_to_active_pane(focus_item: true)`
     đã lo focus.
2. **`AgentView::activate_for_agent`** — activate item trong pane của nó thay vì
   `focus_panel::<AgentPanel>`. Đây là đường một notification được nhận đi qua.
3. **`rail_agents.rs`** — `is_active` đọc `workspace.panes()` tìm `AgentView` của agent đó,
   không hỏi `AgentPanel::has_agent` nữa. Bỏ `use agent_ui::AgentPanel` và
   `Sidebar::agent_panel`. Comment ở dòng 39 (*"These stand for a pane item rather than a
   dock panel"*) từ thời trước dock — giờ nó lại đúng, giữ.

## Files

- sửa: `crates/agent_ui/src/agent_view.rs`
- sửa: `crates/sidebar/src/rail_agents.rs`
- `crates/agent_ui/src/agent_panel.rs` **chưa** xoá — phase 05 lo

## Chỗ phải cẩn thận

- **Tìm view: đọc pane nào, đọc lúc nào.** Quét `workspace.panes()` từ trong một
  `update_in` của workspace là an toàn; đọc một pane đang giữa update thì abort process chứ
  không phải trả về `None`. Đi qua borrow đã có trong tay.
- **`AgentPanel` vẫn còn trong workspace ở phase này** và vẫn nhận `add_panel`. Nó chỉ
  không còn ai đưa view vào. Cột sẽ tự không vẽ vì `visible_panel()` là `None` —
  `render_centre_with_own_columns` lọc theo đúng điều kiện đó.
- **`restore_tabs` ở `zed.rs:687` vẫn chạy** và vẫn replay vào panel. Ở phase này nó sẽ
  dựng lại agent trong cột — nghĩa là có thể thấy agent ở *cả hai* chỗ sau restart. Chấp
  nhận trong một phase; phase 05 xoá nó. Ghi vào phase 05 để không quên.

## Todo

- [x] `open_inner` → center pane, giữ nguyên hop spawn/update
- [x] `activate_for_agent` → activate item
- [x] `rail_agents.rs` đọc panes
- [x] Test: mở agent → là item của một pane trong `workspace.panes()`
- [x] Test: mở agent hai lần cùng agent → một tab, không phải hai
- [x] Test: `always_new` → hai tab
- [x] Test: mở file khi tab agent đang focus → file vào **cùng** pane, agent vẫn còn
- [x] `cargo check -p agent_ui -p sidebar` xanh

## Success criteria

Bấm rail → agent hiện thành tab cạnh tab code, cùng một thanh tab. Mở file lúc đó → tab mới
kế bên, agent không bị thay thế. Nút rail sáng đúng.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Re-entrancy khi với vào workspace | Giữ `cx.spawn_in` → `update_in`; không gọi thẳng trong handler |
| Đọc pane đang mid-update → abort | Chỉ đọc qua borrow đã có; test dispatch action thật, không dựng element bằng tay |
| Agent hiện ở cả hai chỗ sau restart | Đã biết, có chủ đích, phase 05 dọn |
