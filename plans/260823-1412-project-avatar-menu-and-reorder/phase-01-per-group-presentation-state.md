# Phase 01 — State theo group: chữ viết tắt + màu

## Context Links

- Biên bản: `plans/reports/brainstorm-260823-project-avatar-menu-and-reorder.md`
- `crates/workspace/src/persistence/model.rs:66` — `SerializedProjectGroup`
- `crates/workspace/src/multi_workspace.rs:2114` — `MultiWorkspace::serialize`
- `crates/workspace/src/multi_workspace.rs:939` — `restore_project_groups`
- `crates/theme/src/schema.rs:17` — `try_parse_color(&str) -> Result<Hsla>`

## Overview

- **Priority:** P1 · **Status:** **done** (23/08) · **Phụ thuộc:** —
- Hai field mới theo project group, persist được, không UI. Đây là phase duy nhất chạm
  persistence, và nó đi trước mọi thứ đọc nó.

## Key Insights

- `SerializedProjectGroup` là **serde JSON trong KVP**, không phải bảng SQL. Thêm field
  với `#[serde(default)]` là tương thích ngược thật sự — khác hẳn `workspaces` (đọc theo
  vị trí, thêm cột là lệch field im lặng).
- `from_group(key, expanded)` chỉ có **một** caller. Đổi thành `from_group(&ProjectGroupState)`
  gọn hơn là nối thêm hai tham số, và chặn được việc lần sau thêm field thứ ba lại phải
  đổi signature lần nữa.
- Màu người dùng chọn có thể tối thui hoặc chói. Chữ trên avatar **không được** tin vào
  `Color::Default` — phải tính từ độ sáng của màu nền. Đó là lý do phase này có một hàm
  thuần, test được, thay vì để UI tự đoán.
- Chữ viết tắt tự đặt phải giới hạn độ dài. `project_initials` (`rail.rs:31`) trả tối đa
  2 ký tự; ô avatar được vẽ cho 2 ký tự. Nhận 10 ký tự rồi vẽ tràn là lỗi của phase này,
  không phải của người dùng.

## Requirements

**Functional**

- FR1 — `ProjectGroupState` mang thêm `initials: Option<SharedString>` và
  `colour: Option<Hsla>`.
- FR2 — `SerializedProjectGroup` mang thêm `initials: Option<String>` và
  `colour: Option<String>` (hex `#RRGGBB`), cả hai `#[serde(default)]`.
- FR3 — `MultiWorkspace::serialize` ghi hai field đó; `restore_project_groups` đọc lại.
  Hex không parse được → coi như `None` kèm một dòng `log::warn!`, **không** panic.
- FR4 — `MultiWorkspace::set_project_initials(&key, Option<String>)` và
  `set_project_colour(&key, Option<Hsla>)`: cắt chữ viết tắt về tối đa 2 ký tự sau khi
  trim, chuỗi rỗng → `None` (tức trả về mặc định tính từ tên), rồi `serialize`.
- FR5 — `MultiWorkspace::project_presentation(&key) -> (Option<SharedString>, Option<Hsla>)`
  để sidebar đọc mà không cần biết cấu trúc bên trong.
- FR6 — Hàm thuần `label_colour_for(background: Hsla) -> Hsla` (hoặc `Color`): chọn chữ
  sáng/tối theo độ sáng của nền. Ngưỡng và cách tính ghi trong doc comment.

**Non-functional**

- Record KVP đã ghi trước phase này (không có hai field) phải deserialize thành công.
- Không thêm dependency mới. `theme::try_parse_color` đã có; hướng ngược lại (Hsla → hex)
  tự viết, có test round-trip.

## Architecture

State sống cùng group, **không** trong `sidebar_state: Option<String>` — đây là thuộc
tính của project, không phải của thanh sidebar. Ai mở project đó ở window khác cũng phải
thấy cùng màu, và `sidebar_state` là blob theo window.

Setter nằm trên `MultiWorkspace` vì nó là chủ sở hữu `project_groups` và là nơi duy nhất
gọi `serialize`. Sidebar chỉ gọi setter; nó không được sửa Vec đó trực tiếp.

## Related Code Files

**Sửa**
- `crates/workspace/src/persistence/model.rs` — hai field + `from_group` signature
- `crates/workspace/src/multi_workspace.rs` — `ProjectGroupState`, serialize, restore,
  ba method mới
- `crates/workspace/src/persistence/model.rs` hoặc một module nhỏ cạnh nó — `label_colour_for`
  + hex encode/decode

## Implementation Steps

1. Thêm field vào `ProjectGroupState` (mặc định `None`), `cargo check` phải xanh ngay —
   không ai construct nó ngoài `multi_workspace.rs`.
2. Thêm field vào `SerializedProjectGroup` với `#[serde(default)]`. Test: deserialize một
   chuỗi JSON **không** có hai field đó → thành công, cả hai `None`.
3. Đổi `from_group` sang nhận `&ProjectGroupState`; sửa call site duy nhất.
4. Hex encode/decode + test round-trip; hex rác → `None` + warn.
5. `restore_project_groups` đọc hai field.
6. Ba method FR4/FR5 + test: đặt chữ viết tắt 5 ký tự → còn 2; đặt chuỗi rỗng → `None`.
7. `label_colour_for` + test bảng: nền rất tối → chữ sáng, nền rất sáng → chữ tối, và
   một cặp ở giữa để ngưỡng không phải là số ngẫu nhiên.
8. Test round-trip qua persistence thật: set màu + chữ viết tắt, `serialize`, đọc lại,
   khớp.

## Todo List

- [x] `ProjectGroupState` + `SerializedProjectGroup` hai field, `serde(default)`
- [x] Test: JSON cũ (thiếu field) vẫn deserialize
- [x] `from_group(&ProjectGroupState)`, call site duy nhất sửa theo
- [x] Hex encode/decode + test round-trip + hex rác → None + warn
- [x] `set_project_initials` / `set_project_colour` / `project_presentation`
- [x] Test: chữ viết tắt bị cắt về 2, rỗng → None
- [x] `label_colour_for` + test bảng ba mức
- [x] Test round-trip qua `serialize` + `restore_project_groups`
- [x] clippy sạch, `cargo test -p workspace`

## Success Criteria

1. Một record KVP ghi bởi bản build hôm nay (không có hai field) mở lên không lỗi.
2. Đặt màu `#101010` → `label_colour_for` trả chữ sáng; `#f5f5f5` → chữ tối.
3. `set_project_initials("ABCDE")` → đọc lại ra 2 ký tự.
4. Set rồi restart (test: serialize + restore) → cả hai field còn nguyên.
5. Hex `"không-phải-màu"` trong record → group vẫn load, màu `None`, có warn.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Thêm field làm record cũ không đọc được | `#[serde(default)]` + test riêng cho JSON thiếu field |
| Màu lưu dạng `Hsla` float → round-trip lệch | Lưu **hex string**, không lưu float; test round-trip |
| Chữ viết tắt dài làm tràn ô avatar | Cắt ở setter, không ở chỗ vẽ — một chỗ chặn, mọi đường vào đều qua |
| Đổi `from_group` làm hỏng chỗ khác | Grep xác nhận đúng một caller trước khi đổi |

## Security Considerations

- Chữ viết tắt là text người dùng nhập, đi vào KVP rồi ra `Label`. Không nội suy nó vào
  path, lệnh, hay id nào. Cắt độ dài ở setter cũng là chặn record phình vô hạn.

## Next Steps

Phase 02 đọc `project_presentation` để vẽ và gọi setter từ menu.

## Ghi chú khi làm xong

- **Test bắt được một lỗi thật, không phải lỗi test.** Setter đầu tiên tìm group trong
  `project_groups` và im lặng không làm gì nếu không thấy — mà `derived_project_groups`
  **tổng hợp** group của chính window khi Vec còn rỗng, tức project vừa mở (trường hợp phổ
  biến nhất) sẽ nhận màu rồi đánh rơi. Sinh ra `presentation_state_for`; và lần sửa đầu
  (`ensure_project_group_state` vô điều kiện) lại tạo group ma cho key lạ, nên phải chặn
  bằng "key phải thuộc danh sách đang hiển thị" trước.
- `label_colour_for` dùng **độ sáng cảm nhận** (BT.601), không phải `Hsla::l`. Test có cặp
  vàng/xanh cùng `l`: đọc `l` sẽ cho cùng một màu chữ và đặt chữ trắng lên vàng.
- Hex lưu dạng **chuỗi**, không phải ba float — float round-trip qua JSON bị lệch.
- Test tương thích ngược **không** hardcode JSON: nó serialize record thật rồi *xoá* hai key
  mới. Lần đầu tôi tự viết JSON và sai hình dạng `SerializedPathList` (`order` là String,
  không phải array) — test đỏ vì lý do sai.
