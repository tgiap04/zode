# Phase 04 — Mũi tên, và cái nó mở ra

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** mũi tên `>` mở ra chi tiết

## Việc

Mỗi dòng agent có `>`. Mở ra chi tiết từng cửa sổ — thứ dòng gọn không nói được:

```
✳ Claude                    >
   └─ Session (5h)   11%  reset 21/08 17:26
      Weekly         11%  reset 27/08 09:00
      Fable (weekly)  0%  không có mốc reset
```

Ba thứ chỉ có ở đây: **`long_name()`** thay cho tag hai ký tự, **mốc reset tuyệt đối**
thay cho đếm ngược (đếm ngược tiện, mốc thật thì kiểm được), và **lý do** khi agent im.

Cửa sổ không có `resets_at` phải nói ra là *không có mốc*, chứ không để trống — trống đọc
như dữ liệu bị mất, mà đây là câu trả lời thật của một cửa sổ scoped theo model.

Mốc tuyệt đối in theo **giờ địa phương**: người đọc đang so nó với đồng hồ trên máy họ.
Tooltip vẫn giữ RFC 3339 UTC như hiện tại — hai chỗ, hai mục đích, không đổi cái đang có.

## Related code files

- `crates/agent_usage/src/usage_panel.rs` — dòng agent thành `ContextMenuEntry`-style có submenu, hoặc mở rộng in-place
- `crates/agent_usage/src/agent_usage.rs` — helper format mốc tuyệt đối

## Todo

- [x] Chọn **mở rộng tại chỗ** (không submenu neo cạnh) — đúng preview bạn đã chọn
      (cây `└─` dưới dòng agent), và không phải lồng một `PopoverMenu` trong một
      `PopoverMenu` đang ở deferred layer
- [x] `long_name()` được dùng ở đây (đã có từ phase 01)
- [x] Format mốc reset tuyệt đối, giờ địa phương
- [x] Cửa sổ không mốc → in "no reset window" chứ không để trống
- [x] Agent im → submenu mang `reason` nguyên văn
- [x] Test: 3 cửa sổ Claude ra 3 dòng, dòng scoped nói không có mốc
- [x] Test: agent im lặng vẫn mở được submenu và submenu nói lý do
- [x] `cargo check -p agent_usage`

## Bẫy

- **Đừng thêm request nào.** Mở submenu là xem lại dữ liệu đã có trong tay, không phải cớ
  để fetch.
- **Múi giờ là chỗ dễ trôi test.** Hàm format nhận `DateTime<Utc>` và trả chuỗi theo
  offset truyền vào, chứ không tự đọc timezone hệ thống — nếu không, test xanh ở đây và
  đỏ trên CI.

## Success criteria

Mũi tên mở ra ba dòng đúng ba cửa sổ, tên đầy đủ, mốc reset đọc được và khớp với đếm ngược
ở ngoài. Codex chưa login → mở ra thấy lý do, không phải panel trống.
