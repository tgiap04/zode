---
title: 'Agent Claude Code + Codex: nút trên rail, màn hình ở center'
description: >-
  Khôi phục ACP slice (~45k dòng) từ chính history của fork, thêm hai built-in
  agent vào AgentServerStore đang sống, dựng nút rail và màn hình agent ở center
  cạnh editor với hai mode terminal / chat UI, kèm đường báo-và-hướng-dẫn-cài khi
  chưa có CLI — 7 phase, P2 đã dùng được thật.
status: pending
priority: P2
effort: 4-6w
branch: feat/rail-agents-claude-code-codex
tags:
  - feature
  - agent
  - acp
  - ui
  - rust
  - fork
blockedBy: []
blocks: []
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
created: 2026-08-12
---

# Agent Claude Code + Codex: nút trên rail, màn hình ở center

**Context:** [brainstorm-260811-rail-agents-claude-code-codex.md](../reports/brainstorm-260811-rail-agents-claude-code-codex.md) — 11 quyết định đã chốt, dữ kiện đã xác minh, kiến trúc, rủi ro. **Không re-litigate bảng "Quyết định đã chốt".**

## Mục tiêu

Hai nút agent trên project rail (icon riêng). Click mở một màn hình ở center cạnh code editor — chat UI hoặc terminal, chọn lúc mở. Chưa cài CLI thì được báo, kèm lệnh cài đúng OS và một nút mở terminal sẵn lệnh.

## Phases

| # | Phase | Effort | Status | Blocked by |
|---|---|---|---|---|
| 00 | [Khôi phục 4 crate ACP](phase-00-restore-acp-crates.md) | 4-6d | **completed** | — |
| 01 | [Built-in agent + resolve binary](phase-01-builtin-agents-and-resolution.md) | 2-3d | **completed** | — |
| 02 | [Nút rail + terminal mode ở center](phase-02-rail-buttons-and-terminal-mode.md) | 3-4d | **completed** | 01 |
| 03 | [Đường chưa-cài-CLI](phase-03-missing-cli-ux.md) | 2-3d | **completed** | 02 |
| 04 | [Chat UI thành center item](phase-04-chat-ui-center-item.md) | 8-12d | **completed** | 00, 02 |
| 05 | [Permission + diff review + ACP terminal](phase-05-permissions-diff-terminal.md) | 4-6d | pending | 04 |
| 06 | [Serialize, luật split, polish](phase-06-persistence-and-polish.md) | 3-4d | pending | 04 |

**00 và 01 chạy song song được** — 01 chỉ chạm `crates/project` + assets, 00 chỉ chạm crate mới khôi phục. 02 chỉ cần 01. Đây là lý do 02 đứng trước 04: terminal mode dùng **chung đúng đường resolve binary** với chat UI, nên nó là bài kiểm tra rẻ cho 01 trước khi đổ 45k dòng vào.

## Quyết định đã chốt (từ brainstorm)

1. ACP slice ~45k LOC, restore từ `c3e2ac3^` · 2. Ưu tiên CLI local, npx opt-in · 3. Mỗi màn hình một mode
4. Hardcode 2 nút rail · 5. Click = Chat UI, chuột phải = menu · 6. Vị trí center theo bên của rail
7. Agent thứ hai = pane thứ hai · 7b. Split **ngang** mặc định, kéo thả đổi được, layout được serialize
8. Toast **và** empty state · 9. "Cài ngay" mở terminal sẵn lệnh, chưa Enter · 10. Serialize tab, session mới

## Hai câu "còn treo" đã đóng trong lúc lập plan

**1. Lệnh cài — đã lấy từ docs chính hãng.** `npm i -g @anthropic-ai/claude-code` **không** deprecated (nằm ở mục "Advanced installation options"); native installer mới là "Recommended". Bảng đầy đủ ở [phase 03](phase-03-missing-cli-ux.md). Binary native cài vào `~/.local/bin/claude` → symlink sang `~/.local/share/claude/versions/` — khớp đúng máy dev.

**2. Drift đo được, và thấp hơn giả định.** Commit chạm từng crate kể từ `c3e2ac3`:

| Crate | Commit | | Crate | Commit |
|---|---|---|---|---|
| `gpui` | **0** | | `workspace` | 27 |
| `editor` | **0** | | `project` | 11 |
| `language` | **0** | | `settings` | 7 |
| `multi_buffer` | **0** | | `ui` | 3 |
| `buffer_diff` | **0** | | `theme` | 3 |
| `prompt_store` | **0** | | `terminal` | 1 |

Toàn bộ bề mặt API mà 45k dòng UI slice dùng nhiều nhất — `gpui`, `editor`, `language`, `multi_buffer`, `buffer_diff` — **không đổi một dòng** kể từ điểm cắt. Phần tăng của `workspace` nằm ở `multi_workspace.rs` (+1003) và test của nó (+1328), tức **code mới của fork**; API host `Item` gần như đứng yên: `pane.rs` **+9**, `workspace.rs` +70, `pane_group.rs` +38. Ước lượng effort của 00 và 04 dựa trên con số này.

## Ba phát hiện lúc lập plan làm đổi kiến trúc

**1. Tầng resolve + registry đã có sẵn và gần như chạy được — chỉ thiếu một mắt xích.**
`agent_server_store.rs` đã cài trọn đường registry: `AgentRegistryStore::try_global` (`:433`, `:647`, `:864`), nhánh `CustomAgentServerSettings::Registry` dựng server qua `node_runtime` + `registry_id` (`:551`, `:572`), `refresh_if_stale` khi settings có registry agent (`:437`), và `cx.observe` registry để rebuild (`:648`). Nhưng `AgentRegistryStore::init_global` **chỉ được gọi ở `crates/remote_server/src/headless_project.rs:233`** — app desktop không gọi, nên `try_global` trả `None` và mọi registry agent im lặng không resolve. Phase 01 vì thế rẻ hơn dự tính: **wire global + khai hai entry settings**, không phải viết resolve mới.

**2. Binary local không nói ACP. Chat UI bắt buộc chạy npx adapter.**
`claude` v2.1.227 chỉ có `--output-format=stream-json`, `mcp`, `agents` — **không có subcommand ACP nào**. Registry ACP (`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, đã fetch thật, version 1.0.0) phân phối cả hai dạng npx:

```
claude-acp  → npx @agentclientprotocol/claude-agent-acp@0.66.0   (Anthropic · Zed · JetBrains)
codex-acp   → npx @agentclientprotocol/codex-acp@1.1.14          (OpenAI · Zed · JetBrains)
```

Package đã đổi org — **không** còn là `@zed-industries/claude-code-acp` như brainstorm ghi. Hệ quả lên quyết định #2: nó đúng trọn cho **terminal mode** (binary local, auth/config/MCP của người dùng); với **chat UI** thì npx không phải fallback mà là đường duy nhất.

**3. Đã chốt thêm (2026-08-12): chat UI dùng gate mềm.** Kiểm CLI local trước; thiếu thì empty state nói thật *"chưa thấy `claude` trên máy"* kèm **hai** nút: `[Cài CLI]` (khuyến nghị, mở terminal sẵn lệnh) và `[Chạy bằng npx]`. Giữ đúng consent của #2 — không im lặng tải — mà không chặn người chỉ muốn thử.

## Rủi ro xuyên phase

| # | Rủi ro | Phase | Đối phó |
|---|---|---|---|
| R1 | PATH của app GUI trên macOS không thấy `~/.local/bin` → báo "chưa cài" trong khi đã cài | 01, 03 | Resolve **bắt buộc** qua `ProjectEnvironment` (shell env thật), không dùng env của process. Test hồi quy đặt binary giả ngoài `PATH` mặc định |
| R2 | Drift làm 45k dòng không compile | 00, 04 | Đã đo (bảng trên): 6 crate nền 0 commit. Vẫn restore **bottom-up, mỗi crate một commit**, `./script/clippy` xanh mới sang crate sau. Nếu 00 vượt 6d → dừng, ước lượng lại 04–06 |
| R3 | `agent_settings` không trim → settings schema mọc key chết của agent native | 00 | Trim trong chính phase 00; test schema không xuất hiện key model/provider |
| R4 | Ba pane + sidebar hết chiều ngang trên laptop | 02, 04, 06 | Pane agent có min-width; split ngang là **mặc định**, không phải bắt buộc — kéo thả đổi được và layout được serialize (7b) |
| R5 | npx kéo package lúc runtime = supply chain | 01, 03 | Version **ghim** trong default settings. Chạy npx phải qua một click tường minh (gate mềm), không tự động |
| R6 | ACP `terminal/*` không wire → agent treo giữa hội thoại | 05 | Nằm trong định nghĩa done của 05, không để trôi sang "polish" |
| R7 | File port vượt xa luật 200 dòng (`thread_view.rs` 9.311 dòng) | 04 | **Ngoại lệ có chủ ý** — chẻ nó ra là phá khả năng đối chiếu upstream, đổi lấy con số đẹp. Không ai được "sửa" về sau |
| R8 | Registry là dependency mạng | 01 | Version ghim sẵn trong settings; registry chỉ để nâng version. Mất mạng ⇒ vẫn resolve được từ giá trị ghim |

## Gate mỗi phase

`./script/clippy` sạch · test của crate bị chạm xanh · **mỗi phase từ 02 trở đi phải dùng được thật trong app**, không chỉ compile.

## Ghi chú cross-plan

Không có blocker sống. Ba plan còn `status: pending` (`260726-1531` hard-fork, `260805-1913` multi-project, `260807-1300` themes) đều đã hạ cánh trong code (`c3e2ac3`, `crates/sidebar`, họ theme 2026) — status của chúng là **stale**, không phải việc đang chạy. Plan này khôi phục một lát mỏng của thứ `260726-1531` đã xoá; đó là bổ sung có chủ ý, không phải revert, và không cần `blockedBy`.

## Bước tiếp theo

`/tkm:takumi plans/260812-0008-rail-agents-claude-code-codex` — hoặc `/tkm:create-plan red-team plans/260812-0008-rail-agents-claude-code-codex` trước nếu muốn ép blueprint qua review đối kháng.
