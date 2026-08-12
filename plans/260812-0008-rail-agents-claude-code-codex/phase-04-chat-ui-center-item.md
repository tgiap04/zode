# Phase 04 — Chat UI thành center item

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** in_progress *(~20% phần cơ học)* · **Effort:** 8-12d · **Blocked by:** 00, 02

> **Trạng thái 2026-08-12.** 26.413 dòng đã khôi phục lên đĩa và **cố ý chưa nối vào module tree** — xem khối comment trong `crates/agent_ui/src/agent_ui.rs`. Cây xanh hoàn toàn (`cargo check --workspace --all-targets` exit 0, 123 test xanh); đưa một module vào `agent_ui.rs` là **bước cuối** của việc port nó, không phải bước đầu. `crates/agent_settings` cũng đã khôi phục + trim một phần nhưng **park khỏi workspace** — lý do và cách gỡ ở mục "Nợ kỹ thuật" cuối file.

Phase lớn nhất. Mang lát ACP của `agent_ui` về, và **thay `agent_panel.rs` (dock Panel) bằng `agent_view.rs` implement `workspace::Item`** — đây là chỗ upstream và zode rẽ đôi: upstream để agent trong dock, quyết định #6 đặt nó ở center.

## Key insights

- **Không port `agent_panel.rs` (7.027 dòng).** Nó là `Panel` của dock: `position`, `size`, `set_active`, zoom, `PanelEvent`. Center item cần `Item` + `Focusable` + `EventEmitter<ItemEvent>`. Đọc nó để lấy phần *nội dung*, vứt phần *khung dock*.
- Danh sách port (≈25k dòng): `conversation_view.rs` 7.307 · `conversation_view/thread_view.rs` 9.311 · `message_editor.rs` 4.445 · `completion_provider.rs` 2.655 · `mention_set.rs` 1.324 · `entry_view_state.rs` 616 · `agent_connection_store.rs` 245 · `ui/mention_crease.rs` 312 · `ui/agent_notification.rs` 198.
- **Không port:** `thread_metadata_store.rs` (3.938), `threads_archive_view.rs` (1.640), `thread_worktree_archive.rs` (1.433), `thread_import.rs` (1.119), toàn bộ `agent_configuration/`, `language_model_selector.rs`, `model_selector.rs`, `profile_selector.rs`, `inline_assistant.rs` + `buffer_codegen.rs` + `inline_prompt_editor.rs` + `terminal_*`, `agent_registry_ui.rs`, `config_options.rs`, `mode_selector.rs`. Chúng phục vụ agent native hoặc tính năng ngoài scope.
- Coupling phải gỡ, đã đếm: `conversation_view.rs` → `agent::` 11, `language_model` 7 (gồm `LanguageModelRegistry` — nhánh native, xoá cùng); `thread_view.rs` → 5 + 1 (`LanguageModelEffortLevel`, `Speed`); `message_editor.rs` → 3 + 1. Tổng ~30 call site.
- `conversation_view.rs` cũng import `GEMINI_TERMINAL_AUTH_METHOD_ID` từ `agent_servers` — nhánh Gemini, xoá.
- Drift thấp (bảng ở plan.md): `gpui` `editor` `language` `multi_buffer` `ui` — 0 hoặc 3 commit. Bề mặt mà 25k dòng này chạm gần như đứng yên.
- Template cho `Item`: `crates/image_viewer/src/image_viewer.rs` hoặc `crates/git_ui/src/project_diff.rs` — cả hai đều gọn và đã `impl SerializableItem` (dùng lại ở phase 06). `Item::tab_content_text` (`item.rs:183`) là method bắt buộc duy nhất không có default.
- Chat UI chạy npx adapter (phát hiện #2 của plan). Đường tới lệnh đã có từ phase 01; phase này chỉ **tiêu thụ**.

## Requirements

**Functional**
1. `AgentView` implement `Item`: tab có icon agent + tên, đóng/kéo/split được như mọi tab khác.
2. Click nút rail (mặc định) mở **Chat UI**; `Open as Terminal` chuyển vào menu chuột phải (quyết định #5 — lật lại mặc định của phase 02).
3. Gửi prompt → stream nội dung trả về theo thời gian thực.
4. Tool call render được: tên, tham số, trạng thái, kết quả.
5. `@`-mention file/symbol hoạt động qua `completion_provider` + `mention_set`.
6. Session mới qua `New Session` trong menu chuột phải.
7. Agent chết / adapter lỗi → view báo lỗi đọc được, không panic, mở lại được.

**Non-functional:** không đưa `language_model`/`language_models`/`agent` vào `Cargo.toml` của `agent_ui` — nếu compiler đòi, nghĩa là còn nhánh native chưa cắt.

## Architecture

```
AgentView (impl Item)  ← center pane, bên theo sidebar_side
   ├─ AgentConnection (agent_servers) ── stdio ──> npx @agentclientprotocol/…
   ├─ AcpThread (acp_thread)          ── state hội thoại, tool call, plan
   └─ render: conversation_view → thread_view → entry_view_state
              message_editor + mention_set + completion_provider
```

Phần `agent_panel.rs` bị thay: mọi thứ liên quan `Panel`/`Dock`/`PanelEvent` → API `Item`.

## Related code files

**Tạo:**
- `crates/agent_ui/src/agent_view.rs` — `impl Item for AgentView`, thay vai `agent_panel.rs`
- `crates/agent_ui/src/conversation_view.rs`, `conversation_view/thread_view.rs`, `message_editor.rs`, `completion_provider.rs`, `mention_set.rs`, `entry_view_state.rs`, `agent_connection_store.rs`, `ui/mention_crease.rs`, `ui/agent_notification.rs` — khôi phục từ `c3e2ac3^`, đã trim

**Sửa:**
- `crates/agent_ui/src/agent_ui.rs` — `init`, đăng ký `Item`
- `crates/sidebar/src/rail_agents.rs` — click mặc định đổi sang `AgentViewMode::Chat`
- `crates/agent_ui/Cargo.toml`

**Xoá:** không

## Implementation steps

1. Khôi phục từng file theo thứ tự phụ thuộc: `entry_view_state` → `mention_set` → `completion_provider` → `message_editor` → `thread_view` → `conversation_view`. **Mỗi file một commit**, clippy xanh giữa các commit.
2. Ở `conversation_view.rs`: xoá nhánh agent native (`NativeAgentServer`, `LanguageModelRegistry`), xoá nhánh Gemini (`GEMINI_TERMINAL_AUTH_METHOD_ID`). Kỳ vọng là file co lại đáng kể — ghi lại số dòng trước/sau.
3. Ở `thread_view.rs`: `LanguageModelEffortLevel`/`Speed` chỉ dùng cho hiển thị → thay bằng kiểu cục bộ hoặc bỏ chỗ hiển thị đó.
4. `agent_view.rs`: dựng `Item` theo template `image_viewer`/`project_diff`; `tab_content_text` = tên agent; `tab_content` = icon agent + tên.
5. Nối `AgentConnection` với lệnh npx từ phase 01; giữ trạng thái `MissingBinary` của phase 03 nguyên vẹn (gate mềm).
6. Đổi mặc định click ở `rail_agents.rs` sang `Chat`; đưa `Open as Terminal` vào menu.
7. Bơm session mới: `New Session` tạo `AgentView` mới trong **cùng pane** (tab thứ hai), không split thêm.
8. Test: `cx.draw()` view ở ba trạng thái — đang kết nối, có hội thoại, lỗi. **Nhớ**: `cx.draw` không publish frame; muốn đo layout thật thì phải để window vẽ qua `run_until_parked`.
9. Ghi phase note: mỗi API phải sửa vì drift + số dòng cắt được của từng file.

## Todo

- [ ] 9 file khôi phục, mỗi file một commit, clippy xanh giữa từng commit
- [ ] **Nhận từ phase 00:** khôi phục + trim `agent_settings` ở đây, nơi `conversation_view`/`message_editor` là consumer thật dẫn đường cho việc trim (rủi ro R3 chuyển sang phase này)
- [ ] **Nhận từ phase 00:** icon duy nhất còn thiếu là `IconName::Thread`, đã khôi phục sẵn — không còn icon nào phải vá
- [ ] Nhánh agent native + Gemini đã cắt sạch (`Cargo.toml` không có `agent`/`language_model*`)
- [ ] `AgentView` impl `Item`, tab có icon + tên agent
- [ ] Click rail mặc định = Chat; Terminal vào menu chuột phải
- [ ] **Nhận từ phase 02:** dựng luôn menu chuột phải (`Open as Terminal`, `New Session`) ở đây — phase 02 hoãn lại vì lúc đó menu chỉ có một mục trùng click và một mục chưa có khái niệm session
- [ ] **Nhận từ phase 02:** `AgentView` đã là `Item` rồi — phase này chỉ thêm nhánh `State::Chat`, **không** thay item
- [ ] **Nhận từ phase 03:** thêm nút `[Chạy bằng npx]` vào hàng nút của `missing_binary::render` để hoàn tất gate mềm — empty state đã dựng sẵn chỗ, phase 03 chỉ làm gate cứng vì Chat chưa có
- [ ] Stream prompt → phản hồi hiện dần
- [ ] Tool call render được
- [ ] `@`-mention chạy
- [ ] `New Session` = tab mới cùng pane
- [ ] Agent lỗi → báo đọc được, mở lại được
- [ ] Test vẽ 3 trạng thái

## Success criteria

Mở app → bấm icon Claude → chat UI ở center cạnh editor → gửi "đọc file X và tóm tắt" → thấy chữ chảy về, thấy tool call đọc file, thấy kết quả. Đóng tab, mở lại, vẫn chạy. `./script/clippy` xanh, `cargo test -p agent_ui` xanh.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R2 | Drift làm việc port đội lên | Mỗi file một commit ⇒ dừng được ở bất kỳ đâu mà cây vẫn xanh. Vượt 12d → dừng, báo lại |
| R7 | `thread_view.rs` 9.311 dòng vượt xa luật 200 dòng | **Ngoại lệ có chủ ý**, ghi ở plan.md. Chẻ ra là phá khả năng đối chiếu upstream |
| — | Cắt nhánh native tay đôi làm hỏng đường ACP | Để compiler dẫn đường: gỡ dep trong `Cargo.toml` **trước**, rồi sửa theo lỗi. Đừng cắt bằng mắt |
| R4 | Chat UI + editor + pane thứ hai hết chiều ngang | Min-width đã đặt từ phase 02; kiểm lại bằng mắt ở phase này |

---

## Đã làm được (2026-08-12)

| Việc | Trạng thái |
|---|---|
| Khôi phục 9 file lát ACP (26.413 dòng) lên đĩa | ✅ |
| `Cargo.toml` của `agent_ui`: 79/86 dep upstream có sẵn trong fork | ✅ |
| `crates/agent_ui/src/ui.rs` — khai lại chỉ 2 module đã port; `documentation_aside_side` đọc `sidebar_side` thay vì vị trí dock | ✅ |
| `crates/agent_ui/src/actions.rs` — trích action + `DEFAULT_THREAD_TITLE` + `humanize_token_count` từ `agent_ui.rs` gốc | ✅ |
| Viết lại enum `Agent`: bỏ variant `NativeAgent` | ✅ |
| `agent_settings` khôi phục, gỡ `language_model` (`language_model_to_selection`, `temperature_for_model`), gỡ `collaboration_panel` khỏi `PanelLayout` | ⚠️ park |
| Nối module vào `agent_ui.rs` | ❌ chưa |

**7 dep upstream không tồn tại trong fork** — đúng bằng stack agent native đã định cắt: `agent`, `ai_onboarding`, `eval_utils`, `language_model`, `language_models`, `rules_library`, `streaming_diff`.

## Bảng lỗi còn lại — dữ liệu để làm tiếp

Lần compile đầu: **75 lỗi, 100% là lỗi phân giải tên**, không một lỗi kiểu hay borrow nào. Drift vẫn gần bằng không; đây thuần là "module tôi chọn không port". Sau khi viết lại `Agent` và thêm `actions.rs`: **còn 61**.

| Nhóm | Số | Cách xử |
|---|---|---|
| `agent::` (crate native) | 16 | Cắt — chủ yếu nhánh `NativeAgent` |
| `language_model::` | 6 | Cắt |
| `crate::thread_metadata_store` | 2 | Cắt (không có lịch sử thread — quyết định #10) |
| `crate::AgentPanel`, `crate::AgentDiffPane` | 4 | **Rewire sang `AgentView`** — đây là việc thiết kế thật của phase, không phải cắt máy móc |
| `crate::profile_selector`, `ModeSelector`, `ModelSelectorPopover`, `super::config_options` | 4 | Cắt (chọn model/profile thuộc agent native) |
| `crate::agent_diff`, `crate::diagnostics` | 2 | `agent_diff` port ở phase 05; `diagnostics` (252 dòng) port hoặc cắt |
| `theme_settings::AgentFontSize` | 1 | Fork đã bỏ — dùng buffer font size |
| `zed_urls::upgrade_to_zed_pro_url` | 1 | Cắt (upsell Zed Pro) |
| `ExternalSourcePrompt` | 3 | Port file `external_source_prompt.rs` hoặc cắt nhánh |
| Dev-dep test: `pretty_assertions`, `base64`, `semver` | vài | Thêm vào `[dev-dependencies]` |

## Nợ kỹ thuật do phase này tạo ra — phải dọn khi làm tiếp

1. **`crates/agent_settings` đang nằm ngoài workspace.** Nó compile được sau khi gỡ `language_model`, nhưng **5 test đỏ** vì `assets/settings/default.json` của fork không có key `"agent"`. Vá bằng cách nhét nguyên `AgentSettingsContent` của upstream vào `default.json` chính là thứ R3 cấm. Cách đúng: trim `agent_settings` xuống **~15 key UI mà lát ACP thật sự đọc** (`max_content_width`, `thinking_display`, `expand_edit_card`, `expand_terminal_card`, `message_editor_min_lines`, `show_turn_stats`, `enable_feedback`, `cancel_generation_on_terminal_stop`, `new_thread_location`, `notify_when_agent_waiting`, `play_sound_when_agent_done`, `agent_buffer_font_size`, …), rồi mới thêm `pub agent: Option<AgentSettingsContent>` vào `SettingsContent` **và** một section tương ứng trong `default.json`. Ba thứ đó phải đi cùng nhau trong một bước.
2. **`PanelLayout` trong `agent_settings` vẫn theo dõi dock của mọi panel.** Vô nghĩa với thiết kế agent-ở-center. Là ứng viên xoá khi trim.
3. `crates/agent_ui/Cargo.toml` hiện khai 68 dep, trong đó nhiều cái chỉ phục vụ module còn parked. Rà lại sau khi nối xong module.
