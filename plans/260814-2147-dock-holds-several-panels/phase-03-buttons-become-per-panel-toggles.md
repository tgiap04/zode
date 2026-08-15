# Phase 03 — Nút rail/status bar thành toggle từng panel

**Context:** [plan.md](plan.md) · [phase-01](phase-01-a-dock-holds-a-set-of-visible-panels.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-14) · **Blocked by:** 01

Chồng panel dựng được rồi thì phải có cách bật/tắt từng cái. Hôm nay nút rail nghĩa là "đổi panel đang hiện"; nó phải thành "bật/tắt panel này".

## Chỗ phải sửa

`sidebar/rail_panels.rs:66-92` hiện đọc `(is_open, active_index, close_dock)` rồi suy ra `is_active = is_open && Some(index) == active_index`, và:

- panel đang hiện → hành động là **đóng cả dock** (`close_dock`)
- panel khác → `toggle_action()` để đưa nó ra trước

Với nhiều panel, đúng phải là:

- [ ] `is_active` = panel này nằm trong `visible` (nhiều nút cùng sáng là **đúng**, không phải lỗi)
- [ ] Bấm panel đang hiện → ẩn **riêng nó**; nếu nó là cái cuối cùng thì đóng dock
- [ ] Bấm panel chưa hiện → thêm vào chồng, **không** đá cái đang hiện ra

Cùng luật đó cho `PanelButtons` (`dock.rs:1247`) ở status bar.

## Nút rail của agent đã đúng sẵn

`sidebar/rail_agents.rs` tính `is_active` bằng `AgentPanel::has_agent` — theo *pane đang mở*, không theo `active_index`. Nên nó vốn đã cho phép hai nút cùng sáng và **không phải đổi** ở phase này. Đây là mẫu để phần panel bám theo, không phải ngoại lệ cần dọn.

## Bàn phím

- [ ] Rà mọi action `Toggle*Panel` (`ToggleRightDock`, và toggle riêng của từng panel) — chúng phải theo cùng luật bật/tắt, nếu không bàn phím và chuột nói hai thứ khác nhau
- [ ] Ngữ nghĩa `close_panel_on_toggle` (`workspace.rs:4251`) phải xét lại: "đóng panel" giờ là bỏ khỏi chồng, không phải đóng dock

## Test

- [ ] Bật hai panel một dock → **cả hai** nút rail cùng ở trạng thái active
- [ ] Tắt một → nút đó tắt, nút kia còn sáng, dock vẫn mở
- [ ] Tắt nốt cái cuối → dock đóng
- [ ] Panel không nằm bên rail vẫn ra status bar như cũ (`rail_draws_panel` không đổi)

## Rủi ro

`git_ui/commit_modal.rs` đọc `active_panel_index`. Phase 01 giữ hàm đọc dẫn xuất nên nó không gãy, nhưng cần xác nhận cái nó thật sự muốn là "panel được focus gần nhất" chứ không phải "panel duy nhất đang hiện" — hai thứ này chỉ trùng nhau khi chồng có một phần tử.

---

## Xong (2026-08-14)

Người dùng chọn **kiểu VS Code**: bấm panel chưa hiện thì thêm vào chồng, không đá cái đang hiện ra.

### Ba đường, một luật

- `Workspace::open_panel` và `focus_or_unfocus_panel` → `show_panel` thay cho `activate_panel`
- `Workspace::close_panel` → `hide_panel_by_id`, tức **ẩn riêng panel đó**; dock chỉ đóng khi đó là cái cuối
- Rail: `is_active` đọc `is_panel_visible(panel_id)` — **nhiều nút cùng sáng là đúng**

### Không đụng `close_panel_on_toggle`

Setting này mặc định `false` ("toggle chỉ trả focus về editor, không đóng panel") và là lựa chọn có chủ ý của người dùng. Ngữ nghĩa phím tắt **giữ nguyên**; chỉ đổi *nghĩa của "close"* — trước là đóng cả dock, giờ là ẩn một panel. Ít bất ngờ hơn hẳn.

Rail thì khác: nó vốn đã ghi "clicking the panel already on screen closes the dock, the way VS Code does", tức rail luôn là nút ẩn. Nay ẩn **riêng panel đó**, gọi thẳng `dock.hide_panel_by_id` — không có action nào đặt tên được cho một panel cụ thể, và closure không giữ borrow nào của `Sidebar` nên không tái nhập.

### `activate_panel` vẫn độc quyền

Cố ý. Nó là thứ caller gọi khi muốn hiện **một panel thay cho** những cái khác (ví dụ `restore_state`). Có assertion riêng trong test giữ tính chất đó — chồng không được lặng lẽ biến nó thành một cách thêm panel nữa.

### Test cũ: sửa đúng một cái, và đó là của tôi

**212 test workspace có sẵn xanh nguyên, không sửa dòng nào.** Cái duy nhất đỏ là `a_dock_shows_no_more_than_one_panel` — test bất biến tôi viết ở phase 01. Bất biến đó là **giàn giáo của phase 01**, và phase này tồn tại để dỡ nó. Đã viết lại thành `a_dock_stacks_panels_instead_of_swapping_them` (giữ nguyên phần rà mọi đường công khai + dọn khi `remove_panel`), chứ không xoá.

Thêm `the_rail_lights_every_panel_that_is_showing` bên `sidebar`. Kiểm chứng ngược: trả `is_panel_visible` về cách đọc theo `active_panel_index` cũ → đỏ với `left: 1, right: 2`.

### Gates

213 workspace · 18 sidebar · 43 agent_ui · 97 project_panel · 86 git_ui · 58 terminal_view · 12 outline_panel — tất cả xanh. clippy sạch, `cargo check --workspace --all-targets` sạch, build ok.
