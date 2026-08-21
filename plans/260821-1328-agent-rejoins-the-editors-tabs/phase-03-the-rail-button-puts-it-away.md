# Phase 03 — Nút rail cất agent đi, không đóng nó

**Status:** ✅ done (2026-08-21) · **Priority:** P1 · **Người dùng thấy:** bấm lần nữa về lại tab code

## Mục tiêu

`ToggleAgent` giữ được nghĩa "toggle" khi không còn dock nào để `set_open(false)`.
Bấm lần nữa → về item vừa xem trong cùng pane. Cất đi, không đóng: không mất thread, không
mất scroll, và không phải khởi động lại một agent process.

## Việc

`agent_ui::init`, handler `ToggleAgent`:

1. Item đang hiện hành của `workspace.active_pane()` có phải `AgentView` của agent này?
2. Đúng → lấy item **trước đó** của pane đó từ `Pane::activation_history()` (public,
   `pane.rs:836`), activate nó.
3. Sai → đường mở như phase 02 (`AgentView::open`).

Bỏ nhánh `agent_dock().set_open(false)` và hàm `agent_is_showing` (nó hỏi
`agent_dock().is_open()`, một câu không còn nghĩa).

## Chỗ phải cẩn thận

- **Pane chỉ có mình tab agent** → `activation_history` không còn gì để về. Đứng yên, đừng
  đóng. Plan cũ ghi thẳng: *"a lit toggle that does nothing when pressed is the whole
  complaint"* — nhưng đóng một session vì người dùng bấm hai lần thì tệ hơn nhiều. Trường
  hợp này phải có test riêng.
- **`activation_history` giữ item đã đóng?** Kiểm tra: nếu entry trỏ tới item không còn
  trong pane thì lùi tiếp, đừng activate một index đã chết.
- **Không phải `workspace.active_pane()` mà là pane đang giữ agent** trong trường hợp người
  dùng đã split và agent ở pane khác. Quyết: chỉ toggle khi agent là item hiện hành của pane
  **đang active** — bằng không thì đó là "đưa tôi tới agent", tức đường mở.

## Files

- sửa: `crates/agent_ui/src/agent_ui.rs`

## Todo

- [x] `ToggleAgent`: nhánh về-item-trước qua `activation_history`
- [x] Bỏ `agent_is_showing` và nhánh `set_open(false)`
- [x] Test: mở file → mở agent → toggle → về file; toggle nữa → về agent
- [x] Test: pane chỉ có agent → toggle → agent vẫn còn, vẫn hiện hành
- [x] Test: agent ở pane không active → toggle đưa tới agent, không cất đi
- [x] `cargo check -p agent_ui` xanh

## Success criteria

Nút rail bấm được đi bấm được lại, không lần nào làm chết session hay để lại pane rỗng.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Activate một item đã đóng | Lùi qua các entry chết trong `activation_history` |
| Toggle đóng mất session | Test riêng cho pane chỉ có agent |
