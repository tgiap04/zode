# 2026-08-05 — Multi-project switching: hạ tầng đã có, chỉ đang bị khoá

Phiên tư vấn thiết kế (không code) cho yêu cầu: mở nhiều project trong một window, chuyển qua lại
như đổi channel Discord/Slack, project background bị hibernate.

Báo cáo thiết kế: `plans/260805-1913-multi-project-window-switching/brainstorm-report.md`.

## Phát hiện đáng giữ lại

**Fork này đã có sẵn multi-workspace, nhưng nó chết lâm sàng vì trait `Sidebar` không còn
implementor nào.**

`crates/workspace/src/multi_workspace.rs` (2159 dòng + 905 dòng test) đã dựng đủ: nhiều
`Entity<Workspace>` trong một window, `retained_workspaces`, `project_groups`, persistence qua
`MultiWorkspaceState`, `MoveProjectToNewWindow`. Mọi window đã được bọc bởi `MultiWorkspace`
(`workspace.rs:2031`, `9591`, `10178`).

Nhưng implementor của trait `Sidebar` (`multi_workspace.rs:75`) từng nằm trong `agent_ui` — crate bị
xoá ở lần hard-fork (`plans/260726-1531-remove-auth-cloud-hard-fork/`). `grep "impl Sidebar"` trên
toàn cây → 0 kết quả. Dây chuyền hệ quả:

- `sidebar = None` → `sidebar_open` luôn `false`
- trong `activate()`: `if self.sidebar_open { retain_workspace(...) }` không bao giờ chạy
- và `if !self.sidebar_open && !old_active_was_retained { detach_workspace(old) }` **luôn** chạy

→ Hôm nay chuyển project = **thả** project cũ. Các action `NextProject` / `PreviousProject` /
`ToggleWorkspaceSidebar` (`cmd-alt-j`, `default-macos.json:550`) dispatch vào `None`, im lặng
không làm gì.

Bài học: khi hard-fork xoá một crate UI, thứ chết theo không chỉ là UI — **chính sách nghiệp vụ bị
gate sau trạng thái UI cũng chết theo mà không có lỗi nào báo lên**. Retention lẽ ra không nên phụ
thuộc `sidebar_open`. Đây là lý do quyết định đầu tiên trong thiết kế là tách hai thứ đó ra.

Kèm theo: `cycle_project` / `cycle_thread` được uỷ quyền cho trait `Sidebar` chứ không nằm trong
`MultiWorkspace`, nên gỡ gating mà không dời logic cycling thì retention chạy được nhưng vẫn không
có cách nào chuyển project.

## Ràng buộc kỹ thuật đã xác minh trong code

- `stop_local_language_server` (`crates/project/src/lsp_store.rs:11041-11095`) **xoá sạch
  diagnostics**: buffer diagnostics set rỗng, `diagnostic_summaries` và `local.diagnostics` xoá theo
  server id. Mọi thiết kế kiểu "stop LSP nhưng giữ state trên RAM" phải trả giá này — hoặc chấp nhận
  wake lên thấy sạch squiggle và badge lỗi trong khi chờ index lại.
- Bộ đệm vòng cho log terminal **đã có sẵn**: alacritty grid là ring buffer,
  `MAX_SCROLL_HISTORY_LINES = 100_000` (`terminal.rs:343`), mặc định 10000
  (`assets/settings/default.json:1604`). Việc cần làm là siết nó cho project background, không phải
  dựng nó. `term.lock().set_options(...)` (`terminal.rs:1297`) là điểm móc để đổi lúc runtime.
- `Worktree::restart_background_scanners` (`worktree.rs:1121`) cho phép pause/resume watcher: task
  nằm trong `Vec`, drop là cancel. Đổi lại là snapshot bị cũ trong lúc pause.
- Không có khái niệm suspend/hibernate nào trong `project/`, `workspace/`, `terminal/`. `Project`
  không có `deactivate()`. Có cơ chế stop, không có chính sách.

## Đề xuất bị loại, kèm lý do

- **HashMap cho O(1) context switching** — không cần. GPUI đã cho sẵn: state nằm trong entity,
  `Render` chỉ vẽ workspace active (`multi_workspace.rs:2043`). N project là 3–10, `Vec` + so sánh
  entity nhanh hơn hash. Thêm cấu trúc cho một vấn đề không tồn tại.
- **SIGSTOP/SIGCONT để "ngủ đông" tiến trình** — chỉ tiết kiệm CPU, RAM của LSP vẫn nằm nguyên đó.
  Kèm rủi ro pipe buffer đầy, LSP client timeout, watcher chết. Chọn idle-shutdown theo bậc thay thế.

---

# Phiên lập kế hoạch (cùng ngày) — 4 sự thật tìm được khi khảo sát sâu

Plan: `plans/260805-1913-multi-project-window-switching/` (7 phase, red team 14 finding, validation 4 câu).

## 1. Cả một crate `sidebar` từng tồn tại — lấy lại được từ git

`c3e2ac3` xoá `crates/sidebar/` gồm `sidebar.rs` (5339 dòng), `sidebar_tests.rs` (11168 dòng),
`thread_switcher.rs` (277 dòng). Đây chính là implementor của trait `Sidebar` đang thiếu. Lấy lại:
`git show c3e2ac3^:crates/sidebar/src/sidebar.rs`.

Chỉ 5 trên ~20 dependency đã chết (`acp_thread`, `action_log`, `agent`, `agent_settings`, `agent_ui`).
Phần project-list dùng được nguyên. `crates/recent_projects/src/sidebar_recent_projects.rs` là mảnh của
sidebar cũ **còn sống trong cây và chưa có ai gọi**.

**Bài học chung:** trước khi thiết kế lại một tính năng trong repo đã qua hard-fork, `git log -S` cái
symbol thiếu. Cái đã bị xoá thường còn nguyên trong history, kèm test.

## 2. Settings của Zode không có kiểu duration dạng chuỗi

Khuôn duy nhất có sẵn: `debounce_ms: Option<u64>` (`settings_content/src/workspace.rs:1024`) và
`AfterDelay { milliseconds: DelayMs }` (`:541`). Bản đầu của plan viết `"hibernate_after": "5m"` —
red team bắt được, sửa thành `hibernate_after_ms`. Viết setting mới thì đọc file content trước, đừng
suy từ cách các IDE khác biểu diễn.

## 3. Có test đang ghim chính hành vi mà tính năng mới muốn đổi

`project_tests.rs:3866` `test_diagnostic_summaries_cleared_on_server_restart` khẳng định
`stop_local_language_server` **phải** xoá diagnostics. Cùng nhóm `:3788`, `:3940`.

Nghĩa là hibernate không được tái dùng đường restart — phải là đường riêng (tách thân hàm thành inner có
cờ `clear_diagnostics`), và 3 test đó phải còn xanh nguyên. Nếu không rà test trước, cách "tự nhiên"
nhất (sửa `stop_local_language_server` cho thôi xoá) sẽ phá hợp đồng của restart.

## 4. Remote/SSH: diagnostics bị xoá bởi message từ host, không phải bởi code local

Với project remote, `stop`/`restart` đi qua proto (`lsp_store.rs:11140-11150`, `:11183-11211`). Nhưng
việc xoá `diagnostic_summaries` xảy ra ở `LspStore` phía host, rồi host đẩy
`proto::UpdateDiagnosticSummary` count = 0 xuống client. Nên giữ summary cho remote **không** làm được
bằng cách sửa code local — phải bỏ qua message đến trong lúc hibernate.

Cùng loại bẫy với #3: hành vi nhìn như "của mình" thực ra do bên khác quyết định.
