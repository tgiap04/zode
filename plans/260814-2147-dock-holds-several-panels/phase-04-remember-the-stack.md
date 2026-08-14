# Phase 04 — Nhớ trạng thái xếp chồng

**Context:** [plan.md](plan.md) · [phase-02](phase-02-stack-them-with-resize-handles.md) · [phase-03](phase-03-buttons-become-per-panel-toggles.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-14) · **Blocked by:** 02, 03

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

---

## Xong (2026-08-14)

### Không đụng schema sqlite — plan sai chỗ này

Plan bắt thêm cột vào `workspaces` + migration append-only. **Không làm vậy**, vì lúc đọc code thấy codebase đã có sẵn cơ chế tốt hơn cho đúng loại dữ liệu này: `persist_panel_size_state` / `load_persisted_size_state` lưu state theo workspace vào **key-value store**, khoá `{workspace_id}:{panel_key}`, không dính gì tới bảng `workspaces`.

Dùng lại y vậy (`DOCK_STACK_KEY`, khoá `{workspace_id}:{dock_label}`) được toàn bộ thứ plan muốn mà **không phải chạm schema**:

| | thêm cột (plan) | KVP (đã làm) |
|---|---|---|
| Migration trên DB thật | có, không hoàn tác được | **không có** |
| Sửa SELECT/INSERT | ~9 chỗ, sai thứ tự là lệch trường im lặng | 0 |
| Install cũ | phải test hàng cũ | khoá vắng → rơi về `active_panel`, đúng hành vi cần |

Rủi ro lớn nhất của phase này biến mất cùng với cột.

### Ghi ở đâu

- `show_panel` / `hide_panel_by_id` / `activate_panel` → `persist_stack` (defer, vì đang ở giữa update của chính dock)
- Kéo tay chia tỉ lệ: `pane_axis::compute_resize` chỉ gọi `workspace.serialize_workspace`, **không** báo cho dock — nên `serialize_workspace_internal` cũng ghi stack của cả ba dock. Đó là chỗ duy nhất nghe được cú kéo.

### `persistent_name` là **static**, và điều đó định hình cả phase

`Panel::persistent_name()` không nhận `&self` — nó đặt tên cho một **kiểu** panel, không phải một thực thể. Hệ quả:

1. Bản ghi chỉ phân biệt được các panel **khác kiểu**. Đúng với thực tế (mỗi kiểu đăng ký một lần trong một dock), và cũng là giả định `restore_state` vốn đã dựa vào.
2. Test đầu tiên tôi viết dùng **hai `TestPanel`** — cùng tên → `indices` thành `[0, 0]` → chỉ một panel hiện. Test **bắt được ngay** ("left: 1, right: 2").

Đã sửa cả hai đầu: `apply_stack_state` **khử trùng lặp** index (bản ghi đến từ đĩa, tên lặp không được biến thành hai section rồi lệch với `flexes`), và test round-trip chuyển sang `agent_ui` — nơi có **hai kiểu panel thật** (`AgentPanel` + `TestPanel`), đúng luôn kịch bản người dùng.

### Ba đường lùi an toàn, đều có test

- Bản ghi rỗng (install cũ) → **từ chối**, giữ nguyên cái đang hiện
- Bản ghi chỉ gọi tên panel build này không có → **từ chối**
- `flexes` không khớp số section vừa resolve → rơi về chia đều (`pane_axis` có `debug_assert` so hai cái này, lệch là panic lúc vẽ)

Kiểm chứng ngược: cho `apply_stack_state` chỉ khôi phục panel đầu → đỏ "left: 1, right: 2".

### Gates

214 workspace · 44 agent_ui · 97 project_panel · 86 git_ui · 58 terminal_view · 18 sidebar. clippy sạch, `cargo check --workspace --all-targets` sạch, build ok.

### Abort lúc khởi động — và lý do test không bắt được (`ccd151f`)

Lần chạy đầu sau khi ship: **abort ngay lúc mở app**.

```
cannot read workspace::Workspace while it is already being updated
```

`restore_state` của tôi gọi `self.workspace.read_with(...)` để lấy bản ghi từ KVP. Nhưng `restore_state` chạy từ `Dock::add_panel`, mà cái đó chạy **bên trong một update của `Workspace`** — đọc lại workspace qua handle ở đó là tái nhập, và GPUI abort cả process chứ không phải trả lỗi.

Sửa đúng theo cơ chế đã có sẵn cho chính vấn đề này: workspace **đưa** bản ghi cho dock (`serialized_stack`), y như nó vốn đưa `serialized_dock`. Dock không với ra ngoài.

**Vì sao 214 test xanh mà app vẫn chết** — đây mới là phần đáng ghi nhớ: `restore_state` return ngay ở dòng đầu nếu `serialized_dock` là `None`, và **không test nào set nó**. Toàn bộ đường restore chưa từng được chạy, trong khi nhìn thì như đã phủ. Lỗi kiểu này chỉ hiện ra ở lần khởi động **thứ hai** của người dùng — sau khi có gì đó được ghi xuống.

Test mới `restoring_a_dock_does_not_read_the_workspace_it_is_inside` set `serialized_dock` rồi thêm panel. Kiểm chứng ngược: trả lại đoạn `read_with` cũ → đỏ với **đúng chuỗi và đúng vị trí** người dùng gặp (`entity_map.rs:164:32`).

## Sau phase này

Món nợ song sinh đáng làm liền tay: `AgentPanel` cũng chưa serialize tab/split bên trong nó ([rail-agents phase-08](../260812-0008-rail-agents-claude-code-codex/phase-08-agents-share-a-pane-group.md) → "Còn nợ"). Hai chỗ cùng một hình dạng — lưu layout của một nhóm co giãn — nên làm gần nhau thì tái dùng được cách làm, nhưng **không gộp**: một cái nằm ở `dock.rs` dùng chung, một cái ở `agent_ui`.
