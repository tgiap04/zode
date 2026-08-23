# Phase 03 — Render đúng mẫu trong ảnh

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** đúng mẫu tham chiếu

## Mục tiêu

`Vec<UsageWindow>` thành đúng chuỗi trong ảnh:

```
✳  53% used 1h 17m · 10% used 6d 12h · 0% used Fable
```

## Luật render

Đã đối chiếu với cả ba hàng dữ liệu thật (xem report). Cho mỗi `UsageWindow`:

```
"{percent}% used" + nếu có resets_at → " " + đếm ngược
                  + nếu không, mà có label → " " + label
```

| dữ liệu thật | ra | trong ảnh |
|---|---|---|
| 53%, resets 12:29 | `53% used 1h 17m` | `50% used 1h 17m` |
| 10%, resets 27th | `10% used 6d 12h` | `10% used 6d 12h` |
| 0%, không reset, model Fable | `0% used Fable` | `0% used Fable` |

Nối bằng ` · `. Icon của agent đứng đầu nhóm (`agent_icon` trong `agent_ui` vẽ đúng
glyph đó — nhưng **không** depend `agent_ui`: nhân đôi một hàm hai nhánh `match`
nhẹ hơn là một dependency, và `agent_ui::agent_icon` đã ghi rõ nó là chủ ý).

## Đếm ngược

`1h 17m` và `6d 12h` — hai đơn vị lớn nhất, không bao giờ ba. Quá hạn → không hiện
đếm ngược (dữ liệu đang cũ, nói "0m" là nói sai).

| còn lại | hiện |
|---|---|
| ≥ 1 ngày | `6d 12h` |
| ≥ 1 giờ | `1h 17m` |
| < 1 giờ | `17m` |
| ≤ 0 | (bỏ) |

## Việc

1. `fn format_countdown(remaining: Duration) -> Option<String>` — thuần, dễ test.
2. `fn render_window(w: &UsageWindow, now: DateTime<Utc>) -> String`.
3. Render nhóm: icon + các cửa sổ nối bằng ` · `, dùng `Label` với `LabelSize::Small`
   theo đúng lối các status item khác.
4. `now` **truyền vào**, không gọi `Utc::now()` bên trong — nếu không thì không có
   test đếm ngược nào là tất định được.

## Files

- sửa: `crates/agent_usage/src/agent_usage.rs`

## Todo

- [x] `format_countdown` + `render_window`, `now` là tham số
- [x] Icon per agent, các cửa sổ nối ` · `
- [x] Test: đúng ba hàng dữ liệu thật → đúng ba chuỗi trong bảng trên
- [x] Test: mọi ngưỡng đếm ngược (ngày / giờ / phút / quá hạn)
- [x] Test: `resets_at` và `label` cùng `None` → chỉ `"{n}% used"`
- [x] `cargo check -p agent_usage` xanh

## Success criteria

Chuỗi sinh ra trùng khớp ảnh tham chiếu ở cả ba mục, và mọi nhánh đếm ngược có test.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Test đếm ngược phụ thuộc giờ máy | `now` là tham số, không gọi `Utc::now()` bên trong |
| Nói "0m" khi dữ liệu đã cũ | Quá hạn thì bỏ đếm ngược hẳn |
