# Phase 05 — Cột agent hạ xuống

**Status:** ✅ done (2026-08-21) · **Priority:** P1 · **Người dùng thấy:** cột agent biến mất

## Mục tiêu

Xoá. Không còn ai đưa view vào `AgentPanel` sau phase 02, nên tất cả những gì dưới đây là
code chết — và một cột luôn rỗng vẫn tốn một `Entity<Dock>`, một settings observer, và một
lời giải thích cho người đọc sau.

Thuần xoá, không kèm hành vi mới: mọi thứ người dùng cần đã có chỗ ở phase 02–04.

## Việc

**`crates/agent_ui`**
- Xoá `agent_panel.rs` cùng test của nó. Bỏ `mod agent_panel` / `pub use AgentPanel`.
- Xoá `AgentColumnState`, `AgentTabState`, `AGENT_COLUMN_KEY`, `persist_tabs`, `restore_tabs`
  — `SerializableItem` ở phase 01 đã thay chỗ, và đường KVP này còn sót lại thì sau restart
  agent sẽ hiện hai lần (đúng cái phase 02 đã ghi là nợ).
- Xoá `MAX_SIDE_BY_SIDE_AGENTS`, `apply_tab_bar_buttons`, hai predicate
  `can_drop` / `can_split`, `set_split_for_drop`, `zoomed_pane`.

**`crates/zed`**
- Bỏ `add_agent_panel`, `AgentPanel::load`, và lời gọi `restore_tabs` (`zed.rs:687`).

**`crates/workspace`**
- Bỏ field `agent_dock`, `agent_column_bounds`, hàm `agent_dock()`.
- Bỏ `DockColumn::Agent` và nhánh `storage_key` của nó.
  `OWN_COLUMN_ORDER` → `[DockColumn::Database]`.
- Bỏ `Panel::fills_the_center` + `PanelHandle::fills_the_center` và nhánh zoom trong
  `render_centre_with_own_columns` — **chỉ** agent dùng nó. `fills_the_window` là của
  database, **không được đụng**.
- `TestPanel::new_agent` → chuyển các test own-column sang `DockColumn::Database`.

**`crates/database_ui`**
- `database_panel_tests.rs:73` assert DatabasePanel không nằm trong `agent_dock` —
  không còn cột đó để mà assert. Bỏ khối đó, giữ nguyên assert về left/right dock.

## Chỗ phải cẩn thận

- **Workspace test dùng cột agent làm cỗ xe thử own-column nói chung** (`TestPanel::new_agent`,
  `workspace.rs:13578–14082`). Đây là phần cơ học nhưng đông nhất của phase: chuyển sang
  `Database` là đổi **định danh cột**, không phải đổi điều đang được kiểm. Đọc từng assert,
  đừng sed hàng loạt — vài cái nói về "cột kia", và sau đây chỉ còn **một** cột.
- **`Dock::stack_key()` trả `"agent"`** cho cột này. Bản ghi cũ trong KVP thành mồ côi. Không
  migrate: nó chỉ giữ chiều rộng của một cột không còn tồn tại. Ghi vào changelog là bỏ.
- **Comment ở `workspace.rs:5419`** nhắc `AgentPanel` như ví dụ panel zoom được mà không có
  `pane()` — sửa comment, đừng để nó trỏ vào một type đã xoá.
- `zed.rs:2472` cũng có comment nói *"this fork moved the agent out to a column of its own"*
  — giờ nó sai. Sửa.
- `conversation_view.rs:2688` có comment nhắc `AgentPanel`. Sửa.

## Files

- xoá: `crates/agent_ui/src/agent_panel.rs`
- sửa: `crates/agent_ui/src/{agent_ui.rs, agent_view.rs, conversation_view.rs}`
- sửa: `crates/workspace/src/{workspace.rs, dock.rs}`
- sửa: `crates/zed/src/zed.rs`
- sửa: `crates/database_ui/src/database_panel_tests.rs`

## Todo

- [x] Xoá `agent_panel.rs` + khai báo module
- [x] Xoá đường KVP `AgentColumnState` / `restore_tabs`
- [x] Bỏ `agent_dock`, `agent_column_bounds`, `DockColumn::Agent`, `OWN_COLUMN_ORDER` → `[Database]`
- [x] Bỏ `fills_the_center` (giữ `fills_the_window`)
- [x] Chuyển test own-column sang `DockColumn::Database`, đọc từng assert
- [x] Sửa 3 comment trỏ vào cột/panel đã xoá
- [ ] ~~Test: sau restart, agent hiện **một** lần, đúng vị trí tab~~

**Đã làm, sau khi hai lý do "không làm được" của tôi lần lượt sai.**

Lần một tôi bảo không crate nào ngoài `crates/workspace` chèn được hàng `workspaces` (FK của
`agent_views`) vì `save_workspace` là `pub(crate)`. Reviewer chỉ ra `WorkspaceDb::next_id()`
là `pub`, và `editor::items` đã dùng đúng cách đó từ crate khác.

Lần hai tôi bảo vòng đầy đủ cần cả máy móc restore của workspace. Cũng sai:
`Workspace::set_database_id()` là `pub` — dựng workspace trên một hàng thật rồi chạy thẳng
`serialize` → `deserialize` là đủ.

Giờ có hai test: `a_tab_comes_back_as_the_agent_mode_and_name_it_was_left_as` (tầng DB) và
`a_named_tab_survives_being_written_down_and_brought_back` (trọn vòng, tên đặt qua dispatch
rename thật). Cộng thêm: đường KVP cũ bị xoá hoàn toàn nên không còn nguồn thứ hai để agent
hiện hai lần.

Mảnh còn lại: *vị trí* tab giữa các tab editor sau restart — cái đó do máy móc restore của
workspace quyết, không phải `SerializableItem` này.
- [x] `cargo check --workspace` xanh, `cargo clippy` không cảnh báo dead code mới

## Success criteria

Không grep nào còn ra `agent_dock`, `AgentPanel`, `DockColumn::Agent`, `fills_the_center`.
Cột database vẫn hoạt động y như trước — nó là own column duy nhất còn lại và không phần nào
của nó bị đụng.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Xoá luôn machinery database đang cần | Chỉ bỏ `DockColumn::Agent`; `is_own_column`, resize, storage_key vẫn còn cho Database |
| sed hàng loạt làm test mất ý nghĩa | Đọc từng assert; vài cái nói về "cột kia" mà giờ không còn |
| `fills_the_window` bị xoá theo | Ghi rõ trong todo; nó là của database |
