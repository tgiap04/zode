---
type: brainstorm-report
date: 2026-08-05
lens: CTO
level: medium
status: sealed
---

# Nhiều project trong một cửa sổ — hibernate tài nguyên và chuyển project tức thì

## Commission

Người dùng mở nhiều project trong cùng một window và chuyển qua lại như đổi channel Discord/Slack.
Project không được focus bị đưa vào trạng thái ngủ đông: tiến trình nặng (LSP, DAP, kết nối DB) tạm
dừng, chỉ giữ state UI trên RAM. Chuyển project phải cho cảm giác tức thì. Log terminal chạy ngầm
phải bị chặn bằng bộ đệm vòng để không tràn bộ nhớ.

## Xưởng: cái đã có sẵn (đọc trước khi thiết kế)

Đề bài giả định xây từ đầu. Thực tế phần lớn hạ tầng đã nằm trong cây code, đang bị tắt.

- `crates/workspace/src/multi_workspace.rs` (2159 dòng) đã có `MultiWorkspace`: nhiều
  `Entity<Workspace>` sống song song trong **một** window, `retained_workspaces`, `project_groups`,
  `activate()`, `add()`, `MoveProjectToNewWindow`, persistence qua `MultiWorkspaceState`
  (`persistence/model.rs:110`), và 905 dòng test (`multi_workspace_tests.rs`). Mọi window đã được
  bọc bởi `MultiWorkspace` (`workspace.rs:2031`, `9591`, `10178`).
- `Render` chỉ vẽ workspace active (`multi_workspace.rs:2043`). **O(1) context switching đã có
  sẵn từ GPUI**: state nằm trong entity, đổi con trỏ `active_workspace` + `cx.notify()`. Đề xuất
  HashMap giải một vấn đề không tồn tại — N project là 3–10, `Vec` + so sánh entity nhanh hơn hash.
- **Thiếu UI sidebar.** `grep "impl Sidebar"` → 0 kết quả; implementor cũ nằm trong `agent_ui`, đã
  bị xoá ở lần hard-fork. Hệ quả dây chuyền: `sidebar = None` → `sidebar_open` luôn `false` → nhánh
  `if self.sidebar_open { retain_workspace(...) }` trong `activate()` không bao giờ chạy, còn
  `if !self.sidebar_open && !old_active_was_retained { detach_workspace(old) }` **luôn** chạy. Hôm
  nay đổi project = **thả** project cũ. `NextProject`/`PreviousProject`/`ToggleWorkspaceSidebar`
  (`cmd-alt-j`, `default-macos.json:550`) đều gọi vào `None` → no-op.
- **Bộ đệm vòng terminal đã xong sẵn.** `MAX_SCROLL_HISTORY_LINES = 100_000` (`terminal.rs:343`),
  mặc định `max_scroll_history_lines: 10000` (`assets/settings/default.json:1604`), grid alacritty
  vốn là ring buffer. Rủi ro thật không phải cơ chế buffer mà là **10k dòng × M terminal × N
  project** cộng lại.
- **Không có khái niệm suspend/hibernate nào** trong `project/`, `workspace/`, `terminal/` (grep
  sạch). `Project` không có `deactivate()`. LSP có `stop_local_language_server` /
  `stop_language_servers_for_buffers` — có cơ chế dừng, không có **chính sách** nào gọi chúng theo
  focus hay idle.

Ba ràng buộc kỹ thuật quyết định thiết kế:

1. `stop_local_language_server` **xoá sạch diagnostics** (`lsp_store.rs:11041-11095`): buffer
   diagnostics set rỗng, `diagnostic_summaries` và `local.diagnostics` bị xoá theo server id. "Stop
   LSP nhưng giữ state trên RAM" không miễn phí như đề xuất giả định.
2. `cycle_project`/`cycle_thread` được uỷ quyền cho trait `Sidebar`, không nằm trong
   `MultiWorkspace`. Tách retention khỏi sidebar mà không dời cycling vào `MultiWorkspace` thì
   retention chạy nhưng không có cách chuyển project. Trait còn dính `toggle_thread_switcher` /
   `is_threads_list_view_active` — rác của `agent_ui`.
3. Mọi đòn tắt tài nguyên đều có điểm móc sẵn: `Worktree::restart_background_scanners`
   (`worktree.rs:1121`, task trong `Vec` → drop là cancel), `LspStore::shutdown_all_language_servers`,
   `PrettierStore` per-project, và `term.lock().set_options(...)` (`terminal.rs:1297`) cho phép siết
   `scrolling_history` lúc runtime.

## Các đường đã cân

| Đường | Được | Mất |
|---|---|---|
| **1. Retention trước, đo rồi mới ngủ** — gỡ gating, đo RSS/LSP/buffer theo project, chính sách sau | Không regression; chính sách sinh từ số thật | Trong lúc chờ, "warm không giới hạn" đúng nghĩa: 6 project Rust = 6 rust-analyzer |
| **2. Governor 3 trạng thái** *(chọn)* | Đúng commission; wake không nhìn như project vừa hỏng | Động vào `lsp_store.rs` (14.7k dòng); tạo invariant mới "diagnostics có thể stale" |
| **3. Cold switch** — rời project là serialize + drop | RAM phẳng tuyệt đối; gần đúng hành vi hôm nay | Mỗi lần chuyển là restore cả workspace + start lại LSP — đúng thứ cần tránh |

Đường 3 giữ làm mốc: mọi thứ xây ra phải hơn nó về latency, và nó là chỗ rơi về nếu governor sai.

## Hướng đã chốt — Đường 2

`Project` mang state machine `Active → Warm → Hibernated`. `MultiWorkspace` lái nó từ
`MultiWorkspaceEvent::ActiveWorkspaceChanged` cộng một timer idle mỗi project.

| | Active | Warm | Hibernated |
|---|---|---|---|
| LSP / DAP / Prettier | chạy | chạy | stop |
| Worktree scanner + fs watcher | chạy | chạy | drop; rescan khi wake |
| Terminal + tiến trình user | chạy | chạy | **vẫn chạy**, scrollback siết ~2k dòng |
| State UI (tab, pane, cursor) | RAM | RAM | RAM |
| Diagnostics | live | live | giữ summary, bỏ per-line |

Quyết định trong buổi tư vấn:

- **Q: Điểm vào đầu tiên? → A: Tách retention khỏi sidebar trước.** Retention thành chính sách độc
  lập, không phụ thuộc trạng thái UI. Kéo theo: dời `cycle_project` vào `MultiWorkspace`, dọn
  method thread khỏi trait `Sidebar`.
- **Q: Tiến trình nặng ở background? → A: Shutdown theo bậc khi idle.** Rời focus vẫn giữ sống vài
  phút (`hibernate_after`, mặc định 5 phút, `null` để tắt) để switch qua lại tức thì; quá ngưỡng thì
  stop hẳn.
- **Q: Số project warm đồng thời? → A: Không giới hạn, dựa vào chính sách idle.** Không có LRU cap.
- **Q: Tiến trình user trong terminal khi project ngủ? → A: Luôn giữ chạy, chỉ chặn bộ nhớ log.**
  Không bao giờ tự dừng tiến trình của user; siết `scrolling_history` cho terminal ở background.
- **Q: Diagnostics khi wake? → A: Chỉ giữ summary, bỏ squiggle.** Giữ `diagnostic_summaries` để
  badge project panel và số đếm status bar không nhìn như project vừa sạch lỗi; thả diagnostics
  per-line trong buffer. Tránh hẳn việc map lại theo server id (id đổi sau restart).

Kèm theo, tôi khuyến nghị và ghi vào thiết kế: **cầu chì theo áp lực bộ nhớ**. Timer idle không chặn
được đỉnh — chạm 10 project trong 5 phút thì cả 10 warm cùng lúc, không có gì đứng giữa và OOM. Giữ
"không giới hạn" làm quy tắc danh nghĩa, thêm trigger hibernate ngay các project ít dùng nhất khi RSS
tổng vượt ngưỡng.

## Hình dạng thi công (phác, ~5–6 phase)

1. **Gỡ gating + cycling** — retention độc lập với `sidebar_open`; `cycle_project` về
   `MultiWorkspace`; dọn trait `Sidebar`; giữ `MultiWorkspaceState.sidebar_open` để tương thích dữ
   liệu cũ. Test: `multi_workspace_tests.rs` phải xanh khi không có sidebar nào.
2. **`ProjectActivity` state machine** trên `Project` + driver trong `MultiWorkspace`; settings
   `hibernate_after`; chưa tắt gì cả (no-op transitions) để tách nguyên nhân khi debug.
3. **LSP hibernate/wake** — `LspStore::hibernate()` stop server nhưng giữ `diagnostic_summaries`;
   `wake()` restart theo buffer đang mở. Prettier + DAP theo cùng đường.
4. **Worktree pause/resume** — drop `_background_scanner_tasks`, `restart_background_scanners` khi
   wake, đối chiếu buffer đang mở với đĩa.
5. **Terminal** — siết `scrolling_history` per-project qua `set_options`; xác minh alacritty fork
   thu nhỏ grid history thật.
6. **Cầu chì bộ nhớ + đo lường** — RSS tổng, số LSP server sống, bytes buffer terminal.

## Cần canh

- **Diagnostics stale ở tầng summary.** Badge lỗi có thể không còn đúng sau khi file thay đổi ngoài
  editor trong lúc project ngủ. Phải quyết cách hiển thị (làm mờ? tooltip "chưa index lại"?) chứ
  không im lặng.
- **Snapshot worktree cũ khi wake.** `git checkout` bên ngoài trong lúc hibernate → buffer báo clean
  nhưng đĩa đã khác. Đây là loại bug có mùi mất dữ liệu; phải reconcile trước khi cho edit.
- **Wake của rust-analyzer vẫn tốn hàng chục giây.** Hibernate không xoá được sự thật đó, chỉ làm nó
  đỡ giật. Cần indicator trạng thái đang index, không để user đoán.
- **Giới hạn inotify trên Linux** (~8192 watch/user mặc định) — drop watcher của project ngủ là lợi
  ích, không chỉ là chi phí.
- **`lsp_store.rs` 14.7k dòng.** Mọi thay đổi ở đây cần test trước, và không được gộp vào phase khác.

## Đo thành công

- Chuyển giữa hai project **warm**: < 100ms tới frame đầu (mốc phải thắng Đường 3).
- Wake một project **hibernated**: UI (tab, pane, cursor, terminal) hiện ngay < 100ms; LSP xanh lại
  không blocking UI, có indicator.
- 5 project mở, 4 hibernated: tổng RSS ≤ 1.5× RSS của một project đơn lẻ đang active.
- Tiến trình user trong terminal: **không bao giờ** bị dừng hay kill bởi governor. Kiểm bằng test
  chạy tiến trình dài trong project background.
- `multi_workspace_tests.rs` xanh, cộng test mới cho từng chuyển trạng thái.

## Việc tiếp theo

Lập kế hoạch chi tiết theo `/tkm:create-plan` với báo cáo này làm đầu vào. Phase 1 không phụ thuộc
gì; phase 3–5 độc lập với nhau và có thể chạy song song sau khi phase 2 xong.

## Còn để mở

- Ngưỡng RSS cho cầu chì bộ nhớ — cần số đo từ phase 6 mới chốt được.
- Hình dạng UI sidebar (danh sách project phẳng, hay nhóm theo `ProjectGroupKey` như hạ tầng đã
  dựng sẵn) — chưa bàn, không chặn phase 1–5.
- Alacritty fork của Zed có thật sự thu nhỏ grid history trong `set_options` hay không — xác minh ở
  phase 5.
