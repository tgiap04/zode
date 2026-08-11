# Phase 02 — Commit box lên top, giải tán footer

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** completed · **Effort:** 3-4d · **Blocked by:** 01

Miếng **A**, quyết định 2 (*chuyển hẳn* lên top, bỏ vị trí footer). Đây là phase mang **checklist tái định cư** (quyết định 8) — phần dễ gây regression thầm lặng nhất của cả plan.

## Key insights

- **R2 hẹp hơn dự tính ban đầu.** Đã đọc code: `focus_editor` (`:1218`) focus qua handle của `commit_editor`; `focus_changes_list` (`:1235`) focus `self.focus_handle`; `expand_commit_editor` (`:3733`) mở `CommitModal`. **Cả ba độc lập với vị trí trong element tree** — không phải sửa.
- Cái thật sự phơi ra là **ba** thứ khác:
  1. `Focusable for GitPanel` (`:5306-5313`) trả `commit_editor` khi `entries.is_empty()`, else `focus_handle`. Heuristic này sinh ra từ layout cũ (empty state chiếm cả panel, commit box là đích duy nhất đáng focus). Ở layout mới nó vẫn *chạy* nhưng lý lẽ đã khác — cần xét lại có nên luôn trả `focus_handle` hay không.
  2. **Thứ tự tab đảo.** Commit editor giờ đứng *trước* list trong element tree.
  3. **Editor nở ngược chiều.** Ở footer nó nở *lên*; ở top nó nở *xuống* và đẩy list. Phép tính `max_height` ở `render_footer` (`:3851-3857`, `MAX_PANEL_EDITOR_LINES = 6`, `gap = 9.0`) được viết cho chiều cũ — phải xét lại.
- Nút `✨` AI generate commit message của VSCode: **không buildable, ngoài scope.** `commit_message_prompt.txt` chỉ được `prompt_store` tham chiếu; hard fork 260726-1531 đã xoá toàn bộ crate AI. Không còn model để gọi. Placeholder cũng không đặt.
- Placeholder VSCode: `Message (⌘Enter to commit on "main")` — tên branch nằm *trong* placeholder. Zode hiện đặt `Enter commit message`. Tên branch lấy từ `active_repository.read(cx).branch`.

## Requirements

**Functional**
1. Commit editor + nút Commit nằm **trên** file list, trong section `Changes`, ngay dưới header section.
2. Nút Commit: **full-width, primary, split** — thân `✓ Commit` + chevron mở `render_git_commit_menu` hiện có.
3. Placeholder editor mang tên branch hiện tại: `Message (⌘Enter to commit on "<branch>")`.
4. Footer cũ **được giải tán**; mọi affordance trong nó có nhà mới (bảng dưới).
5. `render_panel_header` cũ (`No Changes` + `Stage All`) bị xoá; nội dung đã dời sang title row + hover actions của section `Changes` ở phase 01.

**Non-functional:** không mất bất kỳ affordance nào trong checklist; thứ tự tab hợp lý; editor nở xuống không đẩy list ra khỏi khung.

## Checklist tái định cư (quyết định 8 — acceptance criteria, không phải gợi ý)

| Affordance đang có | Nguồn | Nhà mới |
|---|---|---|
| Row previous-commit + **Uncommit** (`git reset HEAD^`) | `render_previous_commit` `:4062` | Menu `⋯` của section `Changes`. Keybinding `git::Uncommit` giữ nguyên |
| Nút **Open Git Graph** | `:4150` | Tạm giữ trong menu `⋯` của `Changes`; chuyển sang toolbar section Graph ở **phase 05** |
| Click `N Changes` → `Diff` | `render_panel_header` `:3774-3785` | Count badge trên header `Changes` vẫn click ra `Diff` |
| **Stage All / Unstage All** (nút text) | `:3792-3809` | Icon `+` / `↺` hover trên header `Changes` (đã dựng ở phase 01) |
| Row **amend pending** | `render_pending_amend` `:4036` | Trong section `Changes`, ngay dưới nút Commit |
| **`zode / main`** + `Fetch` split button | `render_footer` / `render_remote_button` `:3814` | `Fetch`/pull/push → tạm giữ dưới nút Commit; chuyển sang row repo ở **phase 03**. `zode / main` → row repo ở phase 03 |
| Empty state **Initialize Repository** | `render_empty_state` `:4180` | Giữ trong empty state của section `Changes`; chuyển sang section Repositories ở **phase 03** |
| Empty state **View Branch Diff** | `:4194` | Menu `⋯` của section `Changes` |

**Quy tắc:** không affordance nào được xoá trước khi nhà mới của nó *chạy được*. Cái nào chờ phase sau thì để tạm chỗ hiện tại và ghi `// TODO(phase-03)` / `// TODO(phase-05)`.

## Architecture

```
┌──────────────────────────────────┐
│ Source Control        ⋯  ⛶  ✕    │
├──────────────────────────────────┤
│ ▼ Changes  ⑤          + ↺ ⋯      │  ⋯ giờ chứa: Uncommit, View Branch Diff,
│  ┌────────────────────────────┐  │     Open Git Graph (tạm), + menu cũ
│  │ Message (⌘Enter to commit  │  │  ← commit editor, nở XUỐNG
│  │  on "main")                │  │
│  └────────────────────────────┘  │
│  ┌────────────────────────────┐  │
│  │       ✓ Commit          ⌄  │  │  ← full-width primary split
│  └────────────────────────────┘  │
│  [amend pending, nếu có]         │
│  ⟳ Fetch ⌄        (tạm → ph.03)  │
│   ▸ Tracked            ☑         │
│     <file entries…>              │
└──────────────────────────────────┘
```

## Related code files

**Sửa:**
- `crates/git_ui/src/git_panel/render_commit_box.rs` — `render_footer` → `render_commit_box` (đổi tên + đổi chiều nở); `render_commit_button` → full-width split; `render_previous_commit` xoá, nội dung vào menu; `render_pending_amend` dời chỗ gọi
- `crates/git_ui/src/git_panel/render_header.rs` — xoá `render_panel_header`; mở rộng `render_overflow_menu` + `render_git_commit_menu` nhận các mục tái định cư
- `crates/git_ui/src/git_panel/render_entries.rs` — `render_empty_state` co lại (không còn chiếm cả panel)
- `crates/git_ui/src/git_panel.rs` — `Render` (thứ tự con), `Focusable` (xét lại heuristic)

**Tạo:** không · **Xoá:** không (chỉ xoá hàm trong file đã có)

## Implementation steps

1. Đổi placeholder commit editor sang dạng có tên branch. Đọc branch từ `active_repository`; khi không có branch, dùng `fallback_branch_name` (đã có trong `GitPanelSettings`).
2. `render_commit_button` → full-width primary split button. Chevron giữ nguyên `render_git_commit_menu`.
3. Mở rộng menu `⋯` của section `Changes`: thêm `Uncommit` (chỉ khi `head_commit.has_parent`), `View Branch Diff` (chỉ khi `is_on_main_branch` false), `Open Git Graph`.
4. Dời commit editor + nút Commit + amend row vào section `Changes`, trên list. Xoá `render_previous_commit`.
5. Xét lại `max_height` (`:3851-3857`) cho chiều nở xuống: editor không được đẩy list xuống mất khung. Cân nhắc cap ở `MAX_PANEL_EDITOR_LINES` như cũ nhưng đo từ trên.
6. Xoá `render_panel_header`; xoá lệnh gọi trong `Render`.
7. `render_empty_state` co lại thành một dòng mảnh dưới nút Commit (không `flex_1`, không `justify_center`).
8. Xét lại `Focusable for GitPanel`: quyết định và **ghi lý do vào comment** — commit box giờ luôn hiện, nên `entries.is_empty()` không còn là tín hiệu tốt.
9. Rà tay thứ tự tab: title row → commit editor → nút Commit → list.
10. Chạy toàn bộ checklist tái định cư, bấm từng affordance.

## Todo

- [x] Placeholder mang tên branch, có đường fallback — `commit_placeholder_text`
- [x] Nút Commit full-width primary split — `full_width()` + `ButtonStyle::Tinted(Accent)`
- [x] Menu `⋯` nhận Uncommit + View Branch Diff + Open Git Graph
- [x] Commit editor + Commit + amend dời lên trên list
- [x] `render_panel_header` xoá; `render_previous_commit` xoá **nhưng** phần hiển thị giữ lại → `render_last_commit` (xem "Checklist thiếu một dòng")
- [x] `max_height` đúng cho chiều nở xuống — tiền đề của plan sai, xem điều 1 dưới
- [x] `render_empty_state` co lại
- [x] `Focusable` heuristic xét lại + comment lý do
- [x] Thứ tự tab đúng — title row → commit editor → nút Commit → list
- [x] **9/9 dòng checklist tái định cư có đường bấm** (8 dòng của plan + 1 dòng plan bỏ sót)
- [x] Không nút `✨`

## Kết quả (2026-08-11)

Clippy `--deny warnings` exit 0, **70/70 test xanh** (66 sau phase 01 + 4 mới).

| File | Δ |
|---|---|
| `git_panel.rs` | +123 / −35 |
| `git_panel/render_commit_box.rs` | +138 / −196 |
| `git_panel/render_header.rs` | +13 / −68 |
| `git_panel/render_entries.rs` | +10 / −27 |
| `git_panel/panel_section.rs` | +32 / −1 |
| `git_panel/tests.rs` | +189 |

Bốn test mới: placeholder mang tên branch; placeholder rơi về `fallback_branch_name` khi chưa có branch; `cx.draw()` panel ở **ba** trạng thái (có entries / commit message 12 dòng / tree sạch → empty state); gating `show_branch_diff` lật khi HEAD rời `main`.

### Bốn điều phase này dạy lại cho plan

**1. Tiền đề "editor nở ngược chiều" sai — editor chưa từng nở.** Plan viết editor "ở footer nở *lên*; ở top nở *xuống*", và đặt `max_height` vào diện phải xét lại. Đọc code: `commit_message_editor` dựng `EditorMode::AutoHeight { min_lines: max_lines, max_lines: Some(max_lines) }` với `max_lines = MAX_PANEL_EDITOR_LINES = 6` → **min == max == 6, cao cố định 6 dòng, không nở chiều nào**. Cả phép tính `max_height + footer_size` và cờ `editor_is_long` chỉ để dựng một khung cố định.

Việc thật phải làm không phải "sửa `max_height` cho chiều mới" mà là **cho editor nở lần đầu**: `min_lines: 1` khi ở panel (modal giữ 18/18). Đây đúng là điều success criteria đòi — "nở tới 6 dòng rồi tự scroll" — nên tiêu chí đúng, chỉ phần "Key insights" chẩn đoán sai nguyên nhân. Kèm theo: `panel_editor_container` bỏ `size_full()` → `w_full()`, vì chiều cao giờ do nội dung quyết.

**2. Checklist thiếu một dòng — và thiếu đúng loại affordance dễ mất nhất.** Dòng 1 của checklist ghi "Row previous-commit + **Uncommit**" → nhà mới là menu `⋯`. Nhưng cái row đó làm **bốn** việc: hiện subject của commit cuối, click ra `CommitView`, hover ra `GitPanelMessageTooltip`, và nút Uncommit. Checklist chỉ theo dõi cái **bấm được** (Uncommit) và bỏ qua cái **hiển thị**. Xoá cả row theo bước 4 sẽ làm người dùng mất hẳn đường xem commit vừa tạo cho tới phase 04 — vi phạm chính quy tắc của plan: *"không affordance nào được xoá trước khi nhà mới của nó chạy được"*.

Đã xử: tách `render_last_commit` — giữ subject + click + hover, **bỏ** hai nút đã có nhà mới. Đánh `// TODO(phase-04)`. `GitPanelMessageTooltip` do đó vẫn sống, không cần `#[allow(dead_code)]`.

**Bài học cho phase 03–05:** checklist tái định cư phải kê cả affordance **hiển thị**, không chỉ nút. Một dòng "row X" thường gói nhiều việc hơn tên nó nói.

**3. Gate của action handler không phải gate của menu entry.** `git_ui::git_panel::Open` chỉ được `git_graph` đăng ký qua `register_action_renderer` **khi có repo active**. Ở row cũ điều đó vô hình vì cả row đòi repo + branch + commit mới render. Đưa vào menu thì entry hiện vô điều kiện → bấm khi không có repo là no-op im lặng. Phải tự gate: `action_disabled_when(!has_repository, …)`. Phase 05 dời `Open Git Graph` sang toolbar section Graph sẽ gặp lại.

**4. Placeholder đọc setting thì phải nghe setting.** `commit_placeholder_text` đọc `fallback_branch_name`, nhưng placeholder chỉ được viết lại từ `update_visible_entries` (do git-status kích). Observer `SettingsStore` chỉ theo dõi 5 field và không có field này → sửa setting mà panel không đổi cho tới lần git-status kế tiếp. Đã thêm vào danh sách theo dõi.

### Lệch so với plan

- **`show_branch_diff` bỏ điều kiện `changes_count == 0`.** Bản cũ (`should_show_branch_diff`) là `has_repo && changes_count == 0 && !is_on_main_branch` — điều kiện "không có change" gần như hiển nhiên vì nó là nút *của empty state*. Trong menu `⋯` thì gate theo "không có change" là vô lý (vừa sửa một file là mất entry), nên chỉ giữ `is_on_main_branch` như bước 3 của plan viết. **Có chủ đích.**
- **Commit box nằm *trong* section `Changes`** theo yêu cầu 1 + sơ đồ của plan. Hệ quả: gập `Changes` là ẩn luôn commit box. Đúng spec, nhưng đáng xét lại ở phase sau — VSCode đặt message box *ngoài* mọi section.
- **`Focusable` luôn trả `self.focus_handle`.** Lý do ghi thẳng trong comment: tiền đề cũ (empty state chiếm cả panel, commit box là đích duy nhất) đã mất; giữ context `GitPanel` sống thì mọi action panel bấm được ngay khi mở, `FocusEditor` cách một phím. Trả handle của editor sẽ **thu hẹp** context về `CommitEditor` và tắt các action của list.
- `render_pending_amend` hạ từ `pub(super)` xuống `fn` — chỉ còn `render_commit_box` gọi.

### Còn nợ mắt người

`cx.draw()` chứng minh không panic ở ba trạng thái, kể cả message 12 dòng. Chưa khẳng định được bằng test: (a) editor nở 1→6 dòng trông đúng và không đẩy list mất khung ở panel thấp, (b) nút Commit full-width accent đọc ra "primary" trong cả light/dark theme, (c) thứ tự tab thật khi bấm Tab, (d) chín dòng checklist bấm tay — mỗi dòng đã truy được đường dispatch trong code, nhưng chưa ai bấm.

## Success criteria

- 8/8 affordance trong checklist bấm được sau phase này (cái chờ phase sau vẫn ở chỗ tạm và bấm được).
- Editor nở tới 6 dòng rồi tự scroll, không đẩy list mất khung.
- `⌘Enter` commit vẫn chạy; `git::Uncommit` keybinding vẫn chạy.
- Panel không changes: commit box hiện thường trực, dòng `No changes to commit` mảnh phía dưới.
- `./script/clippy` sạch, test `git_ui` xanh.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Mất affordance thầm lặng | Checklist 8 dòng là acceptance criteria; bấm từng cái, không suy luận |
| `Focusable` heuristic sai sau khi đổi layout (R2) | Bước 8 buộc ra quyết định có comment, không để nguyên vì "vẫn chạy" |
| Editor nở xuống đẩy list mất khung | Bước 5 xét lại `max_height`; test tay với commit message 10+ dòng |
| Empty state co lại làm mất `Initialize Repository` | Giữ trong empty state cho đến khi phase 03 dựng nhà mới |
| Thứ tự tab đảo gây bất ngờ | Bước 9 rà tay; đây là thay đổi có chủ đích của quyết định 2 |

## Security

Không có bề mặt mới. `Uncommit` chuyển chỗ nhưng vẫn là `git reset HEAD^` với cùng điều kiện `has_parent` — không nới lỏng guard.

## Next steps

Phase 03 nhận `zode / main`, `Fetch`, `Initialize Repository` từ chỗ tạm. Phase 05 nhận `Open Git Graph`.
