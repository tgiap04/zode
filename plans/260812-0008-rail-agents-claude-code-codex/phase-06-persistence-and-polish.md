# Phase 06 — Serialize, luật split, polish

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** pending · **Effort:** 3-4d · **Blocked by:** 04

Đóng commission: tab agent sống qua lần khởi động lại, luật split được viết ra rõ ràng, và những chỗ còn thô được vuốt lại. Chạy song song được với phase 05.

## Key insights

- Quyết định #10: **serialize tab, session mới**. Không hứa resume hội thoại — `session/load` phụ thuộc agent có hỗ trợ hay không, và một lời hứa hỏng còn tệ hơn không hứa. Ai muốn tiếp tục thật thì dùng `/resume` trong terminal mode.
- Template `SerializableItem` đã có sẵn trong cây: `crates/image_viewer/src/image_viewer.rs`, `crates/git_ui/src/project_diff.rs`, `crates/keymap_editor/src/keymap_editor.rs`, `crates/component_preview/src/component_preview.rs`.
- Quyết định 7b: split **ngang là mặc định**, không phải bắt buộc. Pane của Zed đã cho kéo thả tab sẵn — việc ở đây là **đừng ép lại** layout người dùng đã đặt, và serialize nó.
- `workspace/src/persistence.rs` chỉ đổi +14 dòng kể từ điểm cắt → cơ chế serialize pane group gần như y nguyên.
- R4 (hết chiều ngang) là rủi ro UX rõ nhất còn lại. Min-width đã đặt từ phase 02; phase này là chỗ **kiểm bằng mắt trên màn laptop thật**, không phải bằng test.

## Requirements

**Functional**
1. Đóng app, mở lại → tab agent trở lại đúng pane, đúng bên, đúng agent; hội thoại trống.
2. Người dùng kéo pane agent sang chỗ khác (dọc, hoặc sang bên kia editor) → layout đó được giữ qua lần khởi động lại.
3. Mở lại mà CLI đã bị gỡ → tab hiện empty state của phase 03, không phải lỗi trần.
4. Agent bị xoá khỏi settings → tab cũ của nó không làm vỡ khôi phục workspace.
5. Min-width pane agent giữ editor còn đọc được ở 3 pane + sidebar trên màn 13".

**Non-functional:** không ghi gì nhạy cảm (nội dung hội thoại, token) vào DB serialize.

## Architecture

```
SerializedAgentView { agent_id, mode }        ← chỉ hai trường, cố ý
   └─ khôi phục → AgentView ở trạng thái mới
        ├─ CLI còn        → sẵn sàng nhận prompt
        └─ CLI đã gỡ      → MissingBinary (phase 03)
```

Vị trí pane **không** do view giữ — nó nằm trong pane group của workspace, đã được serialize sẵn. View chỉ nói nó là agent nào, mode nào.

## Related code files

**Sửa:**
- `crates/agent_ui/src/agent_view.rs` — `impl SerializableItem`, đăng ký khi `init`
- `crates/agent_ui/src/agent_ui.rs` — đăng ký serializable item
- `crates/sidebar/src/rail_agents.rs` — vuốt lại tooltip/menu nếu cần sau khi dùng thật

**Tạo:** không · **Xoá:** không

## Implementation steps

1. `SerializableItem` cho `AgentView` theo template `image_viewer`/`project_diff`; blob chỉ mang `agent_id` + `mode`.
2. `#[serde(default)]` cho mọi trường mới, để blob cũ đọc được — cùng bài học R3 của plan git panel.
3. Khôi phục với `agent_id` không còn trong settings → bỏ qua item đó êm, không làm hỏng cả workspace.
4. Khôi phục khi CLI đã bị gỡ → rơi vào `MissingBinary` của phase 03.
5. Kiểm kéo thả: đổi pane agent sang dọc, restart, khẳng định vẫn dọc. Đây là yêu cầu 2 của 7b.
6. **Mở app trên màn 13" thật**: rail + panel + 2 pane agent + editor. Đọc code còn được không? Nếu không, chỉnh min-width hoặc đổi mặc định của agent thứ hai sang tab-cùng-pane và **ghi lại quyết định đó**.
7. Rà lại tooltip, nhãn menu, chữ trong empty state sau khi đã dùng thật vài ngày.

## Todo

- [ ] `impl SerializableItem`, blob chỉ `agent_id` + `mode`
- [ ] `#[serde(default)]` mọi trường mới
- [ ] Agent không còn trong settings → khôi phục êm
- [ ] CLI đã gỡ → rơi vào `MissingBinary`
- [ ] Kéo pane sang dọc → restart vẫn dọc
- [ ] **Mở trên màn 13" thật, đọc code được** (R4) — mắt người, không phải test
- [ ] Rà chữ: tooltip, menu, empty state

## Success criteria

Mở 2 agent, kéo pane thứ hai xuống dưới, đóng app, mở lại → đúng layout đó, hai tab agent đúng chỗ, hội thoại trống. Gỡ `codex` khỏi máy rồi mở lại → tab Codex hiện hướng dẫn cài chứ không phải lỗi. Trên màn 13" vẫn đọc được code.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R4 | 3 pane + sidebar hết chiều ngang | Bước 6 là bài kiểm bắt buộc bằng mắt. Được phép đổi mặc định nếu thực tế nói vậy — nhưng phải **ghi lại**, không đổi lặng |
| — | Blob cũ làm vỡ khôi phục sau này | `#[serde(default)]` từ đầu, kể cả khi hôm nay chỉ có 2 trường |
| — | Cám dỗ resume hội thoại | Ngoài scope theo #10. Muốn làm thì mở plan mới, kèm store cho thread |
