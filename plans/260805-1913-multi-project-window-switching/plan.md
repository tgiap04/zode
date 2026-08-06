---
title: 'Multi-project trong một window: retention, hibernate tài nguyên, project switcher'
description: >-
  Mở khoá hạ tầng multi-workspace đã có sẵn trong cây code, thêm governor 3 trạng
  thái (Active/Warm/Hibernated) cho LSP-worktree-terminal, và dựng lại UI project
  switcher từ crate sidebar đã bị xoá ở lần hard-fork
status: pending
priority: P1
effort: 3.5w
branch: feat/multi-project-hibernation
tags:
  - feature
  - workspace
  - performance
  - rust
  - tech-debt
blockedBy: []
blocks: []
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
created: 2026-08-05
---

# Multi-project trong một window: retention, hibernate tài nguyên, project switcher

## Overview

Người dùng mở nhiều project trong cùng một window và chuyển qua lại như đổi channel Discord/Slack.
Project không được focus rơi vào hibernate theo ngưỡng idle: LSP/DAP/Prettier stop, worktree scanner
drop, **terminal và tiến trình user vẫn chạy**. State UI luôn nằm trên RAM để switch tức thì.

Điểm khởi đầu không phải con số không. `crates/workspace/src/multi_workspace.rs` (2159 dòng, +905 dòng
test) đã có toàn bộ retention, project groups và persistence — code upstream **nguyên bản, chỉ mất 42
dòng qua fork**. Nó ngủ vì trait `Sidebar` không còn implementor nào (crate `sidebar`, 16.785 dòng, bị
xoá ở `c3e2ac3`), và retention bị gate sau `sidebar_open`.

**Đang có một dead end thật:** `cli_default_open_behavior` mặc định `existing_window` → `zode <dir>`
thứ hai đẩy workspace vào window hiện tại qua `OpenMode::Add` → retained nhưng không render và không
có cách nào quay lại. Phase 1 là fix bug trước khi là tính năng.

Thiết kế đã chốt trong `brainstorm-report.md`. Plan này không mở lại các quyết định đó.

## Cross-Plan Dependencies

| Relationship | Plan | Status |
|-------------|------|--------|
| Builds on | [remove-auth-cloud-hard-fork](../260726-1531-remove-auth-cloud-hard-fork/plan.md) | Phases 1–12 completed (frontmatter còn `pending` — stale) |
| Will invalidate | `docs/generated/feature-list.md`, `docs/system/architecture.md` | Cần regenerate sau Phase 7 |

Không có quan hệ blocking. Hard-fork là **nguyên nhân** của trạng thái hiện tại (nó xoá `agent_ui` +
`sidebar`), việc của nó đã xong; plan này dọn phần nợ nó để lại.

## Phases

| Phase | Name | Status |
|-------|------|--------|
| 1 | [Tách retention khỏi sidebar và đưa cycling về MultiWorkspace](./phase-01-decouple-retention-and-cycling.md) | Pending |
| 2 | [ProjectActivity state machine và governor driver](./phase-02-project-activity-governor.md) | Pending |
| 3 | [LSP hibernate và wake, giữ diagnostic summary](./phase-03-lsp-hibernate-wake.md) | Pending |
| 4 | [Worktree pause resume và đối chiếu đĩa](./phase-04-worktree-pause-resume.md) | Completed |
| 5 | [Chính sách bộ nhớ terminal](./phase-05-terminal-memory-policy.md) | Pending |
| 6 | [Đo lường và cầu chì áp lực bộ nhớ](./phase-06-instrumentation-memory-fuse.md) | Completed (Go/TS measurement gap còn mở) |
| 7 | [Project switcher sidebar salvage từ git](./phase-07-project-switcher-sidebar.md) | Pending |

**Thứ tự phụ thuộc:** 1 → 2 → {3, 4 song song được} → 6 → 5. Phase 7 chỉ cần Phase 1, chạy song song
với 2–6 được. Phase 3 là phase đắt và nhạy cảm nhất — không gộp với bất kỳ phase nào khác.

Phase 5 chạy **sau** Phase 6 (red team Finding 7): phần đo terminal dùng chung hạ tầng `sysinfo` mà
Phase 6 dựng, hai đường đo riêng sẽ cho hai con số không so được với nhau. Số thứ tự file giữ nguyên để
không phải đổi tên và đổi liên kết.

## Red Team Review

### Session — 2026-08-05
**Findings:** 14 (12 accepted, 2 rejected) · **Severity:** 1 Critical, 4 High, 7 Medium
**Báo cáo đầy đủ:** [reports/red-team-report.md](./reports/red-team-report.md)

| # | Finding | Severity | Disposition | Applied To |
|---|---------|----------|-------------|------------|
| 1 | `hibernate_after` dạng chuỗi thời lượng là hư cấu — settings dùng ms | Critical | Accept | Phase 2, Phase 6 |
| 2 | Bỏ sót đường retention thứ ba (`activate_provisional_workspace`) | High | Accept | Phase 1 |
| 3 | Autosave đua với hibernate ⇒ format-on-save chết im lặng | High | Accept | Phase 3 (FR8) |
| 4 | Code salvage có thể mang lại symbol auth đã bị xoá | High | Accept | Phase 7 |
| 5 | Summary stale che lỗi ở file *không* đổi | High | Accept | Phase 3 (FR4b), Phase 4 (FR6) |
| 6 | `app_will_quit` không flush state từng workspace | Medium | Accept | Phase 1 (bước xác minh) |
| 7 | Phase 5 đứng sai thứ tự (cần hạ tầng đo của Phase 6) | Medium | Accept | plan.md, Phase 5 |
| 8 | "Hai workspace chia một Project" là giả định chưa chứng minh | Medium | Accept | Phase 2 (bước 0) |
| 9 | Cầu chì và timer đánh nhau, không ai định nghĩa ai thắng | Medium | Accept | Phase 6 (FR4b) |
| 10 | Phase 7 có thể không thuộc plan này | Medium | Accept → hỏi user | Validation Log |
| 11 | Phase 3 FR7 (remote/SSH) được khẳng định nhưng không thiết kế | Medium | Accept | Phase 3 (FR7) |
| 12 | Test bất biến quan trọng nhất có thể không bao giờ được viết | Medium | Accept | Phase 2, Phase 5 |
| R1 | "Đường quit làm mất state workspace retained" | — | **Reject** | Bị bác bởi `zed.rs:1431-1445` |
| R2 | "4 setting là over-configuration" | — | **Reject** | Đường thoát cho cơ chế tự động, không phải gold plating |

**Sweep nhất quán:** đã rà `plan.md` + 7 phase file sau khi áp; không còn mâu thuẫn chưa giải quyết.
Chi tiết ở cuối báo cáo red team.

## Validation Log

### Session 1 — 2026-08-05
**Trigger:** `--level max` chạy validation bắt buộc, sau red team
**Questions asked:** 4

#### Questions & Answers

1. **[Scope]** Phase 7 (sidebar UI) chiếm ~40% effort và là crate mới. Giữ trong plan này hay tách riêng?
   - Options: Giữ trong plan này (Recommended) | Tách plan riêng, blockedBy plan này | Giữ trong plan, làm ngay sau Phase 1
   - **Answer:** Giữ trong plan này
   - **Rationale:** Đề bài gốc nói thẳng "click vào sidebar" — thiếu nó thì tính năng chưa trọn. Phase 7 chỉ phụ thuộc Phase 1 nên không làm chậm phần hibernate. Red team Finding 10 đóng lại.

2. **[Assumptions]** `retain_background_projects` mặc định là gì ngay sau Phase 1, khi chưa có hibernate?
   - Options: false, flip sau Phase 3 (Recommended) | true ngay từ Phase 1
   - **Answer:** false, flip sau Phase 3
   - **Rationale:** Phase 1 fix dead end mà không đẩy người dùng vào cảnh N project sống cùng lúc trong khoảng chưa có governor. Thêm một bước flip vào cuối Phase 3.

3. **[Risks]** Red team bắt được: hibernate lúc autosave đang chờ làm format-on-save chết im lặng. Rào thế nào?
   - Options: Chỉ chặn khi autosave đang bật (Recommended) | Chặn mọi khi còn buffer dirty | Hibernate luôn, chỉ await request đang bay
   - **Answer:** Chỉ chặn khi autosave đang bật
   - **Rationale:** Mặc định `"autosave": "off"` (`default.json:988`) nên phần lớn người dùng không có cuộc đua này. Buffer dirty với autosave tắt không được chặn hibernate, nếu không thì ai hay để tab dirty sẽ mất hẳn tính năng.

4. **[Scope]** Hibernate cho project remote/SSH: v1 có làm không?
   - Options: Ngoài scope v1 (Recommended) | **Làm luôn trong Phase 3** | Plan riêng ngay sau plan này
   - **Answer:** Làm luôn trong Phase 3 *(người dùng chọn khác khuyến nghị)*
   - **Rationale:** Đường proto đã có sẵn cho cả stop (`lsp_store.rs:11140-11150`) và restart (`:11183-11211`). Nhưng phần khó là summary phía local bị wipe bởi `UpdateDiagnosticSummary` count = 0 do host đẩy xuống, không phải bởi code local — phải xử lý tường minh. Phase 3: 3–4 ngày → 5–6 ngày; plan: 3w → 3.5w.

#### Confirmed Decisions
- Phase 7 nằm trong plan này, chạy song song với 2–6.
- `retain_background_projects` default `false`; flip `true` ở bước cuối Phase 3.
- Hoãn hibernate chỉ khi autosave đang bật **và** có buffer dirty.
- Remote/SSH hibernate nằm trong scope Phase 3, kèm chi phí đã ghi rõ.

#### Impact on Phases
- Phase 1: FR5 default đổi thành `false`; risk row "retain nặng RAM" đã chốt cách xử lý.
- Phase 3: FR7 viết lại (remote in-scope + vấn đề summary bị wipe từ host); FR8 viết lại (gate theo autosave); thêm bước 6c, 6d; effort 5–6 ngày; 2 risk row mới.
- Phase 7: giữ nguyên vị trí, không tách.

## Dependencies

- `crates/workspace` (multi_workspace, workspace_settings, persistence), `crates/project`
  (project, lsp_store, worktree qua `crates/worktree`, terminals), `crates/terminal`,
  `crates/settings_content`, `crates/recent_projects`, `crates/migrator` (chỉ nếu đổi chỗ `SidebarSide`)
- `sysinfo` (đã có, qua `crates/system_specs`) cho đo RSS ở Phase 6
- Nguồn salvage Phase 7: `git show c3e2ac3^:crates/sidebar/src/sidebar.rs`
- Build gate: `./script/clippy` (KHÔNG dùng `cargo clippy`), `cargo test -p workspace -p project -p terminal`
