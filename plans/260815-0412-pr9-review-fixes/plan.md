# Soát PR #9 và sửa những gì soát ra

**Status:** ✅ done (2026-08-15) · **Priority:** P1 · **Branch:** feat/rail-agents-claude-code-codex

## Phạm vi thật của lần soát

PR 57k dòng, nhưng ~47k là code upstream Zed được khôi phục/port (`62630bc`, `4db7933`,
`779bb07`). Chỉ ~4k dòng là viết mới ở đây, và đó là phần được đọc: `workspace/{dock,workspace}.rs`,
`sidebar/rail*`, `agent_ui/{agent_panel,agent_view}.rs`, `project/agent_server_store.rs`,
`git_graph_core`. Phần upstream **không** được audit — nói ra để người sau không tưởng là đã soát cả.

## Lỗi cao nhất, và cách nó thoát khỏi mọi test đã có

`fills_the_center` trả `zoomed_pane.is_some()` mà không upgrade weak, còn `zoomed_pane` chỉ được
xoá ở `ZoomOut`. Đóng agent cuối để lại handle đứng nguyên → agent mở lần sau về là editor biến mất.

Test zoom đã có khẳng định đúng chuyện nó khẳng định (bật/tắt cờ khi bấm nút) và mù hoàn toàn với
chuyện này, vì nó chưa bao giờ **đóng** cái gì. Probe đo thẳng:

| | dock_open | fills |
|---|---|---|
| sau khi đóng tab | false | **true** |
| sau khi mở lại | true | **true** |

## Ba lỗi, một nguyên nhân

Zoom kẹt, `_subscriptions` phình vô hạn, và `show_new` chẻ cột không giới hạn — cả ba đều là
panel **không buông** thứ nó đã thôi vẽ. Gộp vào một commit vì tách ra là ba commit cùng sửa một
thói quen.

`AgentPanel::render` cũng thôi lease workspace. Nó chạy được vì render của panel diễn ra ở giai
đoạn layout, sau khi `Workspace::render` đã trả về — nhưng crate này đã trả giá hai lần cho đúng
hình dạng đó, và cái `.ok()` đi kèm không đỡ gì: `WeakEntity::update` chỉ trả `Err` khi entity đã
chết, lease đang sống thì abort.

## Test tautological — ba lần trong một buổi

1. Guard flex âm: `TestPanel` có một `persistent_name`, nên mọi record dedupe về **một** section;
   `flexes.len() == 1` bị chặn ở kiểm độ dài, kiểm dấu không bao giờ chạy. Test pass cả khi bỏ fix.
   Đóng bằng `OtherTestPanel` — panel **type** thứ hai, thứ mà stack hai section vốn cần. Nó cũng
   đóng luôn khoản nợ ghi ở phase 02: "stack hai loại panel khác nhau chưa từng có test".
2. Test khôi phục tab: pass cả khi bỏ **cả hai** đường ghi lúc mở agent — vì `AgentView::start`
   tự emit `UpdateTab`, và subscription đó ghi hộ. Đóng bằng cách khẳng định record **trước** khi
   rename, cộng một nhánh đóng tab (đường duy nhất không có ai emit hộ).
3. `an_unchanged_stack_...`: assert đầu tiên của tôi sai giả định — `show_panel` đã tiêu thụ thay
   đổi qua `persist_stack`. Sửa test theo hành vi thật thay vì sửa code theo test.

Quy tắc rút ra: **falsify từng nửa một**, không falsify cả cụm. Bỏ cả cụm thì một đường ghi khác
che mất và test vẫn xanh.

## Khôi phục tab: test bắt được lỗi abort trước khi ship

`restore_tabs` mở cột → `Panel::set_active` → chạy ngược vào chính panel đang bị lease. Abort.
`cx.defer_in` **cũng** abort, vì nó trao cho closure đúng cái `&mut Self` cần thoát khỏi.
`window.defer` mới đúng — nó không lease entity nào.

Đây là lần thứ ba crate này gặp bẫy tái nhập (`ccd151f`, `Panel::position`, và lần này).

## Sự thật bị mô tả PR nói sai

`SerializableItem for AgentView` bị gỡ ở `c056596` khi agent chuyển vào dock — máy khôi phục item
của workspace không với tới item nằm trong dock panel. Từ đó **không tab agent nào sống qua
restart**, trong khi mô tả PR vẫn ghi "Tabs survive a restart". `agent_views` thành bảng chết mang
comment mô tả việc nó không còn làm.

Đã dựng lại: ghi qua key-value store (không phải `agent_views`, vốn đọc theo vị trí), replay theo
thứ tự mở — thứ tự chính là layout, vì `show_new` chẻ tới cap rồi mới tab.

## Đã sửa

| # | | Commit |
|---|---|---|
| 1 | zoom kẹt · subscription phình · pane không giới hạn · render thôi lease workspace | `4919c3b` |
| 2 | `is_open` trong predicate · flex âm lọt qua · basis element đụng · bớt cấp phát mỗi frame · thôi ghi stack không đổi | `01a4897` |
| 3 | bỏ đọc cột `width` chết · bớt `String` mỗi frame ở rail · sửa comment tự mâu thuẫn | `44d0fdd` |
| 4 | nói thật về `agent_views` | `0b425ef` |
| 5 | khôi phục tab agent + tên + tự chạy | `88c84c4` |

## Chưa làm, và vì sao

- **`web/frontend` (63 file, ~10k dòng) vẫn nằm trong PR.** Người dùng chọn chỉ sửa mô tả. Tách ra
  là viết lại lịch sử branch, và đó là quyết định của chủ branch.
- **Nợ cũ chưa trả:** rename vẫn là **bản sao thứ hai** của `TerminalView`. Bản thứ ba thì phải tách.
- **Layout của tab bị kéo sang pane khác** không khôi phục được — record ghi cái gì đang mở, không
  ghi nó được đặt ở đâu.

## Cổng

53 agent_ui · 220 workspace · 18 sidebar · 2 git_graph_core · clippy sạch · `cargo build -p zode` ok.
Mọi fix đều đã falsify: bỏ fix → test đỏ, đặt lại → xanh.
