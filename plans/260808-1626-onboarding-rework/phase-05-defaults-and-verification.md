# Phase 05 — Mặc định cỡ chữ, và kiểm chứng chạy thật

> ## ⚠ Sửa sai sau khi báo xong (2026-08-08)
>
> Bản vẽ chỉ định **một** file: `assets/settings/default.json`. Sai. File thật sự quyết định
> một cài mới nhìn thấy gì là `assets/settings/initial_user_settings.json` — nó được **gieo vào**
> `settings.json` của người dùng lần chạy đầu, và **mọi khoá trong đó đè lên** `default.json`.
> Nó đang ghi cứng `ui_font_size: 16`, `buffer_font_size: 15`, `theme: One Light/One Dark`.
>
> Hệ quả: đổi mặc định ở đây **và** ở plan `260807-1300` (theme Dark/Light 2026) chưa từng
> tới tay ai. Người dùng phát hiện, không phải test — vì không test nào đọc file gieo.
>
> **Đã sửa:** seed thành `{}` (đúng khuôn `initial_local_settings.json` repo đã dùng sẵn), khoá
> bằng test `the_seeded_user_settings_override_nothing` trong `crates/settings/src/settings.rs`.
>
> **Lỗi kéo theo:** ghi khoá đầu tiên vào object rỗng làm nát khối comment đầu file — hoisted vào
> trong ngoặc, gộp một dòng, và URL bị cắt tại `//` của `https://`. Lỗi có sẵn ở
> `settings_json.rs`, xưa nay ẩn vì `initial_local_settings.json` cũng rỗng nhưng ít ai nhìn.
> Đã sửa + test `first_key_into_an_empty_object_keeps_the_comments_intact`.
>
> **Còn nợ:** bước 7 (changelog) chưa làm — `docs/project-changelog.md` không tồn tại.

## Context Links

- Tiêu chí gốc: [`plan.md`](plan.md) § "Định nghĩa hoàn thành"
- **Depends:** 01, 02, 03, 04 — tất cả

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** 01–04 · **Effort:** ~0.25d

Đổi hai giá trị mặc định, rồi mở màn onboarding thật xem nó ra sao.

## Key Insights

- **Đổi `default.json` là thay đổi mọi người dùng zode thấy ngay**, không phải tinh chỉnh nội bộ.
  Đây là lần thứ hai plan trong repo này chạm mặc định nhìn thấy được (lần đầu: theme mặc định,
  plan `260807-1300`) — cùng loại rủi ro, cùng cách xử: ghi changelog.
- **`ui_font_size: 13` là con số cần nhìn tận mắt.** Người dùng đã chọn "cả hai → 13" sau khi được
  cảnh báo. 13px cho *chrome* (tab, sidebar, status bar) trên màn lớn là khá nhỏ. Nếu nhìn thật
  thấy khó chịu thì **báo lại**, đừng tự sửa — đó là quyết định của họ.
- **Chỉ có một đường tới màn onboarding.** `main.rs:1201` cho thấy nhánh onboarding nằm sau
  `else if`; truyền bất kỳ đường dẫn nào trên dòng lệnh là nó **không bao giờ chạy tới**. Nên
  `make dev` (truyền `.`) sẽ **không** mở màn này. Bắt buộc `make dev PROJECT=`.
- Phase này **không sở hữu file code nào** ngoài `default.json`. Phát hiện lỗi thì trả về phase
  tương ứng, không vá tại chỗ.

## Requirements

**Chức năng:** `buffer_font_size` và `ui_font_size` mặc định là 13.
**Phi chức năng:** Mọi kiểm chứng phải **lặp lại được**, không phải "tôi thấy ổn".

## Architecture

Không có. Hai giá trị JSON, và một vòng kiểm chứng bằng mắt.

## Related Code Files

**Sửa:** `assets/settings/default.json` (`:39`, `:69`), `docs/project-changelog.md`
**Không sửa:** không file code nào

## Implementation Steps

1. **Đổi hai giá trị** — `buffer_font_size` 15 → 13 (`:39`), `ui_font_size` 16 → 13 (`:69`).

2. **Cổng tự động:**
   ```
   ./script/clippy
   cargo test -p onboarding -p settings -p settings_ui -p ui -p workspace -p zode
   ```
   Chạy **không lọc**. `cargo test -p X <filter>` in `N filtered out` mà vẫn thoát 0 — đọc dòng đó
   trước khi kết luận crate đã xanh.

3. **Rà sót toàn kho:**
   ```
   grep -rn "Zed" crates/onboarding/
   grep -rn "VsCodeSettings\|vscode_import" crates/
   ```
   Cả hai phải rỗng. Còn dòng nào ⇒ trả về phase 04 hoặc 01.

4. **Dựng và mở màn onboarding thật:**
   ```
   make build
   make reset-all FORCE=1
   make dev PROJECT=
   ```
   `PROJECT=` để rỗng là **bắt buộc** — xem Key Insights.

5. **Đối chiếu bằng mắt, từng mục:**
   1. Tiêu đề ghi "Welcome to Zode", **không có logo**, phần đầu trang không lệch
   2. **Không** còn mục nhập từ VS Code / Cursor
   3. Phần theme: chỉ còn nút Light/Dark/System, **không** còn ô xem trước
   4. Section font: đổi font UI → giao diện đổi **ngay**; đổi font editor → chữ code đổi ngay;
      đổi cỡ chữ → cả hai đổi
   5. Không chữ "Zed" nào trên màn hình
   6. Cỡ chữ mặc định trông có dùng được không — **báo lại nếu thấy nhỏ**, đừng tự sửa

6. **Kiểm cảnh hỏng** — điều mà test không dựng được: xoá theme khỏi registry là chuyện của test
   phase 02. Ở đây kiểm cảnh thật gần nhất: mở lại lần hai (`make dev PROJECT=`) và xác nhận màn
   onboarding **không hiện lại** (`first_open` đã ghi).

7. **Changelog** — ghi vào `docs/project-changelog.md`: đổi mặc định cỡ chữ là **thay đổi nhìn thấy
   ngay**, và ba điểm conflict mới khi rebase (`vscode_import.rs` đã xoá, `mod components` thành
   `pub`, `VectorName::ZedLogo` nếu đã xoá).

## Todo List

- [ ] `buffer_font_size` → 13, `ui_font_size` → 13
- [ ] `./script/clippy` sạch
- [ ] Test 6 crate, **không lọc**, đọc dòng `filtered out`
- [ ] Hai lệnh grep rà sót — rỗng
- [ ] `make build` + `make reset-all FORCE=1` + `make dev PROJECT=`
- [ ] Sáu mục đối chiếu bằng mắt
- [ ] Mở lần hai — onboarding không hiện lại
- [ ] Changelog

## Success Criteria

Toàn bộ todo xanh. Bước 4 và 5 **không được thay bằng suy luận** — chưa mở màn onboarding thật thì
phase này chưa xong, dù mọi test có xanh.

Báo cáo phải tách rõ **đã chạy thật** và **suy luận**: bước 2, 3 là cơ học; bước 5, 6 là quan sát tay.

## Test Design

Phase này **cố ý không có test tự động**. Cái nó đo là màn hình đầu tiên người dùng nhìn thấy —
bố cục, khoảng trắng, cỡ chữ có dùng được không. Không có `assert` nào cho những thứ đó.

Điều test *đã* phủ nằm ở phase 02–04: không panic, ghi đúng khoá settings, không còn chuỗi "Zed".
Phase này phủ đúng phần còn lại: **nó trông thế nào**.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Chạy `make dev` (không `PROJECT=`) ⇒ **không thấy màn onboarding**, tưởng hỏng | Key Insights + bước 4 nói rõ |
| Quên `reset-all` ⇒ `first_open` đã ghi, onboarding không hiện | Bước 4 có sẵn trong chuỗi lệnh |
| Báo "xanh" từ một lần chạy đã lọc | Bước 2 bắt đọc dòng `filtered out` |
| 13px quá nhỏ mà tự ý sửa | Bước 5.6 nói rõ: **báo lại**, không tự quyết |
| Đổi mặc định mà không ai biết | Bước 7 |

## Security Considerations

Không có.

## Next Steps

Xong plan. Plan `260726-1531` khi chạy sẽ phải khảo sát lại `crates/onboarding` — số dòng đã đổi
hoàn toàn.
