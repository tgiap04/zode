# Phase 04 — Menu `+` của editor mở được một agent

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** mở session thứ hai từ tab bar editor

## Mục tiêu

Giữ lại đường "mở session thứ hai của một agent đang chạy" trước khi phase 05 xoá chỗ nó
đang nằm. Đứng **trước** 05 có chủ đích: không được có cửa sổ nào mà tính năng này không còn
đường nào tới.

## Bối cảnh

`AgentPanel::apply_tab_bar_buttons` (`agent_panel.rs:451`) thay hẳn menu `+` mặc định, và
comment nói rõ tại sao:

> The default offers New File, Open File, … every one of which opens something into the
> **centre** group, not into this column, so the menu named actions that could not land
> where it was drawn.

Agent về center thì lý do đó tự tan: menu mặc định lại đúng, và chỉ cần **thêm** vào nó.
`agent_panel.rs:458` cũng ghi đây là *"the only place a second session of an agent already
running can be started"* — nên nó phải có chỗ mới trước khi mất chỗ cũ.

## Việc

Thêm vào menu `+` của pane trong center: một mục `New {display_name}` cho mỗi
`project::BUILTIN_AGENTS`, dispatch `zed_actions::agent::NewAgent { agent, mode: None }`.
Dựng từ `BUILTIN_AGENTS` chứ không viết tên Claude/Codex ra — agent thứ ba xuất hiện ở đây
ngay ngày nó được thêm, đúng như bản cũ đã làm.

Tìm chỗ dựng menu `+` mặc định của pane trong `crates/workspace/src/pane.rs` (hoặc chỗ
`zed.rs` cấu hình nó) và nối vào đó.

## Chỗ phải cẩn thận

- **`crates/workspace` không được depend `agent_ui`.** Nếu menu mặc định dựng trong
  `workspace`, thì mục agent phải được **tiêm vào** từ ngoài (như `zed.rs` vẫn làm với
  toolbar item), không phải hard-code trong `workspace`. Xác định điểm tiêm trước khi sửa.
- `NewAgent` đã được register ở `agent_ui::init` rồi — không cần action mới.
- Không thêm "Split" hay "Zoom" gì cả: pane của center đã có sẵn của nó.

## Files

- sửa: `crates/workspace/src/pane.rs` **hoặc** `crates/zed/src/zed.rs` (tuỳ điểm tiêm)

## Todo

- [x] Xác định điểm tiêm mà không tạo dependency `workspace → agent_ui`
- [x] Mục `New {agent}` dựng từ `BUILTIN_AGENTS`
- [x] Test: mỗi mục dispatch `NewAgent` với id một builtin agent thật
  — `every_agent_the_new_menu_offers_can_actually_be_opened`: lặp qua `BUILTIN_AGENTS`,
  dispatch action đúng như menu dispatch, assert đúng tab đó mở ra
- [x] Test: chọn mục → có thêm một tab agent, cái cũ vẫn còn
  — `a_deliberate_new_session_stands_beside_the_first`

**Còn một mảnh nhỏ chưa kiểm, ghi rõ:** *nhãn* của mục menu. Nó nằm trong closure
`PopoverMenu::menu` chỉ chạy khi popover mở, và `ContextMenu.items` là private trong
`crates/ui` — nên chữ trên mục không với tới được từ test. Cái **đã** kiểm là thứ thật sự
vỡ được: mỗi id agent menu chào mời đều mở đúng tab của nó.
- [x] `cargo check` xanh

## Success criteria

Menu `+` ở thanh tab editor có "New Claude Code" / "New Codex", và chọn nó mở một session
thứ hai thành tab mới — cái đang chạy không bị đụng tới.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Tạo dependency vòng `workspace → agent_ui` | Tiêm từ `zed.rs`, theo đúng lối toolbar item đã có |
| Mục menu trỏ agent không tồn tại | Test dựng từ `BUILTIN_AGENTS` + assert `builtin_agent(id).is_some()` |
