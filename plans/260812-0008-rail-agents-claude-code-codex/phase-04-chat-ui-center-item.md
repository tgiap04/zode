# Phase 04 — Chat UI thành center item

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 8-12d · **Blocked by:** 00, 02

> **Xong 2026-08-12.** Toàn bộ lát ACP đã nằm trong module tree và compile sạch. *(Ghi chú lịch sử: trong lúc làm, 26.413 dòng từng được khôi phục lên đĩa mà cố ý chưa nối vào module tree — xem khối comment trong `crates/agent_ui/src/agent_ui.rs`. Cây xanh hoàn toàn (`cargo check --workspace --all-targets` exit 0, 123 test xanh); đưa một module vào `agent_ui.rs` là **bước cuối** của việc port nó, không phải bước đầu. `crates/agent_settings` đã trim xong và về lại workspace.)*

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


---

## Phiên 2 (2026-08-12) — trả nợ `agent_settings` và port helper

### `agent_settings` đã trim xong — nợ #1 đã trả

Trim **theo bằng chứng**, không đoán: grep từng field của `AgentSettings` trên toàn bộ 26k dòng lát ACP, loại false positive (`dock` hoá ra là `TerminalSettings.dock`; `default_model`/`profiles` chỉ xuất hiện ở nhánh chọn model đang cắt), còn lại **14 field có consumer thật**.

| Tầng | Trước | Sau |
|---|---|---|
| `AgentSettingsContent` (sinh JSON schema người dùng thấy) | 32 field | **14** |
| `AgentSettings` | 32 field | **14** |
| `assets/settings/default.json` | không có key `agent` | 14 key kèm chú thích |

Cắt kèm: `PanelLayout`/`WindowLayout` (mô tả agent nằm ở cạnh nào của dock — vô nghĩa khi agent ở center), `agent_profile.rs`, cỗ máy compile `tool_permissions`, `language_model_to_selection`, `temperature_for_model`.

**Đổi một hành vi có chủ ý:** upstream đọc mọi field bằng `.unwrap()`, khiến `default.json` thành thứ chịu lực — thiếu một key là panic lúc khởi động. Bản trim mang default của chính nó, nên section trong `default.json` giờ là **tài liệu, không phải điều kiện**. Có test khoá lại.

**Test bắt được lỗi thật:** `default.json` tôi viết dùng `"thinking_display": "automatic"` trong khi variant hợp lệ là `auto`/`preview`/`always_expanded`/`always_collapsed`. 7 crate đỏ cùng lúc vì settings parse hỏng ở init. Đây đúng là loại lỗi mà đọc code không bắt được.

### Helper đã port (đang park cùng consumer của chúng)

| Module | Dòng | Vì sao |
|---|---|---|
| `outline.rs` | 218 | Từ crate `agent`. `mention_set` cần nó để gửi outline thay vì toàn văn khi file quá lớn — cắt đi là mention một file lớn sẽ thổi bay context |
| `mention_image.rs` | ~150 | Thay `language_model::LanguageModelImage`: decode → resize theo giới hạn provider → PNG → base64. Giới hạn kích thước giữ nguyên của upstream vì đó là thứ họ đã học được |
| `diagnostics.rs` | 252 | `mention_set` dùng để gom diagnostic vào mention |
| `actions.rs`, `ui.rs` | 335 + 20 | Đã xanh ở phiên trước |

`Agent` enum đã bỏ variant `NativeAgent`; `agent_connection_store`, `ui/mention_crease` đã cắt xong nhánh dock panel và `open_thread` (mention thread giờ trơ — fork không lưu thread).

### Vì sao helper vẫn park

Clippy chạy `-D warnings`, nên module `mod` private không có consumer là **dead code = lỗi**. Các helper này chỉ tồn tại để phục vụ `mention_set`/`message_editor` đang park, nên chúng chết theo. Bật lại cùng lúc với consumer, không sớm hơn.

### Còn lại — 22 lỗi trong 4 module

`message_editor` · `mention_set` · `completion_provider` · `entry_view_state`, chủ yếu:

| Nhóm | Cách xử |
|---|---|
| `thread_store` xuyên 5 file (~80 ref) | Gỡ tham số — kho thread của agent native |
| `crate::thread_metadata_store`, `crate::AgentPanel` trong `completion_provider` | Cắt |
| `Arc<str>` vs `SharedString` (2 chỗ trong `mention_set`) | `.into()` |
| `agent_ui_font_size`/`agent_buffer_font_size` trên `ThemeSettings` | Fork đã bỏ → dùng `ui_font_size`/`buffer_font_size` |
| `agent::NativeAgentServer` trong test của `mention_set` | Cắt |

Sau đó mới tới `conversation_view` (7.307) + `thread_view` (9.311) — hai file lớn nhất, **chưa động tới**.


---

## Phiên 3 (2026-08-12) — 6/7 module đã cắt sạch nhánh native

**Đã đưa về 0 lỗi:** `completion_provider`, `entry_view_state`, `mention_set`, `message_editor` (cộng `actions`, `ui`, `agent_connection_store`, và 4 helper). Chỉ còn **`conversation_view` + `thread_view`**.

### Đã cắt

| Chỗ | Thay bằng |
|---|---|
| `confirm_mention_for_thread` (`mention_set`) gọi `agent::NativeAgentServer` + `NativeAgentConnection` để lấy summary thread | Lỗi trung thực. Error message gốc của upstream đã tự nói *"only supported for the native agent"* |
| `thread_store: Option<Entity<ThreadStore>>` xuyên `mention_set` → `message_editor` → `entry_view_state` | Gỡ hẳn tham số. `has_thread_store()` trả `false` — giữ lại để completion delegate đọc y như upstream |
| `MessageEditor::insert_thread_summary` | No-op có tài liệu. Upstream cũng gate cả thân hàm trên việc có thread store |
| `collect_session_matches` (`completion_provider`) đọc `ThreadMetadataStore` global, lọc theo `agent::ZED_AGENT_ID` | `Vec::new()` — không có thread nào để liệt kê |
| Thread đang mở được đề xuất làm mention candidate, đọc qua `workspace.panel::<AgentPanel>()` | Cắt — không có panel |
| `humanize_token_count` | **Đã lấy lại.** Nó bị cắt mất cùng `ManageProfiles` ở phiên trước — lỗi của tôi, compiler bắt được |
| `agent_configuration::configure_context_server_modal::default_markdown_style` | Module mới `markdown_style.rs` (28 dòng). Chỉ hàm style đi theo, modal thì không |

### Vì sao cả lát vẫn park

`conversation_view` là **consumer duy nhất** của 6 module kia. Clippy chạy `-D warnings`, nên `mod` private không ai dùng là dead code = lỗi. Cả lát phải vào cùng lúc hoặc park cùng lúc — không có trạng thái giữa. Đây là lý do các module đã-xanh vẫn nằm ngoài module tree.

### Việc còn lại — đây là phần thiết kế, không phải cắt máy móc

`conversation_view` + `thread_view` còn **43 lỗi**, nhưng con số không phản ánh đúng độ khó. Phân bố:

| Symbol | Số chỗ | Bản chất |
|---|---|---|
| `AgentPanel` | **17** | `conversation_view` gọi ngược vào dock panel: mở diff, follow agent, đổi thread… Mỗi chỗ cần một quyết định `AgentView` thay bằng gì |
| `ThreadStore` | **16** | Kho thread native |
| `ThreadId` / `ThreadMetadataStore` | 12 | Lịch sử thread |
| `LanguageModelRegistry` | 6 | Registry model native |
| `AgentProfileId`, `ProfileSelector`, `ModeSelector`, `ModelSelectorPopover`, `ConfigOptionsView` | 12 | Chọn model/profile/mode — thuộc agent native |
| `From<LanguageModelCompletionError> for ThreadError` | 1 impl, 13 variant | Map lỗi của LLM provider sang lỗi thread |
| `AgentFontSize`, `upgrade_to_zed_pro_url`, `shared_agent_thread_url`, `show_undo_reject_toast`, `AgentDiffPane` | 6 | Fork đã bỏ |

**17 chỗ `AgentPanel` là trọng tâm thật của phase này** — đúng như plan đã ghi từ đầu. Chúng không cắt được: `conversation_view` được viết để sống trong `AgentPanel` và gọi ngược lên nó. Mỗi chỗ phải trả lời "trong thiết kế center-pane, ai làm việc này?" — và một số câu trả lời sẽ là "`AgentView`", số khác là "không ai, bỏ tính năng đó".


---

## Phiên 4 (2026-08-12) — PHASE 04 XONG

`cargo check --workspace --all-targets` 0 lỗi · `./script/clippy` 0 lỗi · **121 test xanh** (27 `agent_ui`, 16 `sidebar`, 48 `acp_thread`, 14 `agent_servers`, 14 `project`, 2 `agent_settings`) · `cargo build -p zode` xanh.

### Plan của tôi sai một chỗ, và nó là chỗ đắt nhất

Tôi từng xếp `model_selector`, `config_options`, `mode_selector` vào diện **"không port"** chỉ dựa trên tên file. Kiểm lại: cả ba có **0 tham chiếu native**. Chúng là tính năng **ACP thật** mà agent ngoài phơi ra — `session/set_mode`, `session/set_config_option`, `AgentModelSelector`. Claude Code có mode; Codex có model. Suýt cắt mất.

Đã port thêm ~2.300 dòng vì phát hiện này: `config_options` (880), `model_selector` (840), `mode_selector` (211), `model_selector_popover` (101), cùng `ui/{hold_for_default, model_selector_components, undo_reject_toast}` và `external_source_prompt` (162), `markdown_style` (28), `outline` (218), `mention_image` (~150), `diagnostics` (252).

### 17 chỗ `AgentPanel` hoá ra chỉ là 3 chỗ production

Con số 17 làm tôi hoãn hai phiên. Thực tế: phần lớn nằm trong test module, và cả 3 site production đều là **một vị ngữ duy nhất** — *"người dùng có đang nhìn agent không?"*, dùng để quyết có notify hay không.

Viết lại thành: `workspace.panes()` → pane nào có `AgentView` là active item → nó có giữ đúng conversation này không. Đúng nghĩa "trên màn hình", không phải "đang mở".

### Đã cắt (mỗi cái kèm lý do trong code)

| Nhánh | Vì sao |
|---|---|
| `From<LanguageModelCompletionError> for ThreadError` — 13 variant | Mô tả request **editor** tự gửi. Agent ngoài tự nói với provider của nó rồi báo lại qua ACP |
| `ProfileSelector` + `impl ProfileProvider for Entity<agent::Thread>` | Profile là tập tool **Zed tự enforce**; agent ngoài tự quản tool của mình. Gỡ field này mở khoá cascade 123 → 3 lỗi |
| thinking-mode / effort-level / fast-mode | Ghi vào `LanguageModelSelection`. ACP có `SessionConfigOption` cho việc này, và `config_options` đã render |
| `as_native_connection` / `as_native_thread` + 20 caller | Downcast sang agent in-process; luôn trả `None`, mọi caller đi nhánh còn lại |
| `share_thread`, `sync_thread`, feedback `submit`, organization gating | Đều nói với backend của Zed |
| `upgrade_button` (Zed Pro upsell) | Fork này không bán gì |
| `token_limit_callout` | ACP báo tổng token nhưng không báo ngưỡng để so |

### Ba lần tôi tự làm hỏng và compiler bắt được

1. `humanize_token_count` bị truncate mất cùng `ManageProfiles` (phiên trước).
2. Vòng brace-matching đếm lệch một cấp → ăn luôn `}` đóng `impl ThreadView`, **xoá 2.479 dòng**. Phải restore từ HEAD rồi replay lại 23 cut bằng script.
3. Cut test module "tới EOF" trong khi `mod tests` nằm **giữa file** → xoá 2.542 + 684 + 136 dòng production của `message_editor`, `mention_set`, `entry_view_state`. Restore và cắt lại bằng brace-matching.

Bài học chung: **cắt bằng chỉ số/EOF là sai; phải brace-match, và phải kiểm số dòng trước/sau mỗi lần cắt lớn.**

### Còn nợ

- **Test của upstream cho `conversation_view`/`thread_view`/`message_editor`/`mention_set`/`entry_view_state` đã bị xoá**, không phải port. Chúng dựng `AgentPanel` và native thread. Coverage cho view phải viết lại từ đầu — chưa làm.
- `insert_dragged_files`, `insert_selections`, `reauthenticate` giữ lại kèm `#![allow(dead_code)]` có ghi chú: caller duy nhất của chúng là dock panel. Là affordance thật, cần re-home lên `AgentView`.
- `agent_diff` chưa port (phase 05); `AgentDiff::set_active_thread` và đường nhảy tới diff pane đang là no-op có ghi chú.
