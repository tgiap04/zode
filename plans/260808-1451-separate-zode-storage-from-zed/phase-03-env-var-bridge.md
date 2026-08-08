# Phase 03 — Cầu nối `ZODE_*` → `ZED_*`

## Context Links

- Biên bản: [`brainstorm-260808-separate-config-from-zed.md`](../reports/brainstorm-260808-separate-config-from-zed.md) § "Quyết định 4"
- Lý do lệch khỏi biên bản: [`plan.md`](plan.md) § "Một chỗ lệch khỏi biên bản, có lý do"
- Chạy **song song được** với phase 01, 02, 04

## Overview

**Priority:** P2 · **Status:** pending · **Depends:** — · **Effort:** ~0.25d

Cho phép người dùng đặt `ZODE_<X>` thay cho `ZED_<X>`, mà không sửa một call site nào.

## Key Insights

- **Đây là phase có giá trị thấp nhất trong plan, và phải giữ nó như vậy.** Biến môi trường không
  phải nơi lưu trữ — chúng chỉ va chạm khi người dùng tự `export`. Làm to phase này là phản lại
  chính lý do nó được thu hẹp.
- **Làm đúng chữ "một accessor" trong biên bản lại tốn 30 call site.** Ngữ nghĩa giữ nguyên,
  cơ chế đổi: bắc cầu một lần ở đầu `main()`, quét `ZODE_<X>` rồi gán sang `ZED_<X>`. Mọi chỗ đọc
  hiện có không phải sửa. **1 điểm conflict thay vì 30.**
- **Cái giá: `std::env::set_var` là `unsafe` ở edition 2024** (`Cargo.toml:206`). Nó unsafe vì
  đua với thread khác đang đọc env. Đặt ở **câu lệnh đầu tiên** của `main()` — trước
  `Args::parse()`, trước mọi `LazyLock` — thì tiến trình còn đơn luồng và điều kiện đua không tồn
  tại. Phải viết rõ lập luận đó ngay tại `unsafe` block, không phải chỉ `#[allow]`.
- **Thứ tự là điều kiện đúng đắn, không phải sở thích.** Đặt cầu sau một `LazyLock` đã đọc env thì
  cầu **âm thầm vô tác dụng** — không lỗi, không cảnh báo, chỉ là biến không có hiệu lực.
  `RELEASE_CHANNEL_NAME` (`release_channel/src/lib.rs:11`) đọc `ZED_RELEASE_CHANNEL` và là cái dễ
  bị chạm sớm nhất.
- **Chỉ runtime.** 22 chỗ `option_env!`/`env!` là build-time, cầu nối không với tới và **không nên**
  với tới — chúng do build script đặt, không phải người dùng.

## Requirements

**Chức năng:** `ZODE_<X>` có tác dụng ở mọi chỗ `ZED_<X>` có tác dụng. Đặt cả hai thì `ZODE_` thắng.
Chỉ đặt `ZED_` thì hành vi **không đổi so với hiện tại**.
**Phi chức năng:** Không sửa call site nào. Không đụng `script/bundle-*` hay CI.

## Architecture

```
fn main() {
    bridge_zode_env_vars();   // ← câu lệnh đầu tiên, trước mọi thứ khác
    STARTUP_TIME.get_or_init(...);
    ...
}
```

Hàm quét `std::env::vars()`, với mỗi khoá `ZODE_<X>` thì `set_var("ZED_<X>", value)`.

**Ghi đè vô điều kiện** — nếu cả `ZODE_X` và `ZED_X` cùng được đặt thì `ZODE_X` thắng, đúng như
quyết định 4. Không ghi đè có điều kiện, vì "chỉ đặt khi `ZED_X` trống" cho ngữ nghĩa ngược lại.

## Related Code Files

**Sửa:** `crates/zed_env_vars/src/zed_env_vars.rs` (hàm cầu nối), `crates/zed/src/main.rs` (gọi nó)
**Đọc để hiểu ràng buộc:** `crates/release_channel/src/lib.rs:11-18`, `crates/terminal/src/terminal.rs:124`
**Không sửa:** 30 call site đọc `ZED_*`, 22 chỗ build-time, `script/`, `.github/`

## Implementation Steps

1. **Viết `bridge_zode_env_vars()`** trong `crates/zed_env_vars` — crate này đã là chỗ tập trung
   cho env var, đặt ở đây là đúng chỗ, không phải tạo module mới.

2. **`unsafe` block kèm lập luận an toàn viết thẳng vào comment**: gọi trước khi tiến trình spawn
   thread nào, nên không có bạn đọc đồng thời. Đây là loại comment "tại sao" mà `CLAUDE.md` yêu
   cầu — không phải mô tả code làm gì.

3. **Gọi ở dòng đầu `main()`** (`crates/zed/src/main.rs:174`), **trước** `STARTUP_TIME`.

4. **Quyết định về các binary khác.** `crates/remote_server` và `crates/cli` có `main()` riêng.
   Xem chúng có đọc `ZED_*` runtime không; **có thì bắc cầu, không thì bỏ qua** — đừng thêm cho
   đủ bộ.

   → **Kết luận khi thi công: cả hai đều không cần.** `remote_server` không có chỗ nào đọc
   `ZED_*` lúc chạy. `cli` chỉ **đặt** `ZED_FORCE_CLI_MODE` cho tiến trình con
   (`cli/src/main.rs:940, 1348`) — nó là bên sản xuất biến, không phải bên tiêu thụ, nên bắc cầu
   ở đó không có tác dụng gì.

5. **Không đụng `ZED_TERM`.** Nó được *bơm ra* môi trường terminal tích hợp
   (`terminal.rs:124`), không phải đọc vào. Script người dùng có thể đang bám vào nó.

## Todo List

- [ ] `bridge_zode_env_vars()` trong `zed_env_vars`
- [ ] `unsafe` block kèm lập luận an toàn bằng chữ
- [ ] Gọi ở dòng đầu `main()`
- [ ] Kiểm `remote_server` / `cli` có cần không, ghi kết luận vào phase file này
- [ ] Test: `ZODE_X` được bắc sang `ZED_X`
- [ ] Test: `ZODE_X` thắng khi cả hai cùng đặt
- [ ] Test: khoá không có tiền tố `ZODE_` không bị đụng
- [ ] Test khoá thứ tự gọi (xem Test Design)

## Success Criteria

- `ZODE_STATELESS=1 zode` chạy stateless; `ZED_STATELESS=1 zode` vẫn chạy stateless
- Đặt `ZODE_STATELESS=0 ZED_STATELESS=1` → **không** stateless (`ZODE_` thắng)
- `grep -rn 'env::var("ZED_' crates/ | wc -l` **không đổi** so với trước phase — đây là bằng chứng
  đo được rằng không call site nào bị sửa
- `./script/clippy` sạch

## Test Design

Ba test đầu là hàm thuần: cho hàm nhận một iterator cặp khoá-giá trị và trả về danh sách cần gán,
thay vì đọc/ghi env thật. Test env thật thì giao thoa giữa các test chạy song song.

**Test thứ tư khó và quan trọng nhất** — khoá việc cầu nối phải chạy trước mọi thứ đọc env.
Không có cách nào test trực tiếp bằng unit test. Hai lựa chọn, chọn một và ghi lý do:

- **(a)** Một test đọc `crates/zed/src/main.rs` dạng văn bản, khẳng định `bridge_zode_env_vars()`
  xuất hiện trước `STARTUP_TIME`. Thô, nhưng bắt đúng cái regression thật: ai đó chèn code lên trên nó.
- **(b)** Không test, thay bằng comment cảnh báo tại chỗ gọi.

Khuyến nghị **(a)**: thứ tự ở đây là điều kiện đúng đắn, và chế độ hỏng là *âm thầm*. Một test thô
mà bắt được vẫn hơn một comment không ai đọc.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Cầu chạy sau một `LazyLock` đã đọc env → vô tác dụng, **âm thầm** | Bước 3 + test thứ tự |
| `unsafe set_var` bị coi là tùy tiện khi review | Bước 2 bắt viết lập luận bằng chữ, không dùng `#[allow]` |
| Phase phình ra thành "đổi tên hết cho sạch" | Success Criteria có phép đếm `grep` — call site đổi là hỏng tiêu chí |
| Đụng `ZED_TERM` làm gãy script người dùng | Bước 5 nói rõ: chỉ đọc vào mới bắc cầu |

## Security Considerations

Cầu nối **sao chép giá trị env trong cùng tiến trình**, không ghi ra đâu cả. Không log giá trị —
`ZED_ADMIN_API_TOKEN` nằm trong danh sách 40 biến, log ra là rò bí mật.

## Next Steps

Mở khoá phase 05.
