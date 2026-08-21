# Phase 01 — Mỗi cửa sổ biết nó là cửa sổ gì

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** không gì cả (cố ý)

## Việc

`UsageWindow` hiện mang `percent`, `resets_at`, `label`. Panel cần thêm một thứ nữa: cửa
sổ này **là loại gì** — để in `5h` / `wk` / `30d` ở dòng gọn, và "Session (5h)" / "Weekly"
ở submenu. Hai nguồn nói điều đó bằng hai cách khác nhau, nên kiểu mới phải chịu được cả
hai mà không nghiêng về bên nào.

```rust
pub enum WindowKind {
    /// Claude `kind: "session"` — cửa sổ trượt.
    Session,
    /// Claude `weekly_all` / `weekly_scoped`.
    Weekly,
    /// Codex: không nói loại, nhưng nói độ dài.
    Span(Duration),
    /// Nguồn nói một thứ build này chưa biết.
    Unknown,
}
```

Hai hàm: `short_tag()` → `"5h"` / `"wk"` / `"30d"` / `""`, và `long_name()` →
`"Session (5h)"` / `"Weekly"` / `"30-day window"` / `"Window"`.

**`Session` → `"5h"` là một giả định, và nó có căn cứ:** cùng response mang một field
tên đúng nghĩa `five_hour` bên cạnh `limits[]`, và ảnh mẫu in `5h`. Ghi nhận xét vào chỗ
đó — nếu Anthropic đổi độ dài cửa sổ session, đây là dòng nói sai đầu tiên.

`Span` phải in tag từ độ dài chứ không tra bảng: `43200` phút của payload thật là **30
ngày**, và không có tên nào cho nó.

## Related code files

- `crates/agent_usage/src/agent_usage.rs` — `UsageWindow`, `WindowKind`
- `crates/agent_usage/src/claude.rs` — `Limit` đọc thêm `kind`, `parse_windows` map nó
- `crates/agent_usage/src/codex.rs` — `RateLimitWindow` đọc `windowDurationMins`

## Todo

- [x] `WindowKind` + `short_tag()` + `long_name()`
- [x] `UsageWindow.kind`
- [x] `claude::Limit.kind` → `WindowKind` (`session` / `weekly_*` / còn lại `Unknown`)
- [x] `codex::RateLimitWindow.window_duration_mins` → `Span`
- [x] Test: fixture thật của Claude cho ra `[Session, Weekly, Weekly]`
- [x] Test: fixture thật của Codex cho ra `Span(30 ngày)` → tag `"30d"`
- [x] Test: `kind` lạ → `Unknown`, tag rỗng, **không panic và không mất row**
- [x] `cargo check -p agent_usage` + `cargo test -p agent_usage`

## Bẫy

Một `kind` không nhận ra **không được làm mất cửa sổ đó**. `parse_windows` đang dùng
`filter_map`, nên rất dễ vô tình biến `kind` lạ thành row bị bỏ — tức số tổng thấp hơn sự
thật, im lặng. `Unknown` tồn tại chính vì thế.

## Success criteria

Cả hai parser trả về `kind` đúng trên payload đã ghi thật. Số row không đổi so với trước
phase này — không thêm, không mất. Thanh status trông y nguyên.
