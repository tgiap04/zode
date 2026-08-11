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
| 01 | [PanelSection + title row + zoom](phase-01-panel-section-and-title-row.md) | B | 2-3d | pending | 00 |
| 02 | [Commit box lên top, giải tán footer](phase-02-commit-box-to-top.md) | A | 3-4d | pending | 01 |
| 03 | [Section Repositories](phase-03-repositories-section.md) | C | 2-3d | pending | 01 |
| 04 | [Section Commits](phase-04-commits-section.md) | E | 3-4d | pending | 01 |
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
| R1 | Hai vùng scroll lồng nhau (section Graph + Commits, trong panel cũng scroll) | 04, 05 | Section có chiều cao **cố định + resize handle**, không tự co giãn theo nội dung. Chứng minh ở 04 trước khi làm 05. |
| R2 | Commit box đổi chỗ phá giả định của `Focusable for GitPanel` + `focus_changes_list` / `focus_editor` / `expand_commit_editor` | 02 | Rà cả 3 action + `Focusable` trong cùng phase; test focus order. |
| R3 | Collapse state không persist → mở panel thấy 4 section bung ra, tệ hơn hiện tại | 01 | `SerializedGitPanel` (`git_panel.rs:254`) + `serialize()` đã có — thêm field, không dựng hạ tầng mới. |
| R4 | Nhúng graph → `get_graph_data` chạy mỗi lần mở panel | 05 | Lazy: chỉ khởi tạo `Entity<GitGraph>` khi section Graph expand lần đầu. |
| R5 | **Hai tầng header**: section `Changes` + group header `Tracked`/`Untracked` (`render_list_header`, đang là list entry có checkbox staging) | 01, 02 | Không xoá group header (nó mang checkbox staging). Giảm nhấn thị giác: group header nhỏ hơn, không có disclosure riêng. |
| R6 | Phase 00 là diff lớn dạng no-op → khó review | 00 | ✅ Đã xử: 5 commit riêng, clippy + 60/60 test xanh giữa mỗi commit. |
| R7 | Hàm mới đặt trong submodule của `git_panel` mà được gọi từ `git_panel.rs` sẽ **không compile** — cha không thấy private của con | 01–05 | Đánh `pub(super)`. Không phải nới visibility: mức hiệu dụng y hệt `fn` trần trong module cha. Chi tiết trong phase 00 → "Hai điều phase này dạy lại cho plan". |

## Gate mỗi phase

`./script/clippy` sạch · test `git_ui` (+ `git_graph` ở phase 05) xanh · panel dùng được thật sau mỗi phase.

## Bước tiếp theo

`/tkm:takumi plans/260811-1634-vscode-parity-git-panel` — hoặc chạy `/tkm:create-plan red-team plans/260811-1634-vscode-parity-git-panel` trước nếu muốn ép blueprint qua review đối kháng.
