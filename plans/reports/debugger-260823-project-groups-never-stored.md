# Chẩn đoán — project của window không bao giờ được lưu

**Ngày:** 2026-08-23 · **Nhánh:** `feat.release-v0.1.1` · **Phạm vi:** ngoài plan
`260823-1412-project-avatar-menu-and-reorder`; thuộc vùng của
`260805-1913-multi-project-window-switching` (đang `pending`)

## Triệu chứng người dùng báo

1. Bấm `+` trên rail thêm thư mục mới → **project cũ mất**; phải thêm lần nữa mới thấy
   hai project.
2. Mở app lên không nhớ đã từng mở những project nào; mặc định luôn là project `zode`.

## Nguyên nhân gốc

`MultiWorkspace::derived_project_groups` **tổng hợp** group của workspace đang active khi
`project_groups` (danh sách *được lưu*) không có nó. Đó là fallback lúc *vẽ*, không phải
state. Với một window mở bằng `zode <folder>`, không đường nào ghi group đó xuống.

Chứng minh (test `the_windows_own_project_is_stored_before_it_can_be_lost`, phiên bản
chẩn đoán): `displayed=1 stored=0`.

Từ một dữ kiện đó ra cả hai triệu chứng:

- Entry tổng hợp **đi theo cái đang active**. Mở project thứ hai → active đổi → entry cũ
  biến mất. Falsify bản sửa cho ra đúng chữ: `got []` — rail trống hẳn.
  Lần thêm sau mới thấy hai, vì lúc đó `ensure_project_group_state` đã kịp ghi project thứ hai.
- `serialize()` ghi **danh sách được lưu**. Rỗng thì không có gì để lưu, nên không có gì để nạp.

## Sửa

`MultiWorkspace::store_displayed_projects(cx)` — ghi những project rail đang hiển thị mà
danh sách lưu chưa có. Gọi ở đầu `activate()` (trước khi active đổi) và đầu `serialize()`
(trước khi dựng record).

**Chỉ chạy khi danh sách lưu rỗng**, và cái bound đó chịu lực chứ không phải tối ưu:
`remove_project_group` xoá group khỏi danh sách **rồi mới** đóng workspace của nó, nên
trong khoảng đó group ấy vẫn đang active và vẫn được tổng hợp trở lại. Bản đầu của tôi
chạy vô điều kiện và **hồi sinh đúng group đang bị xoá**, làm fallback của removal nhảy
sang project khác — `test_remove_project_group_falls_back_to_neighbor` đỏ với
`no state of type fs::GlobalFs exists`, vì nó rơi vào nhánh *tạo* workspace mới.

Cũng không dùng `materialize_project_groups` ở đây: hàm đó viết lại **thứ tự** lưu theo
thứ tự hiển thị — đúng cho kéo thả (index phải khớp hai bên), sai ở mọi chỗ khác, vì
`remove_project_group` chọn fallback **theo vị trí**.

## Phần thứ hai của triệu chứng 2: không phải bug

`Makefile:21` — `PROJECT ?= .`, và target `dev` chạy `$(BIN) $(PROJECT)`. Nên `make dev`
= `zode .`, tức **truyền tường minh** thư mục hiện tại.

`crates/zed/src/main.rs:762-779`: khi khởi động có open request (có path trên dòng lệnh),
nhánh `handle_open_request` chạy và `restore_or_create_workspace` **không bao giờ** được
gọi. Chỉ khi không có path mới restore. Đây là hành vi của `zed <path>`, không phải lỗi.

Muốn thấy restore: `make dev PROJECT=` (chính Makefile ghi "PROJECT= opens nothing").

## Chưa kiểm được

Vòng lặp đầy đủ *ghi → khởi động lại → nạp* chỉ được kiểm ở tầng ghi (`project_group_keys`
sau `activate`/`serialize`) và ở đường restore đã có test riêng. Chưa chạy tay lần nào với
`make dev PROJECT=`.

## Trạng thái

`workspace` 240 · `sidebar` 24 · `ui` 45 · clippy sạch · `cargo check --bin zode` exit 0.

---

# Vòng hai — kéo thả: model đổi, màn hình không đổi

## Triệu chứng

Kéo thả đổi thứ tự "vẫn không được", thử kéo lên trên cùng và xuống dưới cùng. Người dùng
đoán: section danh sách quá sát nên không nhận được điểm dừng.

## Hai lỗi, cả hai của tôi

**1. `move_project_group` không phát `ProjectGroupsChanged`.** Sidebar chỉ rebuild
`contents.rail_entries` khi nhận event đó, nên rail **vẽ lại bằng danh sách cũ**: model đổi,
màn hình không đổi. Và cú kéo tiếp theo đọc index từ entries cũ nên tính ra `to == from` →
return sớm → thật sự không làm gì.

Test cũ của tôi không bắt được vì nó đọc **model** (`project_groups(cx)`), không đọc cái rail
vẽ. Đã sửa: `rail_order()` đọc `contents.rail_entries`. Falsify bằng cách bỏ event: rail giữ
`["/root_a","/root_b"]` — đúng triệu chứng.

**2. Mọi chỗ trống bị coi là "về cuối".** Handler tôi thêm ở cột rail (bản sửa cho High của
reviewer) append bất kể thả ở đâu. Nên kéo lên **trên cùng** → nhảy xuống cuối (ngược ý), kéo
xuống **dưới cùng** → đã ở cuối → không đổi gì. Cả hai đọc ra "không hoạt động". Đây đúng là
FR17 (nửa trên/nửa dưới) mà tôi đã cố tình thu hẹp ở phase 03 — người dùng gặp đúng chỗ đó.

Làm lại: `Sidebar::drop_gap` là index **khe** (0 = trên cùng, len = dưới cùng). Mỗi hàng nhận
khe theo nửa trên/nửa dưới; khoảng trống dưới hàng cuối là một element riêng nhận `len`;
container chỉ **xoá** khi con trỏ ra ngoài. Có vạch chỉ vẽ tại khe.

**Quy tắc rút ra:** `on_drag_move` **không** lọc theo hitbox (khác `on_mouse_move`, xem
`div.rs:302` vs `322`), và thứ tự dispatch của nó không phải outer→inner như tôi giả định — đo
được: handler container chạy **sau** hàng và ghi đè. Nên mỗi element chỉ được nhận khi con trỏ
nằm trong bounds *của chính nó*, và các vùng không chồng nhau. Khi đó thứ tự không còn quan trọng.

## Còn chưa giải thích

Menu chuột phải mở rồi tắt ngay. Không tái hiện được: test right-click → menu vẽ ra, di chuột
3px, đi qua 4 frame refresh — menu vẫn còn và vẫn giữ focus. Chưa có root cause.

## Trạng thái

`sidebar` 25 · `workspace` 240 · `ui` 45 · clippy sạch · build exit 0.

---

# Vòng ba — menu tắt ngay: `Sidebar::focus_in` giành focus

## Bằng chứng (stderr từ máy người dùng, 6 điểm log tạm)

```
menu opened, focus now = FocusId(29v3)
focusing menu FocusHandle(FocusId(25v3)), focus was FocusId(29v3)
Sidebar::focus_in; focus now = FocusId(25v3)
ContextMenu blurred -> cancel; focus now = FocusId(16v1)
menu got DismissEvent, focus now = FocusId(16v1)
```

## Nguyên nhân gốc

`Sidebar::focus_in` (`navigation.rs:33`) chuyển focus sang filter editor mỗi khi focus vào
**bất cứ đâu** trong cây của sidebar, chỉ trừ khi filter editor đã có focus. `on_focus_in`
nổ cho cả subtree, nên khi `right_click_menu` focus menu (25) sau hai frame, sidebar coi đó
là "sidebar vừa được focus" và giao focus cho filter editor (16). Menu mất focus →
`ContextMenu` có subscription `on_blur` gọi thẳng `cancel` (`context_menu.rs:272-294`) →
`DismissEvent`. Menu vẽ một frame rồi chết.

Đáng ghi: `ManagedView` **không** tự dismiss khi mất focus — nó chỉ là marker trait. Việc
dismiss là một `cx.emit` cụ thể bên trong `ContextMenu`, và đường đó là `on_blur`.

## Sửa

`focus_in` chỉ chuyển tiếp khi **chính handle của sidebar** đang được focus
(`self.focus_handle.is_focused(window)`). Focus vào một thứ *bên trong* sidebar thì đã có
chủ, và giành nó là sai.

## Không có test tự động cho chỗ này

Tôi viết một test rồi **bỏ**: nó xanh cả khi tôi tháo guard ra. Lý do: focus chỉ "dính" khi
đã có frame được vẽ và element nằm trong dispatch tree; trong harness đó focus không bao giờ
dính, nên cả hai assertion đều vô nghĩa. Ship một test đúng-ngẫu-nhiên còn tệ hơn không có
test. Bằng chứng ở đây là trace stderr thật; xác minh là người dùng thử lại.

## Phần lưu nhiều project: đã chạy, đọc trực tiếp từ máy người dùng

`scoped_kv_store` namespace `multi_workspace_state`, window `4294967297`:

```
- /Users/tgiap.dev/devs/zode-kit/zode | initials: null | colour: null
- /Users/tgiap.dev/devs/zode-kit/web  | initials: null | colour: null
```

**Cả hai project đã được ghi**, kèm hai field mới. Nên `store_displayed_projects` làm đúng
việc của nó. Lý do mở lại không thấy là `make dev` = `zode .` → nhánh `handle_open_request`
→ `restore_or_create_workspace` không bao giờ chạy (`main.rs:762-779`). Muốn thấy restore:
`make dev PROJECT=`.

## Trạng thái

`sidebar` 25 · `workspace` 240 · `ui` 45 · clippy sạch · binary đã build lại, log tạm đã xoá
sạch (grep `ZODE-DBG` = 0).

---

# Vòng bốn — cột công cụ đổi chiều rộng mỗi lần chuyển tab

## Triệu chứng người dùng báo

Project panel có scroll dọc, agent history nhiều khi không có, nên "kích thước width nó
khác nhau"; chuyển qua lại width không cố định.

## Không phải scrollbar

Cả hai panel dùng scrollbar **overlay** cho trục dọc:

- `project_panel.rs:7186` — `Scrollbars::for_settings::<ProjectPanelScrollbarProxy>()`
- `session_history/list.rs:271` — `Scrollbars::new(ScrollAxes::Vertical)`

Thứ duy nhất biến scrollbar thành `ReservedSpace::Track` (tức chiếm chỗ layout) là
`with_track_along` (`ui/src/components/scrollbar.rs:451-455`). Project panel chỉ gọi nó cho
trục **ngang**, và chỉ khi bật `horizontal_scroll` — đó là mép dưới, không phải chiều rộng.
Nên scrollbar dọc chiếm 0px ở cả hai panel. Giả thuyết của người dùng sai, dù triệu chứng đúng.

## Nguyên nhân gốc

`Workspace::render_dock` (`workspace.rs:7942`) lấy chiều rộng của dock từ **panel đang hiện**:

```rust
let size_state = dock.stored_panel_size_state(panel.as_ref());
...unwrap_or_else(|| panel.default_size(window, cx))
```

Dock phải là **một cột**: `takes_turns()` đúng cho Right dock, header của nó là một dải tab và
chỉ một panel được vẽ. Nhưng mỗi `PanelEntry` giữ `size_state` riêng, và default riêng —
project panel 240px (`assets/settings/default.json:782`), agent history 420px
(`session_history/panel.rs:255`). Hai panel, hai chiều rộng, một cột: chiều rộng buộc phải nhảy.

Kèm theo: `resize_active_panel` ghi vào entry **đang active**, nên kéo mép cột lúc agent history
đang hiện thì lần chuyển về project panel không thấy giá trị đó.

## Sửa

`Dock::size_governing_index()` — một chỗ duy nhất trả lời "entry nào quyết định kích thước dock".
Dock takes-turns → entry đầu (danh sách xếp theo `activation_priority`, nên entry đầu là panel
chính của cột); dock xếp chồng → entry active, y như cũ.

Mọi điểm đọc/ghi *kích thước dock* đi qua nó: `active_panel_size`, `stored_active_panel_size`,
`resize_active_panel`, và `render_dock` (qua `size_governing_panel()`, lấy cả `has_flexible_size`
và `default_size` từ cùng một chỗ — đọc chiều rộng ở panel này mà lấy chế độ fixed/flex ở panel
kia là đường thứ ba để cột xê dịch).

`min_size` **vẫn** của panel đang hiện: một panel không được vẽ nhỏ hơn mức nó cần, đó là ràng
buộc thật chứ không phải cột lung lay.

Không đụng `stored_panel_size_state(panel)` dùng cho serialize từng panel — chỗ đó đúng là
per-panel.

## Phạm vi ảnh hưởng

`dock_header_draws_panel` = `Left | Right` trừ `OWN_COLUMN_POSITION` (= Left), nên **chỉ Right
dock** takes turns. Bottom dock (terminal, debugger) và cột rail bên trái không đổi hành vi.

## Test

`switching_tabs_in_a_header_dock_keeps_the_column_width` — hai panel 240/420 trong Right dock,
đo `debug_bounds("dock-panel")` trước và sau khi chuyển tab. Đo **hình vẽ thật**, không đo
accessor: accessor và `render_dock` giải kích thước riêng nhau, nên khớp với accessor sẽ không
chứng minh gì về cái mắt thấy.

Falsify (cho `size_governing_index` trả về `active_panel_index` như cũ):
`went from 240px to 420px` — đúng triệu chứng, đúng con số.

## Giới hạn đã biết

Nếu panel chính bị **gỡ** khỏi Right dock, entry đầu thành panel khác và cột lấy chiều rộng đã
lưu của panel đó. Trên thực tế không xảy ra: cả hai panel đều ghim vị trí qua
`position_is_valid`. Không xây phòng bị cho nó (YAGNI).

## Trạng thái

`workspace` 241 · `project_panel` 100 · `database_ui` 48 · `sidebar` 25 · `agent_ui` 29 ·
`cargo build --bin zode` exit 0.

`cargo clippy -p workspace --all-targets` đỏ ở E0004 `RemoteConnectionIdentity::Mock` — có sẵn
từ trước, đã chứng minh bằng stash trong phiên trước.

Một lần `agent_ui::every_agent_the_new_menu_offers_can_actually_be_opened` đỏ khi chạy ba crate
song song (`test_scheduler.rs:111`), xanh 4/4 khi chạy riêng và xanh ở HEAD — theo tải máy, không
theo source.
---

# Vòng năm — cột database: hai border, và driver không tồn tại

Bốn việc trong một yêu cầu: hai lỗi (chẩn đoán ở đây) và hai tính năng.

## 1. Hai border trên cột database

`Dock::render` (`dock.rs:1934`) bọc **mọi** dock trong một card: `p(SURFACE_MARGIN)`
+ `rounded(SURFACE_ROUNDING)` + `border_1` + `bg(panel_background)`. Không có nhánh
loại trừ own column. `DatabasePanel::render` vẽ **card thứ hai giống hệt** bên trong.

Comment ngay tại chỗ đó nói ngược lại: *"`Dock::render` leaves the dock chrome off
for both"*. Nó đúng vào lúc được viết và sai từ khi card dời vào `Dock::render` — lời
khẳng định sống lâu hơn sự thật nó mô tả. Đây là lý do tôi tìm sai chỗ lúc đầu.

Full screen cũng đi qua dock (`render_window_filling_column` → `render_dock`), nên bỏ
card của panel là an toàn ở cả hai chế độ.

**Sửa:** gộp hai element thành một, bỏ card của panel. `grep SURFACE_MARGIN` xác nhận
`database_ui` là nơi duy nhất còn sót; "agent column" trong comment cũ đã sạch từ trước.

**Test:** `the_database_column_draws_a_single_surface_card` — đo `database-tree-column`
so với `dock-panel`. Một card = 3px + 1px = 4px. Falsify: `got 8px against one card of 4px`.

## 2. `command not found: zode-db-postgres`

Hai lớp, cả hai đều là lỗi thật.

**Lớp build.** `target/debug/zode-db-*` không tồn tại. Ba driver là crate riêng ngoài
`default-members`; `make build` chỉ chạy `cargo build -p zode --bin zode`.
`script/build-database-drivers` đã có sẵn và `script/bundle-*` đã dùng nó — chỉ vòng dev
là không ai gọi. Sửa: `make build` gọi thêm `make drivers`. Đã xác minh: sau `make drivers`,
cả ba binary nằm trong `target/debug/`, đúng chỗ `driver_path()` tìm (`current_exe().parent()`).

**Lớp phát hiện.** `driver_path` biết file không tồn tại (`.filter(|p| p.exists())`) rồi
**bỏ đi** tri thức đó, trả về tên trần. Tên trần được đưa cho một **shell**
(`transport.rs:60-62` dùng `ShellBuilder`), nên `spawn()` *thành công*, shell in
`command not found` vào cái mà client đọc là **stderr của driver**, và vòng reconnect lặp
lại mỗi ~4 giây. Sự thật duy nhất đáng biết — binary chưa từng được build — không xuất
hiện ở đâu, còn thông báo xuất hiện thì trông như driver đang nói.

Chính doc comment của registry đặt ra chuẩn ngược lại: *"a driver that is supposed to be
there but is missing should say so plainly rather than quietly not existing."* Nó không làm vậy.

Sửa: giữ nguyên fallback PATH (là hành vi có chủ ý, có ghi chép), nhưng `log::warn!` một
lần lúc đăng ký, nêu đúng file và cách build. Người dùng bản cài đặt bình thường không
thấy gì; checkout thiếu driver thấy ba dòng rõ ràng, một lần.

**Không có test tự động** cho dòng log này. Kiểm chứng là `ls target/debug/zode-db-*`
sau `make drivers`, đã chạy.

## 3–4. Hai tính năng

Quyết định của người dùng qua `AskUserQuestion`: **phiên độc lập** (mỗi host có
connection/scratch riêng), và **đóng cột** khi mở ra ngoài.

`Host::{Column, Standalone}` — đặt tên theo *hành vi*, không theo chỗ đứng: cột được
ghim ở một chiều rộng ai đó đã kéo nên xếp chồng; tab và cửa sổ nhận chiều rộng người
khác định nên tự đo. Ba chỗ, hai hành vi.

Chiều rộng phải **đo** (`canvas` + `cx.defer`, đọc trễ một frame) vì không ai nói cho
view biết: tab rộng bằng pane, cửa sổ rộng bằng lúc kéo. Breakpoint 720px, unmeasured
đoán là rộng — đoán sai kiểu nào cũng mất một frame, và tab mới gần như luôn rộng hơn.

`DatabasePanel::standalone` nhận `WeakEntity<Workspace>` + `Arc<LanguageRegistry>` chứ
không nhận `&Workspace`: cả hai call site không thể đưa được. Một chạy trong workspace
đang bị lease bởi `register_action`, một dựng view trong context của **cửa sổ khác** nơi
không thể borrow `&Workspace` cạnh `&mut App` đang tạo entity.

**`WindowKind::Floating`, không phải `PopUp`** — và đây không phải chuyện thẩm mỹ. Trên
macOS `PopUp` đặt `NSWindowStyleMaskNonactivatingPanel` (`gpui_macos/src/window.rs:672`):
cửa sổ không bao giờ active, không nhận focus bàn phím. Đúng cho cửa sổ thông báo, vô
dụng cho chỗ gõ SQL. `Floating` là panel activating bình thường ở `NSFloatingWindowLevel`,
và vì đó là *window level* toàn cục chứ không phải quan hệ cha-con, nó đứng trên cửa sổ
của app khác — đúng yêu cầu.

**Test:**
- `opening_in_an_editor_tab_takes_it_out_of_the_column` — dispatch **action** thật, không
  gọi hàm: một nút dispatch action chưa ai register thì im lặng không làm gì, và crate
  này đã ship đúng lỗi đó một lần. Falsify: bỏ `hide_the_column` → đỏ đúng chỗ.
- `opening_in_a_floating_window_adds_a_window_without_aborting` — bẫy workspace-lease là
  thứ sẽ abort trong production. Falsify: bỏ `register_action` → `went from 1 to 1`.
- `a_wide_editor_tab_stands_the_table_list_beside_the_data` — **đã sửa sau khi falsify**:
  nó đỏ qua assertion *khác* chỗ tôi định (layout xếp chồng không vẽ `data-view` khi chưa
  có connection), nên dòng hình học chưa bao giờ là thứ phân biệt. Đã viết lại doc và
  message để test nói đúng điều nó chứng minh.
- `the_breakpoint_is_inclusive_and_unmeasured_stands_side_by_side` — luật thuần.

`crate::init(cx)` được thêm vào `init_test` để test chạm được nút theo đường một cú click
đi, không phải gọi hàm phía sau. 50 test cũ vẫn xanh sau khi thêm.

## Chưa kiểm được

`WindowKind::Floating` và window level: cửa sổ trong test không phải `NSPanel`, nên
"nổi trên app khác" nằm ngoài tầm mọi test ở đây. Cần mắt người trên bản build thật.

## Trạng thái

`database_ui` 53 · `zode` 56 · clippy sạch (`database_ui`, `zed_actions`) ·
`make build` exit 0 và có chạy bước driver.
---

# Vòng sáu — view đứng riêng vẽ sai, và cửa sổ nổi không tìm lại được

## 1. Tab / cửa sổ nổi: cây bảng trống

**Lỗi của tôi ở vòng năm.** Tôi đưa `side_by_side()` vào nhánh trên cùng nhưng để hai
renderer con vẫn đọc `self.full_screen`:

- `render.rs:318` `render_tree` — chọn `w(tree_width).h_full().flex_none()` hay
  `h(tree_height)` / `flex_1()`
- `result_grid.rs:109` `render_nothing_chosen` — chọn placeholder rộng hay dòng note

Một quyết định, hai nguồn sự thật. Ở view đứng riêng `full_screen == false`, nên cây bảng
nhận `flex_1()` **trong một h_flex** — đúng điều comment ngay bên trên nó cảnh báo: *"under
a row parent it would draw full width and zero rows, silently and without a panic"*.

Bằng chứng nằm trong ảnh người dùng gửi, không cần suy luận: dòng chữ hiện ra là
*"Run a statement to see results here"* — đó là nhánh `!full_screen` của `result_grid.rs:109`.
Chính nó nói `full_screen` đang là `false` trong view đứng riêng.

**Sửa:** cả hai chỗ đọc `side_by_side()`. Các chỗ `full_screen` còn lại là *thật* nói về
full screen (handler Escape, icon/tooltip của nút, `toggle_full_screen`, `fills_the_window`)
nên để nguyên. `Split::is_horizontal()` (`panel_layout.rs:60`) khoá theo variant, không theo
flag, nên split handle không bị.

**Test:** `a_standalone_view_lays_its_regions_out_sideways` — hai assertion cho hai thứ đã
sai: cây bảng phải rộng đúng `DEFAULT_TREE_WIDTH`, và placeholder phải là bản rộng.
Falsify: `left: 953.5px right: 280px` — đúng vùng trống rộng trong ảnh.

## 2. Không tìm lại được cửa sổ nổi

Hai chuyện tách nhau, và cái người dùng nêu tên không phải cái sai:

**Cmd-Tab trên macOS đổi *ứng dụng*, không đổi cửa sổ.** Không cửa sổ thứ hai nào của zode
có entry riêng ở đó, dù thuộc loại gì. Nên "alt tab không thấy" là đúng kỳ vọng của macOS.

**Nhưng có một giới hạn thật, ở chỗ khác.** `WindowKind::Floating` cấp `PANEL_CLASS`
(`gpui_macos/src/window.rs:676`) — một `NSPanel` vắng mặt khỏi window cycling (Cmd-`),
menu Window, và Mission Control. Mất focus một lần là không còn đường về ngoài con chuột.

Bốn `WindowKind` hiện có, mỗi cái sai một kiểu:

| Kind | macOS | Vấn đề |
|---|---|---|
| `PopUp` | nonactivating panel | không nhận focus bàn phím |
| `Floating` | NSPanel @ floating level | trên app khác, nhưng vắng khỏi mọi danh sách cửa sổ |
| `Normal` | NSWindow @ normal level | tìm được, nhưng nằm dưới thứ bạn switch sang |
| `Dialog` | sheet | dính vào cửa sổ cha |

Cặp thật sự cần — NSWindow **và** floating level — không diễn đạt được, vì `kind` gộp *class*
với *level*.

**Sửa:** thêm `WindowKind::AlwaysOnTop` — `WINDOW_CLASS` + `NSFloatingWindowLevel`. Là thay
đổi **cộng thêm**: mọi backend khác so sánh `==` chứ không `match` vét cạn, nên linux/windows
không vỡ, chỉ rơi về cửa sổ thường.

Cố ý **không** thêm nó vào hai điều kiện ở linux (`wayland/window.rs:195`, `x11/window.rs:575`):
hai chỗ đó đặt *transient-for parent*, tức "con của cửa sổ cha" — đúng cái `AlwaysOnTop`
không được làm. Nên trên Linux/Windows nó thoái hoá thành cửa sổ thường: tìm được, không nổi.
Always-on-top thật trên Linux cần `_NET_WM_STATE_ABOVE`; không làm, và không kiểm được ở đây.

## Chưa kiểm được

Vẫn là window level và các danh sách cửa sổ: cửa sổ trong test không phải `NSWindow` thật.
Cần mắt người — Cmd-` và menu Window giờ phải thấy "Database", và nó phải nằm trên app khác.

## Trạng thái

`database_ui` 54 · `gpui` 115 · `workspace` 241 · `zode` 56 · `make build` exit 0, binary
build lại 18:04 (có bao gồm thay đổi gpui).
---

# Vòng bảy — cửa sổ nổi vắng mặt khỏi Mission Control

## Layout: xác nhận xong

Ảnh người dùng gửi cho thấy cửa sổ Database đúng hình: cây bảng bên trái, dữ liệu bên
phải, SQL dưới, pager `1-130 28 ms`. Bug `full_screen` vs `side_by_side()` đã hết.

## Phép đo bất phân định, và tôi không dựa vào nó

Thử đọc `kCGWindowLayer` của tiến trình zode qua `CGWindowListCopyWindowInfo` (công cụ
Swift nhỏ trong scratchpad). Lần đầu `pgrep` thấy pid 7056, nhưng tới lúc đo thì
**0 cửa sổ**; `pgrep` lần sau trống — tiến trình đã tắt giữa hai lệnh. Nên số 0 đó không
nói gì về Mission Control, và tôi không lấy nó làm bằng chứng.

Đo lại trên một instance tôi tự mở: 6 cửa sổ, cửa sổ chính `layer=0 onscreen=true name=zode`.
Cái này *có* kết luận được một chuyện: thay đổi `WindowKind::AlwaysOnTop` **không** làm hỏng
việc tạo cửa sổ. Đã kill instance đó ngay sau khi đo.

## Chỗ tôi bỏ sót ở vòng sáu

Đổi `NSPanel` → `NSWindow` là cần nhưng **không đủ**. Thứ đưa cửa sổ ra khỏi Spaces,
Mission Control và window cycling là **window level**, không phải class: AppKit quản
(`Managed`) cửa sổ ở normal level và coi mọi thứ trên đó là transient theo mặc định. Nâng
level lên `NSFloatingWindowLevel` là tự loại mình ra.

Lever đúng nằm ngay cạnh trong cùng file — arm `PopUp` đã dùng `setCollectionBehavior_`.
Với `AlwaysOnTop` giờ xin tường minh hai hành vi theo tên:

- `NSWindowCollectionBehaviorManaged` — Spaces và Exposé (tức Mission Control)
- `NSWindowCollectionBehaviorParticipatesInCycle` — Cmd-` và menu Window

Không có hai cái đó, cửa sổ ở trên cùng nhưng không cách nào tìm lại sau khi mất focus —
tức nửa tính năng.

## Chưa kiểm được, và tôi không thể tự kiểm

Mission Control có hiện nó hay không: cần một cú vuốt của người thật. Tôi mở được app và
đo được layer, nhưng không bấm được cái nút mở cửa sổ nổi — không có keybinding và không
có đường điều khiển UI từ đây.

Nếu `Managed` vẫn không đủ thì kết luận là "always-on-top" và "có trong Mission Control"
loại trừ nhau ở tầng OS, và lúc đó lựa chọn thuộc về người dùng: giữ nổi trên app khác,
hay đổi lấy khả năng tìm thấy. Đường sửa khi đó là một toggle bật/tắt always-on-top, và nó
cần một API mới trong gpui (`set_window_level`) vì level hiện chỉ đặt được lúc tạo cửa sổ.

## Trạng thái

`gpui` 115 · `database_ui` 54 · `make build` exit 0.
