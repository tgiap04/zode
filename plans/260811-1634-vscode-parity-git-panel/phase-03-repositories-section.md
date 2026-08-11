# Phase 03 — Section `Repositories`

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** pending · **Effort:** 2-3d · **Blocked by:** 01

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

- [ ] `sorted_repo_ids` cập nhật trong event handler, không sort trong render
- [ ] `RepositoryRow` (+`is_active`, +`repo_id`), `impl Component` preview không vỡ
- [ ] Row active highlight, click đổi active repo
- [ ] `⟳` + `⋯` mỗi row nhận fetch/pull/push
- [ ] `Initialize Repository` có nhà mới
- [ ] Section đứng trên `Changes`
- [ ] `repository_selector` picker vẫn chạy
- [ ] **Test thứ tự row ổn định**
- [ ] Một repo: section vẫn hiện

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
