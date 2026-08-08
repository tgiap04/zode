# Phase 05 — Kiểm chứng chạy song song

## Context Links

- Tiêu chí gốc: [`plan.md`](plan.md) § "Định nghĩa hoàn thành"
- **Phụ thuộc:** phase 01, 02, 03, 04 — tất cả

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** 01–04 · **Effort:** ~0.25d

Phase duy nhất trả lời được câu hỏi thật: **cài cả hai app thì có giẫm nhau không?**

## Key Insights

- **Bốn phase trên xanh không chứng minh gì về mục tiêu.** Chúng chứng minh từng hàm trả về đúng
  chuỗi. Việc hai app cùng chạy được là hành vi ở tầng hệ điều hành — chỉ đo được bằng cách chạy thật.
- **Phải dựng bản `stable`, không phải `dev`.** `main.rs:330-334` bỏ qua toàn bộ kiểm tra
  single-instance trên kênh dev. Kiểm chứng trên bản dev sẽ **xanh giả** — hai app cùng mở được
  vì kiểm tra không chạy, chứ không phải vì đã sửa đúng.
- **Người dùng sẽ mất thiết lập.** Đây là hệ quả cố ý của quyết định "bắt đầu trắng", nhưng nếu
  họ phát hiện sau khi chạy thì nó cảm giác như lỗi. **Nói trước, không nói sau.**
- Phase này **không sở hữu file nào** — không sửa code. Phát hiện lỗi thì trả về phase tương ứng.

## Requirements

**Chức năng:** Zed và zode cùng chạy, cùng lưu, không đọc của nhau.
**Phi chức năng:** Mọi bước kiểm chứng phải **lặp lại được** bởi người khác, không phải "tôi thấy ổn".

## Architecture

Không có. Đây là phase kiểm chứng.

## Related Code Files

**Sửa:** không file nào
**Chạy:** `./script/clippy`, `cargo test`, bản dựng stable

## Implementation Steps

1. **Cổng tự động.**
   ```
   ./script/clippy
   cargo test -p paths -p workspace -p zode
   ```
   Chạy **không lọc**. `cargo test -p workspace <filter>` in `N filtered out` mà vẫn thoát 0 —
   đọc dòng đó trước khi kết luận crate đã xanh.

2. **Rà sót toàn kho.**
   ```
   grep -rn 'join("Zed")\|join("zed")\|Application Support/Zed\|Library/Logs/Zed' crates/
   ```
   Phải rỗng. Còn dòng nào thì là một gốc bị bỏ sót — quay lại phase 01.

3. **Đếm call site env** (khoá phạm vi phase 03):
   ```
   grep -rn 'env::var("ZED_' crates/ | wc -l
   ```
   Phải ra **đúng 30**. Lệch nghĩa là phase 03 đã đi sửa call site, trái với thiết kế.

4. **Dựng bản stable.** Đổi `crates/zed/RELEASE_CHANNEL` sang `stable` (hoặc đặt
   `ZED_RELEASE_CHANNEL=stable`), dựng, và **nhớ trả lại** sau khi xong.

5. **Kiểm chứng chạy song song** — bài kiểm quan trọng nhất:
   1. Mở Zed thật, để đó
   2. Mở zode bản stable vừa dựng
   3. **Cả hai phải cùng chạy.** zode in `zed is already running` rồi thoát ⇒ phase 02 chưa xong
   4. Đảo thứ tự, làm lại

6. **Kiểm chứng tách dữ liệu:**
   1. Trong zode, đổi một thiết lập nhìn thấy được (ví dụ `theme.mode`)
   2. Xác nhận `~/.config/zode/settings.json` **được tạo** và có thay đổi đó
   3. Xác nhận `~/.config/zed/settings.json` **không đổi**
   4. Sửa `~/.config/zed/settings.json` bằng tay → zode **không** phản ứng

7. **Kiểm chứng thư mục dữ liệu:**
   ```
   ls ~/Library/Application\ Support/Zode/ ~/Library/Logs/Zode/
   ```
   Phải thấy `db/` và log. Nếu log vẫn rơi vào `~/Library/Logs/Zed/` ⇒ bỏ sót `logs_dir()`,
   quay lại phase 01 bước 5.

8. **Kiểm chứng cầu env:**
   ```
   ZODE_STATELESS=1 zode      # stateless
   ZED_STATELESS=1 zode       # vẫn stateless
   ZODE_STATELESS=0 ZED_STATELESS=1 zode   # KHÔNG stateless — ZODE_ thắng
   ```

9. **Nói với người dùng, trước khi họ chạy.** Thiết lập cũ không tự chuyển. Đưa đúng một câu lệnh:
   ```
   cp -r ~/.config/zed ~/.config/zode
   ```
   Kèm cảnh báo: câu này chép **cả** những khoá của Zed mà zode không biết — vô hại, zode bỏ qua.
   Ai muốn sạch thì chỉ chép `settings.json`.

## Todo List

- [ ] `./script/clippy` sạch
- [ ] `cargo test -p paths -p workspace -p zode` — **không lọc**, đọc dòng `filtered out`
- [ ] grep rà sót đường dẫn — rỗng
- [ ] grep đếm call site env — đúng 30
- [ ] Dựng bản stable
- [ ] Chạy song song hai app, cả hai chiều
- [ ] Tách settings, bốn bước
- [ ] Thư mục dữ liệu và log nằm dưới `Zode`
- [ ] Cầu env, ba tổ hợp
- [ ] **Trả `RELEASE_CHANNEL` về `dev`**
- [ ] Nói với người dùng về thiết lập cũ + câu lệnh chép

## Success Criteria

Toàn bộ 11 mục trên xanh. Bước 5 và 6 là hai bước không được thay bằng suy luận —
**chưa chạy thật thì phase này chưa xong**, dù mọi test có xanh.

## Test Design

Phase này **cố ý không có test tự động**. Cái nó đo là hành vi hai tiến trình trên một hệ điều
hành thật, với hai app đã cài. Không giả lập được trong `cargo test` mà vẫn còn ý nghĩa.

Báo cáo phải ghi rõ đâu là **đã chạy thật** và đâu là **suy luận** — như bước 2, 3 là kiểm bằng
grep, còn bước 5, 6 là quan sát tay.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Kiểm chứng trên bản dev → **xanh giả** vì kiểm tra bị bỏ qua | Bước 4 bắt dựng stable; Key Insights nêu lý do |
| Quên trả `RELEASE_CHANNEL` về `dev` sau khi kiểm | Có mục riêng trong Todo |
| Báo "workspace xanh" từ một lần chạy đã lọc | Bước 1 bắt đọc dòng `filtered out` |
| Người dùng chạy trước khi được báo, tưởng mất thiết lập là lỗi | Bước 9, và phải làm **trước** khi giao |
| Cài Zed thật để test lại làm bẩn máy | Không tránh được — đây chính là kịch bản cần kiểm |

## Security Considerations

Bước 8 in biến môi trường ra terminal. `ZED_ADMIN_API_TOKEN` nằm trong nhóm 40 biến —
**không** dán output có chứa nó vào báo cáo hay commit message.

## Next Steps

Xong plan. Ghi vào `docs/project-changelog.md`: đổi đường dẫn là **thay đổi phá vỡ** với người
dùng zode hiện có, và `paths.rs` + `mac_only_instance.rs` là điểm conflict cần nhớ khi rebase upstream.
