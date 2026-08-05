# Research 02 — Khảo cổ git: crate `sidebar` đã bị xoá

**Câu hỏi:** UI project switcher phải viết từ đầu, hay có gì cứu lại được?

## Phát hiện: cả một crate `sidebar` từng tồn tại và bị xoá nguyên khối

```
git show c3e2ac3 --stat | grep -i sidebar
 crates/sidebar/Cargo.toml            |    77 -
 crates/sidebar/LICENSE-GPL           |     1 -
 crates/sidebar/src/sidebar.rs        |  5339 -----
 crates/sidebar/src/sidebar_tests.rs  | 11168 ----------
 crates/sidebar/src/thread_switcher.rs|   277 -
```

Commit `c3e2ac3` = `refactor!: remove auth, collab, AI and cloud subsystems (54 crates)`. Lấy lại bằng
`git show c3e2ac3^:crates/sidebar/src/sidebar.rs`.

**16.785 dòng code + test đã tồn tại và đã chạy được** cho đúng cái UI đang cần.

## Nó chứa gì

`impl WorkspaceSidebar for Sidebar` (alias của `workspace::Sidebar`) — chính cái trait hôm nay không
còn implementor nào. Các hàm render, theo tên:

| Phần dùng được | Phần phải bỏ |
|---|---|
| `render_project_header`, `render_project_header_ellipsis_menu` | `render_thread`, `thread_switcher.rs` |
| `render_sticky_header`, `render_list_entry` | `render_acp_import_onboarding` |
| `render_filter_input` (fuzzy filter) | `render_cross_channel_import_onboarding` |
| `render_recent_projects_button` | `ActiveThreadInfo`, `ThreadEntry`, `ThreadEntryWorkspace` |
| `render_remote_project_icon` | `all_thread_infos_for_workspace` |
| `render_sidebar_header`, `render_sidebar_bottom_bar`, window controls | |
| `ListEntry`, `ActiveEntry`, `SerializedSidebar`, `SidebarContents` | |
| `workspace_menu_worktree_labels`, `apply_worktree_label_mode` | |
| `connect_remote`, `root_repository_snapshots` | |

Tỉ lệ thô theo grep: 664 dòng nhắc `thread`, 162 nhắc `agent`/`acp`, 778 nhắc `workspace`/`project`.
Ước lượng phần salvage được: **~2.500–3.000 dòng**, cộng phần test tương ứng.

## Dependency: 5 trên 20 đã chết

```
DELETED: acp_thread, action_log, agent, agent_settings, agent_ui
KEPT:    platform_title_bar, recent_projects, remote_connection, feature_flags,
         menu, editor, git, fs, project, remote, settings, gpui, ui, chrono, ...
```

Toàn bộ dep của phần project-list đều còn. Chỉ phần thread mới dính crate đã chết.

## Một mảnh của sidebar vẫn còn sống trong cây code

`crates/recent_projects/src/sidebar_recent_projects.rs` — `SidebarRecentProjects::popover(...)` nhận
`window_project_groups: Vec<ProjectGroupKey>` và gọi
`multi_workspace.open_project(paths, OpenMode::Activate, ...)` (dòng 271). Đây là popover "thêm
project" mà sidebar cũ mở ra. Nó **chưa bị xoá và chưa có ai gọi** từ UI — đúng thêm một bằng chứng
nữa rằng chỉ mất tầng vỏ.

## Fork gần như không sửa `multi_workspace.rs`

```
git diff c3e2ac3^ HEAD --stat -- crates/workspace/src/multi_workspace.rs
 1 file changed, 42 deletions(-)
```

42 dòng bị xoá, đúng một hàm: `sidebar_side_context_menu` (phụ thuộc `AgentSettings`). Trait `Sidebar`
đã mang tên đó từ trước fork (`c3e2ac3^:multi_workspace.rs:117`), không phải fork đổi tên.

**Hệ quả:** phần retention/persistence trong `multi_workspace.rs` là code upstream **nguyên bản, chưa
từng bị sửa** — nó được viết để chạy cùng một sidebar, và đó là lý do nó ngủ.

## Nợ còn treo lại từ vụ xoá

- `SidebarSide` vẫn định nghĩa trong `crates/settings_content/src/agent.rs` — file settings của tầng AI
  đã chết. Không còn key JSON nào ngoài `default.json` để đặt nó.
- `multi_workspace.rs:20` vẫn import `zed_actions::agents_sidebar::ToggleThreadSwitcher`.
- Trait `Sidebar` vẫn mang `toggle_thread_switcher`, `cycle_thread`, `is_threads_list_view_active`.
- `MultiWorkspaceState` vẫn serialize `sidebar_open` và `sidebar_state`.

## Kết luận cho plan

Phase UI (Phase 7) là **salvage + strip**, không phải viết mới. Ba lợi ích:

1. Rẻ hơn hẳn so với thiết kế lại từ đầu, và có 11k dòng test làm mốc đối chiếu.
2. Giữ hình dạng gần upstream → merge upstream về sau ít đau hơn.
3. Xác nhận `MultiWorkspace` API đủ dùng: nó đã từng được drive bởi chính UI này.

Rủi ro: code salvage mang theo giả định của thời có AI (thread, ACP). Bỏ sót một nhánh là mang nợ vào
lại. Bắt buộc đọc từng hàm trước khi copy, không copy nguyên file.
