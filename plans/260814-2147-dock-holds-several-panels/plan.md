# Dock giữ được nhiều panel cùng lúc

**Status:** planned · **Priority:** P2 · **Branch:** (chưa tạo)
**Bắt nguồn từ:** [rail-agents phase-08](../260812-0008-rail-agents-claude-code-codex/phase-08-agents-share-a-pane-group.md) — mục "Còn nợ"

## Vấn đề

`Dock::render` chỉ vẽ `visible_entry()` — **đúng một** panel. Nên project panel và agent (hay git và agent) không thể cùng hiện ở một bên; chúng thay phiên nhau.

Người dùng muốn xếp chồng chúng thành hai section trong cùng một dock, kiểu VS Code. Đây là tính năng, không phải setting: mô hình dock hiện tại chỉ có một `active_panel_index`.

## Hình dạng

```
        ┌─────────────┬──────────────────┬──────────────┐
        │             │                  │ ▸ Agent      │
  rail  │   editor    │                  ├──────────────┤ ← resize
        │             │                  │ ▸ Git Panel  │
        └─────────────┴──────────────────┴──────────────┘
                                          một dock, N panel
```

## Phase

| # | Việc | Trạng thái | Chặn bởi |
|---|---|---|---|
| 01 | [Dock giữ một *tập* panel đang hiện](phase-01-a-dock-holds-a-set-of-visible-panels.md) — refactor thuần, hành vi không đổi | ✅ done | — |
| 02 | [Xếp chồng + resize](phase-02-stack-them-with-resize-handles.md) | ✅ done | 01 |
| 03 | [Nút rail/status bar thành toggle từng panel](phase-03-buttons-become-per-panel-toggles.md) | planned | 01 |
| 04 | [Nhớ trạng thái xếp chồng](phase-04-remember-the-stack.md) — migration DB | planned | 02, 03 |

Phase 01 cố tình **không đổi gì người dùng thấy**: nó chỉ đổi mô hình dữ liệu từ "một index" sang "một tập", giữ bất biến "tập luôn có đúng một phần tử". Mọi rủi ro của 02–04 đứng sau một bước đã xanh.

## Điều đã khảo sát (không phải phỏng đoán)

**Phạm vi ảnh hưởng, đếm thật:**

| ký hiệu | dock.rs | nơi khác |
|---|---|---|
| `active_panel_index` | 16 | `sidebar/rail_panels.rs`, `git_ui/commit_modal.rs` |
| `visible_panel` | 3 | `workspace.rs` (17) |

**Máy móc đã có sẵn, dùng lại được:** `pane_group.rs:1135` `pane_axis(axis, basis, flexes, bounding_boxes, workspace)` đã giải đúng bài "N section co giãn theo một trục, có tay kéo, tự `serialize_workspace` khi kéo". Nó đang `pub(super)`; `dock` và `pane_group` là module anh em cùng crate `workspace`, nên đổi sang `pub(crate)` là dùng được — **không viết lại layout co giãn**.

**Hai trục kích thước, đừng nhầm:** `PanelSizeState { size, flex }` hiện đo *bề ngang* dock (trục ngang với dock trái/phải), lưu theo từng panel qua `persist_panel_size_state`. Xếp chồng là chia *chiều dài* dock (trục dọc) — trục thứ hai, chưa từng tồn tại. Đây là chỗ dễ sửa nhầm nhất.

## Rủi ro nền

- **Đây là code dùng chung với upstream Zed.** Mọi thay đổi trong `dock.rs` làm việc merge upstream về sau khó hơn. Đổi lại phải xứng đáng — nên phase 01 giữ diff nhỏ và có hình dạng dễ đọc.
- `DockData` đọc từ sqlite **theo vị trí cột** (`persistence/model.rs:202`). Thêm cột phải là migration append-only, và hàng cũ vẫn phải load được. Xem phase 04.
- `Panel::set_active` hiện có nghĩa "anh là cái đang hiện". `AgentPanel::set_active` đang móc `close_if_empty` vào đó (luật "panel rỗng thì tự đóng"). Đổi ngữ nghĩa mà không xem lại chỗ này là âm thầm phá luật vừa ship.

## Định nghĩa xong

Mở agent và git panel cùng một bên, thấy cả hai, kéo được ranh giới giữa chúng, đóng app mở lại vẫn đúng layout đó. Panel nào chưa bật vẫn nằm im trên rail như cũ.
