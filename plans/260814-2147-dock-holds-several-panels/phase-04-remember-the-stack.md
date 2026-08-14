# Phase 04 — Nhớ trạng thái xếp chồng

**Context:** [plan.md](plan.md) · [phase-02](phase-02-stack-them-with-resize-handles.md) · [phase-03](phase-03-buttons-become-per-panel-toggles.md)
**Priority:** P2 · **Status:** planned · **Blocked by:** 02, 03

Dựng được chồng mà mở lại app về một panel thì mỗi phiên làm việc lại phải dựng tay — đúng món nợ `AgentPanel` đang mang (layout tab/split trong panel chưa serialize).

## Cái đang lưu

`persistence/model.rs:196`:

```rust
pub struct DockData { visible: bool, active_panel: Option<String>, zoom: bool }
```

Đọc từ sqlite **theo vị trí cột** (`impl Column for DockData`, `:202`) — `Option::<bool>::column(statement, start_index)` rồi tăng dần. Nên:

- [ ] Cột mới **phải nối vào sau**, không chen giữa — chen vào là mọi hàng cũ đọc lệch trường, im lặng
- [ ] Migration append-only (luật sqlez của repo)
- [ ] Hàng cũ (chưa có cột mới) vẫn load: `active_panel` một mình có nghĩa là chồng một phần tử
- [ ] `Option::<T>::column` trả `None` cho cột thiếu — xác nhận bằng test trên **DB thật đã migrate**, không phải trên struct dựng bằng tay

## Lưu gì

- [ ] Danh sách panel đang hiện, **theo thứ tự** (thứ tự trong chồng là thứ người dùng tự sắp)
- [ ] `flexes` của chồng
- [ ] `active_panel` giữ nguyên, giờ nghĩa là "cái được focus gần nhất" — không bỏ, `visible_panel()` vẫn dựa vào nó

## Test

- [ ] Vòng tròn: dựng chồng 2 panel, kéo lệch tỉ lệ, serialize, load lại → đúng panel, đúng thứ tự, đúng tỉ lệ
- [ ] **Hàng cũ**: ghi một hàng đúng schema cũ, load bằng code mới → một panel, không panic. Đây là test dễ bỏ nhất và là cái duy nhất bảo vệ người đang dùng.
- [ ] Panel đã gỡ khỏi app (tên lạ trong DB) → bỏ qua, không làm hỏng cả dock

## Rủi ro

`WorkspaceDb` là DB thật của người dùng — migration sai không rollback được bằng cách sửa code. Xem `zode_sqlez_fk_test_db_trap` trong memory khi viết test: Domain có FOREIGN KEY phải mở DB của domain cha cùng tên trước.

## Sau phase này

Món nợ song sinh đáng làm liền tay: `AgentPanel` cũng chưa serialize tab/split bên trong nó ([rail-agents phase-08](../260812-0008-rail-agents-claude-code-codex/phase-08-agents-share-a-pane-group.md) → "Còn nợ"). Hai chỗ cùng một hình dạng — lưu layout của một nhóm co giãn — nên làm gần nhau thì tái dùng được cách làm, nhưng **không gộp**: một cái nằm ở `dock.rs` dùng chung, một cái ở `agent_ui`.
