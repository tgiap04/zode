# Phase 03 — Section `Repositories`

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** completed · **Effort:** 2-3d · **Blocked by:** 01

Miếng **C**. Đổi từ **picker modal** sang **danh sách thường trực**. Chạy song song được với 02 và 04.

## Key insights

- `PanelRepoFooter` (`git_panel.rs:5518`, `RenderOnce` + có `impl Component` để preview) **đã render** repo name + branch + remote button. Phase này **reshape** nó thành row-per-repo, **không** viết mới.
- `repository_selector.rs` (315 dòng) là picker modal. Nó **không bị xoá** — VSCode cũng giữ command palette để chuyển repo; picker vẫn là đường bàn phím. Chỉ thôi làm đường *duy nhất*.
- **`GitStore::repositories()` trả `&HashMap<RepositoryId, Entity<Repository>>`** (`git_store.rs:1846`). Thứ tự lặp HashMap **không xác định** → danh sách repo sẽ đổi chỗ mỗi lần render nếu lặp trực tiếp. **Bắt buộc sort** theo `display_name()` (hoặc path) trước khi render. Đây là lỗi đúng-sai, không phải chuyện thẩm mỹ.
- `GitStore::active_repository()` (`:749`) cho biết row nào đang active → highlight.
- Nhận từ phase 02: `zode / main`, `Fetch` split button, `Initialize Repository`.

## Requirements

**Functional**
1. Section `Repositories` (collapsible, phase 01) đứng trên `Changes`.
2. Mỗi repo một row: icon + tên · bên phải: `⑂ <branch>` + `⟳` (fetch/sync) + `⋯`.
3. Click row → đổi active repository. Row active được highlight.
4. Thứ tự row **ổn định** giữa các lần render.
5. Không repo nào → row rỗng mang nút `Initialize Repository` (nhận từ phase 02).
6. `⋯` mỗi row: các mục cấp repo (fetch/pull/push, branch picker, stash…).
7. Một repo duy nhất: vẫn hiện section (VSCode cũng vậy), không tự ẩn.

**Non-functional:** không regression cho `repository_selector` picker; sort không cấp phát lại mỗi frame nếu tránh được.

## Architecture

```
┌──────────────────────────────────┐
│ Source Control        ⋯  ⛶  ✕    │
├──────────────────────────────────┤
│ ▼ Repositories                   │
│  ▣ zode          ⑂ main  ⟳  ⋯    │  ← active: highlight
│  ▣ other-repo    ⑂ dev   ⟳  ⋯    │
├──────────────────────────────────┤
│ ▼ Changes  ⑤          + ↺ ⋯      │
│  … (phase 02)                    │
└──────────────────────────────────┘
```

`PanelRepoFooter` → đổi tên `RepositoryRow`, nhận thêm `is_active: bool` và `repo_id: RepositoryId`. Giữ `impl Component` để preview không vỡ.

Sort: giữ `Vec<RepositoryId>` đã sort trong `GitPanel`, cập nhật khi `GitStoreEvent::RepositoryUpdated` / repo được thêm-bớt — **không** sort trong `render`.

## Related code files

**Sửa:**
- `crates/git_ui/src/git_panel.rs` — `PanelRepoFooter` → `RepositoryRow` (+`is_active`, +`repo_id`); thêm field `sorted_repo_ids: Vec<RepositoryId>` + cập nhật trong subscription hiện có; `Render` chèn section
- `crates/git_ui/src/git_panel/render_header.rs` — `render_repositories_section`
- `crates/git_ui/src/git_panel/render_entries.rs` — `render_empty_state` bỏ `Initialize Repository` (đã sang đây)
- `crates/git_ui/src/repository_selector.rs` — không đổi hành vi; chỉ kiểm nó vẫn mở được

**Tạo:** không · **Xoá:** không

## Implementation steps

1. Thêm `sorted_repo_ids: Vec<RepositoryId>` vào `GitPanel`. Rebuild bằng `repositories()` → sort theo `display_name()` — trong handler event, **không** trong `render`.
2. `PanelRepoFooter` → `RepositoryRow`: thêm `is_active`, `repo_id`; row highlight khi active; click → set active repository.
3. `render_repositories_section`: `PanelSection` nhãn `Repositories`, children = `sorted_repo_ids` map sang `RepositoryRow`.
4. Chuyển `Fetch`/pull/push từ chỗ tạm (phase 02) vào `⟳` + menu `⋯` của row.
5. Chuyển `Initialize Repository` từ `render_empty_state` sang row rỗng của section này.
6. Chèn section vào `Render`, **trên** `Changes`.
7. Kiểm `repository_selector` picker vẫn mở và vẫn đổi được repo.
8. Test: 2 repo giả, khẳng định thứ tự row không đổi qua 2 lần render liên tiếp; khẳng định click đổi active.

## Todo

- [x] `sorted_repo_ids` cập nhật trong event handler, không sort trong render
- [x] `RepositoryRow` (+`is_active`, +`repo`), `impl Component` preview không vỡ — `new_preview` giữ nguyên chữ ký
- [x] Row active highlight, click đổi active repo
- [x] `⟳` nhận fetch/pull/push — **chỉ trên row active**, xem điều 2
- [ ] **`⋯` mỗi row — KHÔNG làm.** Không buildable đúng ở phase này; xem "Yêu cầu 6 hoãn lại"
- [x] `Initialize Repository` có nhà mới — `render_no_repositories`
- [x] Section đứng trên `Changes`
- [x] `repository_selector` picker vẫn chạy — đường riêng qua `zed_actions::git::SelectRepo`, không dính popover đã xoá
- [x] **Test thứ tự row ổn định** — 2 repo đặt tên nghịch thứ tự thêm vào
- [x] Một repo: section vẫn hiện

## Kết quả (2026-08-11)

Clippy `--deny warnings` exit 0, **75/75 test xanh** (70 sau phase 02 + 5 mới). `cargo check -p git_graph` xanh.

| File | Δ |
|---|---|
| `git_panel.rs` | +134 / −95 |
| `git_panel/render_header.rs` | +88 |
| `git_panel/panel_section.rs` | +21 / −8 |
| `git_panel/render_entries.rs` | +9 / −34 |
| `git_panel/render_commit_box.rs` | +1 / −19 |
| `git_panel/tests.rs` | +228 |

Năm test mới: thứ tự row theo tên (2 repo đặt tên **nghịch** thứ tự thêm vào, nên bỏ sort là test đỏ); đổi active repo không đảo thứ tự row; `cx.draw()` với 2 repo ở hai trạng thái gập; `cx.draw()` với **8 repo** ở panel cao 300px *và* 800px; `cx.draw()` khi **không có repo nào**.

### Bốn điều phase này dạy lại cho plan

**1. `PanelSection` không được để mọi section đều `flex_1`.** Phase 01 dựng `PanelSection` cho **một** section duy nhất nên nội dung khi mở luôn `flex_1().min_h_0()`. Thêm section thứ hai là hai section **chia nhau** chiều cao panel — `Repositories` cao bằng nửa panel. Đã sửa: `fills_height()` là **opt-in**, mặc định section cao tự nhiên; chỉ `Changes` gọi nó. Phase 04/05 phải tự quyết định: `Commits`/`Graph` theo plan là **cao cố định + resize handle**, nên **không** gọi `fills_height()`.

**2. Không phải affordance nào cũng scope được về một repo — phải kiểm từng cái.** Ba nhóm khác nhau:

| Affordance | Scope được? | Kết quả |
|---|---|---|
| Đổi active repo | ✅ `Repository::set_as_active_repository` nhận entity | Cả row click được |
| Branch picker | ✅ `branch_picker::popover(_, _, repository, …)` nhận repo | **Mọi row** đều mở được picker của chính nó |
| Fetch/pull/push | ❌ `render_remote_button` dispatch `git::Fetch`/`Push`/`Pull` **toàn cục**, `GitPanel::fetch` giải về `self.active_repository` | Chỉ row active có `⟳` — đặt trên row khác là fetch **sai repo** |

Lần đầu tôi gate cả branch picker theo `is_active` **và viết comment nói ngược lại** — review bắt được. Bài học: khi comment nói "per-row", code phải per-row.

**3. Sort ngoài render vẫn hỏng nếu bắt thiếu event.** Bản đầu chỉ resort ở `RepositoryAdded` / `RepositoryRemoved` / `ActiveRepositoryChanged` / `RepositoryUpdated(_, StatusesChanged|HeadChanged, true)`. Chú ý cái `true` — **chỉ repo active**. Nhưng `work_directory_abs_path` bị đổi **tại chỗ** (`git_store.rs:1568-1571`) khi git root di chuyển trên đĩa, mà `display_name()` dẫn xuất từ path đó → tên đổi, khoá sort đổi, mà event thì có thể là loại khác hoặc trên repo không active. Đã sửa: resort trên **mọi** `RepositoryUpdated`, không lọc loại, không lọc active.

**Còn hở:** nếu một lần rename **không phát event nào cả**, danh sách vẫn lệch tới lần add/remove/activate kế tiếp. Bịt hẳn cần emit một event tại chỗ đổi path trong `crates/project/src/git_store.rs` — ngoài phạm vi phase này (phase chỉ được sửa 4 file trong `git_ui`), và thêm event ở đó có thể lan sang subscriber khác. **Ghi lại làm việc cần làm**, không lặng lẽ bỏ qua.

**4. Section cao tự nhiên thì phải có nắp.** `Repositories` cao tự nhiên (điều 1) nghĩa là 20 repo = 720px, và `Changes` là con **duy nhất** co được (`min_h_0`) nên nó bị bóp về 0, còn panel `overflow_hidden` cắt nốt phần row tràn — vừa mất `Changes` vừa không tới được repo cuối. Đã sửa: `max_h(MAX_REPOSITORY_ROWS_HEIGHT)` = 5 row + `overflow_y_scroll()`. Test vẽ 8 repo ở panel 300px.

### Yêu cầu 6 hoãn lại — menu `⋯` mỗi row

Yêu cầu 6 đòi mỗi row một menu `⋯` chứa "các mục cấp repo (fetch/pull/push, branch picker, stash…)". **Không làm ở phase này.** Lý do: branch picker đã nằm ngay trên row (không cần menu), còn fetch/pull/push và stash đi qua `GitPanel::fetch` / `stash_pop` / `stash_apply`, tất cả đều giải về `self.active_repository` → menu trên row không-active sẽ tác động sai repo.

**Không phải bất khả thi** — review chỉ ra `Repository::fetch` / `stash_pop` nhận entity trực tiếp, đúng như `set_as_active_repository` mà phase này đã dùng. Việc thật cần làm: đổi `GitPanel::fetch` / `push` / `pull` / `stash_*` sang **nhận repo tường minh** thay vì đọc `self.active_repository`. Bounded, nhưng là thay đổi chữ ký lan ra ngoài phase → tách việc riêng.

### Lệch so với plan

- **Popover chọn repo trong row bị xoá**, thay bằng click cả row (plan yêu cầu 3). Picker modal **không** mất: `repository_selector::register` đăng ký `zed_actions::git::SelectRepo` độc lập (`git_ui.rs:69`) — đường bàn phím còn nguyên, đúng Key insight của plan.
- **`render_empty_state` không còn nói về repo.** Khi không có repo, section `Changes` **không** render gì cả — nếu không thì "No Git repositories" (section trên) và "No changes to commit" (section dưới) đá nhau.
- `RepositoryRow` mang `repo: Option<Entity<Repository>>` thay vì `repo_id: Option<RepositoryId>` như plan viết — cần entity để scope branch picker và để click đổi active không phải tra lại `HashMap`.

### Còn nợ mắt người

(a) row active highlight có đọc ra "đang chọn" không, (b) `⟳` chỉ hiện ở row active có gây bối rối không, (c) scroll trong section `Repositories` khi nhiều repo, (d) một repo duy nhất — section có đáng chỗ nó chiếm không.

## Success criteria

- Mở project 2 repo: cả hai hiện, thứ tự **không đổi** giữa các lần render/notify.
- Click repo không active → đổi active, `Changes` cập nhật theo.
- Không repo: `Initialize Repository` bấm được từ section này.
- Picker cũ vẫn mở được bằng bàn phím.
- `./script/clippy` sạch, test `git_ui` xanh.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| **Row đổi chỗ mỗi render** do lặp `HashMap` | `sorted_repo_ids` sort ngoài render; có test khẳng định ổn định |
| Sort trong `render` gây cấp phát mỗi frame | Sort trong event handler; render chỉ đọc `Vec` |
| `impl Component for PanelRepoFooter` (`:5719`) vỡ khi đổi struct | `new_preview` giữ nguyên chữ ký, field mới có default |
| Đổi active repo giữa lúc `Changes` đang staging | Dùng đúng đường `set active repository` hiện có của picker, không viết đường mới |
| Section chiếm chỗ khi chỉ có 1 repo | Đã chốt: vẫn hiện, giống VSCode. Người dùng gập được (phase 01) |

## Security

Không có bề mặt mới. Fetch/pull/push dùng lại `render_remote_button` hiện có → đường askpass (`askpass_modal.rs`) không đổi.

## Next steps

Phase 04 / 05 độc lập với phase này.
