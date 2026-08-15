# Phase 12 — Cột toàn màn hình, và nút ngắt kết nối

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-16) · **Blocked by:** 11

Hai việc rời nhau, gộp một phase vì cả hai đều nhỏ và cùng chạm panel.

## A — Toàn màn hình

### Ba cơ chế đã có, và vì sao không cái nào đủ

| Cơ chế | Vì sao không |
|---|---|
| `Panel::is_zoomed` + overlay `zoomed` của workspace | Sổ sách của nó khoá theo **`DockPosition`**, mà một own column dùng chung position với tool dock bên cạnh. `observe_in` của tool dock sẽ xoá zoom của cột database mỗi lần dock kia notify. Đây đúng là cái bẫy `fills_the_center` được sinh ra để tránh — comment ở `dock.rs` đã viết sẵn |
| `Panel::fills_the_center` | Chỉ lấy chỗ của editor, **dock vẫn còn**. Người dùng yêu cầu ẩn hết |
| Overlay `zoomed` phủ được rail? | **Không.** Rail do `MultiWorkspace` vẽ, là *anh em* của `Workspace`; overlay `inset_0` nằm trong `Workspace` nên không với tới |

### Cách làm

`Panel::fills_the_window` — bậc trên của `fills_the_center`, **chỉ đọc, không bao giờ ghi**, đúng khuôn
cái đàn anh. Panel giữ cờ; hai bên đọc:

- `Workspace::render` — thấy cột nào chiếm cửa sổ thì **thay cả thân**: centre, dock, tất cả. Chặn ở
  một chỗ chứ không luồn qua bốn nhánh `BottomDockLayout` — bốn nhánh là bốn cơ hội để lệch nhau
- `MultiWorkspace::render` — bỏ rail. Đây là nơi **duy nhất** làm được

Không gì ghi ngược vào workspace. Đó là điều khiến nút này an toàn khi bấm từ trong chính render tree
của panel — cái bẫy re-entrancy plan này đã trả giá **hai lần** (`Panel::position` ở `ccd151f`, và
`register_action` ở phase 09).

- [x] Nút Maximize/Minimize ở header, đổi icon và tooltip theo trạng thái
- [x] Action `database::ToggleFullScreen` + phím `cmd-shift-f` / `ctrl-shift-f`
- [x] **Escape thoát toàn màn hình**, và chỉ làm việc đó. Với rail đã ẩn, nút là đường về duy nhất còn
      lại — mà escape là thứ ai cũng thử trước
- [x] Focus vào panel khi vào toàn màn hình, để phím tới được nó thay vì tới thứ đứng sau

Title bar và status bar **giữ lại** — trên macOS nút đóng cửa sổ nằm ở title bar, ẩn nó là nhốt người
dùng lại.

## B — Ngắt kết nối

Trước phase này mở được mà không đóng được: driver chạy tới khi đóng cả cột.

- [x] Nút Power trên hàng connection, **chỉ hiện khi đang mở** — nút mời đóng thứ đã đóng là nút không làm gì
- [x] Mục "Disconnect" trong menu chuột phải, cũng chỉ khi đang mở
- [x] Cơ chế: **thả `Arc<Session>`**. Session giữ client, client giữ transport, transport giết tiến trình
      con. Không có gì để gửi đi — một driver được bảo đi chỗ khác qua cái pipe sắp đóng là tin nhắn
      không ai cần
- [x] Schema, bảng đang mở, grid và query state của **đúng connection đó** bị xoá theo. Một lưới rows
      từ session không còn tồn tại thì không page được, không chạy lại được

## Test

`database_ui` 39 → **42**. `workspace` 224, `agent_ui` 53, `sidebar` 20 — không đổi.

| Test | Canh điều gì |
|---|---|
| `full_screen_takes_the_whole_window_and_gives_it_back` | Workspace **thấy** cột đòi cửa sổ, và vẽ thật ở trạng thái đó — nhánh bỏ centre/dock chỉ chạy ở đây |
| `disconnecting_clears_only_that_connection` | So sánh index là thứ duy nhất giữ hai connection tách nhau; sai là xoá lưới của người khác |
| `a_connection_that_was_never_opened_reports_itself_closed` | Guard của nút, và no-op phải thật sự là no-op |

## Còn lại

- Trạng thái toàn màn hình **không được lưu** qua lần mở app sau. Cố ý: mở app lên thấy một cột che hết
  mọi thứ, không rõ vì sao, là trạng thái tệ hơn là mất một lần bấm.
- Ngắt kết nối không hỏi lại. Nó không mất gì — bấm lại là mở lại.
