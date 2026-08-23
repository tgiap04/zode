# Phase 07 — Panel không có nền

**Status:** ✅ done (2026-08-21), chờ commit · **Priority:** P1 · **Người dùng thấy:** panel đọc được

## Người dùng báo "không có màu nên bị mờ" — nhưng nó không mờ

Ảnh cho thấy **git log và sidebar đọc xuyên qua panel**. Không phải màu nhạt, không phải
blur: panel **trong suốt hoàn toàn**, và cái trông như chữ mờ là hai lớp chữ đè lên nhau.

`UsagePanel` là `ManagedView` tự viết, được vẽ thẳng vào deferred layer — **không có gì
phía sau nó**. `ContextMenu` không bị vì nó tự gọi `.elevation_2(cx)`
(`crates/ui/src/components/context_menu.rs`). Panel của tôi không gọi gì cả.

Đây là lỗi tôi lẽ ra phải thấy: phase 03 chép cấu trúc `PopoverMenu` từ `lsp_button.rs`,
nhưng `lsp_button` trả về một `ContextMenu` — **loại view tự mang chrome**. Tôi lấy cái
khung mà không lấy phần nó dựa vào.

Và không test nào bắt được, vì bẫy đã ghi từ trước: **`cx.draw` không publish frame**, nên
không có test render nào ở đây chứng minh được cái gì về nền.

## Lỗi thứ hai, cùng ảnh, tôi chưa hề thấy

Panel hiện **`wk` hai lần**. Thanh status ngay dưới nói `0% used **Fable**`.

`window_tag` ưu tiên `kind.short_tag()` và chỉ rơi về `label` khi tag **rỗng**. Cửa sổ
`weekly_scoped` có `kind: Weekly` → tag `"wk"` (không rỗng) → nhãn `"Fable"` **không bao
giờ tới được**. Nên hai dòng liền nhau cùng mang `wk`, và tên model mất.

Comment tôi tự viết ở đó nói *"the model name earns the tag's place when the kind has
nothing short to say"* — đúng ý định, sai điều kiện. Nhãn **cụ thể hơn** kind, nên có nhãn
là nhãn thắng, không phải ngược lại.

Tệ hơn cả cái tag: panel và thanh status **nói khác nhau về cùng một cửa sổ**, mà panel
sinh ra để giải thích thanh status.

## Sửa

- `.elevation_2(cx)` trên root `v_flex()` của panel — cùng primitive `ContextMenu` dùng.
- `window_tag()` tách thành hàm thuần, **đảo thứ tự ưu tiên**: `label` thắng `kind`.

## Todo

- [x] `.elevation_2(cx)` trên panel
- [x] `window_tag()` — **`resets_at` trước**: có đếm ngược thì kind, không có mới label
      (bản đầu tôi viết "label thắng vô điều kiện" và đó là hướng sai đối diện — xem dưới)
- [x] Test: cửa sổ scoped mang tên model, và **assert_ne** với `"wk"`
- [x] Test: cửa sổ không tên rơi về kind
- [x] Test: panel và thanh status không được nói khác nhau — **cả 3 tổ hợp**
      `resets_at` × `label`, không chỉ shape fixture có
- [x] **Đảo lại logic để chứng minh test bắt được** — cả hai hướng sai: bản gốc 2/3 đỏ,
      bản "label thắng vô điều kiện" 3/3 đỏ. Phục hồi cả hai lần.
- [x] 66 xanh · clippy 0 warning · `cargo build --bin zode` exit 0
- [x] `reviewer` xem lại — **tìm ra 1 HIGH thật, xem dưới**
- [ ] commit — chờ bạn

## Bài học

**Chép một khung `PopoverMenu` không chép được thứ mà khung đó dựa vào.** `lsp_button` trả
về `ContextMenu` — view tự mang nền, viền, shadow. Một `ManagedView` tự viết thì không có
gì cả, và cái thiếu đó **không phải lỗi compile, không phải test đỏ, không phải clippy
warning** — nó chỉ hiện ra khi có người mở panel lên và nhìn.

Và bẫy `cx.draw` không publish frame làm chỗ này không test được bằng render. Cái test
được là **luật** (`window_tag`), không phải diện mạo. Nên với UI, một vòng chạy thật mắt
nhìn là bước không bỏ được — và ở phase 03–06 tôi đã bỏ nó.

## `reviewer` bắt được: tôi sửa một hướng sai rồi tạo ra hướng sai đối diện

`window_tag` bản sửa đầu của tôi nói *"label thắng kind, vô điều kiện"*. Nhưng
`crate::render_window` — hàm quyết định chữ trên **thanh status** — nói khác:

```rust
// A reset instant beats a label when both are present.
if let Some(resets_at) = window.resets_at { ...countdown... }
else if let Some(label) = &window.label { ...label... }
```

**Hai thứ tự ưu tiên khác nhau.** Tức tôi vừa sửa lỗi "panel và bar nói khác nhau" cho
Claude và mở lại đúng lỗi đó cho Codex — và đường đi nằm trong code tôi viết ở feature
trước: `codex::parse_windows` **clone một `limitName` cấp account vào cả hai cửa sổ**.

```rust
let label = limits.limit_name...;
[limits.primary, limits.secondary].into_iter().flatten()
    .filter_map(|window| window.into_usage_window(label.clone()))
```

Nên nếu một account Codex có `limitName` không null và cả hai cửa sổ đều có `resetsAt`,
panel sẽ tag **hai dòng bằng cùng một chuỗi** trong khi thanh status hiện hai đếm ngược
khác nhau. Đúng lỗi vừa sửa, chuyển sang agent khác.

Sửa: `window_tag` kiểm `resets_at` **trước** — có đếm ngược thì lấy kind, không có mới lấy
label. Khớp thanh status ở **mọi** shape, không chỉ shape fixture tình cờ có.

Cộng thêm một medium: tag slot trước đây chỉ chứa 2–3 ký tự của `short_tag()` nên không ai
lo bề rộng; giờ label vào được đó nên phải `.truncate()`.

**Lý do test cũ không bắt:** `the_panel_and_the_status_bar_name_a_window_the_same_way` chỉ
thử shape `resets_at: None` — đúng shape mà "label thắng vô điều kiện" cho kết quả đúng.
Đã viết lại nó chạy **cả ba** tổ hợp `resets_at` × `label`, thêm 2 test cho shape chưa có
fixture nào, và đảo lại logic để chứng minh: **3/3 đỏ**.

Bài học chồng lên bài học của chính phase này: sửa một lỗi hiển thị bằng cách đảo một điều
kiện là **rất dễ đi quá sang phía bên kia** — và một test viết từ fixture hiện có sẽ đồng
ý với cả hai bản sai, vì fixture chỉ có một shape.
