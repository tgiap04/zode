---
title: 'Agent session history: panel bên phải, đọc lịch sử Claude và Codex'
description: >-
  Dựng crate agent_sessions đọc lịch sử session của Claude (46 file jsonl,
  221MB) và Codex (sqlite threads), rồi một Panel dock phải với icon asteroid:
  group theo project, search, hover toolbar, menu 9 mục, resume/fork qua
  agent_task(), delete vào Trash của OS.
status: completed
priority: P2
effort: 5-7d (delivered in one session)
branch: feat.release-v0.1.1
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
tags:
  - agent
  - panel
  - sessions
  - ui
blockedBy: []
blocks: []
---

# Agent session history

Thiết kế đã chốt trong `plans/reports/brainstorm-260822-agent-session-history.md` —
đọc file đó trước mọi phase. Mọi dữ kiện về format trong plan này đã được kiểm
chứng trên máy ngày 2026-08-22, không phải suy đoán.

**Giữ nguyên nhánh `feat.release-v0.1.1`.** Không tạo nhánh mới.

## Phases

| # | Phase | Trạng thái | Phụ thuộc | Sở hữu file |
|---|-------|-----------|-----------|-------------|
| 01 | [Crate `agent_sessions` + đọc danh sách Claude](phase-01-session-index-crate-and-claude-listing.md) | **done** | — | `crates/agent_sessions/**`, `Cargo.toml` |
| 02 | [Số đếm, lệnh resume, xoá vào Trash](phase-02-claude-counts-resume-and-trash.md) | **done** | 01 | `crates/agent_sessions/**` |
| 03 | [Provider Codex trên sqlite](phase-03-codex-provider-over-sqlite.md) | **done** (chặn đã mở: user chạy `codex`) | 02 + user chạy `codex` | `crates/agent_sessions/src/codex.rs` |
| 04 | [Icon asteroid + vỏ Panel](phase-04-asteroid-icon-and-panel-shell.md) | **done** | — (song song với 01) | `assets/icons/`, `crates/icons/`, `crates/agent_ui/src/session_history/panel.rs`, `crates/workspace/src/dock.rs`, `crates/zed/src/zed.rs` |
| 05 | [Danh sách: group, search, hàng](phase-05-grouped-list-search-and-rows.md) | **done** | 01 + 04 | `crates/agent_ui/src/session_history/{list,row}.rs` |
| 06 | [Hover toolbar + menu 9 mục](phase-06-row-actions-and-menu.md) | **done** | 02 + 05 (03 cho Codex) | `crates/agent_ui/src/session_history/{row,actions}.rs`, `crates/agent_ui/src/agent_view.rs` |

Phase 01 và 04 chạy song song được — khác crate, không chung file nào.

## Chặn đường (đã mở)

Phase 03 từng bị chặn: `threads` rỗng. Người dùng chạy `codex` một lần ngày
23/08 → 1 thread thật, và provider được viết trên dữ liệu đó chứ không đoán.
Chính lần đối chiếu đó phát hiện `has_user_event = 0` (xem bên dưới).

## Sửa lại sau khi đo thật (2026-08-23)

- **46 session, không phải 292.** `find -name '*.jsonl'` đếm cả transcript của
  subagent bên trong `<session>/subagents/`. Glob đúng ở mức session là `*/*.jsonl`
  → 46 file, 221MB, 45 file có hội thoại. Mọi con số hiệu năng trong plan này vì
  thế đều là bi quan.
- **`has_user_event` không dùng làm filter cho Codex được.** Thread thật đầu tiên
  ghi `has_user_event = 0` dù người dùng đã gõ vào đó. FR15 ban đầu dựa vào cột này
  và sẽ ẩn sạch session thật.
- **Danh sách phải theo project đang mở** (yêu cầu bổ sung của người dùng, 23/08):
  panel sống theo từng `Workspace`, lọc theo worktree root của chính nó, nên đổi
  project là đổi danh sách.
- **`uniform_list` không dùng được ở đây.** Hàng group và hàng session khác chiều
  cao; `uniform_list` giãn mọi hàng theo chiều cao của hàng đầu → các hàng vẽ đè
  lên nhau. Dùng `gpui::list`.

## Ràng buộc xuyên suốt

- **Không decode tên thư mục** `-Users-tgiap-dev-devs-zode` để lấy project. `-` vừa
  là separator vừa là ký tự hợp lệ, không tách ngược đúng được. Đọc `cwd` từ nội
  dung file.
- **Mọi field của Claude phải xuống thang được.** Format này không có tài liệu.
  Mất `ai-title` → dùng message đầu; mất `subagents/` → ẩn số đếm; dòng không
  parse được → bỏ dòng đó. Format đổi thì mất một cột, không mất cả panel.
- **Codex vắng mặt là trạng thái hợp lệ**, không phải lỗi. Xoá sạch
  `~/.codex/` thì panel vẫn phải chạy với Claude.
- Số đếm `msgs`/`subagents` là **dữ liệu chỉ để hiển thị** — không sort, không
  search theo chúng. Đó là lý do chúng được tính lazy theo hàng đang thấy.

## Kết quả (23/08)

Ba commit trên `feat.release-v0.1.1`: `37e214d` (crate dữ liệu), `bbe4e5d`
(panel), `75643f7` (tài liệu). Test: `agent_sessions` 20 + 2 real-store ·
`agent_ui` 27 · `workspace` 229 · `project_panel` 100 · `zode` 56 · clippy sạch ·
fmt sạch · build exit 0.

Reviewer: 0 critical, 0 high, 1 medium đã sửa (`paths_to_trash` của Codex dùng
`Path::starts_with` lexical → giờ canonicalize hai phía, có test cho `..` và
symlink) + 1 low đã sửa (tooltip nút fork nói sai lý do khi worktree chết).

### Hai câu hỏi mở đã trả lời

- **"Open Log" trên file 13MB: an toàn, giữ nguyên `Open Log`.** Bằng chứng chứ
  không phải phỏng đoán: `jsonl` **không** nằm trong `path_suffixes` của grammar
  JSON (`crates/grammars/src/json/config.toml`), nên file mở dạng plain text —
  không tree-sitter parse, không highlight 13MB. Và không có size guard nào trong
  `project`/`language`/`editor` chặn hay cảnh báo. Chi phí còn lại là nạp 13MB vào
  rope, việc editor này làm thường xuyên; shaping chỉ trả cho dòng đang thấy.
- **Keybinding: cố tình không thêm.** FR22 chỉ yêu cầu có trong command palette,
  và điều đó đã có. Trên macOS **mọi** `cmd-shift-<chữ>` đã bị chiếm, kể cả
  `cmd-shift-h`; chọn bừa một chord là tạo xung đột hoặc tự phát minh một quy ước
  mà repo này không dùng cho panel. Đường vào là icon asteroid ở header + command
  palette. Muốn chord nào thì nói, thêm một dòng.

## Định nghĩa xong

Panel mở ra với 45 session Claude (46 file, một file không có hội thoại) đọc hết
trong ~355ms ở background, group theo cwd thật **và chỉ của project đang mở**, sort mới
nhất trước; `msgs`/`subagents` của hàng đang thấy khớp với đếm tay trên hai
session; `Resume in Worktree` mở tab agent tiếp đúng hội thoại; path chết thì nút
disabled và badge đọc `Unavailable worktree`; Delete đưa cả `.jsonl` và thư mục
sidecar vào Trash và khôi phục được; xoá `~/.codex/state_*.sqlite` không làm panel
chết.
