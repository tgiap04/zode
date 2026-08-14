# Phase 03 — Nút rail/status bar thành toggle từng panel

**Context:** [plan.md](plan.md) · [phase-01](phase-01-a-dock-holds-a-set-of-visible-panels.md)
**Priority:** P2 · **Status:** planned · **Blocked by:** 01

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
