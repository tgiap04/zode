# Phase 01 — Bốn thư mục gốc (và cái gốc thứ năm bị bỏ sót)

## Context Links

- Biên bản: [`brainstorm-260808-separate-config-from-zed.md`](../reports/brainstorm-260808-separate-config-from-zed.md)
- Chạy **song song được** với phase 02, 03, 04

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** — · **Effort:** ~0.5d

Đổi mọi đường dẫn gốc của zode từ `Zed`/`zed` sang `Zode`/`zode`. ~30 hàm dẫn xuất tự đi theo.

## Key Insights

- **Biên bản nói "4 hàm gốc" là chưa đủ.** Rà lại thì có **năm**: `logs_dir()` trên macOS
  **không** mọc từ `data_dir()` mà đi thẳng `~/Library/Logs/Zed` (`paths.rs:195`). Bỏ sót nó thì
  log của hai app vẫn chồng lên nhau — và log là thứ ít ai để ý nhất, nên sẽ âm thầm.
- Thêm **hai tên file**: `Zed.log` và `Zed.log.old` (`paths.rs:211, 217`).
- `crates/paths` hiện có **0 test**. Đây không phải chỗ để tin vào clippy — clippy xanh không nói
  gì về việc hàm trả về `zode` hay `zed`.
- `set_custom_data_dir()` **panic** nếu `CURRENT_DATA_DIR` hoặc `CONFIG_DIR` đã khởi tạo
  (`paths.rs:69-71`). Cả hai là `OnceLock` **toàn tiến trình**, nên trong một binary test, một test
  gọi `config_dir()` sẽ làm test khác gọi `set_custom_data_dir()` panic. Viết test phải tính điều này.

## Requirements

**Chức năng:** Năm gốc và hai tên file log mang tên Zode/zode trên cả ba nền tảng.
**Phi chức năng:** Không đổi thứ tự khởi tạo, không đổi chữ ký hàm nào — nhánh dẫn xuất phải
tiếp tục biên dịch không sửa.

## Architecture

Không đổi cấu trúc. Chỉ thay hằng chuỗi trong thân năm hàm, cộng doc comment.

## Related Code Files

**Sửa:** `crates/paths/src/paths.rs` — và **chỉ** file này
**Đọc để đối chiếu:** `crates/zed/src/zed/open_listener.rs:293` (socket Linux, hưởng lợi gián tiếp)
**Không sửa:** bất kỳ file nào khác

## Implementation Steps

1. **`config_dir()`** (`:87-105`) — `join("Zed")` → `join("Zode")` (Windows `:94`);
   `join("zed")` → `join("zode")` (Linux `:101`, fallback `:103`).

2. **`data_dir()`** (`:109-129`) — `"Library/Application Support/Zed"` → `.../Zode` (`:114`);
   Linux `:121`; Windows `:125`.

3. **`state_dir()`** (`:132-153`) — macOS `:136`, Linux `:145`, Windows `:150`.

4. **`temp_dir()`** (`:156-181`) — macOS `:162`, Windows `:168`, Linux `:177`, fallback `:180`.

5. **`logs_dir()`** (`:191-200`) — nhánh macOS `:195` `"Library/Logs/Zed"` → `"Library/Logs/Zode"`.
   Nhánh còn lại (`data_dir().join("logs")`) đã theo bước 2, không đụng.

6. **`log_file()` / `old_log_file()`** (`:211, 217`) — `"Zed.log"` → `"Zode.log"`,
   `"Zed.log.old"` → `"Zode.log.old"`. Sửa cả doc comment ở `:208`, `:214`.

7. **Doc comment** — `:1` (module doc), `:21-23`, `:28-30`, `:86`, `:108`, `:155`. Chúng ghi rõ
   đường dẫn cụ thể, để nguyên là **thành sai lệch có chủ đích**, tệ hơn không ghi gì.

8. **Rà sót.** Chạy `grep -n 'Zed\|"zed"' crates/paths/src/paths.rs` và soi từng dòng còn lại.
   Được phép còn lại: `remote_server_dir_relative` (`.zed_server`), `remote_wsl_server_dir_relative`
   (`.zed_wsl_server`), và doc `:340` nhắc "the Zed repository" — cả ba **ngoài phạm vi**.

9. **Xử lý crate đã bị xoá.** Nếu plan `260726-1531` đã chạy trước và `crashes_dir()` /
   `crashes_retired_dir()` không còn, bỏ qua chúng — bám vào cái đang có trong file, không bám
   danh sách trên đây.

## Todo List

- [ ] `config_dir()` — 3 nhánh
- [ ] `data_dir()` — 3 nhánh
- [ ] `state_dir()` — 3 nhánh
- [ ] `temp_dir()` — 4 nhánh
- [ ] `logs_dir()` — nhánh macOS
- [ ] `log_file()` + `old_log_file()` — 2 tên file
- [ ] Doc comment ở 6 chỗ
- [ ] Test: năm gốc kết thúc bằng `zode`/`Zode`
- [ ] Test: `log_file()` nằm dưới `logs_dir()` và tên là `Zode.log`
- [ ] Rà sót bằng grep, xác nhận chỉ còn 3 chỗ được phép

## Success Criteria

- `grep -n 'Zed\|"zed"' crates/paths/src/paths.rs` chỉ còn 3 dòng, tất cả thuộc nhóm remote-server
- `cargo test -p paths` xanh
- **Test phải có răng:** đổi ngược một gốc về `"Zed"` → test đỏ. Chưa mutation-test thì chưa xong.
- `./script/clippy` sạch

## Test Design

Đặt trong `crates/paths/src/paths.rs` dưới `#[cfg(test)] mod tests`.

```rust
// Bốn hàm này là OnceLock toàn tiến trình, nên gộp vào MỘT test:
// gọi rời rạc ở nhiều test thì thứ tự chạy quyết định cái nào khởi tạo
// trước, và test nào đụng set_custom_data_dir sẽ panic.
#[test]
fn every_root_is_namespaced_to_zode() { ... }
```

Khẳng định trên **thành phần cuối** của đường dẫn (`file_name()`), không phải chuỗi tuyệt đối —
`$XDG_CONFIG_HOME` khác nhau giữa máy dev và CI.

**Không** viết test cho `set_custom_data_dir()` trong cùng binary này. Nếu cần thì đặt ở
`crates/paths/tests/` (binary riêng), và nói rõ lý do trong comment.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Sót `logs_dir()` macOS → log hai app vẫn chồng, âm thầm | Bước 5 + test riêng cho `log_file()` |
| Sửa nhầm `.zed_server` → lệch client/server remote | Bước 8 liệt kê tường minh 3 chỗ được phép còn lại |
| Test khẳng định đường dẫn tuyệt đối → đỏ trên CI | Test Design nói rõ: chỉ so `file_name()` |
| `OnceLock` toàn tiến trình làm test giao thoa | Gộp một test; `set_custom_data_dir` sang binary riêng |

## Security Considerations

Không có dữ liệu nhạy cảm. Nhưng lưu ý: thư mục mới **được tạo với quyền mặc định** — giống hệt
hành vi cũ, không đổi.

## Next Steps

Mở khoá phase 05. Đồng thời làm socket Linux (`open_listener.rs:293`) tự tách mà không cần sửa gì.
