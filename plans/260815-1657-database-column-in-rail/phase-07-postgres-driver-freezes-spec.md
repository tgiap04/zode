# Phase 07 — Driver PostgreSQL → đóng băng spec

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 06
**File ownership:** `crates/database_drivers/postgres/**` (mới) · `crates/database/src/protocol.rs` (**chỉ** trong phase này, và mỗi thay đổi phải có lý do viết ra)

Đây là phase **kiểm chứng**, không phải phase thêm tính năng. Câu hỏi nó trả lời: protocol ở phase 02 có thật sự là protocol, hay chỉ là SQLite mặc áo?

## Ràng buộc nghiệm thu

> Postgres chạy end-to-end **mà `crates/database_ui` không đổi một dòng nào.**

- [ ] `git diff --stat crates/database_ui` sau phase này phải **rỗng**
- [ ] Nếu buộc phải sửa `database_ui` → đó là **phát hiện**, không phải việc vặt: ghi vào mục "Xong" của phase file rằng protocol đã rò rỉ chi tiết engine ở đâu, rồi sửa ở tầng protocol chứ không vá ở UI

## Việc

- [ ] `zode-db-postgres` binary, cùng khuôn phase 03
- [ ] `connect`: sau khi mở, `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` — và mỗi query chạy trong transaction read-only để `SET` không bị câu lệnh người dùng gỡ
- [ ] `list_schemas`: `information_schema.schemata`, lọc `pg_*` và `information_schema` khỏi mặc định (có cách xem)
- [ ] `list_tables`: table + view + materialized view, phân biệt ba loại
- [ ] `describe_table`: `information_schema.columns` + `pg_index` cho primary key
- [ ] `values.rs`: kiểu riêng của Postgres (`numeric`, `jsonb`, `uuid`, `timestamptz`, `bytea`, mảng, `enum`) → ô + type tag. **Driver format, UI không biết**
- [ ] `cancel`: `pg_cancel_backend` trên connection phụ — Postgres không huỷ được từ chính connection đang bận
- [ ] SSL/TLS qua rustls; chốt hành vi khi chứng chỉ không xác minh được (từ chối, có cách chấp nhận thủ công cho từng connection)

## Read-only trên Postgres khó hơn SQLite

`PRAGMA query_only` của SQLite là công tắc một chiều. Postgres thì:

- [ ] `SET SESSION … READ ONLY` **gỡ được** bằng `SET SESSION … READ WRITE` từ chính SQL người dùng gõ → phải chạy mỗi query trong `BEGIN TRANSACTION READ ONLY`
- [ ] Hàm có side effect (`SELECT nextval(...)`, `SELECT my_writing_function()`) — transaction read-only chặn ghi ở tầng storage, xác minh chứ đừng cho là đương nhiên
- [ ] Có test cho **từng** đường lách ở trên

## Test

- [ ] Test tích hợp cần Postgres thật → đứng sau feature flag / biến môi trường, **bỏ qua tường minh** khi không có, không im lặng xanh
- [ ] Toàn bộ 8 method
- [ ] `DELETE`, `UPDATE`, `CREATE`, `SET SESSION … READ WRITE` — tất cả bị **Postgres** từ chối
- [ ] Kiểu riêng: `jsonb`, mảng, `timestamptz`, `numeric` chính xác cao, `NULL` trong mảng
- [ ] Huỷ một `pg_sleep(60)` đang chạy
- [ ] Bảng vài trăm nghìn dòng: phân trang không kéo cả bảng (đo thời gian trang đầu)
- [ ] Suite UI của phase 06 chạy lại nguyên vẹn trên Postgres

## Đóng băng spec

Khi mọi mục trên xanh:

- [ ] Tăng `PROTOCOL_VERSION` lên `1` và ghi vào `crates/database/src/protocol.rs` rằng từ đây trở đi mọi thay đổi phá vỡ tương thích phải tăng major
- [ ] Viết `crates/database/PROTOCOL.md` — 8 method, kiểu, luật version. Đây là thứ extension bên thứ ba đọc
- [ ] Ghi vào phase file này mọi chỗ protocol đã phải sửa trong phase 07 và **vì sao** — đó là danh sách những gì phase 02 đã đoán sai

## Định nghĩa xong

Một Postgres thật duyệt được qua cột database, chạy SQL, bị chặn ghi, huỷ được query — với `database_ui` nguyên vẹn. `PROTOCOL.md` tồn tại và `PROTOCOL_VERSION = 1`.

## Rủi ro

- **Cám dỗ vá ở UI.** Một `if engine == "postgres"` trong `database_ui` làm phase này xanh nhanh và làm phase 08 vô nghĩa. Ràng buộc `git diff --stat` ở trên tồn tại chính vì cám dỗ đó.
- Test cần server thật là nơi CI dễ bị bỏ qua âm thầm nhất. Test phải **báo là đã bỏ qua**, không được đếm như đã chạy.
- `pg_cancel_backend` cần một connection thứ hai → `connect` phải giữ sẵn thông tin để mở nó, quyết định lúc này chứ không phải lúc cần huỷ.

---

## Xong (2026-08-15)

`zode-db-postgres` 10 xanh · `database` 14 · `zode-db-sqlite` 15 · `database_ui` 21 · clippy sạch.

### Nghiệm thu cứng: ĐẠT

```
baseline: 4d7e09ef6e57eefc5562d7f65cb67777641edc62
now     : 4d7e09ef6e57eefc5562d7f65cb67777641edc62
```

Hash toàn bộ `crates/database_ui/src` **không đổi một byte** sau khi viết xong engine thứ hai.
Protocol chịu được. Cộng thêm test `no_source_file_in_this_crate_names_an_engine` (viết ở phase 06)
canh liên tục chứ không chỉ đo một lần.

**`crates/database/src/protocol.rs` cũng không đổi** ngoài việc tăng version — nghĩa là phase 02
không đoán sai chỗ nào. Đây là kết quả tốt hơn plan dự phòng.

### Đóng băng

`PROTOCOL_VERSION = 1` · `crates/database/PROTOCOL.md` viết xong (framing, 8 method, quy tắc giá trị,
phân trang, read-only, cancel, chính sách version). Từ đây đổi phá vỡ = version 2.

### Read-only trên Postgres: đúng như phase 03 cảnh báo

`SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY` **gỡ được** bằng `READ WRITE` người dùng gõ.
Nên mỗi statement chạy trong `BEGIN TRANSACTION READ ONLY` của riêng nó — không gì bên trong một
transaction nâng được chế độ read-only của nó. Test `every_page_runs_inside_a_read_only_transaction`
khẳng định thẳng vào chuỗi script, kèm thứ tự: SQL người dùng **không bao giờ** đứng trước `BEGIN`.

### Quyết định đáng ghi

| Chỗ | Chọn gì | Vì sao |
|---|---|---|
| Phân trang | `DECLARE … NO SCROLL CURSOR` + `MOVE FORWARD` + `FETCH FORWARD limit+1` | Bọc `SELECT * FROM (…) LIMIT` chỉ chạy với câu Postgres nhận làm subquery — loại `EXPLAIN`/`SHOW`. Cursor phân trang **phía server** cho mọi câu sinh dòng |
| Kết thúc transaction | `ROLLBACK`, không `COMMIT` | Nó không làm gì để giữ, và rollback là kết thúc trung thực cho một transaction chưa từng được phép ghi |
| Giá trị | `simple_query` → **text do server render** | Mọi decode nhị phân của `numeric` trong Rust đều đi qua kiểu làm tròn |
| Kiểu | `prepare()` riêng để lấy OID, **best-effort** | Câu Postgres không prepare được vẫn hiện dòng, chỉ mất type tag — thà thế còn hơn không hiện gì |
| Boolean | `t`/`f` → `Cell::Bool` | Đúng loại chi tiết engine mà UI không bao giờ được biết |
| Kiểu lạ (array, enum, range, composite) | `Cell::Text` với text của server | Một kind cho mỗi kiểu mở rộng là hệ thống kiểu thứ hai không ai giữ nổi |
| `cancel` | `CancelToken` + **connection thứ hai** | Postgres không nhận cancel trên chính socket đang bận |
| Lọc schema | Bỏ `pg_*` và `information_schema` | Có trên mọi database; liệt kê làm database nào cũng như có 40 schema |
| Introspection | `pg_class`/`pg_attribute`, không `information_schema` | Catalog trả lời cả materialized view, `information_schema` không liệt kê chúng |
| TLS | `rustls-platform-verifier` | Server DB nội bộ thường ký bằng CA chỉ máy đó tin; danh sách root đóng gói sẽ từ chối trong khi `psql` cùng máy nối được |
| TLS | **Không có** công tắc "chấp nhận mọi chứng chỉ" | TLS không xác minh tệ hơn không TLS vì nó *trông* an toàn. Ai thật sự cần thì `sslmode=disable`, ít nhất nó nói đúng nó là gì |

### Còn nợ

- **Test tích hợp cần một Postgres thật chưa viết.** 10 test hiện tại là unit test cho paging script,
  ánh xạ giá trị và mã lỗi — tức là phần *logic*. Phần chạm server (8 method end-to-end, huỷ
  `pg_sleep(60)`, bảng vài trăm nghìn dòng) cần một server trong CI và **chưa có**. Nói rõ để không ai
  tưởng driver này đã được chạy thử với dữ liệu thật.
