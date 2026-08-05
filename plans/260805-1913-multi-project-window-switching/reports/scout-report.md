# Scout report — điểm móc theo từng phase

Khảo sát cây code hiện tại (`main`, sau hard-fork). Mọi số dòng đọc tại thời điểm 2026-08-05.

## Phase 1 — Retention và cycling

| Điểm | Vị trí | Ghi chú |
|---|---|---|
| Gate retention | `multi_workspace.rs:1391-1399` trong `activate()` | `if self.sidebar_open { retain_workspace(...) }` — không bao giờ true |
| Gate detach | `multi_workspace.rs:1406-1408` trong `activate()` | `if !self.sidebar_open && !old_active_was_retained { detach_workspace(old) }` — luôn true |
| `retain_workspace` | `multi_workspace.rs:660-673` | `pub(crate)`; push vào `retained_workspaces` + emit `WorkspaceAdded` |
| `detach_workspace` | `multi_workspace.rs:1453-1485` | Xoá khỏi `retained_workspaces`, clear `session_id`, huỷ serialize task, xoá session binding trong DB (giữ row) |
| `register_workspace` | `multi_workspace.rs:695-716` | Subscribe + `set_multi_workspace` + defer `sync_sidebar_to_workspace` |
| `add()` — **luôn retain** | `multi_workspace.rs:1355-1372` | Không phụ thuộc sidebar. Gọi từ `workspace.rs:1973` (`OpenMode::Add`) và `workspace.rs:9551` (deserialize vào window đang có) |
| Cycling bị uỷ quyền | `multi_workspace.rs:2062-2085` | `NextProject`/`PreviousProject` → `sidebar.cycle_project(...)`; `sidebar` là `None` → no-op |
| Keymap | `assets/keymaps/default-macos.json:550` | `cmd-alt-j` → `multi_workspace::ToggleWorkspaceSidebar` |
| Restore | `workspace.rs:8905-8975` `apply_restored_multiworkspace_state` | Restore **group keys** (`restore_project_groups`, `multi_workspace.rs:726-751`), không phải workspace sống |
| Tests | `multi_workspace_tests.rs` (905 dòng) | Nhiều test gọi `mw.open_sidebar(cx)` trực tiếp để bật retention — sẽ phải sửa theo |

**Kết luận:** có một dead end thật đang tồn tại. `cli_default_open_behavior` mặc định
`"existing_window"` (`default.json:146`, comment ở `:141` viết thẳng "in the current Zed window's
sidebar") → `zode <dir>` thứ hai đẩy workspace mới vào window hiện tại qua `OpenMode::Add` → retained,
**không render, không có cách nào quay lại**. Phase 1 vì vậy là fix bug, không chỉ là mở đường cho
tính năng.

## Phase 2 — State machine + settings

- Layering: `workspace` phụ thuộc `project`, không ngược lại → API `set_activity` đặt ở `Project`,
  driver đặt ở `MultiWorkspace`. Không cần crate mới.
- Điểm móc sự kiện: `MultiWorkspaceEvent::ActiveWorkspaceChanged` (`multi_workspace.rs:63`), đã emit
  trong `activate()`.
- `Project` chưa có gì tương tự: chỉ có `set_active_path` (`project.rs:4597`) → `active_entry`. Không
  có `deactivate`/`shutdown`.
- Settings pattern (3 chỗ, phải đủ cả 3):
  1. `crates/settings_content/src/workspace.rs` — struct nội dung (Option field)
  2. `crates/workspace/src/workspace_settings.rs:72+` — `impl Settings::from_settings`, unwrap default
  3. `assets/settings/default.json` — giá trị mặc định + comment
- `SidebarSide` hiện định nghĩa ở `crates/settings_content/src/agent.rs` — di sản tầng AI. Đổi chỗ
  cần cân nhắc `crates/migrator`.

## Phase 3 — LSP hibernate/wake

| Điểm | Vị trí |
|---|---|
| `stop_local_language_server` | `lsp_store.rs:11023-11131` |
| — xoá diagnostics trong buffer | `:11041-11046` (`update_diagnostics(server_id, DiagnosticSet::new([]...))`) |
| — xoá `diagnostic_summaries` + emit | `:11048-11082` |
| — xoá `local.diagnostics` | `:11085-11093` |
| — xoá `language_server_watched_paths` | `:11094` |
| `shutdown_all_language_servers` | `lsp_store.rs:11138-11167` (dùng `local.lsp_tree.remove_nodes`) |
| `restart_language_servers_for_buffers` | `lsp_store.rs:11177+` |
| Keying server | `local.language_server_ids: HashMap<LanguageServerSeed, UnifiedLanguageServer>` (`:296`), `lsp_tree` (`:322`) |
| Shutdown khi quit | `lsp_store.rs:1307`, gọi từ `:4268` |

**Consumer của `diagnostic_summaries` — đây là các bề mặt sẽ hiển thị dữ liệu stale:**

- `crates/project_panel/src/project_panel.rs:1015`, `:1044` — badge lỗi trên cây file
- `crates/diagnostics/src/diagnostics.rs:463` — panel diagnostics
- `crates/workspace/src/pane.rs:770` — icon tab
- API: `Project::diagnostic_summaries` (`project.rs:4634`)

**Bẫy đã tìm ra:** `crates/project/tests/integration/project_tests.rs:3866`
`test_diagnostic_summaries_cleared_on_server_restart` **khẳng định hành vi xoá hiện tại**. Cùng nhóm:
`:3788` (xoá khi worktree entry bị xoá), `:3940` (xoá khi buffer reload). Nghĩa là hibernate **không
được** tái dùng đường restart — phải là đường riêng, và 3 test này phải còn xanh nguyên.

## Phase 4 — Worktree pause/resume

- `_background_scanner_tasks: Vec<Task<()>>` (`worktree.rs:134`), set tại `:1228`. Drop = cancel.
- `restart_background_scanners` (`worktree.rs:1121-1128`) tạo channel mới + gọi
  `start_background_scanner` (`:1139`). Đã được gọi ở `:454`, `:2030`, `:2048` → đường resume đã có sẵn
  và đã được dùng ở nơi khác.
- Đối chiếu buffer với đĩa: `buffer_store.rs:358` / `:742` / `:1501` `reload_buffers`, và
  `language/src/buffer.rs:1665-1685` (`file_changed`). Đây là chỗ phải nối vào khi wake.

## Phase 5 — Terminal

- `MAX_SCROLL_HISTORY_LINES = 100_000` (`terminal.rs:343`); default `10000`
  (`assets/settings/default.json:1604`); `scrolling_history` chốt lúc `Term::new` (`terminal.rs:361`,
  `:548-559`).
- **Đường đổi config lúc runtime đã có:** `self.term.lock().set_options(self.term_config.clone())`
  (`terminal.rs:1297`). `term_config` là field (`terminal.rs` struct). Chưa xác minh alacritty fork
  (`zed-industries/alacritty` rev `9d9640d4`) có thu nhỏ grid history trong `set_options` — **phải thử
  trước khi thiết kế dựa vào nó**.
- Enumerate terminal của một project: `Project.terminals.local_handles: Vec<WeakEntity<Terminal>>`
  (`terminals.rs:27`), accessor `terminals.rs:585`.
- Terminal của task luôn dùng `MAX_SCROLL_HISTORY_LINES` (`terminal.rs:548-552`) — task output không
  theo setting của user.

## Phase 6 — Đo lường

- `sysinfo` đã là dependency: `crates/system_specs/src/system_specs.rs:38` dùng `RefreshKind` +
  `MemoryRefreshKind`, nhưng chỉ đọc `total_memory()`. RSS theo process lấy được từ cùng crate.
- Chưa có bất kỳ instrumentation bộ nhớ theo project nào trong cây code.

## Phase 7 — Sidebar UI

Xem `research/researcher-02-git-archaeology-deleted-sidebar.md`. Nguồn salvage:
`git show c3e2ac3^:crates/sidebar/src/sidebar.rs` (5339 dòng) +
`sidebar_tests.rs` (11168 dòng). Mảnh còn sống trong cây:
`crates/recent_projects/src/sidebar_recent_projects.rs` (chưa có ai gọi).
