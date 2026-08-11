---
title: 'Git Panel: parity gần pixel với VSCode Source Control'
description: >-
  Tách git_panel.rs (7.6k dòng) thành module, dựng PanelSection collapsible, đưa
  commit box lên top, thêm section Repositories + Commits, và nhúng git_graph
  dạng compact — 6 phase, mỗi phase ship được độc lập.
status: pending
priority: P2
effort: 3.5-5w
branch: feat/vscode-parity-git-panel
tags:
  - feature
  - ui
  - git
  - fork
  - rust
blockedBy: []
blocks: []
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
created: 2026-08-11
---

# Git Panel: parity gần pixel với VSCode Source Control

**Context:** [brainstorm-260811-vscode-parity-git-panel.md](../reports/brainstorm-260811-vscode-parity-git-panel.md) — 8 quyết định đã chốt, dữ kiện đã xác minh, layout đích, checklist tái định cư. **Không re-litigate các mục trong đó.**

## Mục tiêu

Panel git mở từ rail có cấu trúc + hành vi của VSCode Source Control: title row, 4 section collapsible (`Repositories` / `Changes` / `Graph` / `Commits`), commit box nằm **trên** file list.

## Phases

| # | Phase | Miếng | Effort | Status | Blocked by |
|---|---|---|---|---|---|
| 00 | [Tách git_panel.rs thành module](phase-00-extract-git-panel-modules.md) | — | 2-3d | **completed** | — |
| 01 | [PanelSection + title row + zoom](phase-01-panel-section-and-title-row.md) | B | 2-3d | **completed** | 00 |
| 02 | [Commit box lên top, giải tán footer](phase-02-commit-box-to-top.md) | A | 3-4d | **completed** | 01 |
| 03 | [Section Repositories](phase-03-repositories-section.md) | C | 2-3d | **completed** | 01 |
| 04 | [Section Commits](phase-04-commits-section.md) | E | 3-4d | **completed** | 01 |
| 05 | [Section Graph compact + crate `git_graph_core`](phase-05-graph-section-compact.md) | D | 6-8d | pending | 01, 04 |

Phase 02 / 03 / 04 độc lập với nhau — chạy song song được sau 01. Phase 05 đi sau 04 vì hai section này dùng cùng một cơ chế lazy-load + fixed-height + resize handle; làm 04 trước để cơ chế đó được chứng minh trên section rẻ hơn.

## Quyết định đã chốt (từ brainstorm)

1. Scope: cả 5 miếng A+B+C+D+E · 2. Commit box **chuyển hẳn** lên top · 3. Parity **gần pixel**
4. **Tách file trước** khi đổi layout · 5. Đường 1 — scaffold trước · 6. Title row **đầy đủ cả 3**
7. Graph **compact + escape hatch** ra tab · 8. Affordance bị xoá → **tái định cư hết**

## Ba câu mở đã đóng trong lúc lập plan

| Câu | Kết luận |
|---|---|
| Nút `✨` AI commit message nối vào đâu? | **Không buildable — bỏ khỏi scope.** `commit_message_prompt.txt` chỉ được `prompt_store` tham chiếu; hard fork 260726-1531 đã xoá toàn bộ crate AI (`agent`, `assistant`, `language_model`, `cloud_llm`, `zeta`). Không còn model nào để gọi. |
| `⑂ Auto` của VSCode map sang `LogSource` nào? | `Auto` = `LogSource::Branch(HEAD)` re-resolve khi HEAD đổi; toggle sang `LogSource::All`. `GitGraph` đã subscribe `GitStoreEvent::RepositoryUpdated` nên có hook sẵn. Chi tiết ở phase 05. |
| `Compare main with origin/main` (expandable) | **Ngoài scope.** `LogSource` không có variant `Range`; expand cần thêm backend. Phase 04 chỉ làm dòng ahead/behind tổng hợp (không expand). |

## Phát hiện lúc lập plan làm đổi kiến trúc phase 05

`crates/git_graph/Cargo.toml:28` khai `git_ui.workspace = true` → **`git_graph` phụ thuộc `git_ui`**, nên `git_ui` không thể host `Entity<GitGraph>` (Cargo chặn vòng). Ý tưởng ban đầu của brainstorm không compile được.

**Đã chốt:** tách crate **`git_graph_core`** (lane data + `paint_lanes`), cả `git_ui` và `git_graph` phụ thuộc nó. Đã xác minh vùng lane core (dòng ~310–900) và thân `render_graph` (2180–2533) **không tham chiếu `git_ui` lần nào** → trích ra sạch. Hệ quả tốt: panel không host `Entity<GitGraph>`, nên **toàn quyền layout/scroll** — đúng thứ R1 cần. Giá: phase 05 từ 4-5d lên **6-8d**.

## Rủi ro xuyên phase

| # | Rủi ro | Phase | Đối phó |
|---|---|---|---|
| R1 | Hai vùng scroll lồng nhau (section Graph + Commits, trong panel cũng scroll) | 04, 05 | Section có chiều cao **cố định + resize handle**, không tự co giãn theo nội dung. Chứng minh ở 04 trước khi làm 05. **Phase 03 đã dựng nửa cơ chế:** `fills_height()` opt-in + `max_h` & `overflow_y_scroll` trên `Repositories`. 04/05 **không** gọi `fills_height()`. |
| R2 | Commit box đổi chỗ phá giả định của `Focusable for GitPanel` + `focus_changes_list` / `focus_editor` / `expand_commit_editor` | 02 | ✅ Đã xử: cả 3 action độc lập với element tree, không phải sửa. `Focusable` giờ luôn trả `focus_handle` — lý do ghi trong comment tại `impl Focusable for GitPanel`. |
| R3 | Collapse state không persist → mở panel thấy 4 section bung ra, tệ hơn hiện tại | 01 | ✅ Đã xử: `Option<SectionCollapseState>` trên `SerializedGitPanel`, `#[serde(default)]` ở **cả hai** tầng nên blob cũ và blob thiếu field lẻ đều đọc được. Test đi qua kvp thật. |
| R4 | Nhúng graph → `get_graph_data` chạy mỗi lần mở panel | 05 | ✅ Cơ chế đã chứng minh ở phase 04: `graph_data` (kích fetch) chỉ gọi từ đường expand, render chỉ dùng `get_graph_data` (không kích); có test khẳng định gập ⇒ cache vẫn rỗng. `graph` **và** `commits` đều mặc định gập vì fetch không chặn được từ `git_ui`. |
| R5 | **Hai tầng header**: section `Changes` + group header `Tracked`/`Untracked` (`render_list_header`, đang là list entry có checkbox staging) | 01, 02 | Không xoá group header (nó mang checkbox staging). Giảm nhấn thị giác: group header nhỏ hơn, không có disclosure riêng. |
| R6 | Phase 00 là diff lớn dạng no-op → khó review | 00 | ✅ Đã xử: 5 commit riêng, clippy + 60/60 test xanh giữa mỗi commit. |
| R7 | Hàm mới đặt trong submodule của `git_panel` mà được gọi từ `git_panel.rs` sẽ **không compile** — cha không thấy private của con | 01–05 | Đánh `pub(super)`. Không phải nới visibility: mức hiệu dụng y hệt `fn` trần trong module cha. Chi tiết trong phase 00 → "Hai điều phase này dạy lại cho plan". |

## Gate mỗi phase

`./script/clippy` sạch · test `git_ui` (+ `git_graph` ở phase 05) xanh · panel dùng được thật sau mỗi phase.

## Điều phase 04 sửa lại cho phase 05 — đọc trước khi làm Graph

Phase 05 dùng lại đúng cơ chế phase 04 vừa dựng, nên bốn điều dưới đây áp dụng trực tiếp:

- **`Repository::graph_data` = kích fetch · `Repository::get_graph_data` = chỉ đọc cache.** Render **chỉ** được gọi cái thứ hai. Đây là cách R4 kiểm được bằng test, không phải bằng lời.
- **Dữ liệu dòng nằm ở hai tầng.** `graph_data` cho topology (`sha`, `parents`, `ref_names`); subject/author/time phải lấy riêng qua `fetch_commit_data(sha)` theo từng dòng. Section Graph cần **cùng** cả hai — và nó chia **chung cache** với section Commits, nên mở Graph khi Commits đã mở là gần như miễn phí.
- **Invalidate vô điều kiện trên `HeadChanged`/`BranchListChanged`.** `Repository` xoá sạch `initial_graph_data` ở đó, kể cả khi tên branch không đổi. So tên branch để bỏ qua = section treo mãi. Xem điều 1 của phase 04.
- **`on_drag_move` trả `event.bounds` của element đăng ký listener**, không phải element bị kéo → resize phải tính theo **delta**, không theo khoảng cách tới `bounds.top()`.
- **`fixed_height()` chỉ `overflow_hidden`, nội dung tự scroll.** `fills_height()` và `fixed_height()` loại trừ nhau (có `debug_assert!`). Graph là fixed-height, **không** gọi `fills_height()`.
- **Việc còn nợ, ngoài phạm vi `git_ui`:** fetch của `initial_graph_data` chạy `git log` **không `--max-count`** → luôn stream cả history; `commit_data: HashMap<Oid, …>` không evict. Vì thế `commits` **và** `graph` đều mặc định gập. Cap thật cần sửa `crates/git` + `crates/project`.

## Điều phase 03 sửa lại cho các phase sau

- **`PanelSection::fills_height()` là opt-in — chỉ `Changes` gọi.** Nếu phase 04/05 để `Commits`/`Graph` cũng `fills_height()` thì các section chia nhau chiều cao panel. Theo plan hai section đó **cao cố định + resize handle**, nên **không** gọi nó.
- **Section cao tự nhiên phải có nắp.** `Repositories` giới hạn 5 row + `overflow_y_scroll`. Vì `Changes` là con duy nhất co được, bất kỳ section nào cao không giới hạn đều bóp `Changes` về 0. Ảnh hưởng trực tiếp R1.
- **Kiểm scope từng affordance trước khi đặt lên row/section không-active.** `set_as_active_repository` và `branch_picker::popover` nhận repo tường minh → scope được. `git::Fetch`/`Push`/`Pull` dispatch toàn cục, giải về `self.active_repository` → **không** scope được. Phase 05 (`Open Git Graph` sang toolbar section Graph) gặp lại đúng câu hỏi này.
- **Việc còn nợ, tách khỏi phase 03:** (1) emit event khi `work_directory_abs_path` đổi trong `crates/project/src/git_store.rs` để thứ tự row không lệch sau một lần rename lặng; (2) đổi `GitPanel::fetch`/`push`/`pull`/`stash_*` sang nhận repo tường minh — mở đường cho menu `⋯` mỗi row (yêu cầu 6 của phase 03).

## Điều phase 02 sửa lại cho các phase sau

- **Checklist tái định cư phải kê cả affordance *hiển thị*, không chỉ nút.** Dòng "row previous-commit" của phase 02 gói 4 việc; checklist chỉ theo dõi 1 (nút Uncommit) và suýt làm mất 3 việc còn lại. Phase 03 (`zode / main`, `Fetch`, `Initialize Repository`) và 05 (`Open Git Graph`) phải soi lại từng dòng theo cách này trước khi xoá gì.
- **Gate của action handler ≠ gate của menu entry.** `git_ui::git_panel::Open` chỉ có handler khi có repo active. Ảnh hưởng phase 05 khi dời `Open Git Graph` sang toolbar section Graph.
- **Đọc setting thì phải nghe `SettingsStore`.** Observer trong `GitPanel::new` chỉ theo dõi một danh sách field cố định; thêm chỗ đọc setting mới thì phải thêm field vào đó.
- **`PanelRepoFooter` + `render_last_commit` đang ở chỗ tạm trong commit box.** Phase 03 nhận `PanelRepoFooter`, phase 04 nhận `render_last_commit`. Cả hai mang `// TODO(phase-0X)`.
- **Commit box nằm trong section `Changes`** → gập `Changes` ẩn luôn commit box. Đúng spec phase 02; nếu phase sau muốn commit box thường trực thì phải nhấc ra ngoài mọi section.

## Điều phase 01 sửa lại cho các phase sau

- **Nút `⛶` không cần chạm crate `workspace`.** Cơ chế zoom đã đủ: emit `PanelEvent::ZoomIn/ZoomOut`, `Dock` (`dock.rs:637-668`) gọi lại `set_zoomed`. Rủi ro "cần thêm ở `workspace`" trong phase 01 không thành hiện thực.
- **`ui::CountBadge` không dùng inline được** — nó `absolute()`, thiết kế để phủ lên icon. Section badge phải tự dựng pill. Ảnh hưởng phase 03, 04.
- **Nội dung section phải gate `expanded` ở call site**, không chỉ trong `PanelSection`. Nếu không, element bị dựng rồi bỏ mỗi lần re-render lúc gập. Ảnh hưởng phase 03, 04, 05 — đặc biệt 05, nơi nội dung đắt nhất (R4).
- **Ba variant `PanelSectionKind` còn `#[allow(dead_code)]`.** Phase 03/04/05 phải **xoá dần** attribute đó khi variant của mình được dựng, không để nguyên.
- **`div().flex_1()` tạm khi `Changes` gập** (giữ commit box ở đáy) — phase 02 xoá nó.

## Bước tiếp theo

`/tkm:takumi plans/260811-1634-vscode-parity-git-panel` — hoặc chạy `/tkm:create-plan red-team plans/260811-1634-vscode-parity-git-panel` trước nếu muốn ép blueprint qua review đối kháng.
