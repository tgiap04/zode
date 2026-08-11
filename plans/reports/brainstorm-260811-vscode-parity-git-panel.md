# Brainstorm — VSCode parity cho Git Panel

**Ngày:** 2026-08-11 · **Lens:** CTO · **Level:** medium
**Commission:** đưa UI + chức năng của git panel (mở từ rail) về gần bằng Source Control view của VSCode.

## Commission

Zode hiện tại (`crates/git_ui/src/git_panel.rs`, 7.619 dòng), thứ tự từ trên xuống:

1. Header row — `No Changes` (click → `Diff`) · `⋯` overflow · `Stage All`
2. Changes list, hoặc empty state chiếm toàn bộ chiều cao
3. Footer — `zode / main` + `Fetch` split button → commit editor → `Commit Tracked` split button
4. Previous-commit row — message + `Undo` (Uncommit) + `Open Git Graph`

VSCode Source Control: title row → `▼ Repositories` → `▼ Changes` (message input **trên** list + nút `✓ Commit` full-width) → `▼ Graph` → `▼ GitLens/COMMITS`.

Panel là một flat scroll; VSCode là các section collapsible xếp dọc.

## Dữ kiện đã xác minh

| Dữ kiện | Ý nghĩa |
|---|---|
| Không có remote upstream (chỉ `origin = tgiap04/zode`), hard fork | Mổ lớn `git_panel.rs` **không** gây đau rebase với Zed — mở đường cho phương án nặng tay |
| `ui::Disclosure` đã có (`crates/ui/src/components/disclosure.rs`) | Collapsible section không xây từ đầu |
| `crates/panel/src/panel.rs` giữ `panel_header_container`, `panel_filled_button`, `panel_icon_button` | Nhà cho `PanelSection` nếu cần dùng chung; theo YAGNI để trong `git_ui` trước |
| `git_graph::GitGraph::new(repo_id, git_store, workspace, window, cx)` | Panel host được entity graph riêng, không cần tách render khỏi `Item` |
| `GitGraph::render_graph` là `pub` | Body của graph gọi được từ ngoài qua `graph.update(cx, …)` |
| `git_panel.default_width = 360` (`assets/settings/default.json:945`) | Bảng graph 5 cột 14/62/10/9/5% → 50/223/37/31/18px. Cột author + date vô dụng |
| `LogSource::Branch(SharedString)` + `repo.get_graph_data(source, order)` | Commit log theo branch đã có sẵn |
| `GraphCommitData { sha, parents, author_name, author_email, commit_timestamp, subject }` | Đủ mọi field cho một dòng COMMITS |
| `Branch::tracking_status() -> UpstreamTrackingStatus { ahead, behind }` | Đủ cho dòng "Up to date with origin" |
| `CommitAvatar` (`commit_tooltip.rs`), đã dùng trong `file_history_view` | Avatar có sẵn |
| `Panel::is_zoomed` / `set_zoomed` có default no-op; **GitPanel không implement** | Nút maximize phải implement 2 method + wiring `ToggleZoom` — nhỏ nhưng không miễn phí |

## Phân rã: 5 miếng độc lập

| # | Miếng | Giá | Ghi chú |
|---|---|---|---|
| A | Commit box + Commit button từ footer lên top | TB | Đảo layout, ảnh hưởng focus/keybinding, footer bị mồ côi |
| B | Collapsible sections (Disclosure + persist collapse state) | Nhỏ–TB | Nền tảng cho C, D, E |
| C | Section `Repositories` thường trực (repo + branch + sync + `⋯`) | TB | Đổi từ picker modal sang list |
| E | Section `Commits` kiểu GitLens | TB | **Rẻ hơn dự tính ban đầu** — backend đã đủ, chỉ là UI |
| D | Section `Graph` nhúng inline | **Lớn nhất** | Cần biến thể compact + hai vùng scroll lồng nhau |

**Sửa đánh giá ban đầu:** E bị xếp sai là "cả một extension". Backend đã đủ (`LogSource::Branch` + `get_graph_data` + `tracking_status` + `CommitAvatar`). Ngược lại D đắt hơn dự tính vì 360px không chứa nổi bảng 5 cột. Thứ tự đúng: **B → A → C → E → D**.

## Các đường đã cân

**Đường 1 — scaffold sections trước, rồi lấp dần** ✅ *chọn*
- Ưu: mỗi phase ship được và review được; Phase 0 tự trả giá ngay cho các phase sau; nếu D thất vọng ở 360px thì phase 0–4 vẫn đứng độc lập.
- Nhược: 6 phase, calendar dài nhất; Phase 0 là diff lớn dạng no-op, đọc mệt dù máy móc.

**Đường 2 — viết lại cả render tree một lượt** ❌ *loại*
- 7.6k dòng viết lại một pass là không review được, và panel hỏng suốt toàn bộ thời gian làm D + E.

**Đường 3 — `GitPanelV2` song song, bật bằng setting** ❌ *loại*
- Ưu: panel cũ luôn chạy, dogfood cạnh nhau, parity risk được khoanh vùng.
- Nhược: nhân đôi ~7.6k dòng logic staging/entry rendering — vi phạm DRY nặng nhất có thể, hai bản sẽ trôi khỏi nhau. Chỉ đáng nếu panel cũ **phải** tiếp tục hoạt động cho người khác; trên hard fork một người thì không.

## Quyết định đã chốt

| # | Quyết định | Chọn |
|---|---|---|
| 1 | Scope | **Cả 5 miếng** A + B + C + D + E |
| 2 | Vị trí commit box | **Chuyển hẳn lên top**, bỏ vị trí footer |
| 3 | Độ trung thành | **Parity gần pixel** với VSCode |
| 4 | Refactor | **Tách section ra file riêng trước** khi đổi layout |
| 5 | Đường đi | **Đường 1** — scaffold trước |
| 6 | Title row | **Đầy đủ cả 3**: `Source Control` + maximize + close |
| 7 | Graph ở 360px | **Compact + escape hatch ra tab** — `render_graph_compact` (lanes + subject), giữ `git_graph` tab qua nút "Open in tab" |
| 8 | Affordance bị parity xoá | **Tái định cư hết**, thành checklist acceptance criteria |

## Layout đích

```
┌──────────────────────────────────┐
│ Source Control       ⋯  ⛶  ✕     │  title row (mới; cần impl set_zoomed)
├──────────────────────────────────┤
│ ▼ Repositories                   │  B + C
│  ▣ zode         ⑂ main  ⟳  ⋯     │
├──────────────────────────────────┤
│ ▼ Changes  ⑤          + ↺ ⋯      │  B — count badge + hover actions
│  ┌────────────────────────────┐  │
│  │ Message (⌘Enter to commit  │✨│  A — commit editor lên top
│  │  on "main")                │  │
│  └────────────────────────────┘  │
│  ┌────────────────────────────┐  │
│  │       ✓ Commit          ⌄  │  │  A — full-width primary split button
│  └────────────────────────────┘  │
│  <file entries…>                 │
├──────────────────────────────────┤
│ ▼ Graph     ⑂Auto ◎ ↓ ↑ ⟳ ⛶ ⋯   │  D — compact, ⛶ = open in tab
│  ● commit subject…               │
├──────────────────────────────────┤
│ ▼ Commits (main)    ↑ ↓ ↪ ⑂ ⟳    │  E — LogSource::Branch(current)
│  ☁ Up to date with origin        │     (tracking_status)
│  > 👤 ⟪origin⟫ Merge PR #7…       │     (CommitAvatar + GraphCommitData)
└──────────────────────────────────┘
```

## Checklist tái định cư (quyết định 8 — không được bỏ sót)

| Affordance đang có | Nhà mới |
|---|---|
| Previous-commit row + **Uncommit** (`git reset HEAD^`) | Menu `⋯` của section Changes; keybinding `git::Uncommit` giữ nguyên |
| Nút **Open Git Graph** (mở tab) | Nút `⛶` trên toolbar section Graph |
| Click `N Changes` → **Diff** | Count badge trên header Changes vẫn click ra `Diff` |
| **Stage All / Unstage All** (nút text) | Icon `+` / `↺` hiện khi hover header Changes |
| Empty state **Initialize Repository** | Trong section Repositories khi không có repo |
| Empty state **View Branch Diff** | Menu `⋯` của section Changes |
| Row **amend pending** | Trong section Changes, ngay dưới nút Commit |

## Phases đề xuất

| Phase | Nội dung | Gate |
|---|---|---|
| 0 | Tách `git_panel.rs` → `git_panel/{header,commit_box,entries,sections}.rs`. Thuần di chuyển, zero behavior change | `./script/clippy` + test `git_ui` hiện có xanh |
| 1 | Component `PanelSection` (Disclosure + count badge + hover actions + persist collapse). Bọc list hiện tại thành `Changes`. Title row + `set_zoomed`/`is_zoomed` | Panel vẫn dùng được như cũ, thêm collapse |
| 2 | **A** — commit editor + full-width split Commit button lên top; giải tán footer; chạy checklist tái định cư | Không mất affordance nào trong bảng trên |
| 3 | **C** — section `Repositories` (repo + branch + sync + `⋯`), thay picker | Multi-repo hiện đúng, `Initialize Repository` có nhà |
| 4 | **E** — section `Commits` trên `LogSource::Branch(current)` + `tracking_status()` + `CommitAvatar` | Ahead/behind đúng, phân trang không block UI |
| 5 | **D** — `render_graph_compact` + host `Entity<GitGraph>` + resize chiều cao section + `⛶` open in tab | Không có nested-scroll trap; tab cũ vẫn mở được |

## Cần theo dõi khi làm

1. **Hai vùng scroll lồng nhau (phase 5)** — section Graph và section Commits đều có scroll riêng, nằm trong panel cũng scroll. Đây là bẫy UX rõ nhất của cả commission. Cần chiều cao section cố định + resize handle, không để section tự co giãn theo nội dung.
2. **Focus + keybinding khi commit box đổi chỗ (phase 2)** — `Focusable for GitPanel` hiện trả `commit_editor` khi list trống, `focus_handle` khi có entries. `focus_changes_list` / `focus_editor` / `expand_commit_editor` đều giả định layout cũ. Phải rà lại cả 3 action.
3. **Collapse state phải persist** — nếu không, mỗi lần mở panel lại thấy cả 4 section bung ra là tệ hơn hiện tại. Dùng `serialization` như `sidebar/src/serialization.rs` đang làm.
4. **Graph load cost (phase 5)** — nhúng graph nghĩa là mỗi lần mở panel đều chạy `get_graph_data`. Phải lazy: chỉ load khi section Graph đang expand.
5. **Title row `✕` trùng chức năng rail** — đã chốt là thêm; chấp nhận việc có hai đường đóng panel.
6. **`git_panel.rs` sau phase 0** phải không còn file nào vượt ngưỡng dễ đọc; đây là lý do phase 0 đi trước chứ không đi sau.

## Cách đo thành công

- Mở panel cạnh VSCode, so 4 section: thứ tự, nhãn, vị trí commit box, vị trí toolbar khớp.
- Toàn bộ 7 dòng checklist tái định cư có nhà mới và bấm được.
- `./script/clippy` sạch; test `git_ui` + `git_graph` xanh sau mỗi phase.
- Collapse state sống qua restart.
- Graph không load khi section đang collapse.

## Bước tiếp theo

Chạy `/tkm:create-plan` với báo cáo này làm context → plan 6 phase như bảng trên.

## Còn để mở

- Nút `✨` (AI generate commit message) trong VSCode message input: Zode có `commit_message_prompt.txt` trong `git_ui` — chưa xác minh nó nối vào đâu và có sẵn nút hay không. Cần kiểm ở phase 2.
- Toolbar section Graph của VSCode có `⑂ Auto` (branch scope selector) và `◎` (jump to HEAD). `LogSource` chỉ có `All` / `Branch` / `Sha` — `Auto` map sang cái nào chưa quyết.
- Section GitLens của VSCode còn `Compare main with origin/main` (một dòng expandable). Chưa quyết có làm trong phase 4 hay để riêng.
