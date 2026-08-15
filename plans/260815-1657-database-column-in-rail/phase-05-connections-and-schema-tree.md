# Phase 05 — ConnectionStore + cây connection/schema

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 03, 04
**File ownership:** `crates/database_ui/**` · `crates/database/src/connection_store.rs` · `crates/settings_content` (khối `database`)

Lần đầu dữ liệu thật đi từ đĩa lên màn hình.

## Connection sống ở đâu

Toàn cục + ghim theo project (quyết định #7). Ba nơi, ba loại dữ liệu, không trộn:

| Cái gì | Ở đâu | Vì sao |
|---|---|---|
| Danh sách connection (tên, engine, host, port, db, user) | Settings toàn cục của zode | Máy của bạn, không phải của repo. Xem [`config_dir()` là `~/.config/zode/`](../../crates/paths/src/paths.rs) — **không** phải `zed` |
| Ghim theo project | KeyValueStore, key theo worktree | Trạng thái theo máy + theo project, đúng khuôn `persist_panel_size_state`. **Không** thêm cột vào bảng `workspaces` — bảng đó đọc theo vị trí, cột mới làm lệch field |
| Mật khẩu | Keychain OS qua `credentials_provider` | Không bao giờ trên đĩa dạng thường |

- [ ] `CredentialsProvider` khoá theo `url` (`credentials_provider.rs:11`) → dùng chính connection URL làm key. Chốt dạng chuẩn hoá URL và ghi lại, vì đổi dạng sau này là mất mọi mật khẩu đã lưu
- [ ] Settings **không bao giờ** có trường password. Nếu schema cho phép, sớm muộn ai đó điền vào

## Việc

- [ ] `connection_store.rs`: CRUD connection, ghim/bỏ ghim, đọc-ghi credential, phát sự kiện khi đổi
- [ ] Schema settings `database.connections` trong `settings_content` + `assets/settings/default.json` — **kiểm tra file JSON**, nó ghi đè `#[default]` của Rust
- [ ] Hộp thoại thêm/sửa connection: tên, engine, host/port/db/user, mật khẩu (ghi thẳng vào keychain), nút "Kiểm tra kết nối"
- [ ] Cây dùng `ui::tree_view_item`: connection → schema → bảng/view. Nạp lười từng tầng (`list_schemas` → `list_tables`), không nạp trước cả cây
- [ ] Bấm bảng → `describe_table`, hiện cột + kiểu + primary key
- [ ] Trạng thái mỗi node: chưa nối / đang nối / lỗi (kèm `message` từ protocol) / đã nối
- [ ] Mở lại connection đã ghim khi mở project, **không tự kết nối** — kết nối là hành động người dùng bấm

## Test

- [ ] Cây nạp lười: mở connection gọi `list_schemas` đúng một lần, chưa gọi `list_tables` nào
- [ ] Ghim theo project: hai project khác nhau thấy hai tập ghim khác nhau
- [ ] Mật khẩu ghi vào `CredentialsProvider` giả, và **không** xuất hiện trong settings đã serialize (khẳng định thẳng vào chuỗi JSON)
- [ ] Driver trả lỗi xác thực → node hiện lỗi, cây không sập, `database_ui` không panic
- [ ] Toàn bộ chạy trên `fake_driver` của phase 02 + một fixture SQLite thật
- [ ] `uniform_list`/tree cần cha là cột — bọc trong `v_flex`, không phải `div()` trần (bẫy đã ghi trong memory: `div()` là row, `flex_1` trong đó cho ra 0 dòng mà **không panic**)

## Định nghĩa xong

Thêm một connection SQLite trỏ vào file trong project, bấm nối, mở cây tới một bảng, thấy danh sách cột và primary key. Đóng app mở lại, connection và ghim còn nguyên, mật khẩu vẫn trong keychain và không nằm trong file nào.

## Rủi ro

- **`assets/settings/default.json` là quyền cuối.** Một `#[default]` trong `settings_content` có thể bị nó ghi đè — đọc JSON trước khi kết luận.
- Settings đăng ký qua `#[derive(RegisterSetting)]` + inventory. Nếu test báo "unregistered setting type", **đừng** thêm `DatabaseSettings::register(cx)` vào init để dập — đó là dấu hiệu thiếu derive, và nó sẽ panic trong bản production.
- Cây có thể rất lớn (một Postgres vài nghìn bảng). Nạp lười là bắt buộc, không phải tối ưu.
- Kết nối chậm/treo không được đóng băng UI: mọi lời gọi driver qua `cx.spawn`, cây hiện trạng thái "đang nối" và huỷ được.

---

## Xong (2026-08-15)

`database_ui` 15 xanh · `cargo check -p zode` sạch · clippy sạch, **không còn warning nào**.

### Lệch khỏi plan, có lý do

**Không có hộp thoại thêm/sửa connection.** Plan liệt kê nó. Thay bằng: connection khai trong
`settings.json` dưới khoá `database.connections`, cộng một nút ⚙ trên header cột mở thẳng settings.
Lý do: đó là cách *mọi thứ khác* trong Zode được cấu hình (agent server, terminal, git), và một modal
riêng cho một danh sách ba trường là UI thừa. Mật khẩu vẫn qua keychain — nhưng hiện chưa có đường
*ghi* mật khẩu vào keychain từ trong app; connection cần mật khẩu phải được nạp bằng công cụ khác.
**Đây là khoảng trống thật, ghi ra để không ai tưởng nó đã xong.**

**`ConnectionStore` ở `database_ui`, không ở `database`.** Plan đặt nó trong `database`. Sai tầng:
`database` là protocol + transport, dùng chung với binary driver (`default-features = false`). Nhét
`settings` + `credentials_provider` vào đó là kéo tầng ứng dụng xuống dưới tầng giao thức.

### Quyết định đáng ghi

| Chỗ | Chọn gì | Vì sao |
|---|---|---|
| Khoá keychain | **URL**, không phải tên | Đổi tên connection không được làm mất mật khẩu; trỏ sang host khác thì phải mất |
| Chưa ghim gì | Hiện **tất cả** | Project mới mà danh sách rỗng trông như hỏng, không như chưa cấu hình |
| Ghim | Chuột phải, lưu qua `persist_workspace_state` | Đúng khuôn `persist_panel_size_state`; **không** thêm cột vào bảng `workspaces` |
| Ghim trỏ tới connection đã xoá | Bỏ qua im lặng | Không hồi sinh nó, cũng không giấu những cái còn lại |
| Entry thiếu name/driver/url | **Bỏ** | settings viết tay; node hỏng ở mọi cú bấm tệ hơn không có node |
| `page_size` | `.max(1)` | 0 dòng/trang thì phân trang mãi mãi mà không hiện gì |
| Reload khi settings đổi | Khớp **theo tên**, giữ session đang mở | Settings fire mỗi lần gõ một ký tự trong file |
| URL đổi | Vứt node | Session đang mở là mở tới database khác rồi |
| Driver trả 1 schema | Bỏ hẳn tầng schema trong cây | SQLite chỉ có `main`; một tầng không ai gập là một tầng lãng phí |
| Cây | Phẳng + `uniform_list` | Postgres vài nghìn bảng. Cha là `v_flex` + `min_h_0`, **không** `div()` trần — `div()` là row, `uniform_list` trong đó cho 0 dòng mà không panic |
| Tooltip connection đã nối | Tên driver + url | Chỗ **duy nhất** kiểm được `postgres` trong settings có thật sự tới driver PostgreSQL không |
| Bấm bảng | `describe_table`, hiện cột + kiểu + PK | Tên bảng không nói được đây có đúng bảng mình cần không |
| Hai cú bấm nhanh | Đối chiếu `open_table` **hiện tại** trước khi gán kết quả | Nếu không, cột của bảng thứ nhất rơi xuống dưới bảng thứ hai |

### Icon mới

`assets/icons/database.svg` và `table.svg` (`DatabaseZap` là glyph có tia chớp, gây hiểu nhầm).

### Test mới

`nothing_pinned_shows_everything` · `pinning_one_narrows_the_list_to_it` ·
`a_pin_for_a_connection_that_no_longer_exists_is_simply_ignored` · `toggling_a_pin_reports_which_way_it_went` ·
`the_credential_key_is_the_url_not_the_name` · `the_built_in_drivers_are_distinct_and_all_registered` ·
`editing_one_connection_leaves_the_others_alone` · `removing_a_connection_from_settings_removes_its_node` ·
`a_half_written_connection_is_dropped_rather_than_drawn` · `configured_connections_stay_closed_until_clicked`

### Còn nợ

- **Không có đường ghi mật khẩu vào keychain từ trong app.** Đọc thì có (`read_secret`). Cần một
  lệnh "Set Password" trước khi connection tới server có mật khẩu dùng được.
- Chưa có nút refresh cây khi schema đổi phía server.
