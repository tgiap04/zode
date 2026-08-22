# Phase 05 — Danh sách: group, search, hàng

## Context Links

- Phase 01 (`list()`), phase 04 (vỏ panel)
- Ảnh tham chiếu: ảnh 1 và 3 của buổi consultation (bố cục hàng, hover toolbar)
- `crates/project_panel/src/project_panel.rs` — khuôn `uniform_list` + sticky + scrollbar
- `crates/git_ui/src/git_panel.rs` — khuôn ô search trong panel
- `crates/fuzzy` — match cho search

## Overview

- **Priority:** P1
- **Status:** pending · **Phụ thuộc:** 01 + 04
- Kết quả: panel hiện 292 session thật, group theo project, search được, hàng đủ 8 dữ
  kiện như ảnh. Nút trên hàng **chưa** hoạt động (phase 06).

## Key Insights

- `uniform_list` **không có chiều cao nội tại** và `div()` là một *row*. `flex_1` trong
  một `div()` trần cho ra **0 hàng, không panic**. Parent phải là column
  (`v_flex()`), và một test `cx.draw()` chứng minh được **không có gì** — nó không
  publish frame.
- `cx.debug_bounds` luôn `None` sau `cx.draw()`. Muốn đo layout thật: dock panel, để
  `run_until_parked` vẽ cửa sổ.
- `.flex_1().self_stretch()` dưới `div()` trần ra chiều ngang đầy, chiều dọc 0. Dùng
  padding ở parent + `size_full` ở con.
- Group là **cwd đọc từ nội dung file**, không phải tên thư mục encode (`-` không tách
  ngược được). Nhãn group = phần đuôi của cwd cho đủ phân biệt (`zode`, `devs/zode`).
- `msgs`/`subagents` chỉ tính cho hàng đang render. `uniform_list` cho `visible_range`
  → đúng ~20 hàng. Chưa có số thì hiện `…`.
- Hàng Codex không có `messages` (`Option::None`) → **ẩn cột**, không hiện `0`.

## Requirements

**Functional**

- FR27 — Ô search trên cùng, placeholder `Search sessions`, filter fuzzy trên title +
  preview + branch + cwd. Rỗng → hiện hết.
- FR28 — Group theo cwd, header group có tên rút gọn + **số session** trong group,
  collapse được. Group sort theo session mới nhất trong nó.
- FR29 — Hàng hiện: title (1 dòng, ellipsis), preview 2 dòng (`Agent:`/`You:` + text),
  icon agent + tên, `N msgs`, `N subagents` (ẩn nếu 0 hoặc None), thời gian tương đối,
  model, badge worktree, chip branch.
- FR30 — Badge: `Current worktree` nếu cwd khớp một worktree của project đang mở;
  `Unavailable worktree` nếu path không còn tồn tại; không hiện gì nếu tồn tại nhưng
  không thuộc project này.
- FR31 — Chevron mở rộng hàng: preview dài (~10 dòng), session id, log path, cwd đầy
  đủ, thời điểm tạo. Trạng thái mở giữ trong RAM.
- FR32 — `msgs`/`subagents` tính lazy ở background executor cho hàng đang thấy,
  memoise theo session id trong panel.
- FR33 — Nút refresh ở header panel: re-`stat` toàn bộ, parse lại file đã đổi.
- FR34 — Panel mở lần đầu → `list()` chạy ở background, hiện spinner, không block UI.
- FR35 — Provider *unavailable* (Codex thiếu DB) → một dòng ghi chú nhỏ, không phải lỗi đỏ.

**Non-functional**

- Từ lúc panel mở tới lúc hàng đầu hiện: **< 200ms** trên máy này. Đo bằng test
  `#[ignore]`, không phỏng đoán.
- Cuộn 292 hàng không tụt frame: không đọc file nào trong `render`.

## Architecture

```
session_history/
  panel.rs   state: sessions, filter, expanded, counts cache, availability
  list.rs    group + filter thuần (không cx) → Vec<Row> ; uniform_list
  row.rs     một hàng, nhận &Row, không tự đọc gì
```

`list.rs` chứa hàm thuần `group_and_filter(&[SessionSummary], &str) -> Vec<Row>` —
test được không cần window, và là chỗ duy nhất chịu luật group/search.

`counts` cache: `HashMap<SessionId, CountState>` với `Pending`/`Ready`. `render` chỉ
đọc; hàng chưa có thì phát một task background (một lần, không phát lại khi re-render).

## Related Code Files

**Tạo mới**
- `crates/agent_ui/src/session_history/{list,row}.rs`

**Sửa**
- `crates/agent_ui/src/session_history/panel.rs` — state + gọi `list()` + render

**Đọc để hiểu**
- `crates/project_panel/src/project_panel.rs` — `uniform_list` + `visible_range` + scrollbar
- `crates/git_ui/src/git_panel.rs` — search editor trong panel

## Implementation Steps

1. `list.rs`: `Row { Group{label,count,collapsed} | Session{...} }` + `group_and_filter`.
   Test thuần trước: 5 summary trên 2 cwd, filter rỗng/khớp/không khớp, thứ tự group.
2. `panel.rs`: gọi `list()` trong background khi panel lần đầu visible, `cx.notify()`.
3. `row.rs`: bố cục hàng theo FR29, dùng `Label`/`Icon` sẵn có. Chưa nút.
4. `uniform_list` trong `v_flex()`, có scrollbar theo khuôn project panel.
5. Search editor + filter; `advance_clock` nếu có debounce.
6. Lazy counts theo `visible_range` + memoise; hiện `…` khi Pending.
7. Badge worktree: so cwd với `project.visible_worktrees()`; kiểm tồn tại qua `Fs`.
8. Chevron mở rộng.
9. Test vẽ thật: dock panel, `run_until_parked`, `debug_bounds` cho ≥ 1 hàng có chiều
   cao > 0 (bẫy `uniform_list` 0 hàng).

## Todo List

- [ ] `group_and_filter` + test thuần (group, sort, filter)
- [ ] Gọi `list()` background + spinner + notify
- [ ] Bố cục hàng đủ 8 dữ kiện
- [ ] `uniform_list` trong parent column + scrollbar
- [ ] Search editor + filter
- [ ] Lazy counts theo visible_range + memoise + `…`
- [ ] Badge worktree 3 trạng thái
- [ ] Chevron mở rộng
- [ ] Group collapse
- [ ] Nút refresh
- [ ] Ghi chú provider unavailable
- [ ] Test vẽ thật: hàng có chiều cao > 0, số hàng khớp fixture
- [ ] Test `#[ignore]` đo thời gian mở panel trên dữ liệu thật
- [ ] clippy + `cargo test -p agent_ui`

## Success Criteria

1. Panel hiện đúng số session mà `list()` trả, group theo cwd thật, group `devs/zode`
   có số đếm khớp `ls ~/.claude/projects/-Users-tgiap-dev-devs-zode/*.jsonl | wc -l`
   (trừ session bị lọc).
2. Gõ `implement` vào search → chỉ còn hàng có title/preview khớp.
3. Test vẽ thật: hàng đầu tiên có `size.height > 0` — bẫy `uniform_list` bị bắt.
4. `msgs` của hàng đang thấy hiện số thật sau < 500ms; hàng chưa thấy không sinh IO
   (đếm số lần `counts()` được gọi trong test).
5. Session có cwd đã bị xoá → badge `Unavailable worktree`.
6. Đo mở panel < 200ms tới hàng đầu.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| `uniform_list` render 0 hàng, im lặng | Test đo `debug_bounds` chiều cao > 0 trên frame thật, không `cx.draw()` |
| Cuộn nhanh → phát hàng trăm task counts | Chỉ phát cho `visible_range`, memoise `Pending`, không phát lại |
| Preview chứa markdown/ANSI của terminal | Hiện text thô, cắt ký tự điều khiển. Không render markdown ở phase này. |
| 292 hàng + search mỗi keystroke | `group_and_filter` là hàm thuần trên `Vec` sẵn trong RAM; đo nếu chậm, chưa tối ưu trước |
| Nhãn group rút gọn gây trùng tên | Rút từ đuôi cwd cho tới khi đủ phân biệt trong tập hiện có |

## Security Considerations

- Preview là nội dung hội thoại thật: không log, không ghi ra đâu, chỉ vẽ.
- Kiểm tồn tại path qua `Fs`, không `std::fs` — để `FakeFs` test được và không đụng đĩa
  thật trong test.

## Next Steps

Phase 06 gắn nút vào hàng đã có.
