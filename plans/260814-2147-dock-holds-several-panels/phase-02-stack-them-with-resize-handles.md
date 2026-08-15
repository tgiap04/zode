# Phase 02 — Xếp chồng và kéo được ranh giới

**Context:** [plan.md](plan.md) · [phase-01](phase-01-a-dock-holds-a-set-of-visible-panels.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-14) · **Blocked by:** 01

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

---

## Xong (2026-08-14)

`pane_axis` dùng lại được thật — đổi `pub(super)` → `pub(crate)` (và `mod element` theo cùng) là xong, **không viết một dòng layout co giãn nào**. Tay kéo, chia đều khi double-click, `min_size`, tự `serialize_workspace` khi kéo: có sẵn hết.

### Một cái bẫy trong chính `pane_axis`

`is_leaf_pane_mask` mặc định là **`true`** (`self.is_leaf_pane_mask.get(ix).copied().unwrap_or(true)`). Để nguyên thì panel nào không được focus sẽ bị **làm mờ** theo `active_pane_modifiers.inactive_opacity`, và panel đang focus mọc thêm viền — hai thứ nói về *editor pane*, không phải dock panel. Đã truyền `vec![false; n]` để tắt cả hai.

### Hai trục, giữ tách bạch

Thêm `stack_flexes` + `stack_bounding_boxes` vào `Dock`. Không đụng `PanelSizeState` (trục bề ngang dock) — comment tại field nói rõ ghi nhầm sang đường kia là *resize cả dock* chứ không phải chia chồng.

`reset_stack_flexes()` gọi từ **mọi** chỗ đổi tập đang hiện (`activate_panel`, `show_panel`, `hide_panel_by_id`, `remove_panel`) — `pane_axis` có `debug_assert!(flexes.len() == children.len())`, lệch là panic lúc vẽ.

### `show_panel` / `hide_panel_by_id`

Giờ mới thêm (phase 01 cố tình hoãn — YAGNI). `hide` khoá theo **`EntityId` chứ không phải index**: index dịch chuyển dưới `add_panel`/`remove_panel`, mà nút đóng được bấm trên một header vẽ từ vài frame trước.

### Đường một-panel không trả phí

`render_showing` tách hai nhánh: một panel → **đúng element cũ**, không header, không `pane_axis`. 213 test `workspace` xanh trong đó 212 là test cũ không sửa dòng nào — đó là bằng chứng nhánh cũ không đổi.

### Test

`two_panels_in_one_dock_split_it_between_them` **đo bounds thật** (`debug_selector` theo vị trí trong chồng): cả hai có diện tích, cùng bề ngang, xếp trên–dưới không chồng lấn; ẩn một cái thì cái đó thôi được vẽ.

Kiểm chứng ngược: cho stack dùng **sai trục** (`self.position.axis()` thay vì trục vuông góc) → đỏ với `150px × 1073px` cạnh nhau thay vì xếp chồng. Đúng loại lỗi mà smoke test "có vẽ, không panic" sẽ cho qua.

### Còn nợ của phase này

- Kéo lệch tỉ lệ rồi ẩn/hiện lại thì tỉ lệ **reset về đều**. `PaneAxis::insert_pane` cũng làm đúng vậy khi split, nên nhất quán với codebase — nhưng phase 04 lưu tỉ lệ thì nên xét giữ luôn.
- Chưa kiểm 13" với chồng 3 panel (R4).
