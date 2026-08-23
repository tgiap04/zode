# Phase 03 — Panel phía sau cú click

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** ảnh #3

## Việc

Click hiện tại gọi `start_polling` ngay. Đổi thành: click **mở panel**, và ⟳ trong panel
là chỗ refresh. Panel là một view riêng, `PopoverMenu` neo lên trên — đúng khuôn
`lsp_button.rs` đã dùng (`PopoverMenuHandle` + `register_action` để có keybinding).

```
┌─ Usage ────────── all agents  ⟳ ─┐
│ ┌─Detailed─┐ ┌ Compact ┐        │   ← ToggleButtonGroup, ghi setting
├──────────────────────────────────┤
│ ✳ Claude   Resets in 2h 58m    >│
│   5h ▬▭ 11%  wk ▬▭ 11%  Fable 0%│   ← short_tag() từ phase 01
│ ◎ Codex    Refresh failed      >│   ← source.reason khi không có số
└──────────────────────────────────┘
```

Ba chi tiết đáng nói:

- **"all agents"** trong ảnh là nhãn phạm vi, không phải dropdown. Ở đây nó nói thật: cả
  hai agent đều đang được đọc. Nếu một agent bị tắt qua setting thì nhãn phải nói đúng
  cái đang xem, chứ không nói "all" khi đang xem một.
- **Dòng thứ hai của mỗi agent là nơi `short_tag()` được dùng.** Không có phase 01 thì
  chỗ này chỉ còn cách đánh số theo vị trí.
- **Agent im lặng vẫn có dòng riêng** với `source.reason`. Đó là chỗ khác hẳn thanh
  status: thanh status ẩn cái không có gì để nói, panel thì phải *nói tại sao* — người
  dùng mở panel chính vì con số không hiện.

## Related code files

- `crates/agent_usage/src/usage_panel.rs` (mới) — `UsagePanel`, `impl ManagedView`
- `crates/agent_usage/src/agent_usage.rs` — `_popover: PopoverMenuHandle<UsagePanel>`, render đổi sang `PopoverMenu`, bỏ `on_mouse_down` cũ
- `crates/zed/src/zed.rs` — `register_action` cho `ToggleUsagePanel` (khuôn `lsp_button::ToggleMenu`)

## Todo

- [x] `UsagePanel` + `EventEmitter<DismissEvent>` + `impl ManagedView`
- [x] Header: nhãn "Usage", nhãn phạm vi, nút ⟳ (gọi lại `refresh` của indicator)
- [x] `ToggleButtonGroup` Detailed/Compact → `update_settings_file` ghi `agent_usage_display`
- [x] Dòng agent: icon + tên + trạng thái (đếm ngược gần nhất, hoặc `reason`)
- [x] Dòng chi tiết: `short_tag()` + progress bar `w()` tường minh + `%`
- [x] Indicator: `PopoverMenu` thay `on_mouse_down`
- ~~⟳ trên thanh status vẫn refresh trực tiếp~~ — **không làm.** ⟳ giờ nằm trong trigger,
  nên bấm nó mở panel. Tách nó thành nút riêng là phá đúng cái "cả nhóm là một control"
  mà ảnh mẫu thể hiện; refresh nằm ở ⟳ trong panel.
- [x] Action `ToggleUsagePanel` + `register_action`
- [x] Test: **luật render** của panel (`source_status`, `scope_label`) trên state 2 cửa sổ
      + agent im lặng — không phải test dựng panel entity (`cx.draw` không publish frame,
      nên test đó sẽ không chứng minh gì)
- [x] Test: agent im lặng vẫn có dòng, và dòng đó mang `reason`
- [x] Test: nhãn phạm vi nói "all agents" chỉ khi cả hai đang bật
- [x] `cargo check -p agent_usage -p zode`

## Bẫy

- **Panel đọc state của indicator, không giữ bản copy.** Hai bản state là hai câu trả lời
  khác nhau sau một lần refresh. Panel giữ `WeakEntity<AgentUsageIndicator>` và đọc qua
  đó.
- **`cx.draw` không publish frame** — đừng assert bounds. Assert entity và chuỗi đã render.
- **`flex_1`/`self_stretch` dưới `div()` trơn vẽ ra 0.** Progress bar dùng `w(px(…))`.
- **`update_settings_file` cần `<dyn fs::Fs>::global(cx)`**, không có trong test → nhánh
  toggle chỉ chạy khi bấm. Test bấm toggle thì phải dựng fake fs, hoặc tách hàm thuần
  quyết định *giá trị mới* ra khỏi lời gọi ghi và test hàm đó.

## Success criteria

Click vào usage → panel mở đúng bố cục ảnh #3 (không có hai dòng footer). Bấm Compact →
thanh status rút lại ngay và panel đóng lại hay không cũng nhất quán. Bấm ⟳ → số cập
nhật. Codex chưa login → dòng Codex nói lý do, không mất dòng.
