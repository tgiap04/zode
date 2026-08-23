# Phase 08 — Hai dòng, một dấu tick thật, và panel biết đóng

**Status:** 🔨 fixed, chờ docs + commit (2026-08-21) · **Priority:** P1

## Người dùng yêu cầu hai thứ, và ảnh lộ ra thứ thứ ba

Yêu cầu: (1) menu chỉ còn Claude với Codex, (2) click ra ngoài phải đóng dropdown.

Ảnh còn cho thấy điều tôi chưa nói: **mỗi dòng hiện icon hai lần** (`✳ Claude Usage ✳`)
và **không dòng nào có dấu tick**.

## Icon hai lần: lý lẽ phase 05 của tôi sai từ gốc

Phase 05 tôi viết: *"`ContextMenuEntry` thay vì `toggleable_entry`, vì cái sau nhận tick
**hoặc** icon, menu này cần cả hai."* Nghe hợp lý. Sai.

`ContextMenu` vẽ dòng đang bật bằng:

```rust
Icon::new(icon.unwrap_or(IconName::Check))
```

**Slot tick *chính là* slot icon.** Truyền icon vào là **ghi đè dấu tick** bằng glyph đó.
Cộng với `icon_position(End)` của tôi, mỗi dòng bật hiện glyph hai lần, mỗi dòng tắt hiện
một lần — và không có checkmark nào. Tức trạng thái bật/tắt vẫn *được* biểu diễn, chỉ bằng
một ký hiệu không ai đọc được là tick.

Nên `ContextMenu` của repo này **về cấu trúc không thể** có cả icon lẫn tick. Đây không
phải giới hạn tôi gặp phải — đây là điều tôi lẽ ra phải đọc code để biết trước khi viết
một câu bình luận khẳng định điều ngược lại.

Chọn: **tick, bỏ icon.** Menu mà việc duy nhất của nó là nói cái gì đang bật thì không được
tiêu slot tick vào trang trí.

## Hai dòng: người dùng đảo lại lựa chọn cũ, và họ đúng

Ở AskUserQuestion đầu tiên họ chọn *"usage từng agent + 5 item status bar sẵn có"*. Xem
thật rồi họ đổi ý. Lý do đứng vững: **chuột phải vào con số usage để tắt cursor position
là một menu về chuyện khác.** Năm item đó đã có nhà riêng trong Settings Editor và settings
file — và chính tôi vừa thêm 3 setting mới vào `settings_ui/page_data.rs` ở phase 06, nên
đường đó là thật.

Xoá theo: `encoding_for_toggle` và trường `icon`/`starts_group` của bảng, cùng 2 test về
enum ba trạng thái — cả cụm đó giờ mô tả thứ không tồn tại.

## Click ra ngoài không đóng: `PopoverMenu` không làm việc đó

`PopoverMenu` chỉ đăng ký **một** mouse handler, và nó chỉ phủ **chính trigger**:

```rust
// Mouse-downing outside the menu dismisses it, so we don't
// want a click on the toggle to re-open it.
if phase == DispatchPhase::Bubble && child_hitbox.is_hovered(window) { ...dismiss... }
```

Comment đó **giả định** có cái khác lo click ra ngoài. Với `ContextMenu`, cái đó là
`cx.on_blur(&focus_handle, …)` dựng ngay trong constructor của nó. `UsagePanel` có
`FocusHandle` nhưng **không có subscription nào** — nên không gì đóng nó.

Sửa: `_blur: Subscription` emit `DismissEvent`, cộng `.track_focus(&self.focus_handle)` để
focus có chỗ đậu — `PopoverMenu` *có* focus view này ở frame sau khi mở, nên blur sẽ chạy.

## Todo

- [x] Bảng còn 2 dòng agent; xoá `icon`, `starts_group`, `encoding_for_toggle`
- [x] `build_toggle_menu` dùng `toggleable_entry` → checkmark thật
- [x] `_blur` + `track_focus` trên panel
- [x] Test: menu là **đúng** `["Claude Usage", "Codex Usage"]`
- [x] Test: menu không ghi setting của item thuộc crate khác
- [x] 66 xanh · clippy 0 warning · `cargo build --bin zode` exit 0
- [ ] `doc-writer` — docs đang mô tả menu 8 dòng và caveat encoding, cả hai đã sai
- [ ] commit

## Bài học

**Một câu comment khẳng định hành vi của component khác là một claim, không phải một ghi
chú.** Tôi viết "`toggleable_entry` nhận tick hoặc icon nên tôi cần `ContextMenuEntry`" mà
không đọc chỗ nó vẽ. Bốn dòng code đọc là xong, và nó nói ngược lại.

Và lần thứ hai trong hai phase liền, lỗi nằm ở **thứ tôi cho rằng framework tự làm**: nền
panel (phase 07) và dismiss khi click ra ngoài (phase 08). Cả hai đều là việc mà
`ContextMenu` tự làm cho mình, nên mọi thứ *dựa trên* `ContextMenu` đều đúng — và một view
tự viết thì không. Chép khung từ `lsp_button.rs` mang theo giả định đó ba lần.
