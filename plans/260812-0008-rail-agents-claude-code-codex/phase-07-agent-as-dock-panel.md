# Phase 07 — Agent thành dock panel, không còn là tab của editor

**Context:** [plan.md](plan.md) · [phase-06](phase-06-persistence-and-polish.md)
**Priority:** P1 · **Status:** completed · **Blocked by:** —

Người dùng chỉ ra: agent đang là **một tab của code editor**, trong khi nó phải là một section riêng mà editor không đặt tab vào được.

## Chỗ rò, đo được

`AgentView` mở bằng `split_item` nên nó là một `Item` trong một `Pane` của center pane group. `Workspace::open_path` nhận `pane: Option<...>` rồi rơi về `pane.unwrap_or_else(|| active_pane)` (`workspace.rs:4589`). Nghĩa là **mọi** đường mở file — click ở project panel, cmd-P, go-to-definition — đều đổ tab editor vào pane agent bất cứ khi nào nó đang active.

Ẩn tab bar / chặn kéo-thả (`can_drop_predicate`, `set_should_display_tab_bar`) **không** vá được: chỗ rò nằm ở `active_pane`, không ở tab bar.

## Quyết định

**Dock panel.** Đảo lại quyết định #6 (center pane) và #7 (agent thứ hai = pane thứ hai) của brainstorm. Ba lựa chọn đã đặt lên bàn kèm hình (dock / khoá pane center / section tự viết); người dùng chọn dock.

Dock không nằm trong `workspace.panes`, nên `open_path` không bao giờ trỏ tới được. Đây là tính chất của kiến trúc chứ không phải một lớp chặn phải bảo trì.

## Được miễn phí — và vì thế xoá được code

| Việc | Trước | Sau |
|---|---|---|
| Seam với editor | `border_l_1/r_1` tôi tự vẽ | Dock tự vẽ (`dock.rs:1153`) + resize handle |
| Nhớ width | canvas đo + `resize_pane` + cột `width` | Dock tự nhớ size |
| Khôi phục tab | `SerializableItem` + bảng `agent_views` | Dock tự serialize trạng thái |
| Vị trí trái/phải | `agent_split_direction` | `Panel::position` |

Toàn bộ máy đo width — thứ đã gây deadlock cả app hôm nay — biến mất cùng lý do tồn tại của nó.

## Ràng buộc phải giữ

1. **Không sinh nút chung trên rail.** Rail đã có hai nút Claude/Codex riêng. `Panel::icon() -> None` làm cả `rail_panels.rs:78` lẫn `dock.rs:1272` bỏ qua panel ⇒ **không sửa một dòng nào trong `crates/workspace`**.
2. **Lazy start.** Panel được dựng lúc workspace load nhưng **không** khởi động agent. Chỉ start khi thật sự được mở lần đầu — nếu không thì mọi phiên khởi động đều spawn npx cho người chưa từng dùng agent.
3. **Một panel giữ một agent.** Đổi sang agent kia = kết thúc cái cũ, đúng kỷ luật teardown đã dựng ở phase trước (`end_previous_mode` + tripwire).
4. Mode vẫn nhớ theo từng agent (`agent_preferences.mode`). Cột `width` thành vô dụng — ngừng ghi, không xoá (migration chỉ được thêm, không được sửa).

## Việc

- [ ] `impl Panel for AgentView` + `load()` + `set_active` lazy start
- [ ] Gỡ `impl Item`, `impl SerializableItem`, `mod persistence` phần `agent_views`
- [ ] Gỡ máy đo width (canvas, `pending_width`, `recorded_width`, `resize_pane`) và border tự vẽ
- [ ] `AgentView::open` → focus panel + đặt agent, thay cho `split_item`
- [ ] Đăng ký panel trong `zed.rs` cạnh 5 panel còn lại
- [ ] Sửa test theo hình mới

## Định nghĩa xong

Mở agent → nó là một section riêng cạnh editor. Click file trong project panel lúc agent đang focus → file mở ở **center**, không chen vào agent. Kéo tab editor sang vùng agent → không thả được. Đóng/mở lại app → dock trở lại đúng bên, đúng width.

---

## Xong (2026-08-13)

`AgentView` giờ là `Panel` trong dock, đăng ký ở `zed.rs` cạnh 5 panel còn lại.

| Yêu cầu | Trạng thái |
|---|---|
| Editor không đặt tab vào được | ✅ dock không nằm trong `workspace.panes`, `open_path` không có đường tới |
| Không sinh nút chung trên rail | ✅ `icon() -> None`; **không sửa dòng nào trong `crates/workspace`** |
| Lazy start | ✅ kiểm runtime: mở workspace 45s, **không** tiến trình npm/adapter nào |
| Đổi agent = kết thúc cái cũ | ✅ `show()` đi qua `restart` → `end_previous_mode` + tripwire đã có |
| Mode nhớ theo agent | ✅ `agent_preferences.mode` |

### Xoá được bao nhiêu

Chuyển sang dock làm **biến mất lý do tồn tại** của một loạt code viết hôm qua:

- máy đo width (canvas + `resize_pane` + debounce + cột `width`) — 67 dòng, chính là chỗ đã deadlock cả app
- `impl Item` (20 dòng) và `impl SerializableItem` (77 dòng)
- `save_agent` / `get_agent` / `delete_unloaded` / `save_width` (75 dòng)
- border tự vẽ ở cạnh giáp editor — dock tự vẽ (`dock.rs:1153`) kèm resize handle

Tổng cộng ~240 dòng bị gỡ, không phải thêm. Hai vòng vá seam và một deadlock toàn app đều là hệ quả của việc chọn sai chỗ đặt agent ngay từ đầu; đặt đúng chỗ thì cả ba biến mất cùng lúc.

### Test đã đổi theo

- `the_agent_pane_opens_on_the_rail_side` → `the_panel_docks_on_the_side_the_setting_names` (`Panel::position` thay `SplitDirection`).
- `the_view_draws_inside_a_pane_without_deadlocking` — **xoá**. Nó canh chừng việc render hỏi workspace về pane của mình; giờ view không ở trong pane nào và render không hỏi workspace điều gì, nên cái bẫy đó không còn phát biểu được.
- `writing_one_preference_leaves_the_other_alone` → `each_agent_remembers_its_own_mode`. Bất biến cũ ("hai cử chỉ ghi hai cột") mất chủ thể khi width về tay dock.

33 test `agent_ui` xanh · clippy sạch · `cargo check --workspace --all-targets` sạch.

### Còn nợ

Log có `cannot deserialize AgentView, descriptor not found` cho các tab agent **cũ** còn trong `items` của workspace. Workspace bỏ qua item đó êm, nhưng dòng log sẽ lặp lại mỗi lần mở những workspace ấy cho tới khi xoá hàng cũ.
