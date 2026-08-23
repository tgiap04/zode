# Phase 02 — Actions dùng chung + menu chuột phải trên rail

## Context Links

- Phase 01 (`project_presentation`, `set_project_initials`)
- `crates/sidebar/src/rail.rs:127-216` — `render_rail_item`, chỗ avatar được vẽ
- `crates/sidebar/src/context_menu.rs` — menu ellipsis 2 mục đã có + `stable_id_for_group`
- `crates/workspace/src/multi_workspace.rs:1204` — `remove_project_group`
- `crates/workspace/src/multi_workspace.rs:1246-1286` — `open_project_group_in_new_window`
  (**gọi `remove_project_group` ở dòng 1269**)
- `crates/agent_ui/src/session_history/actions.rs:133` — khuôn `window.prompt` xác nhận
- `crates/agent_ui/src/session_history/row.rs` — khuôn `right_click_menu` + `ContextMenu`

## Overview

- **Priority:** P1 · **Status:** **done** (23/08) · **Phụ thuộc:** 01
- Menu chuột phải trên avatar với 5 mục hoạt động, cộng một module actions mà cả hai menu
  dùng chung. Mục "Đổi màu…" **chưa có** ở phase này (phase 05).

## Key Insights

- `right_click_menu` là **đúng** wrapper ở đây — chuột phải là đúng cử chỉ người dùng yêu
  cầu. Bẫy ngược lại vừa trả giá hôm nay ở `session_history/row.rs`: dùng `right_click_menu`
  cho nút ba chấm (cần click trái) thì menu không mở *và* click rơi xuống handler của hàng.
  Ở đây avatar có `on_click` (đổi project); `right_click_menu` không đụng vào click trái.
- `ContextMenu`, không `PopoverMenu` với view tự viết. Panel menu hiện tại dùng
  `PopoverMenu` + `ContextMenu` — đúng, vì trigger của nó là nút ellipsis click trái.
- `open_project_group_in_new_window` **gỡ group khỏi window hiện tại** trước khi mở cửa sổ
  mới. Nên nó phải hỏi xác nhận, cùng hộp với Remove — không phải vì nó nguy hiểm hơn, mà
  vì nó *là* Remove cộng thêm một bước.
- `rail.rs` đang 291 dòng; thêm menu + (phase 03) kéo thả vào `render_rail_item` sẽ vượt
  200 dòng cho một hàm vẽ. Tách `render_rail_item` sang `rail_item.rs` **trong phase này**,
  để phase 03 có chỗ làm việc mà không phải tách giữa dòng.
- Nhập chữ viết tắt cần một ô text. Rail không có chỗ. Modal nhỏ với một `Editor` một
  dòng là khuôn có sẵn trong cây (xem `rename` của project panel / tab rename).

## Requirements

**Functional**

- FR7 — Chuột phải vào avatar project trên rail mở `ContextMenu`.
- FR8 — Mục `Remove Project`: hỏi xác nhận nêu **tên project**, rồi `remove_project_group`.
- FR9 — Mục `Open Project in New Window`: hỏi **cùng hộp xác nhận đó** (nói rõ project sẽ
  rời window này), rồi `open_project_group_in_new_window`. Chỉ hiện khi
  `key.host().is_none()` và có ≥ 2 group — đúng điều kiện menu panel đang dùng.
- FR10 — Mục `Reveal in Finder`: `cx.reveal_path` với path đầu của
  `key.path_list().ordered_paths()`. Path không tồn tại → mục **disabled** kèm lý do.
- FR11 — Mục `Copy Project Path`: path đầy đủ vào clipboard. Nhiều worktree → nối bằng
  newline, không phải chỉ cái đầu (nói dối về một project 3 folder).
- FR12 — Mục `Đổi chữ viết tắt…`: mở modal một dòng, giá trị hiện tại điền sẵn, Enter
  lưu qua `set_project_initials`, Esc bỏ. Để trống + Enter → trở về mặc định.
- FR13 — Avatar vẽ theo `project_presentation`: chữ viết tắt tự đặt thắng
  `project_initials(label)`; màu tự đặt thắng `element_background`, và chữ dùng
  `label_colour_for` khi có màu tự đặt.
- FR14 — Menu ellipsis ở panel **giữ đúng 2 mục** như hiện tại, nhưng hai handler của nó
  gọi sang `project_actions.rs` — không còn bản logic riêng.
- FR15 — Mục nào không dùng được thì **disabled kèm lý do**, không ẩn.

**Non-functional**

- `project_actions.rs` không giữ tham chiếu vào state của sidebar: handler chạy sau, ngoài
  lease. Nhận `ProjectGroupKey` đã clone.
- Không có mục menu nào là nút chết. Chưa làm được thì chưa có mặt (màu, phase 05).

## Architecture

```
sidebar/src/
  project_actions.rs   mọi handler: remove, open-in-new-window, reveal, copy, set-initials
                       + hộp xác nhận. Chỗ DUY NHẤT biết multi_workspace/clipboard/prompt.
  rail_item.rs         render_rail_item (tách từ rail.rs) + right_click_menu
  context_menu.rs      menu ellipsis của panel — giữ 2 mục, gọi project_actions
```

Hai menu, một bộ handler. Đó là cách tôn trọng quyết định "hai menu riêng" mà không để
hành vi rẽ nhánh: danh sách mục khác nhau là ý muốn, `Remove` làm hai việc khác nhau thì
là lỗi.

Hộp xác nhận là **một** hàm trong `project_actions.rs` nhận (tiêu đề, chi tiết, nhãn nút)
— Remove, Open-in-New-Window và (phase 03) kéo-ra-ngoài cùng gọi nó.

## Related Code Files

**Tạo mới**
- `crates/sidebar/src/project_actions.rs`
- `crates/sidebar/src/rail_item.rs`

**Sửa**
- `crates/sidebar/src/rail.rs` — bỏ `render_rail_item`, gọi sang `rail_item.rs`
- `crates/sidebar/src/context_menu.rs` — hai handler gọi `project_actions`
- `crates/sidebar/src/sidebar.rs` — khai báo hai module mới

## Implementation Steps

1. Tách `render_rail_item` sang `rail_item.rs`, **không đổi hành vi**; test hiện có phải
   xanh nguyên.
2. `project_actions.rs`: hàm xác nhận dùng chung + `remove_project`, `open_in_new_window`.
3. `context_menu.rs` gọi hai hàm đó; xoá logic trùng. Test: menu panel vẫn ra đúng 2 mục.
4. `right_click_menu` trên avatar với `Remove` + `Open in New Window`; định danh bằng
   `stable_id_for_group`, không bằng index.
5. `reveal` + `copy_path` (+ disabled khi path không còn).
6. Modal đổi chữ viết tắt + gọi setter phase 01.
7. FR13: avatar đọc `project_presentation`.
8. Test dispatch/click: chuột phải ra menu, và mục `Remove` gọi tới đúng project khi list
   có 3 group.

## Todo List

- [x] Tách `rail_item.rs`, test cũ xanh nguyên
- [x] `project_actions.rs` + hàm xác nhận dùng chung
- [x] `remove_project` / `open_in_new_window` (cả hai đều hỏi)
- [x] `context_menu.rs` gọi sang, xoá logic trùng, vẫn 2 mục
- [x] `right_click_menu` trên avatar, id theo `stable_id_for_group`
- [x] Reveal + Copy Path (nhiều worktree → nhiều dòng), disabled khi path chết
- [x] Modal đổi chữ viết tắt
- [x] Avatar đọc `project_presentation` (chữ + màu + màu chữ)
- [x] Test: chuột phải ra menu (probe `MENU_ITEM-…`)
- [x] Test: menu của avatar thứ 2 trong 3 gọi đúng project
- [x] clippy + `cargo test -p sidebar -p workspace`

## Success Criteria

1. Chuột phải avatar → menu; click trái **vẫn** đổi project (không bị menu cướp).
2. `Remove` trên avatar và trên ellipsis panel gọi **cùng một hàm** (kiểm bằng đọc code:
   không còn hai đường gọi `remove_project_group` từ sidebar).
3. Bấm Cancel trong hộp xác nhận → project còn nguyên trong rail.
4. Project 3 worktree → `Copy Project Path` cho 3 dòng.
5. Đặt chữ viết tắt `ZO` cho project tên `examio_be` → avatar hiện `ZO`, tooltip vẫn path.
6. Đặt màu tối → chữ trên avatar sáng, đọc được.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| `right_click_menu` cướp click trái làm mất chức năng đổi project | Test khẳng định click trái vẫn `activate_or_open_workspace_for_group` |
| Menu mở rồi list reorder (phase 03) → menu tác động sai project | `stable_id_for_group` + handler giữ `ProjectGroupKey` đã clone, không index |
| Handler reach vào workspace dưới lease → abort | Handler chạy từ callback của `ContextMenu`, nhận key đã clone; không đọc lại sidebar state |
| Tách file làm lệch hành vi | Bước 1 là refactor thuần, test cũ là bằng chứng |
| Modal chữ viết tắt không có chỗ nào đóng | Esc + click ra ngoài, theo khuôn modal có sẵn |

## Security Considerations

- `Copy Project Path` đưa path thật vào clipboard — đúng yêu cầu, nhưng không kèm gì khác
  (không token, không tên máy remote nếu người dùng không thấy nó trên UI).
- Chữ viết tắt là text người dùng: chỉ đi vào `Label`, không vào path/lệnh/id.
- Remove và Open-in-New-Window là hai hành vi phá huỷ duy nhất; cả hai qua một hộp xác
  nhận nêu tên project.

## Next Steps

Phase 03 thêm kéo thả vào `rail_item.rs` vừa tách. Phase 05 thêm mục màu.

## Ghi chú khi làm xong

- **FR9 và FR15 xung đột nhau, test phát hiện.** FR9 nói "chỉ hiện khi ≥2 group", FR15 nói
  "không dùng được thì disable chứ đừng ẩn". Phân xử: FR15 dành cho mục *tạm thời* không
  dùng được (Reveal khi path đã mất → disabled). "Open in New Window" với một project không
  phải tạm thời — cửa sổ duy nhất nó chuyển tới là chính cửa sổ này, tức không có hành động
  nào tồn tại → **ẩn**. Lý do ghi trong code tại chỗ quyết định.
- Tách thêm `project_menu.rs` ngoài plan: `rail_item.rs` chạm 282 dòng khi mang cả menu.
- **`cx.listener` không dùng được trong trigger closure** của `right_click_menu`: closure là
  `Fn`, dựng lại mỗi frame, còn listener chỉ trao được một lần. Dùng `WeakEntity` + `update`.
- **`cx.theme().colors()` là tham chiếu** vay từ `cx`; closure `'static` không giữ được, nên
  bốn màu được copy ra biến trước.
- Test khẳng định **cả hai** phía cùng một selector: `MENU_ITEM-Remove Project` phải *có*
  sau chuột phải và *không có* sau chuột trái. Một selector viết sai không thể làm cả hai
  assertion cùng đúng, nên chúng bảo chứng cho nhau.
- Bằng chứng DRY không phải lời hứa: `grep remove_project_group crates/sidebar/src/*.rs` chỉ
  còn `project_actions.rs` (và một test cũ).

### Hồi quy layout do tôi gây ra, người dùng phát hiện, đã sửa

Bọc hàng avatar vào `right_click_menu` làm **hàng co từ 48px xuống 32px** và dán sát lề
trái: element đó `request_layout` bằng `gpui::Style::default()` và **không impl `Styled`**,
nên nó shrink-wrap theo con, và `w_full` của hàng resolve về đúng 32px của ô vuông.

Ba triệu chứng cùng một nguyên nhân: ô vuông thôi căn giữa trong cột, nền hover thôi trải
hết hàng, và vạch chỉ project đang mở (`left_0`) nằm ngay mép cửa sổ.

Sửa: chiều rộng tường minh `.w(RAIL_WIDTH)` thay `w_full` — hàng không còn phụ thuộc vào
việc parent có chịu style hay không.

Test giờ khẳng định hàng **trải hết chiều rộng rail và bắt đầu ở mép rail**, đo trên frame
thật. Falsify: `left: 32px right: 48px`. Không assertion trạng thái nào thấy được lỗi này —
đúng loại hồi quy chỉ hình học bắt được.

