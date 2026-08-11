# Phase 04 — Section `Commits`

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** pending · **Effort:** 3-4d · **Blocked by:** 01

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

- [ ] `PanelSection` có fixed height + resize handle, height persist
- [ ] `CommitsSectionState`, `_load_task` drop = cancel
- [ ] Lazy load chỉ từ đường expand
- [ ] Dòng tracking status, đủ 4 nhánh (up-to-date / ahead-behind / Gone / None)
- [ ] Commit row: avatar + subject + thời gian tương đối + chip ref name
- [ ] Phân trang load-more theo khuôn `file_history_view`
- [ ] Click row mở `CommitView`
- [ ] Re-load khi branch đổi
- [ ] Cap bộ nhớ, có comment lý do
- [ ] **Test: gập ⇒ không load** (đây là cái phase 05 dựa vào)
- [ ] Nhãn section mang tên branch

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
