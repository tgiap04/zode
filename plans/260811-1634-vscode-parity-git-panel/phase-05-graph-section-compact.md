# Phase 05 — Section `Graph` compact (qua crate `git_graph_core`)

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** pending · **Effort:** 6-8d · **Blocked by:** 01, 04

Miếng **D**, quyết định 7 (**compact + escape hatch ra tab**). Phase đắt nhất và rủi ro nhất. Đi **sau** phase 04 vì dùng lại cơ chế fixed-height + lazy-load đã được chứng minh ở đó.

## Phát hiện lúc lập plan: vòng phụ thuộc — kiến trúc đã đổi

`crates/git_graph/Cargo.toml:28` khai `git_ui.workspace = true`. **`git_graph` phụ thuộc `git_ui`**, nên `git_ui` không thể phụ thuộc `git_graph` — Cargo chặn vòng. Ý tưởng ban đầu ("panel host `Entity<GitGraph>` rồi gọi `render_graph`") **không compile được**.

**Quyết định:** tách crate `git_graph_core`, cả `git_ui` và `git_graph` đều phụ thuộc nó.

Hệ quả tốt: panel **không host `Entity<GitGraph>` nữa**. Nó tự giữ một `GraphData` và tự vẽ — ít coupling hơn, và **toàn quyền layout/scroll**, đúng thứ R1 cần.

Đã xác minh khả thi:

| Kiểm | Kết quả |
|---|---|
| Vùng lane core (dòng ~310–900: `GraphData`, `LaneState`, `BranchColor`, `CommitEntry`, `CommitLine`, `CommitLineKey`) tham chiếu `git_ui`? | **0 lần** — trích ra sạch |
| Thân `render_graph` (2180–2533) tham chiếu `git_ui`? | **0 lần** |
| `git_graph` dùng gì từ `git_ui`? | Chỉ 3 item (`CommitAvatar`, `CommitView`, `git_status_icon`) tại 5 call site — **toàn bộ ở commit-detail panel + actions, không ở phần vẽ graph** |
| Phần vẽ lane tham số hoá được? | Được. Là **một** lệnh `canvas(...)`. 11 `self.*` còn lại (`hovered_entry_idx`, `table_interaction_state`, `row_at_position`, `select_entry`, `open_commit_view`, `focus_handle`…) là interaction state — mỗi consumer tự giữ |

## Key insights

- **Không dùng `render_graph_compact` trên `GitGraph`** như bản plan đầu. Panel tự compose từ `git_graph_core`.
- Panel nạp log bằng `repo.get_graph_data(LogSource, LogOrder)` — cùng API phase 04 dùng, `git_ui` đã với tới được.
- `git_panel.default_width = 360` (`assets/settings/default.json:945`). Bảng 5 cột của tab là `0.14/0.6192/0.1032/0.086/0.0516` → **50/223/37/31/18px**. Đây là lý do panel vẽ **lane + subject**, không bảng.
- `⑂ Auto` = `LogSource::Branch(HEAD)` re-resolve khi HEAD đổi; toggle sang `LogSource::All`.
- CLAUDE.md: crate mới khai lib root tường minh → `[lib] path = "src/git_graph_core.rs"`, không `lib.rs`.
- Nhận từ phase 02: nút `Open Git Graph` (đang tạm ở menu `⋯` của `Changes`).

## Requirements

**Functional**
1. Crate `git_graph_core` giữ lane data + lane painting, dùng chung bởi `git_graph` (tab) và `git_ui` (panel).
2. `git_graph` tab **không regression**: vẫn đủ 5 cột, vẫn mọi hành vi hiện có.
3. Section `Graph` (collapsible) giữa `Changes` và `Commits`. **Default collapsed** (đặt ở phase 01).
4. Nội dung: lane canvas + subject. **Không** cột sha/author/date.
5. Toolbar: `⑂ Auto` (toggle `Branch(HEAD)` ↔ `All`) · `◎` (về HEAD) · `↓` pull · `↑` push · `⟳` refresh · **`⛶` mở `git_graph` tab** · `⋯`.
6. Chiều cao cố định + resize handle (dùng lại `PanelSection` của phase 04).
7. Lazy: `GraphData` chỉ nạp khi section expand lần đầu (R4).
8. `Open Git Graph` chuyển từ menu `⋯` của `Changes` sang `⛶`.

**Non-functional:** **không nhân đôi logic lane** — cả tab và panel gọi cùng hàm painting. Không nested-scroll trap (R1). Gập ⇒ không tiêu CPU.

## Architecture

```
crates/
├── git_graph_core/                 ← MỚI
│   ├── Cargo.toml                  [lib] path = "src/git_graph_core.rs"
│   └── src/git_graph_core.rs       GraphData, LaneState, BranchColor,
│                                   CommitEntry, CommitLine, CommitLineKey,
│                                   paint_lanes(...)
├── git_graph/    → git_graph_core, git_ui   (tab: lane + bảng 5 cột)
└── git_ui/       → git_graph_core           (panel: lane + subject)
```

`paint_lanes` tham số hoá, **không** nhận `&self`:

```rust
// git_graph_core
pub fn paint_lanes(
    graph_data: &GraphData,
    bounds: Bounds<Pixels>,
    row_height: Pixels,
    scroll_offset: Pixels,
    accents: &[Hsla],
) -> impl FnOnce(&mut Window, &mut App)
```

Panel state — cùng khuôn `CommitsSectionState` của phase 04:

```rust
struct GraphSectionState {
    graph_data: GraphData,
    log_source: LogSource,        // Auto = Branch(HEAD) | All
    loaded_for_branch: Option<SharedString>,
    scroll_handle: UniformListScrollHandle,
    canvas_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    height: Pixels,               // persist cùng SectionCollapseState
    _load_task: Option<Task<()>>, // drop = cancel
}
// Option<GraphSectionState> — None cho tới lần expand đầu
```

```
├──────────────────────────────────┤
│ ▼ Graph    ⑂Auto ◎ ↓ ↑ ⟳ ⛶ ⋯    │
│  │ ● Merge PR #7 from tgiap04…   │  ← paint_lanes + subject
│  ├─● style: format remaining…    │
│  │ ● refactor(workspace): say…   │
│  ╌╌╌╌╌ resize handle ╌╌╌╌╌       │
├──────────────────────────────────┤
```

## Related code files

**Tạo:**
- `crates/git_graph_core/Cargo.toml`, `crates/git_graph_core/src/git_graph_core.rs`
- `crates/git_ui/src/git_panel/graph_section.rs`

**Sửa:**
- `Cargo.toml` (workspace members) — thêm `git_graph_core`
- `crates/git_graph/Cargo.toml` + `src/git_graph.rs` — bỏ các type đã move, `use git_graph_core::…`, `render_graph` gọi `paint_lanes`
- `crates/git_ui/Cargo.toml` — thêm `git_graph_core`
- `crates/git_ui/src/git_panel.rs` — field `graph_section: Option<GraphSectionState>`, `Render` chèn section
- `crates/git_ui/src/git_panel/render_header.rs` — bỏ `Open Git Graph` khỏi menu `⋯` của `Changes`

## Implementation steps

**Bước 1–3 là refactor thuần trên `git_graph`. Không viết gì cho panel cho tới khi test `git_graph` xanh.**

1. Tạo crate `git_graph_core` (lib root tường minh). Move `BranchColor`, `LaneState`, `CommitEntry`, `CommitLine`, `CommitLineKey`, `GraphData` (dòng ~310–900). `git_graph` phụ thuộc nó + `use`.
2. Lift thân lệnh `canvas(...)` trong `render_graph` thành `git_graph_core::paint_lanes(...)` với tham số tường minh. `render_graph` gọi nó.
3. **Gate:** `./script/clippy` sạch + test `git_graph` xanh + mở tab bằng tay, đủ 5 cột, lane vẽ đúng. **Không đi tiếp nếu bước này chưa xanh.** Commit riêng.
4. `git_ui` phụ thuộc `git_graph_core`.
5. `GraphSectionState` + `Option<…>` trên `GitPanel`. `ensure_graph_loaded` chỉ gọi từ đường expand — cùng khuôn `ensure_commits_loaded` (phase 04).
6. `graph_section.rs`: `uniform_list` các row (subject) + `canvas` gọi `paint_lanes`. Chiều rộng lane cap lại để subject còn chỗ đọc.
7. Toolbar: `⑂ Auto` toggle `LogSource`; `◎` scroll tới HEAD; `↓`/`↑`/`⟳` dùng lại đường remote hiện có; `⛶` dispatch `git_graph::Open`; `⋯`.
8. `Auto` re-resolve khi HEAD đổi: so `loaded_for_branch` với branch hiện tại trong subscription đã có.
9. Bỏ `Open Git Graph` khỏi menu `⋯` của `Changes` — **chỉ sau khi** `⛶` chạy được.
10. Rà nested-scroll bằng tay: cuộn trong section không "ăn" cuộn của panel và ngược lại.
11. Test: (a) gập ⇒ `GraphData` không nạp; (b) expand ⇒ nạp đúng một lần; (c) tab vẫn đủ 5 cột; (d) toggle `Auto`/`All` đổi `LogSource`; (e) lane vẽ giống nhau ở tab và panel với cùng dữ liệu.

## Todo

- [ ] Crate `git_graph_core`, lib root tường minh (không `lib.rs`)
- [ ] Move lane data types, `git_graph` compile lại
- [ ] `paint_lanes` tham số hoá, `render_graph` gọi nó
- [ ] **Gate bước 3: test `git_graph` xanh + tab đủ 5 cột — commit riêng trước khi đi tiếp**
- [ ] `git_ui` → `git_graph_core`
- [ ] `GraphSectionState`, lazy theo expand, `_load_task` drop = cancel
- [ ] `graph_section.rs`: lane + subject, lane width cap
- [ ] Toolbar: `Auto` · `◎` · `↓` · `↑` · `⟳` · `⛶` · `⋯`
- [ ] `Auto` re-resolve khi HEAD đổi
- [ ] `Open Git Graph` rời menu `⋯` sau khi `⛶` chạy
- [ ] **Rà nested-scroll bằng tay** (R1)
- [ ] Test gập ⇒ không nạp; lane khớp giữa tab và panel

## Success criteria

- Test `git_graph` **và** `git_ui` đều xanh; `./script/clippy` sạch.
- `git_graph` tab: đủ 5 cột, lane vẽ đúng, không phân biệt được với trước phase này.
- Section gập: `GraphData` không nạp, `get_graph_data` không chạy.
- Expand ở 360px: lane + subject đọc được, không cột nào bị bóp dưới 40px.
- `⛶` mở tab.
- Cuộn trong section và cuộn panel không tranh nhau.
- `paint_lanes` **chỉ có một bản** — grep không thấy logic lane thứ hai.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Tách crate làm vỡ tab đang chạy tốt | **Gate bước 3** — commit riêng, test xanh, thử tay, trước khi viết dòng nào cho panel |
| `paint_lanes` tham số hoá bỏ sót state ẩn | 11 `self.*` đã được liệt kê; chỉ `graph_data` + `graph_canvas_bounds` + `graph_canvas_content_width` liên quan tới vẽ. Test (e) so lane giữa tab và panel là chốt |
| R1 nested scroll — rủi ro UX rõ nhất của cả plan | Fixed height + resize (đã chứng minh ở phase 04) + rà tay bước 10. Nếu vẫn tranh nhau: section không tự scroll, chỉ hiện N commit đầu + `⛶` |
| R4 nạp khi mở panel | Lazy theo expand, default collapsed từ phase 01, có test |
| Crate mới làm chậm build | `git_graph_core` không phụ thuộc `gpui`-nặng ngoài phần vẽ; đo `cargo build` trước/sau, ghi số vào commit |
| Compact ở 360px vẫn chật | Đây là lý do có `⛶`. Nếu không đọc được → báo lại; phase 00–04 đứng độc lập được |

## Security

Không có bề mặt mới. Pull/push dùng lại đường remote + `askpass_modal` hiện có. `get_graph_data` là đường đọc đã tồn tại.

## Next steps

Hết plan. Chạy `/tkm:write-journal`. Nếu compact ở 360px không đọc được: giữ section chỉ hiện N commit đầu + `⛶`, phần còn lại của plan không bị ảnh hưởng.
