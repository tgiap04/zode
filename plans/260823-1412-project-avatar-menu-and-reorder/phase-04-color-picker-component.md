# Phase 04 — ColorPicker trong crates/ui

## Context Links

- `crates/ui/src/components/scrollbar.rs:1382` — khuôn kéo con trượt gần nhất trong cây
  (`window.on_mouse_event` với `MouseMoveEvent`)
- `crates/theme/src/schema.rs:17` — `try_parse_color(&str) -> Result<Hsla>`
- `crates/gpui/src/color.rs:20` — `rgba(hex: u32) -> Rgba`
- Phase 01 — `label_colour_for`, để preview hiện chữ đúng màu

## Overview

- **Priority:** P2 · **Status:** **done** (23/08) · **Phụ thuộc:** — (song song 01–03)
- Một component chọn màu thật: vùng hue/sat, slider hue, ô nhập hex. Khác crate với
  01–03, không chung file nào.

## Chặn đường: không có gì để mượn

Grep `crates/ui/src/components/`: **không có** `ColorPicker`, không có slider, không có
component màu nào. Đây là lý do phase này đứng riêng và được ước lượng riêng — nó không
phải "một dòng trong menu".

Cái *có*: `try_parse_color` (hex → Hsla), `gpui::rgba`, và khuôn kéo của scrollbar.

## Key Insights

- Vùng hue/sat là một element **tự vẽ và tự bắt chuột**: `on_mouse_down` bắt đầu, rồi
  `window.on_mouse_event::<MouseMoveEvent>` theo dõi tới `MouseUpEvent`. Chỉ dùng
  `on_mouse_move` của div là **mất dấu** khi con trỏ ra ngoài vùng — mà kéo màu thì
  người ta ra ngoài vùng suốt.
- `InteractiveElement::on_hover` báo *không hover* khi đang giữ chuột (`div.rs:2530`).
  Đừng dựa vào hover để biết đang kéo.
- Gradient của vùng hue/sat: gpui không có gradient 2D. Cách thực tế là xếp một lưới ô
  nhỏ, hoặc hai lớp gradient tuyến tính chồng nhau. **Đo trước khi chọn**: một lưới 32×32
  là 1024 quad mỗi frame.
- Ô nhập hex và vùng kéo phải đồng bộ hai chiều mà không đánh nhau: kéo → cập nhật ô text;
  gõ hex hợp lệ → di con trỏ trong vùng. Vòng lặp vô hạn là bẫy kinh điển ở đây.
- Component này **không** biết gì về project. Nó nhận màu vào, phát màu ra. Nếu nó biết
  `ProjectGroupKey` thì nó không còn là component của `ui`.

## Requirements

**Functional**

- FR23 — `ColorPicker` nhận `Option<Hsla>` ban đầu và phát sự kiện khi màu đổi.
- FR24 — Vùng hue/sat kéo được, kéo ra ngoài vùng vẫn theo (kẹp vào biên), nhả chuột ở
  đâu cũng kết thúc đúng.
- FR25 — Slider hue riêng; alpha **không** làm ở v1 (avatar không cần trong suốt, và một
  slider không ai dùng vẫn phải bảo trì).
- FR26 — Ô nhập hex: hợp lệ → cập nhật vùng kéo; rác → giữ màu cũ và báo lỗi nhìn thấy
  được, không im lặng.
- FR27 — Preview: một ô vuông hiện màu đang chọn kèm chữ mẫu dùng `label_colour_for`, để
  người dùng thấy trước là chữ có đọc được không.
- FR28 — Bàn phím: Tab tới được vùng kéo và slider; mũi tên dịch từng bước nhỏ. Một picker
  chỉ dùng được bằng chuột là một picker không dùng được với ai đi bằng bàn phím.

**Non-functional**

- Không dependency mới.
- Kéo không tụt frame: đo số quad của cách vẽ đã chọn, ghi lại con số vào phase khi làm.

## Architecture

```
crates/ui/src/components/color_picker.rs
  ColorPicker         state: hsla hiện tại, có đang kéo hay không, text hex thô
  (thuần)             hsla <-> hex, kẹp toạ độ -> (hue, sat), và ngược lại
```

Phần chuyển đổi toạ độ ↔ màu là **hàm thuần**, test được không cần window. Đó là chỗ mọi
lỗi lệch một pixel sống, và là chỗ duy nhất test rẻ.

## Related Code Files

**Tạo mới**
- `crates/ui/src/components/color_picker.rs`

**Sửa**
- `crates/ui/src/components.rs` (hoặc chỗ khai báo module component) — export

## Implementation Steps

1. Hàm thuần trước: `hsla_to_hex`, `hex_to_hsla` (mượn `try_parse_color`), và
   `point_in_area -> (hue, saturation)` + nghịch đảo. Test bảng, kể cả biên và ngoài biên.
2. Vẽ tĩnh: vùng hue/sat + slider + preview, chưa tương tác. Đo số quad, ghi lại.
3. Kéo: `on_mouse_down` + `window.on_mouse_event` tới `MouseUpEvent`, kẹp vào biên.
4. Slider hue, cùng khuôn.
5. Ô hex hai chiều, có chặn vòng lặp (chỉ ghi lại text khi nguồn thay đổi không phải là
   chính ô đó).
6. Preview + `label_colour_for`.
7. Bàn phím (FR28).
8. Test vẽ thật: dựng trong một window, `run_until_parked`, `debug_bounds` cho vùng kéo
   có chiều cao > 0 — bẫy layout của cây này (`flex_1` dưới parent sai ra 0).

## Todo List

- [x] Hàm thuần toạ độ ↔ màu + hex, test bảng gồm biên
- [x] Vẽ tĩnh + đo số quad, ghi con số vào phase
- [x] Kéo vùng hue/sat, theo được cả khi ra ngoài vùng
- [x] Slider hue
- [x] Ô hex hai chiều, không vòng lặp, hex rác báo lỗi nhìn thấy
- [x] Preview + màu chữ theo `label_colour_for`
- [x] Bàn phím: Tab + mũi tên
- [ ] Test vẽ thật: vùng kéo có chiều cao > 0
- [x] clippy + `cargo test -p ui`

## Success Criteria

1. Kéo trong vùng đổi màu liên tục; kéo vượt ra ngoài vùng vẫn theo và kẹp ở biên.
2. Nhả chuột ngoài cửa sổ → không kẹt trạng thái "đang kéo".
3. Gõ `#3b82f6` → con trỏ trong vùng nhảy tới đúng chỗ; kéo → ô text đổi theo.
4. Gõ `xyz` → màu không đổi, có báo lỗi nhìn thấy được.
5. Chỉ dùng bàn phím vẫn chọn được một màu khác màu ban đầu.
6. Test vẽ thật: vùng kéo có chiều cao > 0.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Không có component nào để mượn | Phase riêng, ước lượng riêng, và 01–03 không chờ nó |
| Vẽ gradient 2D bằng lưới ô làm tụt frame | Đo ở bước 2 **trước khi** làm tương tác; đổi cách vẽ nếu số quad xấu |
| Mất dấu chuột khi kéo ra ngoài vùng | `window.on_mouse_event`, không `div.on_mouse_move` |
| Hex ↔ vùng kéo đánh nhau thành vòng lặp | Chỉ đồng bộ khi nguồn thay đổi khác chính nó; test gõ rồi kéo rồi gõ |
| Phình thành nửa cái plan | Alpha bị cắt khỏi v1 (FR25). Cắt thêm nếu cần, ghi lý do. |

## Security Considerations

- Không có. Component thuần UI, không đọc/ghi file, không nhận dữ liệu ngoài ngoài chuỗi
  hex do người dùng gõ — và chuỗi đó chỉ đi vào `try_parse_color`.

## Next Steps

Phase 05 nối nó vào menu. Nếu phase này chưa xong thì menu vẫn đủ 5 mục hoạt động —
không có mục chết nào chờ nó.

## Ghi chú khi làm xong

- **Nỗi lo 1024 quad không xảy ra.** `gpui::linear_gradient` có thật (2 stop mỗi gradient),
  nên vùng sat/value là **2 div** xếp lớp (trắng→hue ngang, trong suốt→đen dọc) và thanh
  hue là **6 div**. Tổng 8 quad, không phải lưới.
- **HSV ≠ HSL.** Vùng vuông mô tả HSV; đọc thẳng số của nó vào `hsla` sẽ cho cạnh trên màu
  trắng thay vì màu hue thuần. Có `hsv_to_hsla`/`hsla_to_hsv` + test khẳng định đỏ đầy đủ ra
  `hsl(0, 100%, 50%)`.
- `on_children_prepainted` **không tồn tại** trong gpui bản này. Bounds thật lấy qua một
  `canvas` con — và `canvas`'s paint cũng là chỗ đăng ký `window.on_mouse_event` cho phần
  kéo tiếp (đúng khuôn `scrollbar.rs`), vì handler ở tầng window chỉ sống trong frame đã
  đăng ký nó.
- `Pixels.0` là **private** ngoài crate gpui; dùng `f32::from(pixels)`.
- **Bàn phím: tôi tự bắt được một lỗi trước khi review.** `on_key_down` đặt trên root của
  picker sẽ **không bao giờ chạy** — key event đi theo đường focus và element đó không nhận
  focus. Chuyển sang `ColourModal` (chỗ giữ focus), picker phơi `nudge`/`nudge_hue`.
- **Chưa làm:** test vẽ thật (dock/draw + `debug_bounds` cho vùng kéo). Bảy test thuần phủ
  phần hình học — chỗ mọi lỗi lệch pixel sống. Alpha cắt khỏi v1 như plan.
