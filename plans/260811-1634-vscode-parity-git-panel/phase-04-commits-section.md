# Phase 04 — Section `Commits`

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** completed · **Effort:** 3-4d · **Blocked by:** 01

Miếng **E** — bản Zode của section GitLens `COMMITS`. **Backend đã đủ, đây thuần là UI.** Phase này cũng là nơi **chứng minh cơ chế fixed-height + lazy-load** trước khi phase 05 dùng lại nó cho Graph (đối phó R1/R4 ở chỗ rẻ hơn).

## Key insights

Toàn bộ backend đã có, không phải viết mới:

| Cần | Đã có |
|---|---|
| Commit log theo branch | `LogSource::Branch(SharedString)` (`crates/git/src/repository.rs:704-709`) + `repo.get_graph_data(source, order)` |
| Field cho mỗi dòng | `GraphCommitData { sha, parents, author_name, author_email, commit_timestamp, subject }` (`:102-110`) |
| Dòng "Up to date with origin" | `Branch::tracking_status() -> Option<UpstreamTrackingStatus { ahead, behind }>` (`:217`, `:444-446`) |
| Avatar | `CommitAvatar` (`git_ui/src/commit_tooltip.rs`), đã dùng trong `file_history_view` |
| Mở commit | `CommitView` (`commit_view.rs`), đã dùng từ `render_previous_commit` |
| Tiền lệ list phân trang | `file_history_view.rs` — `uniform_list` + `PAGE_SIZE = 50` + `loading_more` + `has_more` |

**Ngoài scope (đã chốt):** dòng `Compare main with origin/main` **dạng expandable**. `LogSource` không có variant `Range`; expand cần thêm backend. Phase này chỉ làm **dòng tổng hợp ahead/behind, không expand**.

## Requirements

**Functional**
1. Section `Commits` (collapsible) đứng dưới `Changes`, nhãn mang tên branch: `Commits (main)`.
2. Dòng đầu: trạng thái so với upstream, từ `tracking_status()`:
   - `Tracked { ahead: 0, behind: 0 }` → `Up to date with origin`
   - `ahead > 0` / `behind > 0` → `↑N ↓M`
   - `Gone` → `Upstream gone`
   - `None` (không upstream) → `No upstream`
3. Mỗi commit một row: avatar + subject + thời gian tương đối. Ref name (`origin`, tên branch) hiện dạng chip khi có.
4. Click row → mở `CommitView` (đúng đường `render_previous_commit` đang dùng).
5. **Chiều cao section cố định + resize handle.** Không tự co giãn theo số commit (R1).
6. **Lazy:** chỉ gọi `get_graph_data` khi section expand lần đầu (R4). Gập lại không huỷ dữ liệu đã tải; mở lại không tải lại trừ khi repo có event.
7. Phân trang: tải `PAGE_SIZE` đầu, tải thêm khi scroll tới cuối. Theo đúng khuôn `file_history_view.rs`.
8. `LogSource::Branch` re-resolve khi HEAD đổi branch.

**Non-functional:** tải commit **không** block UI (`cx.background_spawn` → cập nhật trên foreground). Không giữ log vô hạn: cap số commit trong bộ nhớ (CLAUDE.md — không để buffer phình vô hạn).

## Architecture

```
├──────────────────────────────────┤
│ ▼ Commits (main)     ↑ ↓ ⑂ ⟳     │  ← PanelSection + toolbar
│  ☁ Up to date with origin        │  ← tracking_status()
│  👤 ⟪origin⟫ Merge PR #7…   2h    │  ← GraphCommitData + CommitAvatar
│  👤 style: format remaining…  1d  │
│  … (uniform_list, phân trang)    │
│  ╌╌╌╌╌╌ resize handle ╌╌╌╌╌╌     │  ← chiều cao cố định, kéo được
├──────────────────────────────────┤
```

State trên `GitPanel`:

```rust
struct CommitsSectionState {
    entries: Vec<GraphCommitData>,
    scroll_handle: UniformListScrollHandle,
    height: Pixels,              // persist cùng SectionCollapseState
    loading_more: bool,
    has_more: bool,
    loaded_for_branch: Option<SharedString>,   // để re-resolve khi HEAD đổi
    _load_task: Option<Task<()>>,              // drop = cancel (CLAUDE.md)
}
```

`Option<CommitsSectionState>` — `None` cho tới lần expand đầu. Đây chính là cơ chế lazy mà phase 05 sẽ dùng lại.

## Related code files

**Sửa:**
- `crates/git_ui/src/git_panel.rs` — field `commits_section: Option<CommitsSectionState>`; persist `height` cùng `SectionCollapseState` (phase 01); subscription cập nhật khi branch đổi; `Render` chèn section
- `crates/git_ui/src/git_panel/panel_section.rs` — thêm resize handle + fixed height cho `PanelSection`

**Tạo:** `crates/git_ui/src/git_panel/commits_section.rs`
**Đọc để lấy khuôn:** `crates/git_ui/src/file_history_view.rs` (phân trang, `uniform_list`, avatar), `crates/git_ui/src/commit_tooltip.rs` (`CommitAvatar`)

## Implementation steps

1. `PanelSection` nhận `height: Option<Pixels>` + resize handle. Chiều cao persist cùng `SectionCollapseState`.
2. `CommitsSectionState` + `commits_section: Option<…>` trên `GitPanel`.
3. `ensure_commits_loaded(&mut self, cx)`: nếu `None` → khởi tạo + spawn load `LogSource::Branch(current)`, `PAGE_SIZE` đầu. Task giữ trong `_load_task` để drop = cancel.
4. Gọi `ensure_commits_loaded` **chỉ** từ đường expand section, không từ `render`.
5. `render_commits_section`: dòng tracking status + `uniform_list` các commit row (avatar + subject + thời gian tương đối + chip ref name).
6. Load-more khi scroll tới cuối, theo khuôn `file_history_view.rs`.
7. Click row → `CommitView` (dùng lại đường của `render_previous_commit`).
8. Subscription: khi branch đổi (`loaded_for_branch != current`) → clear + load lại. Khi repo có commit mới → invalidate.
9. Cap số entry trong bộ nhớ; khi vượt, thôi tải thêm (`has_more = false`) hoặc cắt đầu — chọn một và ghi lý do vào comment.
10. Test: (a) section gập → **không** gọi `get_graph_data`; (b) expand → gọi đúng một lần; (c) đổi branch → load lại; (d) 4 nhánh tracking status ra đúng chữ.

## Todo

- [x] `PanelSection` có fixed height + resize handle, height persist — `fixed_height(height, handle)`
- [x] `CommitsSectionState` — **không** cần `_load_task`, xem điều 2
- [x] Lazy load chỉ từ đường expand (+ đường `update_visible_entries`, vẫn ngoài render)
- [x] Dòng tracking status, đủ 4 nhánh — tách thành fn thuần `tracking_status_label`, test trực tiếp
- [x] Commit row: avatar + subject + thời gian tương đối + chip ref name
- [ ] **Phân trang load-more — KHÔNG làm được từ `git_ui`.** Backend bỏ qua `range`; xem điều 3
- [x] Click row mở `CommitView`
- [x] Re-load khi branch đổi — và khi HEAD đổi *trên cùng branch*, xem điều 1
- [x] Cap bộ nhớ, có comment lý do — không giữ bản copy nào; nhưng cache của repo vẫn không chặn, xem điều 3
- [x] **Test: gập ⇒ không load** — khẳng định thẳng vào cache của repository
- [x] Nhãn section mang tên branch

## Kết quả (2026-08-11)

Clippy `--deny warnings` exit 0, **80/80 test xanh** (75 sau phase 03 + 5 mới). `cargo check -p git_graph` xanh.

| File | Δ |
|---|---|
| `git_panel/commits_section.rs` | +335 (mới) |
| `git_panel.rs` | +162 / −7 |
| `git_panel/panel_section.rs` | +32 |
| `git_panel/tests.rs` | +289 / −4 |

Năm test mới: 4 nhánh tracking status (fn thuần, 6 assertion); gập ⇒ **không** hỏi log (khẳng định thẳng `get_graph_data(...).is_none()`, sau khi đã vẽ panel lúc gập); expand ⇒ hỏi đúng một lần, gọi lại là no-op; invalidate **không** được bỏ qua khi tên branch không đổi; chiều cao section round-trip + bị clamp khi ngoài khoảng.

### Bốn điều phase này dạy lại cho plan

**1. `HeadChanged` xoá *cả* cache graph, kể cả khi tên branch không đổi — nên không được so tên branch để quyết định invalidate.** `Repository` có `subscribe_self`: `HeadChanged | BranchListChanged` → `initial_graph_data.clear()`. **Commit trên đúng branch đang đứng cũng phát `HeadChanged`.** Bản đầu của tôi so `loaded_for_branch == current_branch`, thấy giống nhau nên `return` → cache đã bị xoá mà marker vẫn nói "đã tải" → `ensure_commits_loaded` short-circuit **mãi mãi**, section treo ở "Loading commits…" và gập/mở lại không cứu được. Review bắt được; `git_graph.rs:1161` đã làm đúng từ trước (invalidate vô điều kiện trên `HeadChanged`). Bài học chung: **khi dùng chung một cache với module khác, đọc cách module đó xử lý event trước khi tự viết.**

**2. Cấu trúc backend khác hẳn điều "Key insights" của plan viết.** Plan lập bảng "Field cho mỗi dòng: `GraphCommitData { …, author_name, commit_timestamp, subject }`" và coi `get_graph_data` là đường lấy dữ liệu dòng. Thực tế **hai tầng**:

| Tầng | API | Nội dung |
|---|---|---|
| Topology | `Repository::graph_data(source, order, range, cx)` — **gọi là bắt đầu fetch** | `InitialGraphCommitData { sha, parents, ref_names }` — **không có** subject/author/time |
| Hiển thị | `Repository::fetch_commit_data(sha, cx)` — theo từng sha | `GraphCommitData { subject, author_name, commit_timestamp, … }` |

Cộng thêm `Repository::get_graph_data(source, order)` — đọc cache **không** kích fetch. Chính cặp `graph_data` (kích) / `get_graph_data` (không kích) là thứ làm R4 kiểm được: render **chỉ** dùng `get_graph_data`, `graph_data` chỉ gọi từ đường expand. Hệ quả tốt: không cần `_load_task` như plan thiết kế — task nằm trong cache của repository, không phải của panel, nên cũng không có chuyện "drop = cancel" phải quản.

**3. Phân trang không làm được từ `git_ui`, và cache không chặn là vấn đề thật.** `graph_data` nhận `range` nhưng **chỉ dùng để cắt phần trả về**; fetch nó khởi động chạy `git log` **không có `--max-count`** → luôn stream *toàn bộ* history của branch vào `initial_graph_data`. Nên `COMMITS_SECTION_PAGE_SIZE` là vô hiệu, và yêu cầu 7 (phân trang) + bước 9 (cap bộ nhớ) **không thể** đạt bằng cách sửa trong `git_ui`.

Đã xử phần làm được: **`commits` mặc định gập** (như `graph`), để không panel nào phải trả giá cả history khi mới mở. Trước đó phase 01 để `commits` mở sẵn — lúc đó chưa biết fetch là không chặn. Phần còn lại (`--max-count` cho fetch, evict `commit_data`) là việc ở `crates/git` + `crates/project`, ghi lại thành việc cần làm.

**4. `on_drag_move` cho `event.bounds` của element *đăng ký listener*, không phải element bị kéo.** Listener nằm ở root của panel, nên `event.bounds.top()` là đỉnh **panel**, không phải đỉnh section — tính `height = position.y - bounds.top()` sẽ lệch đúng bằng chiều cao của title row + mọi section phía trên, và với layout mặc định thì kẹp ngay vào max 600px. Đã đổi sang **resize theo delta** (`commits_resize_drag_start`), độc lập với bounds của ai. Tiền lệ `workspace.rs` (`previous_dock_drag_coordinates`) cũng làm vậy. **Phase 05 dùng lại `fixed_height` sẽ gặp lại điều này.**

### Lệch so với plan

- **Nested scroll:** `uniform_list` dùng `flex_1()` (không `h_full()`), và hộp `fixed_height` chỉ `overflow_hidden()` — **không** `overflow_y_scroll()`. Hai vùng scroll lồng nhau chính là R1; `file_history_view` cũng chỉ để `uniform_list` tự lo scroll.
- **Lỗi `git log` hiện ra UI** (`branch_log_error` → nhãn `Color::Error`) thay vì treo "Loading…". `git_graph` còn để `todo!()` chỗ này; CLAUDE.md đòi lỗi phải tới được UI.
- `fills_height()` và `fixed_height()` có `debug_assert!` chặn dùng cả hai — phase 05 là caller tiếp theo của đúng API này.
- Nhãn dòng status: `Up to date with <remote>` / `↑N ahead of <remote>` / `↓M behind <remote>` / `↑N ↓M — <remote>` / `Upstream gone` / `No upstream`. Plan chỉ ghi 4 nhánh; tách ahead-only và behind-only ra thành hai câu riêng đọc rõ hơn `↑N ↓M` với một số bằng 0.

### Còn nợ mắt người

(a) kéo resize handle có mượt và đúng chiều không (đã sửa phép tính, nhưng chưa ai kéo), (b) commit row đọc được ở panel hẹp không — avatar + chip + subject + thời gian trên một dòng 28px, (c) "Loading commits…" xuất hiện bao lâu trên repo lớn, (d) section gập mặc định có làm người dùng không biết nó tồn tại không.

## Success criteria

- Section gập: `get_graph_data` **không** chạy. Đây là điều kiện tiên quyết cho phase 05.
- Expand: commit hiện trong dưới một nhịp; UI không đứng.
- Kéo resize handle đổi chiều cao; sống qua restart.
- Đổi branch (`git checkout`) → list và nhãn cập nhật.
- 4 nhánh tracking status ra đúng chữ, kiểm bằng test chứ không bằng mắt.
- `./script/clippy` sạch, test `git_ui` xanh.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| R1 nested scroll | Chiều cao **cố định + resize**, không `flex_1`. Phase này là chỗ chứng minh cơ chế |
| R4 load mỗi lần mở panel | Lazy theo expand; có test khẳng định gập ⇒ không load |
| Log phình vô hạn | Bước 9 cap bộ nhớ, chọn chính sách rõ ràng và ghi lý do |
| `_load_task` bị drop giữa đường gây mất dữ liệu im lặng | Task lưu trong field (drop = cancel là **đúng** ý muốn khi đổi branch); nhưng lỗi phải `.log_err()`, không `let _ =` |
| Thời gian tương đối phải cập nhật | Dùng đúng cách `file_history_view` đang làm; không thêm timer riêng |

## Security

Không có bề mặt mới. `get_graph_data` là đường đọc đã tồn tại, dùng bởi `git_graph` tab.

## Next steps

Phase 05 dùng lại `PanelSection` fixed-height + khuôn lazy-load của phase này cho section Graph.
