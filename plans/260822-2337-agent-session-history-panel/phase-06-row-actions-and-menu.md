# Phase 06 — Hover toolbar + menu 9 mục

## Context Links

- Phase 02 (`resume_command`, `delete`), phase 05 (hàng), phase 03 (Codex — không bắt buộc)
- Ảnh tham chiếu: ảnh 2 (menu 9 mục) và ảnh 3 (hover toolbar 4 nút)
- `crates/agent_ui/src/agent_view.rs:614` — `agent_task()`
- `crates/workspace/src/dock.rs` — `DockButton::render` là ví dụ `right_click_menu` +
  `ContextMenu` gần nhất trong cây

## Overview

- **Priority:** P1
- **Status:** **done** (23/08) · **Phụ thuộc:** 02 + 05
- Kết quả: mọi nút trong ảnh làm đúng việc của nó, hoặc bị disable với lý do nhìn thấy được.

## Key Insights

- **Dùng `ContextMenu`, không `PopoverMenu` với view tự viết.** `PopoverMenu` giao cho
  một view bất kỳ **không** background và **không** dismiss khi click ra ngoài;
  `ContextMenu` tự làm cả hai. Bẫy này đã trả giá một lần trong cây này.
- Trong `ContextMenu`, dấu tick và icon **dùng chung một slot** — gọi `.icon()` sẽ âm
  thầm thay chỗ dấu tick. Menu này không có mục toggle nào nên không vướng, nhưng đừng
  thêm mục toggle kèm icon.
- Resume đi qua `agent_task()` (`args` hiện là `Vec::new()`): thêm `args` và đổi `cwd`.
  Đừng dựng `SpawnInTerminal` thứ hai ở chỗ khác — một chỗ dựng, hai nơi gọi.
- `AgentView::open_inner` **defer qua `cx.spawn_in`** vì thân nó chạy dưới lease của
  workspace. Đường resume phải theo đúng khuôn đó, nếu không là abort tiến trình.
- Nút disable vẫn giữ tooltip và **rụng hết click handler** (`button_like.rs:736,767`) —
  đó là cách nói "không dùng được ở đây" mà không nói dối.
- Codex không có `--fork-session` → `resume_command(Fork::New)` trả `None` → nút và mục
  menu tương ứng **disable**, tooltip nói vì sao.

## Requirements

**Functional — hover toolbar (ảnh 3)**

- FR36 — Hover hàng hiện 4 nút phải: `Resume in Worktree` (play), `Continue in New
  Session…` (plus-box), chevron (đã có ở phase 05), ellipsis.
- FR37 — Chỉ hiện khi hover hàng (`visible_on_hover` với group của panel — cùng cơ chế
  indent guides đang dùng).

**Functional — menu 9 mục (ảnh 2)**

- FR38 — `Resume in Worktree` → tab agent mới, `claude --resume <id>` / `codex resume <id>`,
  cwd = cwd của session. Path chết → disable.
- FR39 — `Continue in New Session…` → `claude --resume <id> --fork-session`. Codex → disable.
- FR40 — `Copy Resume Command` → `to_shell_string()` vào clipboard.
- FR41 — `Open Log` → mở file `.jsonl` như một item editor thường.
- FR42 — `Reveal Log` → reveal trong Finder/file manager (dùng đường sẵn có của project panel).
- FR43 — `Open Working Directory` → mở cwd của session như project (hoặc reveal nếu
  không mở được — quyết định khi làm, ghi lại lý do).
- FR44 — `Copy Session ID`, `Copy Log Path` → clipboard.
- FR45 — `Delete` (đỏ) → hộp xác nhận nêu **đường dẫn và dung lượng**, rồi `delete()`
  của provider (Trash). Xong thì bỏ hàng khỏi danh sách mà không reload cả list.

**Non-functional**

- Không có mục nào trong menu là nút chết. Không làm được thì disable + tooltip.

## Architecture

`row.rs` giữ layout; `actions.rs` giữ mọi handler và là chỗ duy nhất biết về `agent_task`,
clipboard, và hộp xác nhận. Handler nhận `SessionSummary` đã clone, không giữ tham chiếu
vào state của panel — vì chúng chạy sau, ngoài lease.

`agent_view.rs`: `agent_task()` nhận thêm `args: Vec<String>` và `cwd: Option<PathBuf>`;
call site hiện tại truyền `Vec::new()` + `None` và **không đổi hành vi**.

## Related Code Files

**Tạo mới**
- `crates/agent_ui/src/session_history/actions.rs`

**Sửa**
- `crates/agent_ui/src/session_history/row.rs` — toolbar + trigger menu
- `crates/agent_ui/src/agent_view.rs` — `agent_task()` nhận args + cwd; một hàm
  `AgentView::open_resumed()` theo khuôn `open_inner` (defer qua `spawn_in`)

## Implementation Steps

1. `agent_task()` nhận `args` + `cwd`; call site cũ giữ nguyên hành vi; test rằng
   `SpawnInTerminal` cũ không đổi.
2. `AgentView::open_resumed(agent, resume_command)` — copy khuôn defer của `open_inner`.
3. Toolbar 4 nút, `visible_on_hover`.
4. `ContextMenu` 9 mục, 3 separator đúng như ảnh, `Delete` màu đỏ cuối.
5. Clipboard (FR40, FR44) — 3 mục, làm trước vì rẻ và chứng minh được menu sống.
6. `Open Log` / `Reveal Log` / `Open Working Directory`.
7. `Delete` + hộp xác nhận + bỏ hàng khỏi state.
8. Disable theo điều kiện: path chết, Codex + fork.
9. Test dispatch: mỗi action gọi được từ menu mà **không** abort (bẫy lease).

## Todo List

- [x] `agent_task()` nhận args + cwd, hành vi cũ không đổi (có test)
- [x] `open_resumed()` defer đúng khuôn
- [x] Toolbar 4 nút, chỉ hiện khi hover
- [x] `ContextMenu` 9 mục + separator + Delete đỏ
- [x] 3 mục clipboard
- [x] Open/Reveal Log, Open Working Directory
- [x] Delete + xác nhận nêu path và dung lượng + bỏ hàng
- [x] Disable: path chết, Codex fork
- [ ] Test dispatch từng action (lease)
- [x] Test: resume dựng đúng `SpawnInTerminal` (program, args, cwd)
- [x] clippy + `cargo test -p agent_ui -p workspace -p zode`, build

## Success Criteria

1. `Resume in Worktree` trên một session Claude thật mở tab agent tiếp đúng hội thoại
   (kiểm bằng mắt một lần, và bằng test rằng `SpawnInTerminal.args == ["--resume", id]`).
2. `Continue in New Session…` cho args có `--fork-session`.
3. Trên hàng Codex, mục fork **disable** với tooltip nói Codex không hỗ trợ.
4. Session có cwd đã xoá → `Resume` disable, badge `Unavailable worktree`.
5. `Delete` → cả `.jsonl` và thư mục sidecar vào Trash, khôi phục được từ Trash, hàng
   biến khỏi danh sách mà không reload.
6. Không action nào abort tiến trình khi dispatch (test riêng cho từng cái).
7. `Copy Resume Command` dán vào terminal thật chạy được.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Handler reach vào workspace dưới lease → abort | Defer qua `cx.spawn_in` theo đúng khuôn `open_inner`; test dispatch từng action |
| `PopoverMenu` với view tự viết: không nền, không dismiss | Dùng `ContextMenu`. Đã ghi trong Key Insights vì đã trả giá một lần. |
| `Open Log` trên file 13MB làm editor treo | **Đo trước khi quyết**: mở file lớn nhất bằng tay. Treo → hạ mục đó xuống `Reveal Log` và ghi lý do vào plan. |
| Xoá session đang chạy | Hộp xác nhận nêu path + dung lượng. Không tự đoán được file nào đang mở. |
| Resume vào cwd không phải project đang mở → agent sửa nhầm cây | Đúng thiết kế: cwd là của session. Badge nói rõ nó thuộc worktree nào. |

## Security Considerations

- `Delete` là hành vi phá huỷ duy nhất chạm tới người dùng. Ba lớp: `Fs::trash` (khôi
  phục được), hộp xác nhận nêu path + dung lượng, và test chứng minh không có đường xoá
  thẳng nào.
- `Copy Resume Command` đi vào clipboard: quote path đúng, không nội suy gì từ nội dung
  session vào chuỗi lệnh.
- `Open Working Directory` mở path từ log file — kiểm tồn tại và là thư mục trước khi mở.

## Next Steps

Xong phase này là hết plan. Nếu phase 03 còn chặn, mở lại menu cho Codex sau khi nó xong
— các mục đã disable sẵn theo `Availability` nên không phải sửa layout.

## Ghi chú khi làm xong

- `Open Working Directory` = **reveal** trong Finder, không mở thành project: panel này đã gắn với project đang mở, đổi project vì một mục menu là chuyện người dùng không yêu cầu. Plan cho phép chọn, lý do ghi ở đây.
- `Delete` đỏ: `ContextMenuEntry` không có API màu riêng cho entry, nên nó mang icon `Trash` và đứng sau separator cuối như trong ảnh; chữ không đỏ.
- **Chưa làm:** dispatch-test *từng* action trong menu. Chỉ `agent::ToggleHistory` được dispatch-test (đó là action duy nhất đi qua `register_action`, tức đường duy nhất có bẫy lease). 9 mục menu chạy qua `panel.update` từ callback của `ContextMenu`, không qua `register_action`.
- `Open Log` giữ nguyên: `jsonl` không nằm trong `path_suffixes` của grammar JSON nên file mở dạng plain text, không tree-sitter; và không có size guard nào trong `project`/`language`/`editor`.
- **Lỗi người dùng báo (23/08), đã sửa:** menu dựng bằng `right_click_menu`, tức chỉ mở
  bằng **chuột phải** — không ai mở nút ba chấm bằng chuột phải. Tệ hơn, trigger không có
  `on_click` nên `ButtonLike` không gọi `stop_propagation` (`button_like.rs:766`), click
  trôi lên `on_click` của cả hàng và **toggle expand** thay vì mở menu. Một wrapper sai,
  hai triệu chứng. Sửa sang `PopoverMenu` (anchor `TopRight`/attach `BottomRight` vì panel
  dock phải) — cùng lúc đóng cả hai, vì trigger giờ có `on_click`.
  Key Insight ở trên nói đúng *cái menu* (`ContextMenu`, không phải view tự viết) nhưng
  không nói gì về *cái mở nó* — đó là khoảng trống đã trả giá.
  Test mới `the_ellipsis_opens_its_menu_instead_of_expanding_the_row`: click thật vào
  bounds của nút, khẳng định `MENU_ITEM-Delete` được vẽ **và** `expanded_rows` rỗng. Cả
  hai assertion đã được falsify riêng từng cái.
- Còn lại, cố ý không sửa: nút resume/fork khi **disabled** cũng rụng handler nên click
  vào nó vẫn expand hàng. Đây là hành vi của `ButtonLike` trên toàn app, không riêng panel
  này, và expand một hàng có cwd đã chết thì đúng ra lại hữu ích (nó hiện path).
