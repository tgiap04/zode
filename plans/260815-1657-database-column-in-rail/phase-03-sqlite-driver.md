# Phase 03 — Driver SQLite

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 02
**File ownership:** `crates/database_drivers/sqlite/**` (mới) · `Cargo.toml` (members)

Driver thật đầu tiên. Chọn SQLite trước vì nó **không cần server** — test tích hợp chạy trong CI với một file tạm, không có hạ tầng nào phải dựng.

## Bố cục

```
crates/database_drivers/sqlite/
  Cargo.toml            [[bin]] name = "zode-db-sqlite", path = "src/main.rs"
  src/main.rs           vòng lặp stdio, dispatch 8 method
  src/introspect.rs     đọc schema từ sqlite_master + PRAGMA
  src/values.rs         giá trị SQLite → ô của protocol
```

## Việc

- [ ] Vòng lặp đọc JSON-RPC trên stdin, ghi stdout. **stderr là nơi duy nhất được log** — một dòng lạc ra stdout là hỏng khung tin nhắn
- [ ] `initialize` trả `protocol_version` khớp hằng số trong crate `database` (dùng chung hằng số, không chép số)
- [ ] `connect`: mở file, và **ngay sau đó** `PRAGMA query_only = ON`
- [ ] `list_schemas`: SQLite không có schema thật → trả đúng một schema `main` (cộng `temp`/ATTACH nếu có). **Không** giả vờ có nhiều
- [ ] `list_tables`: `sqlite_master` (table + view, phân biệt hai loại)
- [ ] `describe_table`: `PRAGMA table_info` cho cột, `PRAGMA index_list`/`index_info` cho primary key
- [ ] `query`: bọc `LIMIT ?/OFFSET ?`; lấy `limit+1` dòng để biết `truncated` mà không phải `COUNT(*)`
- [ ] `values.rs`: 5 kiểu lưu trữ của SQLite (NULL/INTEGER/REAL/TEXT/BLOB) → ô + type tag. BLOB hiện dạng `<N bytes>`, không nhồi nhị phân vào JSON
- [ ] `cancel`: `sqlite3_interrupt` qua handle của sqlx

## `PRAGMA query_only` — chỗ v1 read-only thành sự thật

Đây là toàn bộ cưỡng chế read-only cho engine này. Nó nằm ở driver vì Zode không được phép đoán câu lệnh.

- [ ] Bật ngay sau khi mở, trước khi câu lệnh người dùng nào chạy được
- [ ] `ATTACH` cũng bị `query_only` chặn ghi — xác minh, đừng cho là đương nhiên
- [ ] Lỗi bị chặn trả về `code` riêng để UI nói "cột này là read-only", không phải "lỗi SQL"

## Test

- [ ] Fixture: một `.sqlite` sinh trong test (bảng có PK, bảng không PK, view, cột mọi kiểu, giá trị NULL)
- [ ] 8 method end-to-end qua `DriverClient` thật, spawn binary thật
- [ ] `DELETE FROM …` → bị **SQLite** từ chối, đúng `code` read-only
- [ ] `CREATE TABLE`, `UPDATE`, `DROP`, `PRAGMA query_only = OFF` — tất cả bị chặn
- [ ] Phân trang: 250 dòng, `limit=200` → trang 1 `truncated=true`, trang 2 đủ 50 và `truncated=false`
- [ ] BLOB không làm hỏng JSON
- [ ] SQL sai cú pháp → `code` khác với `code` read-only
- [ ] Bảng không có primary key → `describe_table` trả `primary_key: None`, không panic

## Định nghĩa xong

`cargo test -p zode-db-sqlite` xanh **không cần bất kỳ DB server nào**, chạy được trong CI. Một fixture SQLite duyệt được đủ 8 method qua tiến trình thật.

## Rủi ro

- Driver này sẽ là bản mẫu cho Postgres và MySQL chép theo. Chỗ nào ở đây lười, hai driver sau chép luôn cái lười đó — `introspect.rs` và `values.rs` đáng được viết cẩn thận hơn mức một driver cần.
- SQLite nghèo nhất trong ba engine (không schema, không user, không kết nối mạng). **Đừng** để sự nghèo đó lọt vào protocol dưới dạng "trường này không cần" — protocol đã chốt ở phase 02, phase này chỉ điền vào.
- `libsqlite3-sys` bundled đã có trong workspace cho `sqlez`; dùng lại đúng version đó, đừng kéo bản thứ hai.

---

## Xong (2026-08-15)

`cargo test -p zode-db-sqlite` 15 xanh · clippy sạch · **không cần DB server nào**, fixture sinh trong test.

### Lệch khỏi plan — và đây là chỗ plan có lỗ hổng

**Plan viết: cưỡng chế read-only bằng `PRAGMA query_only = ON`. Không đủ.**
`query_only` chỉ là một setting, và **người dùng gõ `PRAGMA query_only = OFF` là tắt được**. Một chế
độ read-only mà người dùng gõ một dòng là tắt thì không phải chế độ read-only.

Thay bằng `OpenFlags::SQLITE_OPEN_READ_ONLY` — chính **file handle** không ghi được, không pragma nào
đổi được. Test `the_database_itself_refuses_every_write` chứng minh thẳng: bật `query_only = OFF`
(cho phép thành công, nó chỉ là setting), rồi `DELETE` vẫn bị từ chối với `ErrorCode::ReadOnly`, và
`count(*)` vẫn là 2.

Phụ phẩm đúng ý: `SQLITE_OPEN_READ_ONLY` **không tạo file mới**, nên sai đường dẫn báo "không có
database" chứ không lặng lẽ mở một file rỗng. Có test riêng.

**Phase 07/08 phải kế thừa bài học này**: `SET SESSION … READ ONLY` của Postgres/MySQL cũng gỡ được
từ SQL người dùng — nên mỗi query phải nằm trong `BEGIN TRANSACTION READ ONLY`, đúng như phase file
của chúng đã ghi.

**`rusqlite` thay vì `sqlx`.** Plan viết cả ba driver dùng sqlx. sqlx là async; `server::serve` là
sync (threads), và quan trọng hơn: **cancel cần một handle gọi được từ ngoài trong lúc query đang
chạy**. rusqlite có `InterruptHandle` (`Send + Sync`) làm đúng việc đó; sqlx không có thứ tương
đương cho driver sync. rusqlite 0.32 dùng chính `libsqlite3-sys 0.30` workspace đã pin, nên không
thêm bản sqlite thứ hai. Postgres/MySQL sẽ dùng crate sync có cancel token tương ứng.

### Lỗi thiết kế tìm ra trong lúc viết: `serve` tuần tự làm `cancel` vô dụng

`server.rs` bản đầu đọc-xử lý-trả lời tuần tự. Nghĩa là trong lúc `query` chạy, vòng lặp **không đọc
stdin**, nên request `cancel` không bao giờ tới nơi — method tồn tại mà không bao giờ làm gì.

Viết lại: mỗi request một thread, một writer thread giữ stdout (hai câu trả lời không thể xen giữa
dòng nhau). `Driver` đổi sang `&self` + `Send + Sync`, driver tự giữ lock. Đây là điều kiện để
`cancel` có nghĩa ở cả ba driver.

### Quyết định đáng ghi

| Chỗ | Chọn gì | Vì sao |
|---|---|---|
| Phân trang | Step + skip, **không** bọc `SELECT * FROM (...) LIMIT ?` | Bọc chỉ chạy với câu SQLite nhận làm subquery — loại `PRAGMA` và `EXPLAIN`, đúng hai thứ người ta gõ khi query cư xử lạ |
| `truncated` | Lấy dư 1 dòng rồi dừng | Không tốn `COUNT(*)` |
| `cancel` | So `request_id` với query **đang chạy** rồi mới interrupt | `cancel` đến muộn không được với tay giết query kế tiếp |
| `running` được set **sau** khi lấy được connection lock | | Set sớm hơn thì `cancel` tên query này lại giết query đang thật sự chạy |
| Định danh schema/bảng | `quote_identifier` nhân đôi dấu nháy | Bảng thật sự có thể tên `"; drop table users; --`, và SQLite không bind được identifier |
| `f64` → `{:?}` | | `{}` biến 0.1 thành 0.1000000000000000055511151231257827 |
| Cột không có declared type | Để **rỗng** | SQLite thật sự không biết; ghi `TEXT` là đoán được trình bày như sự thật |
| `BOOLEAN` chứa 0/1 → `Cell::Bool` | Chỉ khi giá trị đúng là 0 hoặc 1 | SQLite không có boolean thật; giá trị ngoài 0/1 thì cột khai gì cũng không phải boolean |
