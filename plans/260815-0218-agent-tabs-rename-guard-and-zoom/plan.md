# Tab agent: đổi tên, chỉ nhận agent, và phóng to nuốt editor

**Status:** ✅ done (2026-08-15) · **Priority:** P2 · **Branch:** feat/rail-agents-claude-code-codex

## Ba việc

1. **Đổi tên tab agent** — hai session Claude cùng tên thì không phân biệt được cái nào là cái nào
2. **Không kéo được tab editor vào cột agent** — hiện tại kéo được
3. **Phóng to**: ẩn code editor, cột agent chiếm trọn center; thu nhỏ về đúng như trước

## Việc 2: lời tôi viết trong comment không đúng với code

`AgentPanel::new_pane` có comment:

> *"An editor tab dragged over this dock is refused at the predicate rather than being talked out of it later"*

Nhưng tôi chỉ đặt `can_split_predicate` — chặn đường **thả vào mép để split**. Đường **thả vào thanh tab** đi qua `can_drop_predicate`, và `Pane::new` đang nhận `None` cho tham số đó. Comment mô tả một hàng rào chỉ dựng một nửa.

`can_drop` chạy trong handler `MouseUpEvent` (`div.rs:2343`), **không** phải lúc render — nên đọc `tab.pane.read(cx)` ở đó an toàn, khác với `can_split` (gọi từ trong update của pane, đã phải né bằng cách đọc qua borrow sẵn có).

## Việc 3: **không** dùng zoom của dock

`Panel::is_zoomed` kéo theo cả bộ máy: `Dock` và `Workspace` đọc nó ở 8 chỗ để dựng `workspace.zoomed` + `zoomed_position`, rồi `render_dock` trả `None` cho dock nào trùng vị trí. **Cột agent dùng chung `DockPosition` với tool dock cạnh nó** — bật zoom kiểu đó là git panel biến mất theo.

Và lớp phủ đó phủ toàn cửa sổ, trong khi yêu cầu là "center chỉ còn cột của agent" — docks vẫn còn.

Nên thêm **một** method có mặc định vào `Panel`:

```rust
fn fills_the_center(&self, window: &Window, cx: &App) -> bool { false }
```

Chỉ đọc, không ghi chéo entity — tránh đúng cái bẫy tái nhập đã trả giá hai lần (`ccd151f`, và `Panel::position` lần sau).

`render_centre_with_agent` hỏi câu đó: đúng thì trả về **mỗi cột** thay vì cột + centre. Bề rộng đã lưu của cột không bị đụng tới, nên thu nhỏ tự về đúng chỗ cũ — không cần nhớ trạng thái gì.

## Phase

| # | Việc | Người dùng thấy gì |
|---|---|---|
| 01 | `can_drop_predicate` chỉ nhận `AgentView` | Kéo tab code vào cột agent không ăn |
| 02 | Đổi tên tab qua menu chuột phải | Đặt tên được cho từng session |
| 03 | `fills_the_center` + xử lý `pane::Event::ZoomIn/Out` | Phóng to nuốt editor, thu nhỏ trả lại |

## Chỗ phải cẩn thận

- **`tab_extra_context_menu_actions`** đã có sẵn trong `Item` (terminal dùng cho "Rename") — đó là chỗ treo mục menu, không phải dựng menu mới.
- Đổi tên là **bản sao thứ hai** của cách làm trong `TerminalView` (`custom_title` + `rename_editor` + subscription blur). Tách ra dùng chung sẽ phải bọc cả editor, subscription và tích hợp `Item` — lớn hơn chính tính năng. Ghi lại là nợ, không giả vờ là không có.
- `AgentPanel::handle_pane_event` đang bỏ qua `ZoomIn`/`ZoomOut` ở nhánh `_ => {}`. Đó là lý do nút phóng to hiện **không làm gì** ngoài đổi trạng thái nút.
- Phóng to khi có **hai** pane agent: chỉ pane được phóng hiện, đúng như `PaneGroup::render` vốn làm với `zoomed`.

---

## Xong (2026-08-15) — `e6b4d6f`, `503faa3`

Gộp còn hai commit: chặn kéo-thả và phóng to đều nằm ở cùng vùng code của panel, tách ra là một commit ở giữa mà nút phóng to vẫn không làm gì.

### Hàng rào dựng một nửa, comment viết như dựng đủ

Comment cũ nói tab editor "bị từ chối ở predicate". Thật ra chỉ `can_split_predicate` (thả vào **mép** để split) được đặt; `can_drop_predicate` (thả vào **thanh tab**) nhận `None`. Comment mô tả trạng thái mong muốn chứ không phải code — và đó chính xác là đường người dùng kéo lọt vào.

`can_drop` chạy trong handler `MouseUpEvent` (`div.rs:2343`), không phải lúc render, nên đọc `tab.pane.read(cx)` ở đó an toàn — khác `can_split`, vốn được gọi từ trong update của pane và phải né bằng cách đọc qua borrow sẵn có. Kiểm bằng cách đọc gpui chứ không đoán, vì dự án này đã trả giá hai lần cho tái nhập.

### Phóng to: **không** mượn zoom của dock

`Panel::is_zoomed` bị đọc ở 8 chỗ trong `dock.rs`/`workspace.rs` để dựng `zoomed` + `zoomed_position`, rồi `render_dock` trả `None` cho dock **trùng vị trí**. Cột agent dùng chung `DockPosition` với tool dock cạnh nó → bật zoom kiểu đó là git panel biến mất theo. Và lớp phủ đó phủ cả cửa sổ, trái yêu cầu "center chỉ còn cột agent".

Thêm `fills_the_center` — **chỉ đọc**, mặc định `false`, không ghi chéo entity. Bề rộng đã lưu của cột không bị đụng, nên thu nhỏ tự về chỗ cũ; không có trạng thái "trước khi phóng" nào phải nhớ.

Nút phóng to trước đây **không làm gì** vì `pane::Event::ZoomIn` rơi vào nhánh `_ => {}` của `handle_pane_event`.

### Kiểm chứng ngược cả hai

- Trả `can_drop_predicate` về `None` → đỏ ở dòng khẳng định tab editor bị từ chối
- Cho `ZoomIn` rơi lại vào catch-all → đỏ ở dòng "zoomed, the column has to claim the centre"

### Phóng to ship ra vẫn hỏng, và test của tôi không thể bắt được — `de80a4e`

Người dùng báo ngay: bấm phóng to thì **mất luôn center mà cột agent cũng không hiện**.

Nguyên nhân: `PaneGroup::render` vẽ pane được truyền vào thành **div rỗng** (`pane_group.rs:576`) — quy ước là lớp phủ tuyệt đối của workspace sẽ vẽ nó. `AgentPanel` **không có** lớp phủ đó. Truyền `zoomed_pane` vào group là bảo "ai đó khác vẽ hộ", mà không ai vẽ. Cột nở hết cỡ rồi vẽ một mặt phẳng trống, trong khi `fills_the_center` đã cho center đứng xuống.

Sửa: phóng to ở đây nghĩa là "chỉ cái này, tại chỗ" → **group đứng ra**, pane được vẽ thẳng.

**Test cũ vô dụng, và đó mới là bài học.** Nó khẳng định cờ `fills_the_center` bật/tắt — đúng, nhưng chưa bao giờ hỏi có gì được vẽ không. Tệ hơn: nó **không mở dock** (thiếu `focus_panel`), nên mọi `debug_bounds` đều `None` vì lý do chẳng liên quan gì tới zoom. Lần sửa đầu tôi suýt nhận là xong dựa trên một test không chạm nổi vào code đang sửa.

Probe mới nói thẳng sự thật:

| | dock-panel | surface | agent-view |
|---|---|---|---|
| Trước sửa | 1920×1073 | 1912×1065 | **None** |
| Sau sửa | 1920×1073 | 1912×1065 | 1912×1037 |

Cột đã chiếm center đúng — chỉ là rỗng. Đúng cái người dùng thấy.

### Nợ đã ghi

Đổi tên là **bản sao thứ hai** của cách làm trong `TerminalView` (`custom_title`/`rename_editor`/subscription blur). Dùng chung sẽ phải bọc editor + subscription + tích hợp `Item` — lớn hơn chính tính năng, nên chưa tách. Bản thứ **ba** thì phải tách.

`Pane::can_drop_predicate()` là accessor mới, đặt sau `#[cfg(any(test, feature = "test-support"))]` — để test hỏi thẳng predicate thay vì dàn dựng một cú kéo thật.

## Định nghĩa xong

Chuột phải tab agent → Rename → gõ tên → Enter, tab đổi tên. Kéo tab file từ editor sang cột agent → không vào. Bấm phóng to → editor biến mất, cột agent chiếm cả center, git panel vẫn còn; bấm lần nữa → về đúng bề rộng cũ.
