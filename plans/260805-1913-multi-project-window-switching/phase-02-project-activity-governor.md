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

### Kết luận Bước 0 — hai workspace có chia một Entity<Project> không?

**Không.** Rà toàn bộ đường tạo `Workspace` (mọi nơi gọi `Workspace::new`/`Workspace::new_local`) trong
`crates/workspace/src/workspace.rs` và `crates/workspace/src/multi_workspace.rs`:

- `Workspace::new_local` (`workspace.rs:1840`) — gọi `Project::local(...)` mới ngay dòng đầu hàm, chỉ
  dùng cho workspace được tạo trong lần gọi đó.
- `open_workspace_by_id` (`workspace.rs`, quanh dòng 9540) — cùng khuôn: `Project::local(...)` mới ở
  đầu hàm, một project cho một workspace.
- `find_or_create_workspace_with_source_workspace`'s nhánh remote/SSH (`multi_workspace.rs`) — gọi
  `Project::remote(...)` mới cho mỗi lần connect, đưa thẳng vào `open_remote_project_inner` → một
  workspace duy nhất.
- Ba nơi tạo workspace trống trong `multi_workspace.rs` (`close_workspace`'s fallback :966,
  `remove_project_group`'s fallback :1029, `create_test_workspace` :1881) — mỗi nơi gọi
  `Project::local(...)` mới riêng.
- Các nhánh "đã có workspace khớp path" (trong `open_paths`, `find_or_create_local_workspace_with_source_workspace`,
  `find_or_create_workspace_with_source_workspace`) không tạo `Workspace` mới — chỉ `activate()` cái đã
  tồn tại (cái đó đã có `Project` riêng từ khi nó được tạo). Không có đường nào lấy một `Entity<Project>`
  đã tồn tại rồi nhồi vào một `Workspace::new` thứ hai.

**Tác động:** dòng risk "Hai workspace chia một Project → double transition" trong bảng Risk Assessment
bị loại — không cần code phòng hộ cho double-transition. Vẫn giữ `set_activity` idempotent (chỉ emit
khi giá trị thật đổi) như đã định, vì đó là vệ sinh code tốt nói chung, không phải vì cần cho case này.

**Bổ sung (đối chiếu độc lập, cùng session implement):** hai điểm đáng ghi thêm, không đổi kết luận trên:

- `Workspace::new` (`workspace.rs:1440`) nhận `project: Entity<Project>` **theo giá trị** — không có gì
  ở mức kiểu dữ liệu ngăn một lời gọi tương lai truyền vào một project đã thuộc workspace khác; đây là
  invariant giữ bằng quy ước ở toàn bộ call site hiện tại (đã grep hết, không loại nào ngoại lệ), không
  phải do compiler khoá. Không cần sửa gì ở phase này, chỉ đáng gắn cờ cho review nếu Phase 3+ thêm một
  đường tái dùng project.
- Hai **window** khác nhau (hai `MultiWorkspace` khác nhau) cùng mở một thư mục tạo ra **hai
  `Entity<Project>` độc lập**, không phải chia sẻ — vì `workspace_for_paths_excluding` (`:1091`) chỉ soi
  `self.workspaces()` của chính `MultiWorkspace` đó, không có registry toàn cục so khớp path xuyên
  window. Hai máy trạng thái độc lập trên hai entity độc lập thì không có nguy cơ double-transition.

### Phát hiện trước khi viết code — FR5 dùng `null` để tắt sẽ không hoạt động, đổi sang `0`

Trước khi thêm field settings, lần theo cách `SettingsStore` merge nhiều lớp
(`crates/settings/src/settings_store.rs::recompute_values`, :1311-1329): `merged` khởi tạo từ
`default_settings`, sau đó lần lượt `merged.merge_from(&user_settings.content)` rồi các lớp khác đè
lên trên. `MergeFrom` cho `Option<T>` (`crates/settings_content/src/merge_from.rs:62-74`) viết:

```rust
fn merge_from(&mut self, other: &Self) {
    let Some(other) = other else { return; };  // other = None (từ lớp override) → bỏ qua, giữ nguyên self
    ...
}
```

**Hậu quả:** nếu field là `Option<u64>` và default.json đặt `300000`, một user đặt
`"hibernate_after_ms": null` trong settings.json CỦA RIÊNG HỌ sẽ deserialize thành `None` ở lớp đó —
giống hệt việc không viết field này ra — nên `merge_from` bỏ qua, và giá trị merge cuối cùng vẫn là
`300000` từ default.json. **`null` do user set sẽ KHÔNG bao giờ tắt được hibernation** nếu default
không phải cũng là `null`. Đây không phải suy đoán: cùng cơ chế này là lý do
`edit_debounce_ms`/`scroll_debounce_ms` (`crates/settings_content/src/language.rs:725-734`, default
700/50 trong `default.json`) — hai field có hình dạng giống hệt yêu cầu của FR5 (Option<u64>, có default
thật, cần một cách để user tắt) — dùng **`0` làm giá trị tắt**, không dùng `null`
("Set to 0 to disable debouncing."). Đường resolve xuống cuối
(`crates/language/src/language_settings.rs:716-717` rồi `crates/editor/src/editor.rs:1372-1378`
`fn debounce_value(ms: u64) -> Option<Duration> { if ms > 0 { Some(..) } else { None } }`) xác nhận đây
là khuôn chính thức của repo cho đúng nhu cầu này, không phải một cách làm tạm.

**Quyết định:** implement `hibernate_after_ms: Option<u64>` (giữ đúng type FR5 yêu cầu, vẫn `Option` vì
lý do layering của settings, không phải vì `None` mang nghĩa "tắt") với **`0` là giá trị tắt hibernate**,
default vẫn `300000`, resolve thẳng xuống `MultiProjectSettings.hibernate_after: Option<Duration>`
(`0` → `None`, khác `0` → `Some(Duration::from_millis(ms))`) ngay tại `workspace_settings.rs` —
mirror `debounce_value` nhưng đặt ở lớp resolve settings thay vì ở consumer, để `MultiWorkspace` không
phải biết về giá trị `0` "phép thuật". FR5, Implementation Step 9, Todo List, và Related Code Files bên
dưới đã sửa theo quyết định này. Đây là quyết định kỹ thuật duy nhất trong phase này lệch khỏi câu chữ
gốc của phase file — lệch có chủ đích, có bằng chứng, ghi lại ở đây theo đúng yêu cầu "không vá sau khi
đã viết code".

## Requirements

**Functional**

- FR1: `Project::activity() -> ProjectActivity` và `Project::set_activity(...)`, emit
  `Event::ActivityChanged` khi đổi.
- FR2: Workspace được activate → project của nó về `Active` ngay lập tức (đồng bộ, không qua task).
- FR3: Workspace rời focus → `Warm`, hẹn timer `hibernate_after`; hết hạn → `Hibernated`.
- FR4: Workspace được activate lại trong lúc `Warm` → huỷ timer, về `Active`, **không** có transition
  nào khác xảy ra.
- FR5: Setting `workspace.multi_project.hibernate_after_ms` — **số millisecond** (default `300000`) hoặc
  `0` để tắt hẳn. Không phải chuỗi `"5m"`: hệ settings của Zode không có kiểu duration dạng chuỗi —
  khuôn có sẵn là `debounce_ms: Option<u64>` (`settings_content/src/workspace.rs:1024`) và
  `AfterDelay { milliseconds: DelayMs }` (`:541`). (red team Finding 1). **Sửa trong implementation
  session:** giá trị tắt là `0`, không phải `null` — `null` do user set ở lớp settings của họ không thể
  override một default không phải `null` (xem "Phát hiện trước khi viết code" ở Key Insights).
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
- `assets/settings/default.json` — `"hibernate_after_ms": 300000`, comment nói rõ `0` = tắt hibernate
  (không phải `null` — xem Key Insights)

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
   `MultiWorkspace::new` (:283). `hibernate_after` đổi thành `0` (tắt) → huỷ mọi timer đang chờ.
10. `./script/clippy` + `cargo test -p project -p workspace`.

## Todo List

- [ ] `ProjectActivity` + field + accessor + event
- [ ] Setting `hibernate_after` (3 chỗ)
- [ ] `hibernate_timers` + `wake_project` + `schedule_hibernate`
- [ ] Nối vào `activate()` / `detach` / `close` / `remove`
- [ ] Observe settings, huỷ timer khi tắt
- [ ] Test: A→B→A trong ngưỡng ⇒ A không bao giờ tới `Hibernated`
- [ ] Test: A→B, đẩy executor qua ngưỡng ⇒ A `Hibernated`, B `Active`
- [ ] Test: `hibernate_after_ms: 0` ⇒ không có transition nào ngoài `Active`/`Warm`
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
| ~~Hai workspace chia một `Project` → double transition~~ | **Đã loại** | Bước 0 kết luận: không có đường code nào tạo ra việc chia sẻ này (xem "Kết luận Bước 0" ở Key Insights). `set_activity` vẫn giữ idempotent như vệ sinh code chung, không phải vì cần cho case này |
| Rò `Task` khi workspace bị drop lúc timer đang chờ | Trung bình | Timer dùng `WeakEntity`; xoá entry tường minh ở đường thoát của từng workspace (`detach_workspace`, được `close_workspace`/`remove` gọi tới cho mọi workspace chúng xoá). Không cần `on_release` riêng cho `hibernate_timers` — nếu cả `MultiWorkspace` bị drop (đóng cửa sổ) mà không đi qua từng `detach` lẻ, `Drop` mặc định của field `HashMap<EntityId, Task<()>>` tự cancel mọi timer còn sót, đúng hành vi mong muốn cho một entity đang biến mất |
| Ngưỡng mặc định quá ngắn → user thấy giật khi quay lại | Trung bình | Mặc định 5 phút; Phase 6 đo rồi hiệu chỉnh bằng số thật |
| `Duration` trong settings không có khuôn sẵn cho chuỗi `"5m"` | Thấp | Bắt chước `focus_follows_mouse.debounce`; nếu chỉ có dạng số thì dùng số giây, đừng tự phát minh parser |
| `null` không thể override một default không phải `null` qua `MergeFrom` (khiến FR5 gốc không hoạt động) | Trung bình | **Đã sửa:** dùng `0` làm giá trị tắt, mirror đúng khuôn `edit_debounce_ms`/`scroll_debounce_ms` (xem Key Insights) |

## Security Considerations

Không có. Một lưu ý: không log đường dẫn project ra log ở mức info khi transition — giữ ở `debug` để
không rỉ cấu trúc thư mục của người dùng vào log file.

## Next Steps

Phase 3/4/5 mỗi phase nối một loại tài nguyên vào `Event::ActivityChanged`. Chúng độc lập nhau và chạy
song song được.
