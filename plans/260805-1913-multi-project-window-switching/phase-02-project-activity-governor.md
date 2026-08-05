# Phase 02 — ProjectActivity state machine và governor driver

## Context Links

- [plan.md](./plan.md) · [Phase 01](./phase-01-decouple-retention-and-cycling.md)
- [reports/scout-report.md](./reports/scout-report.md) § Phase 2
- [research/researcher-01-lsp-hibernation-prior-art.md](./research/researcher-01-lsp-hibernation-prior-art.md) — vì sao ngưỡng idle phải dài

## Overview

- **Priority:** P1 — hạ tầng cho Phase 3/4/5
- **Status:** Pending
- **Effort:** 1 ngày

Dựng state machine `Active → Warm → Hibernated` trên `Project`, cùng driver trong `MultiWorkspace` lái
theo focus + timer idle. **Phase này không tắt bất cứ tài nguyên nào** — mọi transition là no-op có
event. Tách như vậy để khi Phase 3 gây lỗi, ta biết chắc lỗi ở LSP chứ không ở máy trạng thái.

## Key Insights

- `workspace` phụ thuộc `project`, không ngược lại → state sống ở `Project`, quyết định sống ở
  `MultiWorkspace`. Không cần crate mới (KISS).
- `MultiWorkspaceEvent::ActiveWorkspaceChanged` đã có sẵn và đã được emit trong `activate()` — đủ làm
  tín hiệu, không cần thêm hook nào.
- `Project` chưa có khái niệm vòng đời nào ngoài `set_active_path`. Đây là API mới đầu tiên thuộc loại
  này — đặt tên và đặt chỗ cẩn thận vì Phase 3/4/5 đều treo vào nó.
- Timer: dùng `cx.background_executor().timer(...)` trong `cx.spawn`, giữ `Task` trong field để drop là
  cancel. Không dùng `smol::Timer` (rule của repo).

## Requirements

**Functional**

- FR1: `Project::activity() -> ProjectActivity` và `Project::set_activity(...)`, emit
  `Event::ActivityChanged` khi đổi.
- FR2: Workspace được activate → project của nó về `Active` ngay lập tức (đồng bộ, không qua task).
- FR3: Workspace rời focus → `Warm`, hẹn timer `hibernate_after`; hết hạn → `Hibernated`.
- FR4: Workspace được activate lại trong lúc `Warm` → huỷ timer, về `Active`, **không** có transition
  nào khác xảy ra.
- FR5: Setting `workspace.multi_project.hibernate_after_ms` — **số millisecond** (default `300000`) hoặc
  `null` để tắt hẳn. Không phải chuỗi `"5m"`: hệ settings của Zode không có kiểu duration dạng chuỗi —
  khuôn có sẵn là `debounce_ms: Option<u64>` (`settings_content/src/workspace.rs:1024`) và
  `AfterDelay { milliseconds: DelayMs }` (`:541`). (red team Finding 1)
- FR6: Project active **không bao giờ** bị hibernate, kể cả khi cầu chì bộ nhớ (Phase 6) kích.

**Non-functional**

- NFR1: Transition không được block main thread quá 1 frame.
- NFR2: Máy trạng thái phải test được không cần LSP/worktree thật (dùng `TestAppContext` + fake fs).

## Architecture

```
                  activate(ws)
   ┌──────────┐ ──────────────> ┌────────┐
   │ Hibernated│                 │ Active │ <── luôn đúng 1 project mỗi window
   └──────────┘ <────────────── └────────┘
         ▲        timer hết hạn      │ mất focus
         │                           ▼
         └──────────────────── ┌──────┐
                               │ Warm │  (giữ timer)
                               └──────┘

Project { activity: ProjectActivity }              ← trạng thái
MultiWorkspace { hibernate_timers: HashMap<EntityId, Task<()>> }  ← quyết định
```

`hibernate_timers` khoá theo `EntityId` của workspace. Drop entry = cancel timer (ngữ nghĩa `Task` của
GPUI). Không cần huỷ tay.

**Vì sao trạng thái nằm ở `Project` chứ không ở `Workspace`:** LSP, worktree, prettier, terminal đều
thuộc `Project` — đặt state ở đâu khác thì mọi consumer phải đi ngược lên tìm workspace.

**Cần xác minh trước, không được giả định** (red team Finding 8): plan bản đầu lấy lý do "hai workspace
chia một `Entity<Project>`" làm căn cứ. Không tìm được bằng chứng cho việc chia sẻ đó —
`find_or_create_workspace_with_source_workspace` (`multi_workspace.rs:1113-1139`) chỉ nhận
`provisional_project_group_key`. Bước 0 của phase này là đọc đường tạo `Project` và ghi lại sự thật:
nếu mỗi workspace luôn có `Project` riêng thì bỏ risk row "double transition", nếu chia sẻ thật thì giữ
và test nó.

## Related Code Files

**Modify**

- `crates/project/src/project.rs` — `enum ProjectActivity`, field `activity`, `activity()`,
  `set_activity()`, biến thể `Event::ActivityChanged`; khởi tạo `Active` ở cả 2 chỗ `Project::local`/
  `Project::remote` (`:1310`, `:1558` là mốc `active_entry`)
- `crates/workspace/src/multi_workspace.rs` — field `hibernate_timers`, `fn schedule_hibernate`,
  `fn wake_project`, xử lý trong nhánh `activate()` + subscribe `ActiveWorkspaceChanged`
- `crates/settings_content/src/workspace.rs` — `hibernate_after_ms: Option<u64>` trong `MultiProjectContent`
- `crates/workspace/src/workspace_settings.rs` — resolve thành `Option<Duration>`
- `assets/settings/default.json` — `"hibernate_after_ms": 300000`, comment nói rõ `null` = tắt hibernate

**Create**

- Không file mới. `ProjectActivity` đặt trong `project.rs` (rule: ưu tiên file có sẵn).

## Implementation Steps

0. Đọc đường tạo `Project` (`project.rs` `Project::local`) và kết luận: hai workspace có bao giờ chia
   một `Entity<Project>` không? Ghi kết luận vào phase file này trước khi viết code.
1. `#[derive(Copy, Clone, PartialEq, Eq, Debug)] pub enum ProjectActivity { Active, Warm, Hibernated }`
   trong `project.rs`, cạnh các type vòng đời khác.
2. Field `activity: ProjectActivity` (init `Active`), accessor, và `set_activity` — chỉ emit khi giá trị
   thật sự đổi, rồi `cx.notify()`.
3. `Event::ActivityChanged(ProjectActivity)` vào enum `Event` của project.
4. Trong `MultiWorkspace`: `hibernate_timers: HashMap<EntityId, Task<()>>`.
5. `fn wake_project(&mut self, workspace: &Entity<Workspace>, cx)`: xoá timer entry, `set_activity(Active)`.
6. `fn schedule_hibernate(&mut self, workspace: &Entity<Workspace>, cx)`: `set_activity(Warm)`; nếu
   `hibernate_after` là `None` thì dừng ở đây; ngược lại spawn task `timer(d).await` rồi
   `set_activity(Hibernated)` qua weak handle, lưu task vào map.
7. Nối vào `activate()`: `wake_project(new)` + `schedule_hibernate(old)`. Cẩn thận trường hợp
   `old == new` (đã return sớm ở đầu `activate`).
8. Nối vào `detach_workspace`/`close_workspace`/`remove`: xoá timer entry để không rò task.
9. Đọc lại settings khi đổi: dùng `cx.observe_global_in::<SettingsStore>` như khuôn đã có ở
   `MultiWorkspace::new` (:283). `hibernate_after` đổi thành `null` → huỷ mọi timer đang chờ.
10. `./script/clippy` + `cargo test -p project -p workspace`.

## Todo List

- [ ] `ProjectActivity` + field + accessor + event
- [ ] Setting `hibernate_after` (3 chỗ)
- [ ] `hibernate_timers` + `wake_project` + `schedule_hibernate`
- [ ] Nối vào `activate()` / `detach` / `close` / `remove`
- [ ] Observe settings, huỷ timer khi tắt
- [ ] Test: A→B→A trong ngưỡng ⇒ A không bao giờ tới `Hibernated`
- [ ] Test: A→B, đẩy executor qua ngưỡng ⇒ A `Hibernated`, B `Active`
- [ ] Test: `hibernate_after_ms: null` ⇒ không có transition nào ngoài `Active`/`Warm`
- [ ] **Test bất biến (chuyển từ Phase 5, red team Finding 12):** project background có terminal đang
      chạy tiến trình dài ⇒ mọi transition tới `Hibernated` **không** dừng/suspend/kill tiến trình đó,
      và output vẫn tới sau khi wake. Đây là bất biến quan trọng nhất của cả plan — nó ở đây vì Phase 5
      có thể kết thúc bằng no-op.
- [ ] Test: close workspace lúc timer đang chờ ⇒ không panic, không rò task
- [ ] `./script/clippy` sạch

## Success Criteria

- Máy trạng thái test được mà không cần LSP thật; toàn bộ test dùng
  `cx.background_executor().timer` / `advance_clock`, không `smol::Timer`.
- Không transition nào xảy ra khi chỉ có một project trong window.
- `cargo test -p project -p workspace` xanh.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Hai workspace chia một `Project` → double transition | **Chưa xác minh** | Bước 0 kết luận trước. `set_activity` idempotent trong mọi trường hợp (chỉ emit khi giá trị đổi) nên vô hại kể cả khi có chia sẻ |
| Rò `Task` khi workspace bị drop lúc timer đang chờ | Trung bình | Timer dùng `WeakEntity`; xoá entry ở mọi đường thoát (`detach`, `close`, `remove`, `on_release`) |
| Ngưỡng mặc định quá ngắn → user thấy giật khi quay lại | Trung bình | Mặc định 5 phút; Phase 6 đo rồi hiệu chỉnh bằng số thật |
| `Duration` trong settings không có khuôn sẵn cho chuỗi `"5m"` | Thấp | Bắt chước `focus_follows_mouse.debounce`; nếu chỉ có dạng số thì dùng số giây, đừng tự phát minh parser |

## Security Considerations

Không có. Một lưu ý: không log đường dẫn project ra log ở mức info khi transition — giữ ở `debug` để
không rỉ cấu trúc thư mục của người dùng vào log file.

## Next Steps

Phase 3/4/5 mỗi phase nối một loại tài nguyên vào `Event::ActivityChanged`. Chúng độc lập nhau và chạy
song song được.
