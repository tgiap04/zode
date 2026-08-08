# Phase 02 — Định danh single-instance trên macOS

## Context Links

- Biên bản: [`brainstorm-260808-separate-config-from-zed.md`](../reports/brainstorm-260808-separate-config-from-zed.md) § "Va chạm nặng nhất"
- Bảng ba nền tảng: [`plan.md`](plan.md) § "Sửa lại một chỗ biên bản nói chưa đủ chính xác"
- Chạy **song song được** với phase 01, 03, 04

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** — · **Effort:** ~0.25d

zode và Zed đang tranh **cùng một cổng TCP** với **cùng một chuỗi bắt tay**. Phase này tách cả hai.

## Key Insights

- **Chỉ macOS hỏng.** Linux dùng unix socket dưới `data_dir()` nên phase 01 tách hộ; Windows dẫn
  mutex và named pipe từ `app_identifier()` vốn đã là `Zode-Editor-*`. Đừng đụng hai nhánh đó.
- **Hai thứ phải đổi, không phải một.** Đổi cổng mà giữ chuỗi bắt tay thì hai app không còn thấy
  nhau — nhưng nếu về sau ai đó chỉnh cổng trùng lại, chuỗi giống hệt sẽ làm chúng nhận nhầm nhau
  là cùng một app. Đổi chuỗi là hàng rào thứ hai.
- **Lỗi này không lộ trên kênh dev.** `main.rs:330-334` bỏ qua toàn bộ kiểm tra khi
  `RELEASE_CHANNEL == Dev`, mà `crates/zed/RELEASE_CHANNEL` hiện là `dev`. Nên **không thể** kiểm
  chứng bằng cách chạy app — phải test thẳng vào hàm.
- Công thức cổng hiện tại: base `43737`, mỗi kênh cách nhau `USER_BLOCK = 100`
  (Dev 43737 / Preview 43837 / Stable 43937 / Nightly 44037), rồi cộng `uid % max_uid`.
  **`uid` cộng vào sau nên các dải kênh đan xen nhau** — chọn base mới phải tính tới điều đó,
  không chỉ nhìn 4 con số đầu.

## Requirements

**Chức năng:** zode không bao giờ bắt tay thành công với một instance Zed, và ngược lại.
**Phi chức năng:** Giữ nguyên cơ chế đan xen theo uid — nhiều người dùng trên cùng máy vẫn phải
tách nhau như cũ.

## Architecture

Không đổi cấu trúc. Đổi `base port` trong `address()` và bốn chuỗi trong `instance_handshake()`.

## Related Code Files

**Sửa:** `crates/zed/src/zed/mac_only_instance.rs` — và **chỉ** file này
**Đọc để hiểu ràng buộc:** `crates/zed/src/main.rs:330-355` (chỗ kiểm tra bị bỏ qua trên dev)
**Không sửa:** `windows_only_instance.rs`, `open_listener.rs`

## Implementation Steps

1. **Chọn base port mới.** Zed chiếm `43737 + kênh*100 + (uid % max_uid)`. Với `uid` tới hàng nghìn,
   dải của Zed **trải rất rộng và đan xen** — chọn base chỉ cách vài trăm là không an toàn.
   Dịch hẳn sang một base cách xa, và **ghi rõ trong comment con số của Zed** để người sau biết
   tại sao không được kéo lại gần.

2. **Sửa comment khối trong `address()`** (`:19-30`). Comment hiện tại giải thích cơ chế đan xen
   bằng những con số cụ thể của Zed (44238, 44338…). Đổi số mà giữ comment là để lại một lời giải
   thích sai — sửa cả hai.

3. **`instance_handshake()`** (`:73-80`) — bốn nhánh, `"Zed Editor <Kênh> Instance Running"` →
   chuỗi mang tên Zode. Giữ nguyên hình dạng để dễ đối chiếu về sau.

4. **Rà sót.** `grep -n "Zed" crates/zed/src/zed/mac_only_instance.rs` phải trả về rỗng, trừ
   comment cố tình nhắc tới Zed ở bước 1–2.

## Todo List

- [ ] Chọn base port mới, tính cả biên độ `uid`
- [ ] Sửa comment khối `address()` cho khớp số mới
- [ ] `instance_handshake()` — 4 nhánh
- [ ] Test: bốn kênh cho bốn cổng khác nhau
- [ ] Test: không cổng nào rơi vào dải của Zed, kể cả với `uid` biên
- [ ] Test: chuỗi bắt tay không chứa `"Zed Editor"`
- [ ] Mutation-test cả ba test

## Success Criteria

- `cargo test -p zode mac_only_instance` xanh (test gated `#[cfg(target_os = "macos")]`)
- **Có răng:** trả `instance_handshake()` về chuỗi cũ → test đỏ; kéo base port về `43737` → test đỏ
- `./script/clippy` sạch

## Test Design

Ràng buộc: `address()` đọc `uid` của tiến trình hiện tại qua `sysinfo`, nên **không thuần**.
Tách phần tính toán ra một hàm nhận `uid` làm tham số, rồi test hàm đó — đừng test qua `address()`.

```rust
// Dải Zed dùng: [43737, 43737 + 3*100 + max_uid]. Test phải khẳng định
// mọi cổng của Zode nằm NGOÀI dải đó với uid ở cả hai biên (0 và cận trên),
// chứ không chỉ với uid của máy đang chạy test.
```

Ba test:
1. `each_channel_gets_its_own_port` — bốn kênh, bốn giá trị khác nhau
2. `no_channel_collides_with_zeds_range` — quét uid biên
3. `handshake_does_not_impersonate_zed` — không chuỗi nào chứa `"Zed Editor"`

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Base mới vẫn giao với dải Zed vì quên biên độ `uid` | Test 2 quét uid biên, không chỉ uid hiện tại |
| Đụng nhầm Windows/Linux (vốn đã đúng) | Related Code Files ghi rõ "không sửa" |
| Base mới đâm vào cổng của app khác trên máy | Code đã có nhánh dự phòng (`:94-101`): bind lỗi thì chạy tiếp không bắt tay |
| Test viết qua `address()` → phụ thuộc uid của máy chạy test | Test Design bắt tách hàm thuần |

## Security Considerations

Cổng bind vào `127.0.0.1` (`LOCALHOST`, `:12`), không lộ ra mạng. Không đổi điều đó.

## Next Steps

Mở khoá phase 05 — bước kiểm chứng chạy song song hai app cần bản **stable**, vì kênh dev không
chạy kiểm tra này.
