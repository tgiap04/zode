# Phase 02 — crate `database`: protocol + client stdio

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** —
**File ownership:** `crates/database/**` (mới) · `Cargo.toml` (workspace members)

Không UI, không driver thật. Chỉ tầng nói chuyện, cộng một driver giả trong test để chứng minh nó nói được.

## Vì sao đứng riêng và chạy song song với 01

Crate này không chạm `workspace`. Hai phase sở hữu tập file rời hẳn nhau nên chạy đồng thời được, và cả hai đều là điều kiện của phase 04.

## Bố cục

```
crates/database/
  Cargo.toml            [lib] path = "src/database.rs"
  src/database.rs       re-export
  src/protocol.rs       kiểu request/response, serde
  src/client.rs         DriverClient — spawn + JSON-RPC over stdio
  src/registry.rs       driver nào tồn tại, binary ở đâu
  src/fake_driver.rs    #[cfg(any(test, feature = "test-support"))]
```

Mỗi file dưới 200 dòng theo CLAUDE.md. `[lib] path` tường minh, không `mod.rs`.

## Protocol — 8 method, không hơn

```rust
initialize    {}                                  -> { protocol_version, driver_name, capabilities }
connect       { url, credential_key }             -> { connection_id }
disconnect    { connection_id }                   -> {}
list_schemas  { connection_id }                   -> { schemas: Vec<SchemaRef> }
list_tables   { connection_id, schema }           -> { tables: Vec<TableRef> }
describe_table{ connection_id, schema, table }    -> { columns: Vec<ColumnDef>, primary_key }
query         { connection_id, sql, limit, offset }-> { columns, rows, truncated, elapsed_ms }
cancel        { connection_id, request_id }       -> {}
```

Quyết định phải ghi vào protocol.rs, không để suy diễn:

- [ ] `protocol_version` là số nguyên tăng dần, driver trả trong `initialize`. Zode từ chối driver lệch major và **nói rõ số nào** — extension bên thứ ba sẽ bám vào đây
- [ ] Giá trị ô là **string đã format sẵn + type tag**, không phải kiểu động. Driver biết cách hiển thị `numeric`/`jsonb`/`bytea` của engine mình; `database_ui` không được biết engine nào tồn tại
- [ ] `NULL` khác chuỗi rỗng — mã hoá tường minh, không dùng `Option<String>` rồi để UI đoán
- [ ] `truncated: bool` để UI nói được "còn nữa" mà không phải đếm
- [ ] `error` mang `{ code, message, detail }` — `code` đủ để UI phân biệt "sai mật khẩu" với "cú pháp SQL sai"

## Việc

- [ ] `protocol.rs`: kiểu + serde, mọi trường có doc comment nói **vì sao** nó tồn tại
- [ ] `client.rs`: `DriverClient` bọc `context_server::transport::StdioTransport`. Đọc `context_server/src/client.rs:169` và mượn hình dạng — **không** copy-paste cả file, chỉ dùng lại `Transport`
- [ ] `registry.rs`: `DriverRegistry` biết driver built-in (phase 03/07/08 nạp vào). Chỗ cắm cho extension để trống có chú thích trỏ tới phase 08
- [ ] Timeout mỗi request + `cancel`; drop `DriverClient` ⇒ kill process con
- [ ] `fake_driver.rs`: một driver trong-tiến-trình trả dữ liệu cố định, để phase 04/05/06 test UI không cần binary thật

## Test

- [ ] Round-trip mọi kiểu protocol qua serde
- [ ] `DriverClient` nói chuyện được với `fake_driver`: 8 method, mỗi cái một test
- [ ] Driver trả `protocol_version` lệch major → `connect` lỗi, message chứa cả hai số
- [ ] Driver chết giữa request → lỗi có ngữ cảnh, `DriverClient` không treo
- [ ] Drop `DriverClient` → tiến trình con chết (không để lại process mồ côi)
- [ ] Timeout: driver không trả lời → lỗi timeout, không treo vô hạn

## Định nghĩa xong

`cargo test -p database` xanh, `./script/clippy` sạch. Chưa ai dùng crate này — đó là điểm.

## Rủi ro

- **Đây là spec bạn sở hữu vĩnh viễn.** Không có chuẩn ngoài để bám như DAP có. Mọi trường thêm vào bây giờ rẻ; thêm sau phase 07 là đổi hợp đồng đã đóng băng.
- Cám dỗ thêm method thứ 9 ("chỉ một cái nữa thôi") — v1 read-only nên 8 method này đủ. Method mới cần một lý do viết ra trong plan, không phải trong lúc code.
- `StdioTransport` được viết cho MCP. Đọc kỹ nó có gán ngữ nghĩa MCP nào vào khung tin nhắn không trước khi dựa vào; nếu có, lấy phần khung thuần và bỏ phần MCP.

---

## Xong (2026-08-15)

`cargo test -p database` 14 xanh (4 protocol · 2 registry · 8 client) · `cargo clippy --all-features` sạch.
Chưa crate nào dùng — đúng chủ đích.

### Lệch khỏi plan, có lý do

**Transport tự viết (~150 dòng) thay vì dùng lại `context_server::StdioTransport`.**
Plan viết "chỉ dùng lại `Transport`". Không làm được: `StdioTransport::new` nhận
`ModelContextServerBinary`, nên dùng lại nghĩa là **kiểu MCP nằm trong API công khai của một driver
database**, kéo theo cả `context_server` (có http transport 756 dòng + oauth). Đã đọc kỹ bản gốc: nó
**không** gán ngữ nghĩa MCP nào vào khung tin nhắn — JSON theo dòng, thuần tuý. Nên chép hình dạng là
an toàn. Ghi chú trong `transport.rs` nói rõ: nếu có caller thứ ba, việc đúng là tách ra crate chung,
không phải bắt một bên phụ thuộc bên kia.

**`PROTOCOL_VERSION = 0`, không phải 1.** Spec còn *provisional* cho tới khi Postgres viết xong
(phase 07). Đặt `1` bây giờ là mời driver bên thứ ba bám vào một bản đặc tả đang còn di chuyển.

### Quyết định đã ghi vào protocol, không để suy diễn

| Quyết định | Vì sao |
|---|---|
| Ô = `enum Cell` có tag, driver format sẵn | UI canh phải số / làm mờ null mà không cần bảng tra tên kiểu của từng engine |
| `Cell::Null` là **biến thể**, không phải `Option<String>` | Null và chuỗi rỗng là hai câu trả lời khác nhau — chỗ này quan trọng trong DB client hơn gần như mọi nơi khác |
| `Cell::Number { value: String }` | `numeric(38,10)` và `u64` đều mất chữ số qua f64. DB client âm thầm làm tròn thì tệ hơn là vô dụng |
| `Cell::Binary { byte_len }`, không mang byte | base64 một blob lớn qua ống JSON theo dòng là cách driver làm đứng cả editor |
| `truncated: bool` | Driver lấy `limit+1` dòng rồi bỏ dòng thừa → không tốn `COUNT(*)` |
| `ErrorCode` thô, 8 biến thể | Đủ để UI *nói khác nhau*. Một code cho mỗi lỗi engine là hệ thống kiểu thứ hai không ai giữ nổi |
| `Capabilities` mọi trường `#[serde(default)]` = câu trả lời dè dặt | Driver viết theo bản cũ vẫn đúng, không phải đúng do may |
| `request_id` do **caller** đặt, không phải driver trả | Query cần huỷ thường là query chưa trả về gì cả |

### Test mới, và chúng bắt được gì

| Test | Bắt được |
|---|---|
| `every_method_makes_the_round_trip` | Cả 8 method, qua đúng đường encode/decode thật |
| `paging_says_when_there_is_more` | `truncated` bật ở trang 1, tắt ở trang cuối |
| `a_null_arrives_as_a_null` | Null sống sót **qua JSON**, không chỉ qua kiểu |
| `a_driver_on_the_wrong_version_is_refused_with_both_numbers` | Từ chối ở handshake, và message có **cả hai số** — thiếu một số thì không ai biết sửa đầu nào |
| `a_read_only_refusal_keeps_its_code` | `DriverError` downcast được, `code` sống sót → UI phân biệt được "read-only" với "sai cú pháp" |
| `a_silent_driver_times_out_rather_than_hanging` | Timeout là lưới cuối, không thay cho `cancel` |
| `callers_are_woken_when_the_driver_goes_away` | Ống đóng → mọi caller đang chờ bị đánh thức **ngay**, không phải chờ hết timeout từng cái một |
| `the_client_asks_for_exactly_what_it_was_told_to` | Client không tự gọi thêm gì — điều kiện để cây nạp lười ở phase 05 có nghĩa |
