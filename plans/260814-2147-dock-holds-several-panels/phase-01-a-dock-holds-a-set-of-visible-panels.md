# Phase 01 — Dock giữ một *tập* panel đang hiện

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** planned · **Blocked by:** —

Đổi mô hình dữ liệu, **không đổi hành vi**. Kết thúc phase này người dùng không thấy khác gì cả — và đó là mục đích.

## Vì sao tách riêng

`active_panel_index: Option<usize>` bị đọc ở 16 chỗ trong `dock.rs`, cộng `rail_panels.rs` và `git_ui/commit_modal.rs`. Trộn việc đổi mô hình với việc đổi layout thì lúc có bug không biết nửa nào gây ra. Giữ bất biến "tập luôn có đúng một phần tử" cho phép toàn bộ test hiện có đứng nguyên làm lưới an toàn: **bất kỳ test nào đỏ ở phase này đều là hồi quy thật**, không phải "đã đổi hành vi rồi mà".

## Việc

- [ ] `Dock::active_panel_index: Option<usize>` → `visible: BTreeSet<usize>` (thứ tự ổn định để render và serialize không nhảy)
- [ ] Giữ `active_panel_index()` như một hàm đọc dẫn xuất — panel nào **được focus gần nhất** trong `visible`. Đây là thứ `commit_modal.rs` và rail đang thật sự cần, nên chúng không phải đổi ở phase này.
- [ ] `visible_entry()` → `visible_entries()`; `visible_panel()` giữ nguyên chữ ký, trả về cái được focus gần nhất (17 chỗ gọi trong `workspace.rs` không phải đổi)
- [ ] `activate_panel(ix)` giữ ngữ nghĩa cũ: đặt `visible = {ix}`
- [ ] Thêm `show_panel(ix)` / `hide_panel(ix)` — chưa ai gọi ở phase này
- [ ] `set_open(false)` xoá rỗng `visible`; `set_open(true)` khôi phục tập trước đó

## Ngữ nghĩa `Panel::set_active` — quyết trước khi code

Hiện `set_active(true)` nghĩa là "anh là cái đang hiện". Với N panel cùng hiện, nó phải nghĩa là **"anh đang hiện"** (không còn hàm ý độc quyền).

Chỗ này không trừu tượng: `AgentPanel::set_active` đang móc `close_if_empty` vào đó để giữ luật "panel rỗng thì tự đóng" (vừa ship ở `78a4c08`). `OutlinePanel::set_active` cũng lưu cờ `active` của nó.

- [ ] Rà **mọi** `impl Panel::set_active` (agent, outline, project, terminal, debugger, git) và ghi lại mỗi cái đang hiểu nó là gì
- [ ] Test giữ luật cũ: agent rỗng vẫn tự đóng khi được hiện

## Test

- [ ] Toàn bộ test `workspace` hiện có xanh, **không sửa test nào** — sửa một test ở phase này là dấu hiệu đã lỡ đổi hành vi
- [ ] Bất biến: sau mọi thao tác dock công khai, `visible.len() <= 1`
- [ ] `agent_ui` + `sidebar` xanh (chúng đọc dock qua rail)

## Định nghĩa xong

`cargo test -p workspace -p agent_ui -p sidebar` xanh với **không một dòng test nào bị sửa**, và `visible` là nguồn sự thật duy nhất — không còn `active_panel_index` dạng field.

## Rủi ro

`set_open(true)` khôi phục "tập trước đó" cần chỗ nhớ tập đó; làm ẩu thì dock mở lại rỗng và không panel nào hiện — mở ra một dải trống, đúng lỗi vừa sửa ở `78a4c08`. Giữ tập cuối cùng khác rỗng trong một field riêng.
