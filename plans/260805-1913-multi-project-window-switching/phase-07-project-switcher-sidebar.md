# Phase 07 — Project switcher sidebar, salvage từ git

## Context Links

- [plan.md](./plan.md) · [Phase 01](./phase-01-decouple-retention-and-cycling.md)
- [research/researcher-02-git-archaeology-deleted-sidebar.md](./research/researcher-02-git-archaeology-deleted-sidebar.md) — nguồn salvage và bảng phần dùng được / phải bỏ
- Nguồn: `git show c3e2ac3^:crates/sidebar/src/sidebar.rs` (5339 dòng), `sidebar_tests.rs` (11168 dòng)
- Mảnh còn sống: `crates/recent_projects/src/sidebar_recent_projects.rs`

## Overview

- **Priority:** P2 — tính năng chỉ "giống Discord/Slack" khi có sidebar; nhưng Phase 1 đã cho đường
  chuyển bằng keybinding nên phase này không chặn 2–6
- **Status:** Pending
- **Effort:** 4–5 ngày
- **Phụ thuộc:** chỉ Phase 1. Chạy song song với 2–6 được.

Dựng lại `crates/sidebar` từ commit trước fork, **lột sạch phần thread/agent**, giữ phần danh sách
project. Đây là salvage + strip, không phải thiết kế mới.

## Key Insights

- Trait `Sidebar` (`multi_workspace.rs:75`) mất implementor khi `c3e2ac3` xoá crate `sidebar`. Toàn bộ
  API mà `MultiWorkspace` cần đã từng được UI này drive → không có rủi ro "API không đủ".
- 5 trên ~20 dependency đã chết (`acp_thread`, `action_log`, `agent`, `agent_settings`, `agent_ui`).
  Toàn bộ dep của phần project-list còn nguyên.
- `SidebarRecentProjects::popover` (`recent_projects/src/sidebar_recent_projects.rs:271`) **chưa có ai
  gọi** — nó là popover "thêm project" của sidebar cũ, dùng lại được ngay.
- Nợ phải dọn cùng lúc: `SidebarSide` còn nằm trong `settings_content/src/agent.rs`; trait còn
  `toggle_thread_switcher` / `cycle_thread` / `is_threads_list_view_active`;
  `multi_workspace.rs:20` còn import `zed_actions::agents_sidebar::ToggleThreadSwitcher`;
  `MultiWorkspaceState` còn field `sidebar_state`.
- **Không copy nguyên file.** Code cũ mang giả định của thời có AI; bỏ sót một nhánh là mang nợ vào lại.

## Requirements

**Functional**

- FR1: Crate `sidebar` mới, `[lib] path = "src/sidebar.rs"` (rule của repo), implement
  `workspace::Sidebar`.
- FR2: Danh sách project nhóm theo `ProjectGroupKey`, click để `activate`, highlight project đang active.
- FR3: Fuzzy filter theo tên/đường dẫn (salvage `render_filter_input` + `fuzzy_match_positions`).
- FR4: Nút thêm project mở `SidebarRecentProjects::popover`.
- FR5: Context menu mỗi project: close, move to new window (`MoveProjectToNewWindow` đã có).
- FR6: `cmd-alt-j` (`ToggleWorkspaceSidebar`) mở/đóng; sidebar đồng bộ selection với
  `MultiWorkspace::cycle_project` của Phase 1, **không** tự giữ danh sách riêng.
- FR7: Hiển thị `ProjectActivity` từ Phase 2 — thấy được project nào đang ngủ, và badge "đang index"
  khi wake (khớp Phase 3).
- FR8: Vị trí sidebar (trái/phải) qua setting; `SidebarSide` chuyển khỏi `settings_content/agent.rs`.

**Non-functional**

- NFR1: Sidebar không được giữ state trùng với `MultiWorkspace` (một nguồn sự thật duy nhất).
- NFR2: File nào cũng dưới 200 dòng theo `development-rules.md` → chia module thay vì một
  `sidebar.rs` 3000 dòng như bản cũ.
- NFR3: Render sidebar với 10 project không được tốn quá 1 frame.

## Architecture

```
crates/sidebar/
├── src/sidebar.rs           # entry, struct Sidebar, impl workspace::Sidebar + Render
├── src/project_list.rs      # ListEntry, ActiveEntry, dựng danh sách từ MultiWorkspace
├── src/project_item.rs       # render một dòng project + context menu
├── src/filter.rs             # fuzzy filter
└── src/serialization.rs     # SerializedSidebar ↔ MultiWorkspaceState.sidebar_state

Nguồn sự thật: MultiWorkspace.project_groups + workspaces()  ← sidebar chỉ đọc
Sidebar → MultiWorkspace: activate(), close_workspace(), open_project()
MultiWorkspace → Sidebar: observe + MultiWorkspaceEvent
```

`Cargo.toml` mới lấy từ `c3e2ac3^:crates/sidebar/Cargo.toml` **trừ** 5 dep đã chết.

## Related Code Files

**Create**

- `crates/sidebar/Cargo.toml`, `crates/sidebar/LICENSE-GPL` (symlink như các crate khác)
- `crates/sidebar/src/*.rs` theo cây trên

**Modify**

- `Cargo.toml` (workspace members + `[workspace.dependencies]`)
- `crates/zed/Cargo.toml` + `crates/zed/src/main.rs` (hoặc `zed.rs`) — khởi tạo + `register_sidebar`
- `crates/workspace/src/multi_workspace.rs` — **dọn trait**: bỏ `toggle_thread_switcher`,
  `cycle_thread`, `is_threads_list_view_active`; bỏ import `agents_sidebar::ToggleThreadSwitcher`; bỏ
  các action `NextThread`/`PreviousThread`/`NewThread`/`ToggleThreadSwitcher`
- `crates/settings_content/src/agent.rs` → chuyển `SidebarSide` sang `workspace.rs`;
  `crates/migrator/` nếu key JSON đổi tên
- `assets/keymaps/default-*.json` — bỏ binding của action thread đã xoá, giữ `cmd-alt-j`
- `crates/recent_projects/src/sidebar_recent_projects.rs` — nối vào (nếu cần sửa chữ ký)
- `crates/workspace/src/status_bar.rs:80-85` — đã biết về sidebar, kiểm lại khi sidebar thật xuất hiện

## Implementation Steps

1. Dump nguồn ra chỗ tạm: `git show c3e2ac3^:crates/sidebar/src/sidebar.rs > /tmp/sidebar-old.rs` và
   `sidebar_tests.rs` tương tự. **Đọc trước, không copy.**
2. Dựng crate rỗng + `register_sidebar` từ `zed` → xác nhận `cmd-alt-j` mở được một panel trắng. Đây là
   mốc "đường dây đã nối".
3. Port `ListEntry`/`ActiveEntry`/`SidebarContents` — bỏ mọi biến thể thread.
4. Port `render_project_header` + `render_list_entry` + context menu (`render_project_header_ellipsis_menu`),
   bỏ nhánh thread.
5. Port filter (`render_filter_input`, `fuzzy_match_positions`) — kiểm dùng `fuzzy_nucleo` như
   `sidebar_recent_projects.rs` đang dùng, không kéo crate fuzzy thứ hai.
6. Nối `SidebarRecentProjects::popover` vào nút thêm project.
7. Port serialization (`SerializedSidebar` ↔ `sidebar_state`) — kiểm tương thích với blob session cũ:
   blob cũ chứa field thread, phải đọc được mà không panic (bỏ qua field lạ).
8. Hiển thị `ProjectActivity` (FR7) — cần Phase 2 đã xong; nếu chưa, để chỗ trống và làm sau.
9. Dọn trait + action thread + keymap trong **một commit riêng**, sau khi sidebar mới đã xanh.
10. Chuyển `SidebarSide` khỏi `agent.rs`; nếu đổi tên key JSON thì thêm migration trong
    `crates/migrator`, không để settings của user vỡ im lặng.
11. Port test từ `sidebar_tests.rs`: **chỉ** phần project-list. 11k dòng cũ là mỏ vàng nhưng phần lớn
    là thread — chọn lọc, đừng port cho đủ số.
12. **Gate bảo mật (red team Finding 4):** chạy đúng lệnh `rg` mà plan hard-fork đã dùng để bắt lớp lỗi
    symbol-level, trên crate `sidebar` mới:
    ```
    rg -l "sign_in_with_optional_connect|has_credentials|RefreshLlmTokenListener|
           EditPredictionUsage|\.plan\(\)|plan_for_organization|Plan::Zed" crates/sidebar/src
    ```
    Phải ra **0 kết quả**. Căn cứ: `plans/260726-1531-remove-auth-cloud-hard-fork/plan.md` § "four
    findings" #4 — dependency ở mức symbol không hiện trên graph crate, và đã từng làm sót một crate.
13. `./script/clippy`, `cargo test -p sidebar -p workspace`.

## Todo List

- [ ] Dump 2 file nguồn ra chỗ tạm, đọc hết trước khi viết
- [ ] Crate rỗng + `register_sidebar` + `cmd-alt-j` mở panel trắng
- [ ] `ListEntry`/`ActiveEntry`/`SidebarContents` (bỏ thread)
- [ ] Render project + context menu
- [ ] Fuzzy filter (`fuzzy_nucleo`)
- [ ] Nút thêm project → `SidebarRecentProjects::popover`
- [ ] Serialization + đọc được blob session cũ
- [ ] Hiển thị `ProjectActivity` + badge đang index
- [ ] Commit riêng: dọn trait + action thread + keymap
- [ ] `SidebarSide` khỏi `agent.rs` (+ migration nếu đổi key)
- [ ] Port test project-list chọn lọc
- [ ] Gate `rg` symbol auth ⇒ 0 kết quả
- [ ] Mọi file < 200 dòng
- [ ] `./script/clippy` sạch

## Success Criteria

- Mở 3 project, sidebar hiện cả 3, click chuyển được, project active được highlight.
- `cmd-alt-j` mở/đóng; `NextProject`/`PreviousProject` và click sidebar cho cùng kết quả.
- Project đang ngủ nhìn ra được là đang ngủ.
- Session cũ (blob `sidebar_state` thời có thread) load không panic.
- Không còn tham chiếu nào tới `agents_sidebar`/`thread` trong `crates/workspace`.
- `settings.json` của user với `agent.sidebar_side` cũ vẫn hoạt động (qua migration hoặc alias).

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Copy nguyên code cũ ⇒ kéo giả định AI vào lại | **Cao** | Đọc từng hàm; port theo bước 3–7, mỗi bước compile được; cấm `git checkout c3e2ac3^ -- crates/sidebar` |
| Sidebar tự giữ danh sách project ⇒ hai nguồn sự thật lệch nhau | Cao | NFR1; sidebar chỉ đọc từ `MultiWorkspace`; test lệch state |
| Đổi chỗ `SidebarSide` làm vỡ `settings.json` của user | Trung bình | Migration trong `crates/migrator` + test; hoặc giữ alias key cũ |
| Xoá action thread làm vỡ keymap của user | Trung bình | `keymap_file.rs` `bail!` → app panic khi action không tồn tại (bài học Phase 9 của plan hard-fork). Phải xoá trong **cùng commit** với keymap bundled, và kiểm cả 8 file keymap |
| File phình lại như bản cũ (5339 dòng) | Trung bình | NFR2 chia module ngay từ đầu, không "để sau" |
| Salvage test 11k dòng ⇒ port cho đủ số, nhận nợ test không liên quan | Thấp | Chọn lọc theo tiêu chí: test nào khẳng định hành vi project-list thì port |

## Security Considerations

Sidebar hiển thị đường dẫn project. Không log chúng. `connect_remote` (có trong bản cũ) đi qua
`remote_connection` — port thì phải giữ nguyên đường xác thực SSH hiện hành, không tự thêm cách lưu
credential nào.

## Next Steps

- Sau phase này, regenerate `docs/generated/feature-list.md` + `docs/system/architecture.md` (đã ghi
  trong Cross-Plan Dependencies của plan.md).
- Cân nhắc plan riêng: hibernate cho project remote/SSH (cố tình để ngoài scope plan này).
