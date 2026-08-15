# Phase 01 — Dock giữ một *tập* panel đang hiện

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-14) · **Blocked by:** —

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

---

## Xong (2026-08-14)

### Lệch khỏi plan, có lý do

Plan viết `visible: BTreeSet<usize>`. **Không dùng**, vì hai lẽ tìm ra lúc đọc code:

1. `add_panel`/`remove_panel` **dịch chuyển index** — đã có sẵn logic `+= 1` / `-= 1` cho *một* index. Giữ cả một tập index đang dịch chuyển là nhân đúng loại bug đó lên.
2. `BTreeSet` sắp theo index, nên `active_panel_index()` suy ra từ nó sẽ là "index nhỏ nhất", **không phải** "được focus gần nhất" như plan hứa. Plan tự mâu thuẫn.

Thay bằng **cờ `visible: bool` trên từng `PanelEntry`**. Cờ đi theo entry của nó khi `Vec` dịch chuyển, không thể lệch. `active_panel_index` giữ nguyên là field riêng — nó trả lời câu hỏi **khác** ("cái nào đang focus"), giống `PaneGroup` có N pane và một `active_pane`. Hai field, hai khái niệm, không phải hai nguồn sự thật.

Cờ **có tải trọng ngay**, không phải code chết: `visible_entry()` (đường render) giờ đọc từ cờ chứ không từ index.

### Bỏ khỏi plan

`show_panel` / `hide_panel` — plan ghi "chưa ai gọi ở phase này". Đó là định nghĩa của speculative. Thêm khi phase 02 cần. Có thêm `visible_panels()` công khai vì nó có người dùng thật ngay (test bất biến) và phase 03 cần đúng nó.

### Rà `Panel::set_active` — kết quả

| panel | `set_active(true)` làm gì |
|---|---|
| agent | đóng dock nếu không giữ agent nào (`78a4c08`) |
| terminal | **tạo** một terminal nếu chưa có cái nào |
| outline | lưu cờ `active`, refresh |
| project / debugger / git | không override |

Hai panel móc "tôi vừa hiện ra" để phản ứng với việc rỗng. Cả hai **vẫn đúng khi có chồng**: "tôi vừa hiện ra" vẫn đúng nghĩa khi một panel *gia nhập* chồng. Không phải sửa gì, nhưng phải biết trước khi phase 02 đổi layout.

### Test

`a_dock_shows_no_more_than_one_panel` chạy qua **mọi** đường công khai làm dock đổi trạng thái (mở, focus panel 1, activate panel 2, toggle về centre, đóng, mở lại, rồi gỡ cả hai panel), khẳng định sau mỗi bước có tối đa một panel hiện.

Kiểm chứng ngược: đổi `activate_panel` để nó không cất panel kia đi → đỏ ngay ở bước "focusing the second panel left 2 panels showing at once".

### Định nghĩa xong — đạt

**212 test `workspace` xanh, không sửa một dòng test nào** (211 cũ + 1 mới). `agent_ui` 43, `sidebar` 17, `terminal_view` 58 — bốn crate đọc dock đều xanh. clippy sạch, `cargo check --workspace --all-targets` sạch, build ok. fmt: hai file lệch sẵn (`status_bar.rs`, `welcome.rs`) không phải file tôi chạm.
