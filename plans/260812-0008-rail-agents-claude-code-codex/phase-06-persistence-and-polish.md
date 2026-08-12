# Phase 06 — Serialize, luật split, polish

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed *(trừ mục kiểm bằng mắt)* · **Effort:** 3-4d · **Blocked by:** 04

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


---

## Xong (2026-08-12)

`impl SerializableItem for AgentView` + module `persistence` với bảng `agent_views(workspace_id, item_id, agent, mode)`, theo đúng khuôn `git_ui::project_diff`. Đăng ký qua `workspace::register_serializable_item::<AgentView>` trong `agent_ui::init`.

| Yêu cầu | Trạng thái |
|---|---|
| Blob chỉ `agent_id` + `mode` | ✅ hai cột, không hơn |
| Agent không còn trong settings → khôi phục êm | ✅ `deserialize` trả `Err` ⇒ workspace bỏ qua item đó, không vỡ cả layout |
| CLI đã gỡ → rơi vào `MissingBinary` | ✅ `new()` gọi `start()` gọi `resolve_agent_binary` như mọi lần mở khác |
| Kéo pane sang dọc → restart vẫn dọc | ✅ vị trí pane do pane-group của workspace serialize, không phải view |
| **Mở trên màn 13" thật** | ❌ **cần mắt người** |

Refactor kèm theo: constructor `AgentView::new` được tách khỏi `open` — `deserialize` cần nó, và giờ cả hai đường vào view đều đi qua cùng một chỗ.

### Còn nợ

- **Mục R4 (chiều ngang trên màn 13")** là thứ duy nhất của phase này chưa làm được, và nó vốn được ghi là "mắt người, không phải test".
- Không hứa resume hội thoại (quyết định #10). Ai muốn tiếp tục thật thì `/resume` trong terminal mode.

---

## Sau khi dùng thật (2026-08-12)

Hai thứ người dùng chỉ ra ngay lần đầu mở được app.

**Không có chỗ nào để chuyển giữa hai mode.** Quyết định #3 chọn mode *lúc mở* — click = chat, chuột phải = terminal — và điều đó để lại người đang xem một mode không có đường sang mode kia; trên màn hình cũng không có gì gọi tên hai mode. Thêm switch `Chat | Terminal` ở đầu view. Chuyển mode = **restart agent**: hai mode là hai process khác nhau (npx adapter vs CLI local), giữ cái đang nhàn rỗi sống nghĩa là hai agent chạy cho một view — cùng lý lẽ tiết kiệm process của #10.

Đọc lại chỗ đó lộ ra một lỗi thật: **`AgentView::open` bỏ qua `mode`** khi agent đó đã có view mở, nên chuột phải trên rail chỉ focus lại chat đang có. Đó chính là thứ làm người dùng kết luận không có cách nào chuyển. Giờ `open` gọi `set_mode`, và tooltip của rail nói ra cử chỉ chuột phải thay vì để nó ẩn.

**Pane agent dán sát code editor.** Thêm 8px ở đúng cạnh hướng về editor, đọc `agent_split_direction` — cùng setting đã quyết pane mở bên nào, nên gap không bao giờ nằm sai phía. Gap đặt **trong view**, không phải ở pane group của workspace: nới divider ở đó sẽ đẩy mọi split trong cửa sổ ra xa, không riêng seam này.

Đánh đổi phải nói rõ: hàng tab bar phía trên **vẫn liền nhau**, vì gap nằm trong item chứ không trong pane. Muốn inset cả pane thì phải sửa pane group, và đó là quyết định khác — chưa làm.

### Switch có thật sự kill cái kia không — audit chuỗi sở hữu

Yêu cầu: mỗi lần switch phải kill mode kia, không để rò process. Việc kill **hoàn toàn nằm ở `Drop`**, nên câu hỏi thật là: còn ai giữ strong handle không.

| Nơi có thể giữ | Giữ kiểu gì | Kết luận |
|---|---|---|
| `Terminal::drop` (`terminal.rs:2475`) | gửi `Msg::Shutdown` → `terminate_child_process` → `kill_child_process` sau 100ms | ✅ pty chết |
| `AcpConnection::drop` (`agent_servers/src/acp.rs:1037`) | `child.kill()` | ✅ npx adapter chết |
| `ConversationView::on_release` (`conversation_view.rs:563`) | `close_all_sessions` + `remove_window` mọi notification | ✅ session đóng tử tế, không rò cửa sổ OS |
| `AgentDiff` global | `WorkspaceThread { thread: WeakEntity<AcpThread> }` | ✅ weak |
| `AcpConnectionRegistry` | chỉ id + ring buffer có chặn | ✅ không giữ entity |
| `Project.terminals.local_handles` | `Vec<WeakEntity<Terminal>>` + `observe_release` dọn | ✅ weak |
| **`AgentDiffPane`** (`agent_diff.rs:44`) | **`thread: Entity<AcpThread>` — strong** | ⚠️ **ngoại lệ duy nhất** |

**Ngoại lệ:** mở tab diff review rồi switch mode → thread (và connection của nó) sống tới khi đóng tab diff đó. Đây là hành vi của upstream và có lý của nó (diff còn phải keep/reject được), nhưng nó nghĩa là một npx process sống mà không còn UI hội thoại nào. Chưa đổi — đổi vòng đời diff pane là quyết định khác, cần chốt riêng.

Ba việc đã làm thay vì chỉ tin vào phép gán:

1. `restart` **lấy state cũ ra bằng `mem::replace`** rồi drop tường minh trong `end_previous_mode`, kèm ghi rõ chuỗi Drop trong comment.
2. `warn_if_retained` — chỉ debug build (`cfg!(debug_assertions)`), sau 5s kiểm `WeakEntity::upgrade()`; còn sống thì `log::warn!`. Đây là tripwire cho người sửa ownership sau này, vì audit không sống qua lần đổi code kế tiếp.
3. Test `leaving_a_mode_releases_it` — dựng terminal **display-only** (không pty thật), switch, khẳng định weak handle chết. Đã kiểm phản chứng: thay `drop` bằng `mem::forget` thì test fail đúng chỗ.

---

## Nhớ cách người dùng để lại (2026-08-13)

Yêu cầu: chọn CLI hay UI thì lần sau mở phải đúng cái đó, và nhớ cả **width** của pane agent.

**Lưu theo agent, không theo workspace.** Bảng mới `agent_preferences(agent PRIMARY KEY, mode, width)` trong `AgentViewDb`, cố ý **không có `workspace_id`** — khác `agent_views` vốn khôi phục tab của một project. Mode và width là thói quen của người dùng, đi theo họ sang project khác. Khoá theo agent vì "Claude để chat, Codex để terminal" là cặp thói quen hoàn toàn bình thường.

**Click trên rail đổi nghĩa.** `OpenAgent.mode` giờ là `Option<AgentViewMode>`: `None` = "mở như lần trước" (click thường), `Some(Terminal)` = chuột phải. Chọn tường minh thì mới ghi nhớ — click thường không bao giờ ghi đè lên chính preference nó vừa đọc.

**Width đo từ đâu.** `Workspace::bounding_box_for_pane` trong `render` — đó là chỗ duy nhất có width đã layout, vì pane vừa split ra chưa có bounds cho tới khi được vẽ một lần. Ghi debounce 400ms (một cú kéo đi qua hàng chục width, chỉ chỗ nó dừng mới đáng một row), task giữ trong field nên drop view là huỷ luôn lần ghi không còn nghĩa.

**Width áp lại lúc nào.** Chỉ khi pane agent đang là active pane — vì `resize_pane` tác động lên active pane. Tab khôi phục lúc khởi động **không** đi đường này: workspace đã serialize flexes của pane group rồi, ép width lên đó là đánh nhau với layout người dùng thực sự để lại.

### Hai bẫy gặp phải

**1. `INSERT OR REPLACE` sẽ xoá cột của người kia.** Mode và width do hai cử chỉ khác nhau ghi, mỗi cử chỉ một cột. Upsert kiểu replace làm mỗi lần ghi xoá giá trị của lần kia. Thiết kế: `INSERT OR IGNORE` tạo row rồi `UPDATE` đúng một cột, hai câu trong cùng một `write`. Test `writing_one_preference_leaves_the_other_alone` đã kiểm phản chứng — đổi về `INSERT OR REPLACE` thì fail đúng chỗ, width thành `None`.

**2. Domain có FK không mở được test-db một mình.** `sqlez/src/migrations.rs:136` sinh sẵn `DELETE FROM {child_table} ... NOT IN (SELECT ... FROM {parent_table})` cho **mọi** bảng có foreign key, chạy như một phần của migrate. `agent_views` trỏ vào `workspaces`, nên `AgentViewDb::open_test_db(name)` một mình chết ngay ở migration với *no such table: workspaces*. Phải mở `WorkspaceDb::open_test_db(name)` **cùng tên** trước. Đây là lý do trong repo này chưa có DB tầng item nào được unit-test.

### Giới hạn còn lại

- Width áp theo trục ngang. Kéo pane agent xuống dưới editor rồi mở lại thì width nhớ được không có tác dụng — chưa xử lý, vì "kích thước" theo trục nào là câu hỏi khác.
- Hai click rất nhanh vào cùng một agent: đã chặn bằng lần kiểm thứ hai *sau* await, click thứ hai activate cái thứ nhất thay vì mở trùng.

---

## Deadlock toàn app khi mở agent (2026-08-13)

Triệu chứng: mở agent thì load mãi, app như treo hẳn.

**Nguyên nhân, chứng minh bằng stack thật** (`sample` trên tiến trình đang treo, 281/281 mẫu ở cùng một frame):

```
com.apple.main-thread
  AgentView::render
    AgentView::track_width
      Workspace::bounding_box_for_pane
        PaneGroup::bounding_box_for_pane
          PaneAxis::bounding_box_for_pane
            parking_lot RawMutex::lock_slow → park → pthread_cond_wait
```

`PaneAxis::bounding_box_for_pane` (`pane_group.rs:900`) khoá `bounding_boxes`, mà **chính main thread đang giữ** khoá đó trong lúc pane group render các con của nó. Item con hỏi bounds trong `render` của mình = tự khoá chính mình. Main thread park vĩnh viễn ⇒ cả cửa sổ ngừng vẽ, không riêng view agent.

Đây là lỗi tôi tự tạo ở phần đo width (`ea2bf24`).

**Sửa:** không hỏi workspace trong lúc render nữa. Width lấy từ bounds của **chính element này** qua `canvas` (giai đoạn paint), và mọi thứ chạm workspace — kể cả `resize_pane` và cả phép kiểm active-pane — đẩy hết vào `cx.defer_in`, chạy sau khi frame kết thúc.

**Kiểm chứng bằng runtime, không phải suy luận:** chạy lại đúng workspace đã treo (`tickerx`, có tab agent được serialize) →
- trước: 0.1% CPU, không tiến trình con, 281/281 mẫu trong `bounding_box_for_pane`;
- sau: 11% CPU, `npm exec @agentclientprotocol/claude-agent-acp` + tiến trình node của nó **sống như con của zode**, **0 frame** trong `bounding_box_for_pane`.

### Điều này dạy lại cho plan

**Test một pane không tái hiện được deadlock này.** Bản đầu của test dùng `add_item_to_active_pane` → workspace một pane → `PaneGroup.root` là `Member::Pane` → `bounding_box_for_pane` trả `None` **trước khi** chạm mutex, nên test pass ngay cả khi bug còn nguyên (0.21s). Phải `split_item` để có `Member::Axis` — đúng hình mà agent mở ra. Sau khi sửa, test treo với đúng stack `bounding_box_for_pane → RawMutex::lock_slow → pthread_cond_wait`.

Test `the_view_draws_inside_a_pane_without_deadlocking`: **regression này làm test treo chứ không fail** — đó là tín hiệu, và stack chỉ thẳng vào thủ phạm.

### Những gì đã loại trừ (đo, không đoán)

| Nghi ngờ | Bằng chứng loại trừ |
|---|---|
| npx tải gói lâu | npm debug log: `exit 0`, gói resolve từ cache 196ms |
| Adapter hỏng / cần auth | Probe trực tiếp: `initialize` 5ms, `session/new` **517ms**, `authMethods: []` |
| Message ACP quá lớn | `session/new` 32KB, `session/update` 34KB, 106 command |
| Shell env treo | App mở từ CLI ⇒ `get_cli_environment()` trả ngay, không spawn shell |
