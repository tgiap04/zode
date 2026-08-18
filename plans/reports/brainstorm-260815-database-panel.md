# Brainstorm — Database panel trong rail

**Ngày:** 2026-08-15 · **Lens:** CTO · **Level:** medium
**Commission:** thêm nút database vào project rail; ấn vào mở một cột database, kết nối được tới nhiều DB. Tham chiếu UX: TablePro. Viết bằng Rust.

## Commission

Rail hôm nay có bốn tầng: ô project → nút panel của dock cùng phía (`rail_panels.rs`) → nút agent (`rail_agents.rs`) → footer. Yêu cầu này thêm một dock panel mới, nên nó rơi vào **tầng thứ hai** — tầng duy nhất tự động, không phải viết wiring rail nào cả.

TablePro là ứng dụng Swift/AppKit thuần macOS, 20+ engine, plugin system, iCloud sync. Không có gì để port. Nó chỉ là tài liệu tham chiếu phạm vi UX.

## Dữ kiện đã xác minh

| Dữ kiện | Ý nghĩa |
|---|---|
| `dock.rs:1793` `rail_draws_panel(name, position, rail_side, true)` — đúng/sai chỉ dựa trên vị trí dock + tên ≠ `PANEL_ALWAYS_IN_STATUS_BAR` | Một `Panel` mới ở left dock có `Panel::icon()` → **được nút rail miễn phí**. Không sửa `rail.rs`, không sửa `rail_panels.rs` |
| `rail_panels.rs:56-121` gọi `panel.icon()` / `icon_tooltip()` / `toggle_action()` | Chỉ cần implement `Panel` trait cho đủ, nút tự mọc kèm tooltip + keybinding |
| `crates/ui/src/components/data_table.rs` 1.091 dòng — `Table::uniform_list`, `ColumnWidthConfig::redistributable`, `TableInteractionState`, scrollbar | Result grid ảo hóa **đã có sẵn**, không viết lại |
| `crates/context_server/src/transport/stdio_transport.rs` + `client.rs:169` `Client::stdio(binary, working_directory, cx)` | Máy JSON-RPC-over-stdio đã chạy trong repo — copy được nguyên tầng transport |
| `extension_manifest.rs:106-122` đã có `language_servers`, `context_servers`, `agent_servers`, `debug_adapters`, `debug_locators` | `database_drivers` là entry thứ sáu cùng khuôn. Đường ray plugin đã có, không phải dựng mới |
| `crates/credentials_provider` — `trait CredentialsProvider` (keychain OS) | Chỗ hợp lệ duy nhất cho mật khẩu DB |
| `crates/gpui_tokio` tồn tại | Driver sidecar viết bằng Rust dùng sqlx/tokio thoải mái; editor không cần tokio |
| `icons.rs:61` `DatabaseZap` (`database_zap.svg`), `server.svg` | Có icon dùng tạm; một `database.svg` trơn nên thêm |
| `libsqlite3-sys` bundled đã có (cho `sqlez`) | Driver SQLite gần như không thêm chi phí build |
| Arrow Flight SQL bao đủ: catalog/schema enum, table discovery, PK/FK, query, prepared statement | Chuẩn có thật — nhưng Postgres/MySQL không nói Flight SQL, nên mượn được **spec** chứ không mượn được hệ sinh thái |

## Các đường đã cân

**Đường 1 — Sidecar process + JSON-RPC stdio tự định nghĩa, khai báo qua extension** ✅ *chọn*
- Ưu: đúng khuôn Zed đã có 5 lần (`language_servers` … `debug_adapters`). Driver treo/panic không giết editor, kill process là xong. Không kéo dependency DB nào vào binary editor. Transport tái dùng từ `context_server`.
- Nhược: **không có chuẩn** để bám như DAP có Microsoft — spec này bạn sở hữu vĩnh viễn, mọi engine mới về sau đều đo bằng nó. Serialize kết quả qua JSON; read-only + phân trang giữ chi phí này ở mức không đáng kể (~200 dòng/trang).

**Đường 2 — In-process qua `gpui_tokio` + sqlx** ❌ *loại*
- Rẻ hơn để bắt đầu, nhưng lạc khuôn: mọi thứ pluggable trong Zed đều là tiến trình ngoài. Và một query treo chỉ hủy được bằng drop `Task`, không kill được.

**Đường 3 — Arrow Flight SQL** ❌ *loại*
- Chuẩn thật, nhưng kéo tonic + gRPC + arrow-rs vào editor cho một tính năng phụ, mà vẫn phải tự viết adapter cho PG/MySQL. Trả giá dependency mà không nhận lại hệ sinh thái.

**Đường 4 — ADBC trong một sidecar chung** ❌ *loại*
- Được driver có sẵn của Apache Arrow, nhưng binding `adbc_core` còn non và phân phối thư viện C động không khớp cơ chế extension của Zed.

## Quyết định đã chốt

| # | Điểm | Chốt |
|---|---|---|
| 1 | Vị trí UI | **Cột riêng cạnh center**, kiểu `agent_dock` — giữa dock bên rail và editor, bám bên rail. Cây connection/schema + SQL editor + result grid xếp dọc trong cột đó (*sửa 2026-08-15 lúc lập plan: chốt ban đầu là "một dock panel trong left dock", nhưng `dock.rs:1492` `render_stack` đã cho left dock xếp chồng nhiều panel, nên panel ở đó là một section chia dọc với Git/Project — không phải sở hữu cả cột như hình đã vẽ. Giá của cột riêng: dock thứ 5 trong `all_docks()` + nút rail viết tay*) |
| 2 | Kiến trúc driver | **Sidecar process**, mỗi driver một binary |
| 3 | Plugin | **Ngay từ đầu** — `database_drivers` trong `ExtensionManifest`, cùng khuôn `debug_adapters` |
| 4 | Protocol | **JSON-RPC over stdio tự định nghĩa**, transport copy từ `context_server` |
| 5 | Driver tham chiếu v1 | **SQLite + PostgreSQL + MySQL** (cả ba qua sqlx, pure-Rust) |
| 6 | Phạm vi dữ liệu v1 | **Read-only**: duyệt schema/bảng, xem dữ liệu phân trang, chạy SQL, export CSV. Không sửa dữ liệu |
| 7 | Lưu connection | **Toàn cục + ghim theo project**. Mật khẩu ở keychain qua `credentials_provider`, không bao giờ nằm trong file |
| 8 | SQL editor | **Scratch buffer trong panel** — `Editor` thật (highlight, multi-cursor, vim), nội dung lưu theo connection |
| 9 | Cưỡng chế read-only | **Driver mở connection read-only**: `SET TRANSACTION READ ONLY` (PG) · `PRAGMA query_only` (SQLite) · session read-only (MySQL). DB tự từ chối, Zode không đoán câu lệnh |

## Kiến trúc

```
crates/database/            — protocol types, DriverClient (stdio), ConnectionStore
crates/database_ui/         — DatabasePanel: tree + SQL scratch + Table grid
crates/database_drivers/    — 3 binary: zode-db-sqlite, zode-db-postgres, zode-db-mysql

  DatabasePanel ──JSON-RPC/stdio──> driver sidecar ──sqlx──> DB
       │
       ├─ ui::Table            (grid, đã có)
       ├─ Editor               (SQL scratch, đã có)
       ├─ credentials_provider (keychain, đã có)
       └─ Panel::icon()        → nút rail tự mọc
```

Protocol tối thiểu, ~8 method: `initialize` · `connect` · `disconnect` · `list_schemas` · `list_tables` · `describe_table` · `query{sql, limit, offset}` · `cancel`.

Ước lượng: `database` ~2.5k · `database_ui` ~3k · 3 driver ~2.4k · manifest + settings ~0.4k ≈ **8-9k LOC**.

## Rủi ro phải canh

| Rủi ro | Đối sách |
|---|---|
| **Grid trong cột hẹp.** Bảng 20 cột trong 300px gần như không đọc được — đây là nhược điểm đã biết của quyết định #1 | Dock của Zed kéo rộng được. Nếu dùng thật vẫn chật, lối thoát rẻ nhất là một lệnh "Open Results in Editor" đẩy grid thành pane item ở center — thêm sau, không thiết kế trước |
| **Bịa protocol mù.** Ba driver là con số đúng để chống rò rỉ chi tiết một engine, nhưng cũng là ba lần trả giá cho mỗi lần đổi spec | Viết SQLite + Postgres trước, **đóng băng spec sau khi Postgres chạy**, MySQL là bài kiểm tra spec chứ không phải nơi sửa spec |
| **Phạm vi v1 lớn nhất từ trước tới nay của repo.** Plugin + sidecar + 3 driver + panel mới | Cắt theo trục dọc: SQLite end-to-end (nút rail → cây → grid) trước khi viết driver thứ hai |
| **Query treo** | `cancel` trong protocol + kill process là biện pháp cuối; timeout mặc định cho mỗi query |
| **Kết quả lớn** | Server-side `LIMIT/OFFSET`, không bao giờ kéo cả bảng qua stdio |

## Đo bằng gì

1. Nút database hiện trên rail, ấn vào mở panel — không sửa dòng nào trong `rail.rs`/`rail_panels.rs`.
2. Kết nối SQLite từ file trong project, duyệt tới một bảng, xem dữ liệu — end-to-end.
3. Cùng luồng đó chạy trên Postgres **không đổi một dòng nào trong `database_ui`**. Đây là bài kiểm tra thật của protocol.
4. Giết driver sidecar giữa chừng: editor sống, panel báo lỗi tử tế.
5. Mật khẩu không xuất hiện trong bất kỳ file nào trên đĩa.
6. `DELETE FROM ...` gõ vào scratch buffer bị **chính DB** từ chối, trên cả ba engine.
7. `./script/clippy` sạch, test chạy được không cần DB server (SQLite làm fixture).

## Còn để ngỏ

- Đặt tên protocol version + chính sách tương thích khi spec đổi (extension bên thứ ba sẽ bám vào).
- Có thêm `database.svg` trơn hay dùng `DatabaseZap` sẵn có.
