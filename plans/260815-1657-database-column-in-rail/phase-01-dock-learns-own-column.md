# Phase 01 — Dock biết khái niệm "cột riêng"; `database_dock` rỗng

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** —
**File ownership:** `crates/workspace/src/{dock.rs,workspace.rs}` · `crates/agent_ui/src/agent_panel.rs` (chỉ đổi tên chỗ gọi)

Tổng quát hoá một khái niệm đã có, rồi dựng cột thứ hai **rỗng**. Kết thúc phase này người dùng không thấy khác gì cả — và đó là mục đích.

## Vì sao tách riêng

`is_agent_column` là một **`bool`** (`dock.rs:304`), không phải một kind. Nó rẽ nhánh layout ở `:1123` (offset) và `:1138`, rẽ position ở `:559` `set_agent_column_position`, và được serialize ở `:1625`. `add_panel` định tuyến bằng `panel.is_agent_panel(cx)` (`workspace.rs:2638`).

Cột thứ hai bằng cách thêm `is_database_column: bool` thứ hai là nhân đôi mọi nhánh trên và mở đường cho trạng thái vô nghĩa (`is_agent_column && is_database_column`). Một `enum` làm hai cột dùng chung đúng một đường code.

Trộn việc này với việc thêm panel thì lúc có bug không biết nửa nào gây ra. Cột rỗng cho phép mọi test hiện có đứng nguyên làm lưới an toàn: **test nào đỏ ở phase này đều là hồi quy thật**.

## Việc

- [ ] `Dock::is_agent_column: bool` → `column: DockColumn` với `enum DockColumn { Tool, Agent, Database }` (`Tool` = dock trái/phải/dưới như cũ)
- [ ] `mark_as_agent_column()` → `mark_as_own_column(DockColumn)`; `set_agent_column_position` → `set_own_column_position` — ngữ nghĩa không đổi, chỉ hết gắn với agent
- [ ] `:1123`/`:1138` rẽ nhánh theo `column != DockColumn::Tool`, **không** liệt kê từng biến thể — hai cột riêng có cùng chrome (bỏ nền + border của dock, để `SURFACE_MARGIN`/`SURFACE_ROUNDING` của panel hiện ra; xem [plan 0047](../260815-0047-agent-column-menu-and-surface/plan.md))
- [ ] `Panel::is_agent_panel(cx) -> bool` → `Panel::own_column(cx) -> Option<DockColumn>` (mặc định `None`). `AgentPanel` trả `Some(Agent)`
- [ ] `workspace.rs:2638` `add_panel` định tuyến qua `panel.own_column(cx)` → `dock_for_column()`, không phải chuỗi `if`
- [ ] `Workspace::database_dock: Entity<Dock>`, dựng cạnh `agent_dock` (`:1709`), `mark_as_own_column(Database)`, và **cùng** `cx.observe_global::<SettingsStore>` bám bên rail (`:1722`)
- [ ] `render_dock` cho cột database, chèn giữa dock bên rail và center trong cả 4 nhánh của `Workspace::render` (`:7806`, `:7820`)
- [ ] `all_docks()` (`:2204`) trả **5**

## `all_docks()` lên 5 — quyết từng chỗ, đừng để trôi

Mọi hàm lặp qua `all_docks()` phải được ghi lại quyết định, không phải thừa hưởng ngầm:

- [ ] `focus_or_unfocus_panel`, `panel::<T>()`, `close_panel`, `panel_size_state` — **có** tính cột database
- [ ] `capture_dock_state` / `DockStructure` / serialize — **có**, nếu không mở lại app cột biến mất
- [ ] `agent_panel_position` (`:2358`) — không đổi ngữ nghĩa, nó hỏi về agent
- [ ] Mỗi quyết định trên có một test khẳng định đúng chiều

## Test

- [ ] Toàn bộ test `workspace` + `agent_ui` + `sidebar` hiện có xanh, **không sửa test nào**. Sửa một test ở phase này là dấu hiệu đã lỡ đổi hành vi
- [ ] Cột database tồn tại, được vẽ, **rỗng** → không chiếm bề ngang nào (dock rỗng phải đóng, như agent dock rỗng)
- [ ] Đổi `multi_project.sidebar_side` → cột database đổi bên cùng cột agent
- [ ] Bất biến: không `Dock` nào mang hai `DockColumn`

## Định nghĩa xong

`cargo test -p workspace -p agent_ui -p sidebar` xanh với không một dòng test cũ bị sửa. `grep is_agent_column` không còn kết quả nào. Người dùng mở app thấy y hệt hôm qua.

## Rủi ro

- `restore_state` chạy **bên trong update của Workspace** — dock phải được *đưa* dữ liệu, không tự đọc workspace (xem `ccd151f`). Cột database kế thừa đúng cái bẫy đó.
- `DockPosition` vẫn chỉ có Left/Bottom/Right. Cột database mang một trong số đó để `Panel::position` và resize biết chiều, nhưng **được vẽ ở khe khác**. Đừng để chỗ nào suy ra "dock ở Left tức là `workspace.left_dock`" — đó là bẫy plan 2318 đã ghi.

---

## Xong (2026-08-15)

`cargo check --workspace --tests` sạch · `cargo clippy -p workspace -p agent_ui -p sidebar --tests` sạch ·
`workspace` 224 xanh (220 cũ + 4 mới) · `agent_ui` 53 xanh · `sidebar` 18 xanh · 0 fail.
Diff đúng 3 file: `dock.rs`, `workspace.rs`, `agent_panel.rs`.

### Lệch khỏi plan, có lý do

**Plan viết "không sửa test nào". Một dòng đã phải sửa** —
`dragging_the_agent_columns_handle_resizes_the_agent_column` dựng `DraggedDock { agent_column: true }`,
và trường đó đổi tên thành `column: DockColumn::Agent`. Đổi tên trường của một struct, không phải đổi
hành vi: test vẫn khẳng định đúng điều nó vẫn khẳng định, và vẫn xanh. Ghi ra đây để lần sau không ai
tưởng lưới an toàn đã bị nới.

**`is_agent_column: bool` → `column: DockColumn` như plan, nhưng hai bảng tra được đẩy vào `enum`**
(`storage_key()`, `element_basis_offset()`) thay vì để `Dock` tự `match`. `stack_key` và
`stack_element_basis` là hai chỗ *phải* khác nhau giữa hai cột, và để chúng ở hai đầu file là cách
chúng lệch nhau. Đặt cạnh nhau trong `impl DockColumn` thì thêm biến thể thứ ba là sửa một chỗ.

**Một observer cho cả hai cột**, không phải hai. Plan không nói rõ; hai observer sẽ để hai cột nằm ở
hai bên khác nhau trong khoảnh khắc giữa hai lần fire, và thứ tự fire thì không ai bảo đảm.

### Thứ tự hai cột — quyết định mới, plan chưa nói

`OWN_COLUMN_ORDER = [Database, Agent]` từ ngoài vào trong: agent đứng sát code (đúng ý plan
[2318](../260814-2318-agent-gets-its-own-column/plan.md) đã chốt), database nằm ngoài nó. Bên phải thì
đảo lại, để mỗi cột giữ **khoảng cách tới center** chứ không giữ bên tuyệt đối.

### Test mới, và chúng bắt được gì

| Test | Bắt được |
|---|---|
| `a_panel_asking_for_the_database_column_lands_there` | `add_panel` định tuyến theo `own_column()` trước `position`; và `all_docks()` (giờ 5) thật sự với tới cột mới — nếu không `panel::<T>()`/`focus`/`close` mù với mọi thứ trong đó |
| `the_two_own_columns_do_not_share_stack_storage` | `stack_key` của hai cột khác nhau **và** khác dock cùng bên. Đây là lỗi âm thầm: cột rỗng ghi đè bản ghi của cột đang dùng |
| `dragging_the_database_handle_leaves_the_agent_column_alone` | Lý do tồn tại của trường `column`. Hai cột cùng bên, cùng `DockPosition` — kéo tay của cột này mà cột kia đổi bề ngang là lỗi không có test cũ nào thấy. Cũng khẳng định luôn thứ tự: `database_left < agent_left` |
| `both_own_columns_follow_the_rail` | Một observer, hai cột đổi bên cùng lúc, panel không bị lôi vào tool dock |

### Chưa làm, cố ý

Cột database **rỗng** — chưa có panel nào vào nó. `render_centre_with_own_columns` trả center nguyên
vẹn khi không cột nào có panel, nên người dùng không thấy khác gì. Panel là việc của phase 04.
