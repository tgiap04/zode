# Phase 02 — Xếp chồng và kéo được ranh giới

**Context:** [plan.md](plan.md) · [phase-01](phase-01-a-dock-holds-a-set-of-visible-panels.md)
**Priority:** P2 · **Status:** planned · **Blocked by:** 01

Từ đây mới có thứ nhìn thấy được: hai panel cùng một bên, có tay kéo giữa chúng.

## Đừng viết lại thứ đã có

`pane_group.rs:1135` có sẵn `pane_axis(axis, basis, flexes, bounding_boxes, workspace)`:

- chia N phần tử theo một trục bằng `flexes: Arc<Mutex<Vec<f32>>>`
- vẽ tay kéo, xử `MouseDown`/`MouseMove`, double-click để chia đều
- tự gọi `workspace.serialize_workspace` khi kéo xong
- có `min_size` theo trục (`VERTICAL_MIN_SIZE = 100`)

Đó đúng là bài toán này. Nó đang `pub(super)` trong `mod element`; `dock` và `pane_group` là module anh em trong crate `workspace`, nên **đổi sang `pub(crate)`** là dùng được.

- [ ] Đổi `pub(super) fn pane_axis` → `pub(crate)`, thêm doc nói rõ nó phục vụ cả dock
- [ ] `Dock::render`: khi `visible.len() > 1`, gói các panel vào `pane_axis` theo trục **dọc** với dock trái/phải, trục **ngang** với dock dưới
- [ ] `visible.len() == 1` giữ nguyên đường render cũ — không bắt trường hợp thường gặp nhất trả phí cho trường hợp mới

## Hai trục kích thước — chỗ dễ sai nhất

`PanelSizeState { size, flex }` hiện đo **bề ngang** dock (dock trái/phải), lưu theo từng panel qua `persist_panel_size_state`, và `resize_active_panel` là đường ghi.

Xếp chồng chia **chiều dài** dock — trục thứ hai, chưa từng tồn tại. Hai thứ này không được trộn:

- [ ] Bề ngang dock: vẫn một giá trị cho **cả dock** (kéo mép ngoài) — không đổi
- [ ] Chiều dài: `flexes` cho các panel đang hiện, sống trong `Dock`
- [ ] `resize_active_panel` chỉ chạm trục cũ; thêm đường riêng cho trục mới

## Tiêu đề panel

Xếp chồng thì phải biết đâu là đâu, và phải đóng được từng cái.

- [ ] Mỗi panel trong chồng có một thanh tiêu đề: icon + `persistent_name` + nút đóng
- [ ] `visible.len() == 1` thì **không** vẽ thanh này — nếu không mọi panel đơn lẻ hôm nay bỗng mọc thêm một hàng

## Test

- [ ] Hai panel cùng dock → **đo bounds thật** cả hai: cùng bề ngang, chiều cao cộng lại xấp xỉ chiều cao dock. Smoke test "có vẽ không panic" không chứng minh gì về layout — xem `zode_self_stretch_needs_a_flex_parent` trong memory, và `a_shown_agent_actually_occupies_the_panel` là mẫu.
- [ ] Panel đơn lẻ: bounds không đổi so với trước phase này
- [ ] `min_size`: chồng 4 panel trong dock hẹp không cho cái nào về 0

## Rủi ro

Bề cao 13" (R4, đã ghi nợ từ phase 06 của plan rail-agents) quay lại và lần này gắt hơn: chồng 3 panel trên màn 13" thì mỗi cái còn rất ít. `min_size` phải chặn, và chặn *có thể nhìn thấy được* (panel bị đẩy ra khỏi chồng, chứ không co về 0 rồi biến mất im lặng).
