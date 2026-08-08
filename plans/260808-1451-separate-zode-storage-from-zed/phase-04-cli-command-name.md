# Phase 04 — Tên lệnh CLI

## Context Links

- Biên bản: [`brainstorm-260808-separate-config-from-zed.md`](../reports/brainstorm-260808-separate-config-from-zed.md) § cuối "Quyết định 4"
- Chạy **song song được** với phase 01, 02, 03

## Overview

**Priority:** P3 · **Status:** pending · **Depends:** — · **Effort:** ~0.1d

CLI đang tự xưng là Zed trong `--help` và trong mọi thông báo lỗi.

## Key Insights

- **Đây là phase nhỏ nhất và thấp ưu tiên nhất.** Không có va chạm kỹ thuật nào — binary được cài
  là `Contents/MacOS/cli` (`script/bundle-mac:336`), không phải `zed`, nên nó **không** giẫm lên
  CLI của Zed. Đây thuần là chuyện nhất quán tên gọi.
- **Không đổi tên file binary.** `crates/cli/Cargo.toml` khai `name = "cli"` và
  `script/bundle-mac:204, 336` bám vào đúng tên đó. Đổi thì kéo theo build script — vượt phạm vi
  đã chốt ("không đụng `script/bundle-*`"). Chỉ đổi **tên lệnh clap** và **văn bản**.
- `URL_PREFIX` (`main.rs:30`) chứa `"zed://"`. Đây là **URL scheme đã đăng ký**, không phải nhãn —
  đổi nó làm hỏng mọi link `zed://` đang tồn tại, và `crates/zed/src/zed.rs:915-925` có cả luồng
  đăng ký scheme. **Ngoài phạm vi**, để nguyên.

## Requirements

**Chức năng:** `--help` và thông báo lỗi nói "Zode", tên lệnh là `zode`.
**Phi chức năng:** Tên file binary, URL scheme, và mọi thứ `script/bundle-*` bám vào đều **không đổi**.

## Architecture

Không đổi. Chỉ thay chuỗi trong khối thuộc tính `clap` và doc comment.

## Related Code Files

**Sửa:** `crates/cli/src/main.rs` — và **chỉ** file này
**Không sửa:** `crates/cli/Cargo.toml`, `script/bundle-mac`, bất kỳ chỗ nào chạm `zed://`

## Implementation Steps

1. **`name = "zed"`** (`:47`) → `"zode"`.

2. **`before_help`** (`:49-61`) — khối này lặp lại `zed` trong sáu dòng ví dụ. Đổi hết, giữ nguyên
   hình dạng ví dụ.

3. **`after_help`** (`:61`) — `'ps axf | zed -'` → `zode`.

4. **Doc comment các tham số** (`:78, 86-90, 94, 98, 101`) — chúng ghi "Zed" và cả đường dẫn cụ thể
   (`~/Library/Application Support/Zed`, `%LOCALAPPDATA%\Zed`, `$XDG_DATA_HOME/zed`).
   **Những đường dẫn này phải khớp phase 01** — nếu phase 01 đã chạy thì đây là doc đã sai.

5. **`zed_version_string()`** (`:35`) — tên hàm nội bộ. Đổi nếu tiện, không bắt buộc; nó không lộ
   ra người dùng.

6. **Rà sót.** `grep -n 'Zed\|zed' crates/cli/src/main.rs`. Được phép còn lại: `"zed://"` (`:30`)
   và `use util::paths::...` (`:25`).

## Todo List

- [ ] `name`, `before_help`, `after_help`
- [ ] Doc comment tham số — kể cả ba đường dẫn ở `:86-90`
- [ ] Rà sót, xác nhận chỉ còn `zed://` và import
- [ ] Chạy `zode --help` bằng mắt, đọc hết một lượt

## Success Criteria

- `cargo run -p cli -- --help` không còn dòng nào tự xưng Zed, trừ `zed://` trong danh sách scheme
- Ba đường dẫn trong `--help` **khớp** giá trị thật của phase 01
- `./script/clippy` sạch

## Test Design

**Không viết test tự động cho phase này.** Đây là chuỗi văn bản trong `--help`; một test khẳng định
`--help` chứa chữ "Zode" chỉ lặp lại chính dòng code vừa viết, không bắt được lỗi nào có thật.

Kiểm chứng là **đọc bằng mắt một lượt** ở bước todo cuối. Ghi nhận như vậy chứ không giả vờ có phủ test.

**Ngoại lệ đáng cân nhắc:** ba đường dẫn ở `:86-90` *có thể* lệch khỏi `paths.rs` một cách âm thầm.
Nếu muốn khoá, cách rẻ nhất là đọc chúng từ hằng dùng chung thay vì test — nhưng `cli` cố tình
không phụ thuộc `paths` để giữ binary nhỏ. Chấp nhận rủi ro doc lệch, ghi vào bảng dưới.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Đổi `zed://` → hỏng mọi link đã tồn tại và luồng đăng ký scheme | Key Insights + bước 6 ghi rõ để nguyên |
| Đổi `name` trong `Cargo.toml` → gãy `script/bundle-mac` | Related Code Files ghi rõ "không sửa" |
| Doc đường dẫn ở `:86-90` lệch khỏi `paths.rs` về sau | **Chấp nhận có ý thức** — `cli` không phụ thuộc `paths` |

## Security Considerations

Không có.

## Next Steps

Mở khoá phase 05.
