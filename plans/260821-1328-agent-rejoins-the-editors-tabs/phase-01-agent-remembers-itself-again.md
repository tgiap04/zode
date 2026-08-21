# Phase 01 — `AgentView` tự ghi nhớ được lần nữa

**Status:** ✅ done (2026-08-21) · **Priority:** P1 · **Người dùng thấy:** không gì cả — cố ý

## Mục tiêu

Dựng lại đường persistence cho `AgentView` như một item của workspace. Chưa đổi định tuyến:
agent vẫn mở trong dock, nên impl mới nằm đó không ai dùng cho tới phase 02. Đó là điểm của
bước này — toàn bộ rủi ro DB/migration đứng sau một bước đã xanh.

## Bối cảnh

- `c056596` xoá `impl SerializableItem for AgentView`. Lấy lại nguyên văn:
  `git show c056596^:crates/agent_ui/src/agent_view.rs`
- Bảng `agent_views(workspace_id, item_id, agent, mode)` vẫn còn — migration append-only.
  Comment ở `agent_view.rs:1066` nói rõ "Nothing reads or writes this table."
- Cùng commit cũng xoá `AgentViewDb::{save_agent, get_agent, delete_unloaded}`.

## Việc

1. **Migration append thêm một câu** vào `AgentViewDb::MIGRATIONS`:
   `ALTER TABLE agent_views ADD COLUMN name TEXT;`
   Append, tuyệt đối không sửa câu cũ — sửa một migration đã chạy là bỏ rơi mọi bản cài
   đã chạy nó. Cập nhật luôn comment "Nothing reads or writes this table" cho đúng.
2. **Dựng lại `save_agent` / `get_agent` / `delete_unloaded`**, lần này mang thêm `name`.
3. **Dựng lại `impl SerializableItem for AgentView`** từ bản cũ, với hai sửa đổi:
   - `AgentView::new` giờ nhận 6 tham số (bản cũ có thêm một `None` cho width) — bỏ nó.
   - `should_serialize` trả **`true`** cho `AgentViewEvent::UpdateTab`. Bản cũ trả `false`,
     và đó chính xác là lý do tên tự đặt chưa bao giờ sống qua restart.
   - `deserialize` gọi `restore_custom_name` với `name` đọc lên.
4. **`register_serializable_item::<AgentView>(cx)`** trong `agent_ui::init`.

## Files

- sửa: `crates/agent_ui/src/agent_view.rs`
- sửa: `crates/agent_ui/src/agent_ui.rs`

## Vì sao bước này thật sự vô hình

`cleanup(workspace_id, alive_items)` chỉ xoá hàng của item **không** còn sống; bảng đang
rỗng nên không có gì để xoá. `deserialize` chỉ chạy cho item center đã được serialize — chưa
có cái nào. Nên đăng ký sớm là trơ hoàn toàn, không phải đánh cược.

## Todo

- [x] Migration cột `name` (append)
- [x] `save_agent` / `get_agent` / `delete_unloaded` mang `name`
- [x] `impl SerializableItem for AgentView`, `should_serialize` → `true` cho `UpdateTab`
- [x] `register_serializable_item::<AgentView>` trong `init`
- [x] Test: round-trip save → get, gồm cả `name` là `Some` và `None`
  — dùng `WorkspaceDb::next_id()` để tạo hàng `workspaces` cha, theo tiền lệ
  `editor::items` (xem ghi chú ở cuối `plan.md`: tôi đã kết luận sai là không làm được)
- [x] `cargo check -p agent_ui` xanh

## Success criteria

Round-trip qua DB giữ đúng `agent`, `mode`, `name`. Hành vi người dùng **không đổi chút nào**:
agent vẫn mở trong cột như trước. Suite hiện tại vẫn xanh.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Sửa migration cũ thay vì append | Review diff: `MIGRATIONS` chỉ được **dài ra** |
| FK `workspaces` làm chết test db | Mở db của domain cha cùng tên trước (bẫy `sqlez` đã biết) |
| Đăng ký sớm gây restore ngoài ý muốn | Bảng rỗng + không có item center nào đã serialize |
