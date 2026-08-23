# Phase 03 — Kéo thả: đổi thứ tự và kéo ra ngoài

## Context Links

- Phase 02 (`rail_item.rs`, `project_actions.rs` + hộp xác nhận dùng chung)
- `crates/workspace/src/pane.rs:2947-2989` — khuôn `on_drag` / `drag_over::<T>` / `on_drop`
  của tab bar, chỗ tham chiếu gần nhất trong cây
- `crates/workspace/src/multi_workspace.rs:966` — `project_group_keys`, thứ tự thật
- `crates/workspace/src/multi_workspace.rs:2114` — `serialize`, chỗ ghi thứ tự
- `crates/workspace/src/multi_workspace.rs:796-798` — `ensure_project_group_state`,
  bằng chứng activate không sắp xếp lại

## Overview

- **Priority:** P2 · **Status:** **done** (23/08) · **Phụ thuộc:** 02
- Kéo avatar lên/xuống để sắp lại rail, thứ tự sống qua restart; kéo ra ngoài rail thì mở
  project sang cửa sổ mới, sau khi hỏi.

## Key Insights

- Thứ tự **đã** là dữ liệu thật và **đã** được persist. Đây là lý do phase này rẻ: không
  thêm schema, không thêm cột, chỉ `Vec::remove` + `Vec::insert` rồi `serialize`.
- `project_groups.push` ở `multi_workspace.rs:2373` là `#[cfg(test)]` và chèn ở **cuối**,
  còn production `insert(0)`. Một test reorder dựng dữ liệu bằng helper đó rồi kết luận về
  hành vi thật là test đang tự nói chuyện với chính nó. Dựng bằng đường production.
- Kéo-ra-ngoài **là Remove cộng một bước** (`open_project_group_in_new_window` gọi
  `remove_project_group`). Nên nó dùng đúng hộp xác nhận của phase 02, không dựng hộp thứ hai.
- Thả *nhầm* phải rẻ để sửa: kéo trong rail không phá gì (đổi lại thứ tự là xong), còn
  kéo ra ngoài thì có hộp xác nhận. Hai mức rủi ro khác nhau, hai mức gác khác nhau.
- Cần **vạch chỉ chỗ sẽ rơi**, không chỉ đổi con trỏ. Rail là cột hẹp không có tên project;
  không có vạch thì người dùng thả mù.

## Requirements

**Functional**

- FR16 — Kéo avatar phát một payload mang `ProjectGroupKey` (không mang index).
- FR17 — Kéo qua một avatar khác vẽ **vạch chỉ** ở trên hoặc dưới nó, tuỳ con trỏ ở nửa
  trên hay nửa dưới ô.
- FR18 — Thả trong rail: `MultiWorkspace::move_project_group(&key, to_index)` đổi chỗ
  trong `Vec<ProjectGroupState>` rồi `serialize`.
- FR19 — Thả **ra ngoài** rail: hỏi xác nhận (hộp của phase 02, nội dung nói project sẽ
  rời window này), rồi `open_project_group_in_new_window`. Cancel → không đổi gì, kể cả
  thứ tự.
- FR20 — Thả vào chính chỗ cũ, hoặc thả khi chỉ có 1 project → không làm gì, **không**
  ghi state (không sinh một lần `serialize` vô nghĩa).
- FR21 — Thứ tự sau khi kéo sống qua restart.
- FR22 — Project **mới** mở vẫn vào đầu rail (`insert(0)`, hành vi hiện tại), và thứ tự
  tương đối của phần còn lại không đổi.

**Non-functional**

- Kéo không được làm rail nhảy layout: ô đang kéo giữ chỗ hoặc mờ đi, không bị xoá khỏi
  danh sách giữa lúc kéo.
- Ngưỡng "ra ngoài" phải rõ ràng, không phải lệch 1px là mở cửa sổ mới.

## Architecture

`move_project_group` nằm trên `MultiWorkspace` — nó là chủ `project_groups` và là nơi duy
nhất gọi `serialize`. Sidebar chỉ nói "cái này về vị trí kia".

Payload kéo là một struct nhỏ trong `sidebar` mang `ProjectGroupKey`. Mang key thay vì
index vì chính lúc kéo là lúc index đang thay đổi.

`rail_item.rs` giữ toàn bộ phần kéo thả; `project_actions.rs` giữ nhánh kéo-ra-ngoài (vì
nó là hành vi phá huỷ, và mọi hành vi phá huỷ ở feature này sống cùng một chỗ).

## Related Code Files

**Sửa**
- `crates/sidebar/src/rail_item.rs` — `on_drag`, `drag_over`, `on_drop`, vạch chỉ
- `crates/sidebar/src/rail.rs` — vùng thả "ngoài rail" nếu cần một hitbox riêng
- `crates/sidebar/src/project_actions.rs` — nhánh kéo-ra-ngoài
- `crates/workspace/src/multi_workspace.rs` — `move_project_group`

## Implementation Steps

1. `move_project_group(&key, to_index)` + test thuần trên `MultiWorkspace`: dựng 3 group
   **bằng đường production**, chuyển cái thứ 3 lên đầu, khẳng định `project_group_keys`.
2. Test round-trip thứ tự: kéo → `serialize` → `restore_project_groups` → thứ tự khớp.
3. `on_drag` phát payload; ô đang kéo mờ đi.
4. `drag_over` + vạch chỉ trên/dưới theo vị trí con trỏ trong ô.
5. `on_drop` trong rail → `move_project_group`. Thả vào chỗ cũ → no-op, không `serialize`.
6. Vùng ngoài rail: `on_drop` → hộp xác nhận → `open_project_group_in_new_window`.
7. Test: FR22 — sắp tay xong, thêm project mới, khẳng định nó ở đầu và **thứ tự tương đối**
   của 3 cái cũ không đổi.

## Todo List

- [x] `move_project_group` + test dựng bằng đường production (không `test_add_project_group`)
- [x] Test round-trip thứ tự qua serialize/restore
- [x] `on_drag` + ô đang kéo mờ
- [x] `drag_over` + vạch chỉ trên/dưới
- [x] `on_drop` trong rail → đổi thứ tự
- [x] Thả vào chỗ cũ / 1 project → no-op, không ghi state
- [x] Thả ra ngoài → hộp xác nhận → cửa sổ mới; Cancel không đổi gì
- [x] Test FR22: project mới vào đầu, thứ tự tương đối cũ giữ nguyên
- [x] clippy + `cargo test -p sidebar -p workspace`

## Success Criteria

1. Kéo project thứ 3 lên đầu → rail đổi ngay, và sau `serialize`/`restore` vẫn đúng.
2. Thả vào đúng chỗ cũ → không có lần ghi state nào (kiểm bằng test đếm, hoặc bằng
   `stack_state_if_changed`-style guard).
3. Thả ra ngoài + Cancel → rail y nguyên, kể cả thứ tự.
4. Sắp tay `[C, A, B]`, mở project mới `D` → `[D, C, A, B]`.
5. Kéo khi rail có 1 project → không có gì xảy ra, không panic.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Kéo lệch tay làm project bay sang cửa sổ khác | Hộp xác nhận + ngưỡng khoảng cách rõ ràng mới tính là "ra ngoài" |
| Test dựng thứ tự bằng `test_add_project_group` (chèn cuối) rồi kết luận sai | Ghi vào phase: dựng bằng đường production. Đây là bẫy đã biết trước. |
| Index đi lạc khi list đổi giữa lúc kéo | Payload mang `ProjectGroupKey` |
| `serialize` bị gọi mỗi frame khi kéo | Chỉ gọi ở `on_drop`, không ở `drag_over` |
| Vạch chỉ vẽ sai phía → thả không đúng ý | Test: con trỏ ở nửa trên ô thứ 2 → chèn TRƯỚC nó |

## Security Considerations

- Kéo-ra-ngoài là hành vi phá huỷ (gỡ group khỏi window này). Nó dùng đúng hộp xác nhận
  của Remove — cùng một bước, cùng một cửa gác.
- Không có input người dùng nào mới ở phase này; payload là `ProjectGroupKey` nội bộ.

## Next Steps

Phase 05 là mảnh cuối (mục màu). Phase 04 chạy song song, không chờ phase này.

## Ghi chú khi làm xong

- **Hộp xác nhận và hai hành vi phá huỷ chuyển sang crate `workspace`**, không nằm ở
  `sidebar` như plan viết. Lý do bắt buộc: chỗ nhận drop "ngoài rail" là gốc cửa sổ, sống
  trong `multi_workspace.rs`, và `workspace` **không thể** phụ thuộc `sidebar`. Nếu để ở
  sidebar thì có hai bản hộp xác nhận. `project_actions.rs` giờ là façade mỏng.
- `DraggedProject` cũng vì thế sống trong `workspace`.
- **Index hiển thị ≠ index lưu.** `project_groups(cx)` tổng hợp group của chính window khi
  Vec thiếu nó, nên kéo theo index hiển thị sẽ đổi chỗ sai project.
  `materialize_project_groups` hoà hai danh sách trước khi bất kỳ index có nghĩa. Test đỏ ở
  đúng chỗ này (`got ["/root_c", "/root_b"]` — thiếu `/root_a`).
- **FR17 bị thu hẹp, có lý do:** vạch chỉ là viền trên của avatar đang hover, và thả nghĩa
  là "chiếm chỗ của avatar đó". Không làm nửa-trên/nửa-dưới: `drag_over` không cho vị trí
  con trỏ, muốn có nửa thì phải thêm `on_drag_move` + state vị trí. Quy tắc một-chiều vẫn
  tới được mọi vị trí (kéo xuống item cuối = về cuối).
- **Không double-fire** khi thả lên avatar: một `on_drop` khớp ở trong gọi
  `cx.stop_propagation()` (`div.rs:2364`), nên gốc không nhận. Điều này **đọc từ code**,
  chưa có test UI chạy thật — ghi rõ vì đó là chỗ tôi sẽ tin sai nếu tự nhớ.
- Test reorder dựng dữ liệu qua `retain_workspace` (đường production, chèn đầu), không qua
  `test_add_project_group` (chèn cuối) — đúng như plan đã cảnh báo.
- **Chưa làm:** test mô phỏng kéo thả ở tầng UI. Logic đổi chỗ được test ở tầng
  `MultiWorkspace`; phần còn lại là ba dòng nối handler.

### Reviewer bắt được một High, đã sửa

**Kéo-ra-ngoài kích hoạt cả khi thả *trong* rail.** Gốc cửa sổ nhận mọi drop không rơi lên
avatar — tức khoảng trống dưới avatar cuối, block database/agents, và footer đều tính là
"ra ngoài". Nghĩa là kéo xuống cuối để đưa project về cuối (cử chỉ hoàn toàn bình thường)
lại bị hỏi "Move to new window?". "Không rơi lên avatar" đã bị dùng thay cho "đã ra khỏi
rail", và hai điều đó không giống nhau. Đúng rủi ro biên bản thiết kế đã nêu, và đúng chỗ
việc thiếu test kéo thả UI để lọt.

Sửa: cột rail có `on_drop` riêng, coi "trong rail nhưng không trúng avatar" là **về cuối**,
và nó chặn bubble nên chỉ drop thật sự ngoài rail mới tới gốc. Ngữ nghĩa "về cuối" có test
(`usize::MAX` → cuối danh sách).

### Một falsification của tôi ban đầu vô hiệu

Khẳng định "key lạ không tạo project ma" tôi falsify bằng cách thêm `ensure_project_group_state`
vô điều kiện — test vẫn xanh. Lý do: bản sửa trước đó của tôi **không** áp dụng (rustfmt đã
tách `if !self.project_groups(cx)...` thành nhiều dòng nên anchor không khớp), nên pre-check
vẫn còn và chính nó chặn. Bỏ pre-check cho thật (theo Medium 1 của reviewer) rồi falsify
lại: đỏ đúng, `["/nowhere", "/root_b", ...]`. Nếu tôi tin lần falsify đầu thì đã ship một
assertion trang trí.

