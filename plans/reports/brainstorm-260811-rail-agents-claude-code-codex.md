# Brainstorm — Agent Claude Code + Codex trong rail

**Ngày:** 2026-08-11 · **Lens:** CTO · **Level:** medium
**Commission:** thêm hai nút agent (Claude Code, Codex — đúng icon riêng) vào project rail; ấn vào mở một màn hình ở center cạnh code editor, xem được dạng terminal hoặc dạng UI; chưa cài CLI thì báo kèm lệnh cài tương ứng.

## Commission

Rail hôm nay (`crates/sidebar/src/rail.rs`, 296 dòng) có ba tầng: ô project (Discord-style) → nút panel của dock cùng phía (`rail_panels.rs`) → footer (toggle panel list + Open Project). Yêu cầu này thêm tầng thứ tư, và là tầng đầu tiên **không** trỏ tới một dock panel.

## Dữ kiện đã xác minh

| Dữ kiện | Ý nghĩa |
|---|---|
| `rail_panels.rs:48-55` lấy entry từ `Panel::icon` của dock cùng phía; comment ghi rõ pane item "would each need wiring of their own" | Nút rail → center item là đoạn wiring **chưa tồn tại**, không phải mở rộng cái có sẵn |
| `crates/project/src/agent_server_store.rs` — 2.241 dòng, **đang sống**, wired tại `project.rs:1348` (`AgentServerStore::local`) | Tầng resolve/launch agent còn nguyên: `ExternalAgentServer::get_command`, `AgentId`, `AgentServerCommand`, extension agent, versioning, remote/headless |
| `AgentServerStoreState::Local` giữ `node_runtime`, `fs`, `project_environment`, `http_client` (`agent_server_store.rs:150-156`) | Có đúng thứ cần: shell env thật để tìm binary, node để npx, http để gọi registry |
| `agent_registry_store.rs:16` → `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, có `RegistryNpxAgent { package, args, env }` | Registry là của ACP (trung lập), **không** phải server Zed → fork dùng được |
| `settings_content.rs:117` — key `agent_servers`, enum `Custom` / `Extension` / `Registry` đã có schema | Không phải thiết kế settings mới |
| `crates/icons/src/icons.rs:12-13` — `AiClaude`, `AiOpenAi`; assets `ai_claude.svg`, `ai_open_ai.svg` | Yêu cầu "đúng icon từng agent" đã xong sẵn |
| `Cargo.toml:399` — `agent-client-protocol = "=0.11.1"`, **không crate nào dùng** | Dependency đã pin, chờ người dùng |
| Commit `c3e2ac3` "remove auth, collab, AI and cloud subsystems (54 crates)" | Phần protocol + UI restore được từ `c3e2ac3^` trong history của chính repo — không cần fetch upstream, không lệch version gpui tại điểm cắt |
| `terminal.rs:1517` `Terminal::input(bytes)`, `:1665` `paste()` | Điền lệnh vào pty **không** kèm `\r` = "sẵn lệnh, chưa chạy" → khả thi |
| Máy dev: `claude` ở `/Users/tgiap.dev/.local/bin/claude` (native installer), `codex` chưa cài | App GUI macOS có PATH tối giản → `which("claude")` trên process env sẽ **báo sai là chưa cài**. Bắt buộc resolve qua `project_environment` |
| `prompt_store`, `client`, `telemetry`, `markdown`, `multi_buffer`, `buffer_diff`, `terminal_view`, `notifications`, `db`, `picker` — còn | Deps của ACP slice phần lớn đã có |
| Thiếu: `action_log` 3.4k, `agent_settings` 1.7k, `language_model` 1.9k, `language_models` 14k, `streaming_diff`, `ai_onboarding`, `rules_library` | Chỉ hai cái đầu là bắt buộc |

## Số thật của hai cách "port agent_ui"

```
Full agent_ui  : agent_ui 62.5k + agent 105.9k + language_models 14.1k
                 + language_model + agent_settings + ai_onboarding + rules_library
                 + streaming_diff + client/cloud auth        ≈ 190k LOC
ACP slice      : acp_thread 8.0k + agent_servers 4.7k + action_log 3.4k
                 + agent_settings 1.7k (trim)
                 + agent_ui: thread_view 9.3k · conversation_view 7.3k
                   · message_editor 4.4k · completion_provider 2.7k
                   · agent_diff 2.3k · mention_set 1.3k · entry_view_state 0.6k
                                                              ≈ 45k LOC
```

Cắt được vì agent ngoài tự giữ model + auth: `agent`/`language_model`/`language_models` chỉ phục vụ agent **native** của Zed. Đã đếm coupling thực tế trong 4 file lớn nhất của slice: `agent::` 19 chỗ, `language_model` 11 chỗ (`conversation_view.rs` 11+7 · `thread_view.rs` 5+1 · `message_editor.rs` 3+1 · `agent_diff.rs` 0+2) — ~30 call site phải gỡ, không phải kéo lại 106k dòng.

## Các đường đã cân

**Đường 1 — ACP slice ~45k, restore từ `c3e2ac3^`** ✅ *chọn*
- Ưu: được trải nghiệm thật (stream message, tool call, approve permission, diff review, @-mention) mà không tái nhập cloud auth; code đúng version gpui tại điểm cắt nên drift chỉ là 164 commit của fork, không phải khoảng cách với upstream main.
- Nhược: 45k dòng code không phải mình viết, phải hiểu đủ để sửa; `agent_settings` phải trim nếu không settings schema mọc key chết.

**Đường 2 — nguyên agent_ui ~190k** ❌ *loại*
- Thực chất là revert phần lớn `c3e2ac3`. Kéo lại `client`/cloud auth + `language_models` cho một tính năng không cần model provider nào cả. Chỉ đáng nếu zode muốn có agent native riêng.

**Đường 3 — tự viết ACP client mỏng ~3k** ❌ *loại*
- Rẻ nhất trên giấy, nhưng permission prompt, fs/read_write, terminal request, diff review đều phải làm lại từ đầu — và làm lại tệ hơn bản đã chạy trong production của upstream.

## Quyết định đã chốt

| # | Điểm | Chốt |
|---|---|---|
| 1 | Phạm vi port | ACP slice ~45k LOC, restore từ `c3e2ac3^` |
| 2 | Nguồn CLI | Ưu tiên binary local resolve qua `project_environment`; fallback npx/registry là **opt-in một click**, không im lặng tải |
| 3 | Hai mode | Mỗi màn hình một mode, chọn lúc mở (không toggle trong cùng màn hình) |
| 4 | Entry rail | Hardcode đúng 2 nút: Claude Code (`AiClaude`), Codex (`AiOpenAi`) |
| 5 | Click rail | Click = mở Chat UI · chuột phải = menu (`Open as Terminal`, `New Session`) |
| 6 | Vị trí center | Bên của rail: rail trái → pane agent bên trái editor; rail phải → bên phải. Đọc cùng `WorkspaceSettings.multi_project.sidebar_side` mà `rail_side()` đọc, để hai bên không thể lệch nhau |
| 7 | Agent thứ hai | Split thêm pane thứ hai (mỗi agent một pane) |
| 7b | Hướng split | Mặc định **ngang** — hai cột cạnh nhau; người dùng kéo thả đổi thành dọc được, và layout họ đặt phải được serialize giữ lại |
| 8 | Chưa cài CLI | Toast (`workspace::show_notification`) **và** empty state trong màn hình agent |
| 9 | Auto-install | Nút "Cài ngay" mở center terminal **sẵn lệnh, chưa Enter** (`Terminal::input` không kèm `\r`) |
| 10 | Persistence | Serialize tab agent, mở lại là session mới — không hứa resume hội thoại |

## Kiến trúc chốt

| Nơi | Việc |
|---|---|
| `crates/action_log`, `crates/agent_settings` | Restore từ `c3e2ac3^`; `agent_settings` trim còn phần ACP cần |
| `crates/acp_thread`, `crates/agent_servers` | Restore; gỡ `language_model` (2 ref mỗi crate), gỡ nhánh native agent |
| `crates/agent_ui` (mới, chỉ slice) | `conversation_view` + `thread_view` + `message_editor` + `mention_set` + `completion_provider` + `agent_diff` + `entry_view_state`. **`agent_panel.rs` (dock Panel) không port** — thay bằng `agent_view.rs` implement `workspace::Item` |
| `crates/project/src/agent_server_store.rs` | +~150 dòng: hai built-in agent, resolve local-first, lỗi có kiểu `AgentBinaryMissing { agent, install_command, docs_url }` thay vì `anyhow` string — để UI render được empty state |
| `crates/zed_actions` | Nhà cho action mở agent, để `sidebar` không phải depend `agent_ui` (tránh cycle: cả hai đã depend `workspace`) |
| `crates/sidebar/src/rail_agents.rs` (mới, nhỏ) | 2 nút + context menu; dispatch action như `render_rail_footer` đang làm (`rail.rs:266` — dispatch chứ không gọi trực tiếp, tránh double-borrow) |

## Cần canh

1. **164 commit drift.** Code restore sẽ không compile ngay: `Action` derive, theme colors (2026 families), settings rem base, editor API đều đã đổi. Chống: restore + compile **bottom-up từng crate một commit** (`action_log` → `agent_settings` → `acp_thread` → `agent_servers` → `agent_ui`), `./script/clippy` xanh mới sang crate sau.
2. **Luật 200 dòng/file vs code port.** `thread_view.rs` 9.311 dòng. Chẻ nó thành 47 file là phá khả năng đối chiếu với upstream về sau, đổi lấy con số đẹp. **Ngoại lệ có chủ ý** — ghi vào plan để lần sau không ai "sửa" nó.
3. **`agent_settings` không trim = nói dối trong settings UI.** Key của agent native (profile, model, provider) sẽ xuất hiện trong schema mà không điều khiển gì.
4. **Ba pane + sidebar trên laptop là hết chiều ngang.** Rail 48px + panel + Claude + Codex + editor. Cần min-width cho pane agent, và xem mục treo bên dưới.
5. **npx fallback là supply chain.** Registry fetch qua mạng, npx kéo package lúc runtime. Đã chốt opt-in một click; đừng để nó thành đường mặc định im lặng.
6. **ACP `terminal/*` request.** `acp_thread` cho agent chạy lệnh qua terminal của editor. Không wire là agent nào dùng nó sẽ fail giữa hội thoại — phải nằm trong định nghĩa "done" của phase Chat UI.
7. ~~**Lệnh cài phải xác minh lại từ docs chính thức khi làm plan**~~ — ✅ **đã xác minh 2026-08-12**, bảng đầy đủ theo OS nằm ở [phase 03](../260812-0008-rail-agents-claude-code-codex/phase-03-missing-cli-ux.md). Còn một dòng npm/brew của Codex phải đối chiếu lại lúc code.

## Đo bằng gì

- Rail hiện 2 icon đúng thương hiệu; click mở pane đúng bên theo `sidebar_side`, đổi setting là đổi bên.
- Trên máy có `claude` ở `~/.local/bin` (không trong PATH của app GUI): mở được, **không** báo chưa cài.
- Trên máy không có `codex`: toast + empty state hiện lệnh đúng OS; "Cài ngay" mở terminal có lệnh, con trỏ cuối dòng, **chưa chạy**.
- Terminal mode: `claude` chạy TUI đầy đủ ở project root.
- Chat UI: gửi prompt → stream text, tool call render, permission prompt approve/deny được, diff review mở được.
- Đóng/mở app: tab agent trở lại đúng chỗ, session trống.
- `./script/clippy` xanh sau **mỗi** crate restore.

## Thứ tự dựng

| Phase | Nội dung | Ship được gì |
|---|---|---|
| P0 | Restore + compile `action_log`, `agent_settings`(trim), `acp_thread`, `agent_servers` | Chưa gì — nhưng nền có thật |
| P1 | Built-in 2 agent trong `AgentServerStore`, resolve local-first, lỗi có kiểu + test với fake env | Resolve đúng/sai kiểm được bằng test |
| P2 | Rail 2 nút + action + **Terminal mode** ở center đúng bên | Dùng được thật, sớm |
| P3 | Toast + empty state + "Cài ngay" | Yêu cầu #3 xong trọn |
| P4 | Chat UI: port slice, `agent_view.rs` implement `Item` | Yêu cầu #2 xong trọn |
| P5 | Permission prompt + `agent_diff` review + ACP `terminal/*` | Chat UI hết nửa vời |
| P6 | Serialization, luật split pane thứ hai, polish | Đóng commission |

P2 trước P4 có chủ ý: terminal mode dùng chung đúng đường resolve binary với chat UI, nên nó là bài kiểm tra rẻ cho P1 trước khi bỏ 45k dòng vào.

## Còn treo

Không còn điểm thiết kế nào treo. Hai thứ phải xác minh trong lúc dựng plan — **cả hai đã đóng 2026-08-12**:

1. ✅ Lệnh cài chính thức theo từng OS — lấy từ docs nhà phát hành, bảng nằm ở phase 03.
2. ✅ Độ drift thật — đã đo bằng số commit chạm từng crate kể từ `c3e2ac3`: `gpui`, `editor`, `language`, `multi_buffer`, `buffer_diff`, `prompt_store` đều **0 commit**. Thấp hơn giả định. Bảng đầy đủ ở `plan.md`. P0 vẫn là phase trả lời lại câu này bằng compiler; vượt 6d thì dừng và ước lượng lại P4–P6.

**Một quyết định phải mở lại trong lúc lập plan** (đã hỏi và đã chốt): binary local **không nói ACP** — `claude` v2.1.227 không có subcommand ACP nào, và registry ACP phân phối cả hai agent dạng npx adapter (`@agentclientprotocol/claude-agent-acp`, `@agentclientprotocol/codex-acp` — đã đổi org, không còn `@zed-industries/*`). Nên quyết định #2 chỉ đúng trọn cho terminal mode. Chat UI dùng **gate mềm**: vẫn kiểm CLI local, thiếu thì empty state cho hai lối — `[Cài CLI]` và `[Chạy bằng npx]`.

**Kế thừa:** [plans/260812-0008-rail-agents-claude-code-codex](../260812-0008-rail-agents-claude-code-codex/plan.md) — 7 phase.
