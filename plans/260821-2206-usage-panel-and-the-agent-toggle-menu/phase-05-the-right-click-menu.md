# Phase 05 — Menu chuột phải

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** ảnh #4

## Việc

`ui::right_click_menu` đã có sẵn trong repo — không dựng cơ chế mới. Menu là bảy dòng
tick, mỗi dòng ghi thẳng một field của `status_bar`:

```
✓ ✳ Claude Usage          → claude_usage_button
✓ ◎ Codex Usage           → codex_usage_button
───────────────────────
✓ 📄 Active File Name      → show_active_file
✓ 🔤 Active Language       → active_language_button
✓ ⌖  Cursor Position       → cursor_position_button
  ↵  Line Endings          → line_endings_button
✓ ⎘  Active Encoding       → active_encoding_button
```

Năm dòng dưới là **item của crate khác** mà `agent_usage` bật/tắt. Điều đó ổn: nó ghi
setting, không với vào entity của ai. `active_encoding_button` là enum ba trạng thái, nên
tick của nó map `Enabled`/`Disabled` và **giữ nguyên `NonUtf8`** nếu đang là thế — tick
một enum ba trạng thái thành bool là làm mất một trạng thái người dùng đã chọn.

## Related code files

- `crates/agent_usage/src/agent_usage.rs` — `right_click_menu` bọc phần usage; builder menu
- `crates/agent_usage/src/status_bar_items.rs` (mới, nếu builder vượt ~60 dòng) — bảng field ↔ nhãn ↔ getter/setter

## Todo

- [x] `right_click_menu` bọc nhóm usage, `ContextMenu` với **`ContextMenuEntry`** —
      `toggleable_entry` nhận tick *hoặc* icon, menu này cần cả hai
- [x] 2 dòng agent → ghi `claude_usage_button` / `codex_usage_button`
- [x] Separator + 5 dòng status item sẵn có
- [x] `active_encoding_button`: bật → `Enabled`, tắt → `Disabled`
- ~~không phá `NonUtf8`~~ — **không đạt được, và đó là kết luận chứ không phải sót.**
  Checkbox có 2 trạng thái, setting này có 3. Tắt rồi bật lại rơi về `Enabled`, không
  về `NonUtf8`. `NonUtf8` vẫn *được đọc là đang bật* (tick đúng), chỉ không trở lại được
  qua tick. Có test khẳng định đúng chỗ mất đó (`the_encoding_round_trip_does_not_restore_the_conditional_state`).
- [x] Tick phản ánh giá trị đang hiệu lực, không phải giá trị mặc định
- [x] Test: hàm thuần "giá trị mới cho field X khi tick" — cả 7 dòng
- [x] Test: `NonUtf8` được coi là đang-bật, và tắt rồi bật lại không rơi về `NonUtf8` một cách lặng lẽ
- [x] `cargo check -p agent_usage -p zode`

## Bẫy

- **Tắt cả hai agent → không còn gì để chuột phải vào.** Không sửa ở phase này (sửa đúng
  chỗ là làm cả thanh status nhận chuột phải, tức đổi crate `workspace` và tạo phụ thuộc
  vòng). Ghi vào docs ở phase 06 là bắt buộc, không phải tuỳ.
- **Enum ba trạng thái.** Xem trên.
- **`update_settings_file` cần fs global** — tách hàm thuần quyết định giá trị mới ra để
  test được mà không cần fs.

## Success criteria

Chuột phải vào usage → menu 7 dòng, tick khớp settings đang dùng. Bỏ tick Codex → phần
Codex biến khỏi thanh status ngay, và `settings.json` của người dùng có thêm đúng một
dòng. Bỏ tick Line Endings rồi tick lại → không mất trạng thái nào khác.
