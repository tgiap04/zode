# Phase 01 — Tách retention khỏi sidebar và đưa cycling về MultiWorkspace

## Context Links

- [plan.md](./plan.md)
- [brainstorm-report.md](./brainstorm-report.md) — quyết định "tách retention trước"
- [reports/scout-report.md](./reports/scout-report.md) § Phase 1 — bảng điểm móc
- `crates/workspace/src/multi_workspace.rs`, `multi_workspace_tests.rs`

## Overview

- **Priority:** P1 — chặn mọi phase sau, và tự nó fix một dead end đang tồn tại
- **Status:** Pending
- **Effort:** 1–2 ngày

Retention hiện bị gate sau trạng thái UI (`sidebar_open`). Gỡ gate, biến retention thành chính sách
độc lập, và đưa `cycle_project` từ trait `Sidebar` về `MultiWorkspace` để chuyển project được bằng
keybinding mà không cần UI nào.

## Key Insights

- Fork chỉ xoá 42 dòng khỏi `multi_workspace.rs` (`sidebar_side_context_menu`). Phần retention là code
  upstream nguyên bản — nó đúng, chỉ bị treo.
- **Có ba đường retention, không phải hai** (red team Finding 2):
  1. `activate()` (:1374) — gate sau `sidebar_open`
  2. `add()` (:1355) — **luôn** retain, không gate
  3. `activate_provisional_workspace()` (:675-690) — push thẳng vào `retained_workspaces`, không gate,
     gọi từ `workspace.rs:10058`

  Bất đối xứng giữa (1) và (2)/(3) chính là nguồn của dead end. Cả ba phải đi qua cùng một
  `should_retain()`, nếu không setting sẽ nói dối.
- `cli_default_open_behavior: "existing_window"` là **mặc định** → dead end đang chạm tới người dùng
  thật, không phải giả thuyết.
- Nhiều test trong `multi_workspace_tests.rs` gọi `mw.open_sidebar(cx)` chỉ để bật retention. Chúng
  phải đổi sang cơ chế mới, đừng để chúng khoá thiết kế cũ vào chỗ.

### Kết luận điều tra đường quit (bước 9, implementation session)

Liệt kê toàn bộ đường tới `cx.quit()` / `Platform::quit()`:

1. **Action `Quit`** (`crates/zed/src/zed.rs:1356 fn quit`) — đi qua đúng vòng lặp
   `for workspace in multi_workspace.workspaces()` ở `zed.rs:1435`, flush cả serialize từng
   workspace lẫn `multi_workspace.flush_serialization()` + `take_pending_removal_tasks()`, rồi mới
   gọi `cx.quit()`. **Đã bao phủ, không cần sửa.**
2. **Đóng window cuối cùng** — hai cơ chế cùng dẫn tới `cx.quit()` **trực tiếp**, bỏ qua vòng lặp ở
   (1) hoàn toàn: `bind_on_window_closed` (`zed.rs:272-294`, luôn bật trên Linux/Windows, opt-in qua
   setting `on_last_window_closed` trên macOS) và cơ chế `quit_on_empty` dựng sẵn trong GPUI
   (`gpui/src/app.rs` hàm `trail`, ~dòng 1576). Cả hai chỉ chạy sau khi window đã bị đóng.
3. Cả nút đóng cửa sổ gốc hệ điều hành lẫn `CloseWindow` (Cmd+W) đều đổ về **cùng một hàm**:
   `MultiWorkspace::close_window` (`multi_workspace.rs:491`) — xác nhận qua
   `zed.rs:401 window.on_window_should_close(...)` gọi thẳng `multi_workspace.close_window(...)`.
   Hàm này gọi `prepare_to_close` cho từng workspace rồi mới `window.remove_window()`.
   `prepare_to_close` (`workspace.rs:3201`) có nhánh `save_last_workspace` (Linux/Windows, đang đóng
   đúng window cuối) **cố tình bỏ qua** `remove_from_session` (và do đó bỏ qua lần flush
   `serialize_workspace_internal` đi kèm) để giữ session-binding cho việc restore sau này — nghĩa là
   một serialize debounce (`_schedule_serialize_workspace`) chưa kịp chạy có thể bị mất khi
   `remove_window()` kéo theo `cx.quit()` trực tiếp ở bước (2).

**Kết luận:** `app_will_quit` (`multi_workspace.rs:1532`, chỉ await `_serialize_task` +
`pending_removal_tasks`) đúng là điểm hở duy nhất cho đường (2) — nhưng đã KHÔNG sửa `app_will_quit`.
Lý do: hàm này chỉ nhận `&mut Context<Self>` (không có `Window` — xem
`gpui/src/app/context.rs:208`, chữ ký `on_app_quit` cố tình bỏ `Window` vì hook chạy chung cho mọi
window), trong khi flush từng workspace (`Workspace::flush_serialization`) bắt buộc cần `&mut Window`
để đọc bounds/pane layout. Gọi thẳng `flush_all_serialization` (:1759, tên gợi ý trong task) cũng
không hợp: hàm đó `#[cfg(test, feature = "test-support")]`, gán random database id (rác cho test),
và vẫn đòi `Window`. Dựng lại `Window` bằng `App::update_window` bên trong một App-quit hook là việc
chưa từng có tiền lệ trong repo này và rủi ro tái nhập (re-entrancy) không nhỏ cho một finding mức
Medium, không đổi theo `retain_background_projects` (lỗi tồn tại y hệt trước phase này).

**Đã sửa gì:** thay vào đó, thêm bước flush (`workspace.flush_serialization(window, cx)` cho từng
workspace, best-effort qua `.log_err()`) ngay trong `MultiWorkspace::close_window` — nơi đã sẵn có
`Window` — ngay trước `window.remove_window()`. Điều này chặn đúng lỗ hổng ở nguồn (khi đóng window
cuối) mà không cần đụng tới chữ ký `app_will_quit` hay dựng lại `Window` từ App-quit hook.

### Sửa theo code review — hai đường vòng qua should_retain() (Critical)

Review đối kháng sau khi implement bắt được điều cả bản thân lẫn tester đều bỏ sót: `retain_background_projects: false` **không** thực sự chặn được `retained_workspaces` cho đúng kịch bản đầu bài (`zode dirA` rồi `zode dirB`), vì hai đường vòng qua `should_retain()`:

1. **`workspace::open_paths` (:9711, trước khi sửa) gọi `multi_workspace.open_sidebar(cx)` không điều
   kiện** — hàm này gọi `apply_open_sidebar` → `retain_active_workspace(cx)` không qua gate nào cả.
   Đây chính xác là đường `cli_default_open_behavior: existing_window` (mặc định) đi qua — xác nhận
   qua `open_listener.rs` không hề set `open_mode`, nên nó giữ nguyên `OpenMode::Activate`. Kết quả:
   dirA bị ép retain bởi lời gọi này, sau đó `activate()` thấy `old_active_was_retained == true` nên
   không bao giờ detach — bất kể setting là gì.
   **Sửa (bản cuối, sau khi tự phát hiện thêm — xem mục dưới):** `apply_open_sidebar` giờ nhận thêm
   `respect_retention_policy: bool`; `open_sidebar()` (đường sống) truyền `true`, `restore_open_sidebar()`
   (đường phục hồi session) truyền `false` để không vi phạm NFR2. `open_paths` **vẫn gọi
   `multi_workspace.open_sidebar(cx)` như cũ** — không đổi thành gọi `retain_active_workspace` trực
   tiếp — vì `open_sidebar()` còn set `sidebar_open = true` và bắn telemetry mà các test khác (ví dụ
   `test_open_paths_action` ở crate `zed`) phụ thuộc vào; gọi thẳng `retain_active_workspace` sẽ làm
   mất hai side-effect đó. `should_retain()` vẫn giữ private — không cần `pub(crate)` vì không còn nơi
   nào ngoài `multi_workspace.rs` gọi trực tiếp.
2. **`add()` có người gọi sống thật, không chỉ đường restore như giả định ban đầu:**
   `git_ui/src/worktree_service.rs` (tạo/chuyển linked worktree, qua `Workspace::new_local`'s
   `OpenMode::Add` branch) và `find_or_create_workspace_with_source_workspace`'s remote/SSH branch
   (`multi_workspace.rs`) — nhánh SSH còn tệ hơn: gọi đúng `activate()`/`activate_provisional_workspace()`
   đã gate rồi (qua `open_remote_project_inner`), nhưng NGAY SAU ĐÓ gọi `.add()` không điều kiện lên
   kết quả — tự ghi đè quyết định đúng của chính mình trong cùng một request. **Sửa:** thêm
   `MultiWorkspace::add_or_activate()` (add nếu `should_retain()`, activate nếu không) — route nhánh
   worktree qua đây (đổi `Workspace::new_local`'s `OpenMode::Add` arm), xoá hẳn lời gọi `.add()` thừa
   ở nhánh SSH (không cần gate, chỉ cần xoá — logic đúng đã chạy trước đó rồi). `add()` (không qua
   `add_or_activate`) giờ chỉ còn được gọi trực tiếp bởi đúng 3 chỗ restore thật:
   `persistence.rs` (3 chỗ) và `open_workspace_by_id`.

**Bài học cho việc viết test:** toàn bộ test cũ (kể cả test mới viết ở phase này) bật retention bằng
cách gọi `add()`/`activate()`/`activate_provisional_workspace()` trực tiếp — không test nào đi qua
`workspace::open_paths` thật, nên không cái nào bắt được lỗi (1). Đã thêm
`test_open_paths_reusing_existing_window_respects_retain_background_projects_false` trong
`multi_workspace_tests.rs`, gọi thẳng `open_paths()` với `OpenOptions::default()` — xác minh bằng
cách revert tạm lỗi (1) và thấy test fail đúng chỗ (`retained_workspaces` = 2) trước khi phục hồi fix.

### Vòng thứ ba — đường thứ ba qua should_retain(), hợp nhất test helper, và một regression thứ 6

Tự rà lại sau vòng review trên (không phải do reviewer chỉ ra) phát hiện **đường thứ ba**:
`ToggleWorkspaceSidebar`/`FocusWorkspaceSidebar` (Cmd+Alt+J / Cmd+Alt+; — keybinding thật, có sẵn trên
cả 3 platform, dù Phase 7 chưa dựng lại sidebar UI) gọi `open_sidebar()` → `apply_open_sidebar()` →
`retain_active_workspace(cx)` không qua gate — bấm phím này (không hiện gì trên UI vì `self.sidebar`
luôn `None`) vẫn âm thầm ép retain project đang active. **Sửa:** `apply_open_sidebar` nhận
`respect_retention_policy: bool`; `open_sidebar()` (sống) → `true`, `restore_open_sidebar()` (restore)
→ `false` (giữ nguyên NFR2). Test mới:
`test_open_sidebar_does_not_force_retain_when_retain_background_projects_false` — xác minh bằng cách
revert tạm và thấy fail (`retained_workspaces.len()` = 1 thay vì 0) trước khi phục hồi.

Việc gate `open_sidebar()` làm hỏng 6 test cũ dùng `open_sidebar()`/`open_project(..., OpenMode::Add)`
làm công tắc bật retention ngầm cho một workspace ĐẦU TIÊN (không qua `add()`) — 3 ở `persistence.rs`,
3 ở `crates/zed/src/zed.rs` (crate khác, **`cargo test -p workspace` không bao giờ thấy được** — đúng
lỗ hổng review đã cảnh báo). Sửa cả 6 bằng cùng một helper, giờ hợp nhất thành
`MultiWorkspace::test_enable_background_retention()` (`#[cfg(any(test, feature = "test-support"))]`,
`pub`) thay cho 3 bản copy riêng biệt (`workspace.rs`, `multi_workspace_tests.rs`, `persistence.rs`).

**Regression thứ 6, tự bắt được nhờ chạy rộng `cargo test -p zode --bin zode --features
test-support` (không filter) như review yêu cầu:** `test_open_paths_action` — xác nhận qua `git stash`
là đã hỏng **từ trước** vòng sửa (1)/(2) ở trên, không phải do vòng gate `open_sidebar()` mới. Test này
assert `workspaces().count() == 2` sau hai lần `open_paths()` riêng biệt tái dùng cùng window — đúng
hành vi CŨ (bug) mà cả plan này tồn tại để sửa. Đã sửa assertion thành `== 1`, kèm comment giải thích
đây là thay đổi hành vi có chủ đích của Phase 1, không phải lỗi.

**Phát hiện phụ, không sửa (ngoài scope):** chạy đúng 5-6 test bị filter riêng (không phải cả suite)
bằng test-thread song song cho kết quả **flaky** (fail ngẫu nhiên, khác test mỗi lần) — nhưng chạy
`--test-threads=1` HOẶC chạy cả 52 test trong `zed::tests` (song song hay tuần tự đều được) thì xanh
ổn định qua nhiều lần. Nguyên nhân là các test này share một DB/KVP toàn cục qua nhiều thread test
song song (đã có tiền lệ: comment của `unique_test_dir` trong `persistence.rs` tự nhận "to avoid
collisions with other tests sharing the global DB") — đặc điểm sẵn có của hạ tầng test, không phải do
logic retention. Trước đây không thấy được vì cả 5 test này luôn fail (do bug retention) nên tính
flaky của nó chưa từng lộ ra.

## Requirements

**Functional**

- FR1: Chuyển project bằng `NextProject`/`PreviousProject` hoạt động khi `sidebar = None`.
- FR2: Workspace rời focus được retain theo chính sách, không theo trạng thái UI.
- FR3: Mọi workspace retained đều tới được: không có workspace nào nằm trong `retained_workspaces` mà
  không có đường chuyển tới.
- FR4: `MoveProjectToNewWindow` và `close_workspace` giữ nguyên hành vi.
- FR5: Setting mới `workspace.multi_project.retain_background_projects` (bool). **Default `false` ở
  phase này**, flip sang `true` ở bước cuối Phase 3 (quyết định Validation session 1) — để không ai phải
  sống với 6 project sống cùng lúc trong khoảng thời gian chưa có governor. Đặt `false` luôn giữ được
  hành vi hôm nay (một project sống một window).

**Non-functional**

- NFR1: Chuyển giữa hai workspace đã retained: < 100ms tới frame đầu.
- NFR2: `MultiWorkspaceState` cũ (có `sidebar_open`) vẫn deserialize được — không được làm hỏng session
  đang lưu.

## Architecture

```
Trước:  activate() ──gate: sidebar_open?──> retain / detach
Sau:    activate() ──gate: retain_background_projects setting──> retain / detach

Trước:  NextProject action ──> sidebar.cycle_project()   [sidebar = None → no-op]
Sau:    NextProject action ──> MultiWorkspace::cycle_project()
                                   └─ sidebar (nếu có) chỉ đồng bộ highlight qua observe
```

Thứ tự cycling = thứ tự `retained_workspaces` (đã ổn định, push-order). Sidebar khi có sẽ render theo
cùng thứ tự đó, không tự giữ danh sách riêng.

## Related Code Files

**Modify**

- `crates/workspace/src/multi_workspace.rs` — `activate()` (:1374), `add()` (:1355),
  `activate_provisional_workspace()` (:675), `retain_workspace()` (:660), `detach_workspace()` (:1453);
  thêm `cycle_project()`; các `on_action` handler (:2062-2085) trỏ vào `self` thay vì `self.sidebar`
- `crates/workspace/src/multi_workspace_tests.rs` — thay `open_sidebar()`-để-bật-retention bằng setting
- `crates/settings_content/src/workspace.rs` — thêm `multi_project: Option<MultiProjectContent>`
- `crates/workspace/src/workspace_settings.rs` — resolve field mới trong `from_settings`
- `assets/settings/default.json` — `"multi_project": { "retain_background_projects": true }` + comment

**Không đổi trong phase này**

- Trait `Sidebar`: giữ nguyên signature `cycle_project`/`cycle_thread` để không vỡ compile; đánh dấu
  deprecated bằng comment `// TODO(phase-07)`. Việc dọn method thread để Phase 7 làm cùng lúc dựng UI.

## Implementation Steps

1. Thêm `MultiProjectContent { retain_background_projects: Option<bool> }` vào
   `settings_content/src/workspace.rs`, theo đúng khuôn các struct lân cận (mọi field `Option`).
2. Resolve trong `WorkspaceSettings::from_settings` (`workspace_settings.rs:72+`) —
   `.unwrap()` sau default là khuôn hiện hành ở file này (default.json bảo đảm có giá trị).
3. Ghi default (`false`) vào `assets/settings/default.json` cạnh `cli_default_open_behavior`, kèm comment
   giải thích quan hệ với nó và ghi rõ đây là cờ tạm sẽ được flip khi hibernate lên.
4. Trong `activate()`: đổi `if self.sidebar_open` → `if self.should_retain(cx)`; đổi
   `if !self.sidebar_open && !old_active_was_retained` → `if !self.should_retain(cx) && !old_active_was_retained`.
   Thêm `fn should_retain(&self, cx: &App) -> bool` đọc setting.
5. Thêm `pub fn cycle_project(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>)`:
   lấy index của `active_workspace` trong `workspaces()`, cộng/trừ 1 có wrap, `activate()` phần tử đó.
   Danh sách 0 hoặc 1 phần tử → no-op.
6. Trỏ `NextProject`/`PreviousProject` handler vào `Self::cycle_project`. **Giữ** nhánh
   `sidebar.cycle_project` cho trường hợp sidebar muốn tự lái selection — nhưng chỉ khi
   `is_threads_list_view_active` false; nếu logic này rối thì bỏ hẳn nhánh sidebar, Phase 7 sẽ nối lại.
7. Kiểm `close_workspace` (:830) và `remove` (:1801) không giả định `sidebar_open` để suy ra retention.
8. Cho `activate_provisional_workspace` (:675) đi qua `should_retain()` như hai đường kia.
9. **Xác minh đường quit** (red team Finding 6): liệt kê mọi đường dẫn tới quit, chứng minh đường nào
   cũng đi qua vòng lặp `for workspace in multi_workspace.workspaces()` ở `crates/zed/src/zed.rs:1435`.
   Nếu có đường chỉ chạm `app_will_quit` (`multi_workspace.rs:1532`) — hàm đó chỉ await `_serialize_task`
   + `pending_removal_tasks` — thì gọi `flush_all_serialization` (:1759) trong đó.
10. Chạy `./script/clippy` và `cargo test -p workspace`.

## Todo List

- [ ] `MultiProjectContent` trong settings_content
- [ ] Resolve field trong `WorkspaceSettings`
- [ ] Default + comment trong `default.json`
- [ ] `should_retain()` thay 2 gate trong `activate()`
- [ ] `activate_provisional_workspace` đi qua `should_retain()`
- [ ] `MultiWorkspace::cycle_project()`
- [ ] Đấu lại action handler
- [ ] Rà `close_workspace` / `remove` / `collapse_to_single_workspace`
- [ ] Xác minh mọi đường quit đi qua vòng lặp ở `zed.rs:1435`
- [ ] Sửa test đang dùng `open_sidebar()` làm công tắc retention
- [ ] Test mới: `zode dirA` + `OpenMode::Add` dirB → `cycle_project` tới được B
- [ ] Test mới: `retain_background_projects: false` → hành vi cũ (detach khi rời)
- [ ] Test mới: đường provisional cũng tôn trọng `retain_background_projects: false`
- [ ] `./script/clippy` sạch

## Success Criteria

- Mở 3 project vào một window bằng `OpenMode::Add`, `NextProject` đi hết 3 và quay vòng.
- `retain_background_projects: false` → `retained_workspaces` không lớn hơn 1.
- 905 dòng test cũ xanh sau khi đổi công tắc; không test nào bị xoá để cho xanh.
- Deserialize một `MultiWorkspaceState` có `sidebar_open: true` từ session cũ không panic, không mất
  project group.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| `detach_workspace` xoá session binding trong DB; retain nhiều hơn trước làm đổi ngữ nghĩa `session_id` | Cao | Đọc kỹ `:1453-1485` và `persistence.rs` `set_session_binding` trước khi sửa; thêm test restore sau khi retain 3 project |
| Test cũ dùng `open_sidebar()` như công tắc → sửa sai thành che lỗi | Trung bình | Mỗi test sửa phải nêu rõ nó khẳng định điều gì; không đổi assert, chỉ đổi cách bật |
| Retain nhiều workspace làm nặng RAM ngay ở phase này, trước khi có governor | Trung bình | **Đã chốt:** default `false`, flip sang `true` ở bước cuối Phase 3 (Validation session 1) |
| `cycle_project` gọi `activate()` → `serialize()` mỗi lần bấm | Thấp | `serialize` đã debounce qua task; kiểm lại bằng test không sinh task rác |

## Security Considerations

Không có bề mặt auth. Một điểm về dữ liệu: `detach_workspace` ghi DB (`set_session_binding`) trong
background task — giữ nguyên `.log_err()`, không được đổi thành `unwrap`.

## Next Steps

- Phase 2 dựng state machine trên nền retention này.
- Phase 7 (sidebar UI) chỉ cần phase này xong, chạy song song được.
