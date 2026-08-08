# Phase 04 — Đổi chữ Zed → Zode, THAY logo

> ## ⚠ Quyết định đã lật (2026-08-08, lúc bắt đầu thi công)
>
> Bản vẽ chốt **bỏ hẳn logo**. Lý do lúc đó: **chưa có asset Zode nào**, và dựng một logo tạm bợ
> còn tệ hơn không có gì.
>
> Người dùng đã đưa `~/Downloads/zode.png` — 1024×1024 RGBA, **góc alpha = 0** (đã giải PNG để
> kiểm, không tin bằng mắt: viewer tô nền tối làm nó *trông như* nền đục). Tiền đề của quyết định
> cũ không còn.
>
> **Quyết định mới:** dùng logo ở **cả hai chỗ** — icon app (4 kênh × 2 kích thước) và trong app
> (onboarding + welcome). Phần "bỏ logo" bên dưới được thay bằng "thay logo"; phần đổi chữ giữ
> nguyên.
>
> **Ràng buộc kỹ thuật mới:** ảnh là **PNG nhiều màu**, còn `Vector::square` dựng bằng `svg()` và
> **tô một màu theo theme**. Không dùng `Vector` được. Phải chuyển sang `img("images/zode_logo.png")`
> — chuỗi thường vào `ImageSource` thành `Resource::Embedded`, giải qua `AssetSource`, mà
> `crates/assets` đã `#[include = "images/**/*"]`.
>
> **Rủi ro mới cần canh:** logo có nền tối và hiệu ứng ánh kim, thiết kế cho ô icon. Trên
> **Light 2026** (nền trắng) nó sẽ là một ô tối giữa nền sáng. Phải nhìn thật ở cả hai theme, và
> nếu chối mắt thì **báo lại**, đừng tự sửa asset của người dùng.

## Context Links

- Biên bản: [`brainstorm-260808-onboarding-rework.md`](../reports/brainstorm-260808-onboarding-rework.md) § "Logo"
- **Depends:** phase 03 — làm sau cùng là có chủ ý, xem dưới

## Overview

**Priority:** P2 · **Status:** pending · **Depends:** 03 · **Effort:** ~0.25d

Màn onboarding thôi tự xưng là Zed, và thôi mang dấu hiệu của Zed.

## Key Insights

- **Vì sao phase này đứng cuối:** đổi tên trong code sắp bị xoá là công toi. Sau phase 01–03, mọi
  chuỗi còn lại đều là chuỗi sống sót.
- **Bỏ logo là quyết định pháp lý, không phải thẩm mỹ.** Mã nguồn Zed theo GPL-3.0, nhưng **logo là
  nhãn hiệu và không nằm trong phạm vi đó**. Fork mang tên khác mà vẫn dùng dấu hiệu của Zed là vấn
  đề khác hẳn việc trông xấu. Đó là lý do phương án "giữ logo, chỉ đổi chữ" bị loại.
- **`VectorName::ZedLogo` dùng ở ĐÚNG hai chỗ:** `onboarding.rs:290` và
  `workspace/src/welcome.rs:426`. Yêu cầu chỉ nói màn onboarding — nhưng để lại logo Zed ở màn
  welcome thì cùng vấn đề nhãn hiệu vẫn còn nguyên, chỉ là ở màn khác. **Bỏ cả hai.**
- Sau khi bỏ cả hai, `VectorName::ZedLogo` và `assets/images/zed_logo.svg` có thể thành **code chết**.
  Phải kiểm, không đoán — `crates/ui/src/components/image.rs` có test khẳng định đường dẫn của nó
  (`:178`).
- Mục telemetry (`:246, 286, 427, 432`) **sẽ bị plan `260726-1531` xoá hẳn**. Đổi chữ ở đó là công
  sẽ bị vứt — người dùng đã chọn làm, 3 chuỗi, chấp nhận.

## Requirements

**Chức năng:** Không chuỗi "Zed" nào còn trong `crates/onboarding/`; không màn hình nào của zode
hiện dấu hiệu của Zed.
**Phi chức năng:** Bỏ logo không được để lại khoảng trống lệch lạc — phần tiêu đề phải tự cân lại.

## Architecture

Bỏ `Vector::square(VectorName::ZedLogo, ...)` khỏi `h_flex` tiêu đề; `v_flex` chứa headline và
phụ đề trở thành con duy nhất.

## Related Code Files

**Sửa:** `crates/onboarding/src/onboarding.rs`, `crates/onboarding/src/basics_page.rs`,
`crates/workspace/src/welcome.rs`
**Có thể xoá:** `assets/images/zed_logo.svg`, `VectorName::ZedLogo` trong
`crates/ui/src/components/image.rs` — **chỉ khi** rà xong không còn ai gọi
**Không sửa:** `crates/zed/resources/app-icon*` (icon bundle — ngoài phạm vi)

## Implementation Steps

1. **Bỏ logo ở onboarding** (`onboarding.rs:290`) — xoá `.child(Vector::square(...))`. Kiểm phần
   tiêu đề còn cân: `h_flex().gap_4()` giờ chỉ còn `v_flex`, `gap_4` có thể thành thừa.

2. **Bỏ logo ở welcome** (`welcome.rs:426`) — cùng lý do. Màn này logo to hơn (`45px`) nên khoảng
   trống để lại rõ hơn; nhìn thật sau khi sửa.

3. **Đổi chuỗi trong `onboarding.rs`** — `"Welcome to Zed"` (`:294`) → `"Welcome to Zode"`.
   Phụ đề `"The editor for what's next"` là khẩu hiệu của Zed — **quyết định và ghi lại**: đổi
   thành câu của zode, hay bỏ. Để nguyên là vẫn đang mượn giọng của Zed.

4. **Đổi chuỗi trong `basics_page.rs`** — bốn chỗ: `:246`, `:286`, `:427`, `:432`.

5. **Rà toàn crate:**
   ```
   grep -rn "Zed" crates/onboarding/
   ```
   Phải **rỗng**. Còn dòng nào ⇒ xử hoặc ghi lý do giữ lại vào phase file này.

6. **Kiểm logo còn ai dùng không:**
   ```
   grep -rn "ZedLogo\|zed_logo" crates/ assets/
   ```
   Chỉ còn định nghĩa + test đường dẫn (`image.rs:178`) ⇒ xoá cả `VectorName::ZedLogo`, asset, và
   test đó. Còn nơi khác dùng ⇒ **giữ**, ghi rõ ai dùng.

## Todo List

- [ ] Bỏ logo onboarding, cân lại tiêu đề
- [ ] Bỏ logo welcome, nhìn thật khoảng trống
- [ ] `"Welcome to Zode"`
- [ ] Quyết số phận phụ đề "The editor for what's next", ghi lý do
- [ ] 4 chuỗi trong `basics_page.rs`
- [ ] `grep -rn "Zed" crates/onboarding/` — rỗng
- [ ] Quyết số phận `VectorName::ZedLogo` + asset + test `image.rs:178`
- [ ] Nhìn thật cả hai màn

## Success Criteria

- `grep -rn "Zed" crates/onboarding/` **rỗng**
- `grep -rn "ZedLogo" crates/` — rỗng, hoặc mỗi chỗ còn lại có lý do ghi lại
- `./script/clippy` sạch; `cargo test -p onboarding -p ui -p workspace` xanh
- Nhìn thật: hai màn không lệch lạc sau khi mất logo

## Test Design

**Một test cơ học đáng có**, phần còn lại kiểm bằng mắt.

Đáng có: một test khẳng định **không chuỗi "Zed" nào lọt vào `crates/onboarding/`**. Nghe thô,
nhưng nó bắt đúng cái regression thật — ai đó thêm một nhãn mới và gõ theo quán tính. Chi phí gần
bằng 0, tuổi thọ dài.

```rust
// Đọc source thay vì hành vi: "tự xưng đúng tên" không có seam runtime
// nào để quan sát, mà lại là thứ trôi đi âm thầm nhất.
#[test]
fn the_crate_never_says_zed() { ... }
```

Kiểm bằng mắt (không giả vờ có test): hai màn còn cân đối sau khi mất logo. Bố cục không đo được
bằng `assert`.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Bỏ logo để lại khoảng trống lệch | Bước 1, 2 + todo nhìn thật cả hai màn |
| Xoá `VectorName::ZedLogo` khi còn nơi khác dùng | Bước 6 bắt grep trước và ghi kết luận |
| Giữ phụ đề của Zed vì tưởng là chữ trung tính | Bước 3 bắt quyết định tường minh |
| Sót chuỗi ở nơi ít ai đọc | Test cơ học ở Test Design |
| Đụng icon app trong bundle | Related Code Files ghi rõ "không sửa" |

## Security Considerations

Không có. Nhãn hiệu là vấn đề pháp lý, đã xử ở Key Insights.

## Next Steps

Mở khoá phase 05.
