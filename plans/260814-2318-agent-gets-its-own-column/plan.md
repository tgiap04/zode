# Agent là một cột riêng, bám theo bên của rail

**Status:** ✅ done (2026-08-15) · **Priority:** P1 · **Branch:** feat/rail-agents-claude-code-codex

## Vấn đề

Ba điều người dùng yêu cầu **không thể cùng đúng** khi agent còn là panel của left/right dock:

1. Agent là **cột riêng**, không xếp chồng với tool panel nào
2. Agent **bám bên của rail** (`multi_project.sidebar_side`)
3. Mở agent **và git panel cùng lúc**

git/outline buộc phải ở bên rail (`the_panel_docks_line_up_with_the_rails_side` chốt điều đó — panel khác bên thì nút biến mất khỏi rail). Rail trái → git/outline trái. Agent cũng trái → chung dock → hoặc xếp chồng (mất 1), hoặc thay phiên (mất 3).

**Một dock là một cột.** Muốn cả ba thì agent phải có cột của riêng nó.

## Hình dạng

```
rail trái:   [rail|sidebar] │ git/outline │ AGENT │ editor        │ right dock
rail phải:   left dock      │ editor      │ AGENT │ git/outline   │ [sidebar|rail]
```

Agent luôn nằm **giữa dock bên rail và editor** — sát code, không bao giờ tranh chỗ với panel nào.

## Cách làm: một `Dock` thứ tư, không phải cột tự chế

Chỗ ghép trong `Workspace::render` (4 nhánh) đã có hình dạng cần thiết:

```rust
div().flex_row().flex_1()
    .children(self.render_dock(Left,  &self.left_dock,  ..))
    .child( /* center */ )
    .children(self.render_dock(Right, &self.right_dock, ..))
```

`render_dock` **đã lo** bề ngang, tay kéo, `clamp_panel_size`, đóng/mở. Nên agent không cần cột tự chế với máy móc mới — nó cần `Workspace::agent_dock: Entity<Dock>`, chèn thêm một `render_dock` cạnh center.

Dùng lại được **toàn bộ**: width + resize + serialize + stack vừa xây ở [plan trước](../260814-2147-dock-holds-several-panels/plan.md). Cũng nghĩa là hai agent (Claude + Codex) vẫn xếp chồng/split trong cột đó như hiện nay.

## Phase

| # | Việc | Người dùng thấy gì |
|---|---|---|
| 01 ✅ | `Workspace` có `agent_dock`, vẽ cạnh center theo bên rail, **rỗng** | **Không gì cả** — cố ý |
| 02 ✅ | `AgentPanel` vào dock đó thay vì left/right | Agent thành cột riêng |
| 03 ✅ | Đổi bên khi rail đổi bên + dọn `agent.sidebar_side` | Bám rail |

Phase 01 lại là bước vô hình: dock mới tồn tại, được vẽ, nhưng chưa ai vào — mọi rủi ro của 02/03 đứng sau một bước đã xanh.

## Chỗ phải cẩn thận

- **`all_docks()`** đang trả 3 dock; rất nhiều thứ lặp qua nó (`focus_or_unfocus_panel`, `panel::<T>()`, `close_panel`, serialize). Thêm dock thứ tư vào đó là đổi hành vi diện rộng — phase 01 phải quyết dứt khoát cái nào tính và cái nào không, và có test cho từng cái.
- **`add_panel` định tuyến theo `panel.position()`** → `dock_at_position`. `AgentPanel` phải đi vào agent dock chứ không phải left/right dock cùng tên vị trí.
- **`DockPosition` chỉ có Left/Bottom/Right.** Agent dock vẫn mang một trong số đó (để `Panel::position` và resize biết chiều), nhưng **được vẽ ở khe khác**. Đừng để chỗ nào suy ra "dock ở Left tức là `workspace.left_dock`".
- **`agent.sidebar_side` thành thừa** khi vị trí bám rail. Phase 03 quyết: bỏ hẳn, hay giữ làm override.
- `restore_state` chạy **bên trong update của Workspace** — dock phải được *đưa* dữ liệu, không tự đọc workspace (xem `ccd151f`).

## Định nghĩa xong

Rail trái: `rail | git | AGENT | editor`. Bấm git → git hiện, agent **vẫn còn**. Đổi rail sang phải → agent nhảy sang phải editor. Đóng app mở lại: đúng layout đó, đúng agent đang mở.

---

## Xong (2026-08-15) — `323a856`, `a2cd72b`, `fed06a1`

Rẻ hơn ước lượng đầu: `render_dock` đã lo width + tay kéo + serialize, nên agent chỉ cần **một `Dock` thứ tư**, không phải cột tự chế.

Bốn nhánh `BottomDockLayout` bọc center **giống hệt nhau** → gom về một helper `render_centre_with_agent`, cột thêm ở **một** chỗ.

### Ba lần layout cắn, cả ba do test bắt

1. **Bọc center làm 2 test pane đỏ.** Thêm một tầng flex quanh center dịch thanh tab ra khỏi toạ độ test click. Đã `git stash` kiểm ở HEAD (215 xanh) để chắc là lỗi mình. Sửa: **trả center về nguyên vẹn** khi chưa có agent nào mở — load-bearing, không phải tối ưu.
2. **`h_flex()` cho hàng chứa cột** → `items_center` → cột `473px × 0px`. Docks nằm trong `div().flex().flex_row()`, không phải `h_flex`.
3. **Thiếu `self_stretch`** → vẫn 0px, vì nhánh bọc bên ngoài *là* `h_flex` và nó căn giữa con. Centre group mang đúng call đó vì đúng lý do đó.

### Bẫy khoá persistence — đã chặn trước khi nổ

Cột agent mang `DockPosition` dùng chung với dock cạnh nó, mà khoá lưu stack là `{workspace_id}:{position}` → cột (thường rỗng) sẽ **ghi đè bản ghi thật** của dock kia. Đã thêm `Dock::stack_key()` trả `"agent"` **cùng lúc** với việc đưa cột vào `all_docks()`, không tách ra.

### Tái nhập Workspace — lần thứ hai

`Panel::position` được gọi **từ trong** `Workspace::add_panel`. Cho nó `workspace.read_with(...)` là abort — đúng cái `ccd151f` đã trả giá. Lần này **test bắt được ngay** thay vì app chết lúc khởi động. Sửa: đọc thẳng setting, cùng một đáp số.

### Còn hở

Stack **hai kiểu panel khác nhau** giờ chỉ xảy ra ở ba dock thường (git/outline/project) — `agent_ui` không còn dựng được vì agent đã có cột riêng, và test-support chỉ có một `TestPanel`. Cơ chế `apply_stack_state` vẫn có test ở tầng `workspace` (khử trùng lặp, từ chối bản ghi rỗng/lạ, flexes cũ), nhưng đường "hai kiểu khác nhau khôi phục đủ" **không còn test**. Muốn lấp: thêm một kiểu panel thứ hai vào `workspace::dock::test`.
