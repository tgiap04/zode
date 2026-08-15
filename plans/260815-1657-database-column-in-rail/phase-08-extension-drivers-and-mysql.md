# Phase 08 — Extension khai báo driver + driver MySQL

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 07
**File ownership:** `crates/extension/src/extension_manifest.rs` · `crates/extension_host/**` · `crates/database/src/registry.rs` · `crates/database_drivers/mysql/**` (mới)

Hai việc, cùng một mục đích: chứng minh spec đã đóng băng ở phase 07 chịu được người thứ ba.

## Việc A — `database_drivers` trong manifest

Entry thứ sáu cùng khuôn năm entry đã có (`extension_manifest.rs:106-122`).

```rust
#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
pub database_drivers: BTreeMap<Arc<str>, DatabaseDriverManifestEntry>,

pub struct DatabaseDriverManifestEntry {
    /// Tên hiển thị khi chọn engine lúc thêm connection.
    pub name: String,
    /// Sơ đồ trường kết nối (host/port/file/…) để dựng hộp thoại.
    pub connection_schema_path: Option<RelPathBuf>,
}
```

- [ ] Mẫu theo `DebugAdapterManifestEntry` (`:355`) — nhỏ, chỉ giữ thứ Zode cần trước khi chạy binary
- [ ] `ExtensionProvides` thêm biến thể tương ứng để `provides()` (`:127`) liệt kê đúng
- [ ] `extension_host` resolve binary của driver; đi qua `allow_exec` (`:168`) như mọi tiến trình extension khác
- [ ] `DriverRegistry` (phase 02) hợp nhất driver built-in với driver từ extension; **trùng id thì built-in thắng** và có cảnh báo
- [ ] Driver lệch major `PROTOCOL_VERSION` bị từ chối, thông báo nói rõ cả hai số

## Việc B — MySQL, bài kiểm tra spec

MySQL viết **như thể là người ngoài**: chỉ đọc `PROTOCOL.md`, không đọc code của hai driver kia.

- [ ] `zode-db-mysql`, 8 method
- [ ] Read-only: `SET SESSION TRANSACTION READ ONLY`, và mỗi query trong transaction read-only (cùng lý do như Postgres — session set gỡ được)
- [ ] `list_schemas`: MySQL trộn khái niệm database và schema — ánh xạ vào protocol **không đổi protocol**. Nếu không ánh xạ được thì đó là phát hiện, ghi lại
- [ ] `describe_table`: `information_schema.columns` + `statistics` cho primary key
- [ ] Kiểu riêng: `DECIMAL`, `JSON`, `BLOB`, `DATETIME` vs `TIMESTAMP`, `ENUM`, `SET`, và `tinyint(1)` (MySQL không có boolean thật)
- [ ] `cancel`: `KILL QUERY` trên connection phụ

## Luật của phase này

> Protocol **đã đóng băng ở phase 07**. MySQL không được sửa nó.

- [ ] Nếu MySQL không diễn đạt được điều gì qua protocol hiện có → **dừng lại, ghi vào phase file**, rồi mới quyết: (a) ánh xạ trong driver, hoặc (b) tăng major và nói rõ vì sao đáng
- [ ] `git diff crates/database/src/protocol.rs` rỗng là kết quả mong đợi. Không rỗng thì phần "Xong" phải giải thích

## Test

- [ ] MySQL: cùng bộ test như phase 07, sau feature flag, bỏ qua **tường minh** khi không có server
- [ ] Suite UI phase 06 chạy nguyên vẹn trên MySQL; `git diff --stat crates/database_ui` rỗng
- [ ] Extension giả khai báo `database_drivers` → driver hiện trong danh sách chọn engine
- [ ] Extension khai báo driver trùng id built-in → built-in thắng, có cảnh báo
- [ ] Driver extension lệch major → bị từ chối, message chứa cả hai số
- [ ] `provides()` liệt kê đúng với extension chỉ có driver database

## Định nghĩa xong

Ba engine chạy qua cùng một `database_ui` và cùng một protocol. Một extension khai báo được driver thứ tư mà không cần sửa Zode. `PROTOCOL.md` mô tả đúng thứ ba driver thật đang nói.

## Rủi ro

- **`extension_manifest.rs` là code dùng chung với upstream.** Thêm một trường có `#[serde(default)]` là diff nhỏ và lành; đừng nhân cơ hội sửa gì khác ở đó.
- MySQL trộn database với schema — đây là chỗ dễ phải sửa protocol nhất, và cũng là lý do nó được để cuối cùng. Ánh xạ trong driver là câu trả lời mặc định.
- Extension chạy binary tuỳ ý là bề mặt bảo mật. Đi qua `allow_exec` như mọi tiến trình extension khác, không mở đường tắt.
- Nếu phase này lộ ra spec sai ở chỗ căn bản, đừng vá cho xong: ghi lại và để lần sau tăng major. Một protocol sai được giấu đi đắt hơn một protocol version 2.

---

## Xong (2026-08-15)

385 test xanh toàn bộ · `cargo check -p zode` sạch · clippy sạch.

### MySQL tìm ra một lỗi thật, đúng như phase này tồn tại để làm

**`database_ui` quote định danh bằng dấu nháy kép. MySQL từ chối** trừ khi bật `ANSI_QUOTES`, mà đó
không phải mặc định. Tức là bấm một bảng trên cây với MySQL sẽ sinh
`SELECT * FROM "schema"."table"` — **lỗi cú pháp, mọi lần**.

Ba đường xử lý đã cân:

| Cách | Vì sao không / có |
|---|---|
| Driver MySQL tự bật `ANSI_QUOTES` khi connect | Người dùng gõ một câu tắt nó là hỏng lại. Cùng loại lỗi với `PRAGMA query_only` |
| Thêm method `preview_table` vào protocol | **Phá vỡ** → version 2, và mọi driver bên thứ ba pin vào 1 chết |
| ✅ Thêm `identifier_quote` vào `Capabilities` | Trường **optional có mặc định dè dặt** → theo đúng chính sách trong `PROTOCOL.md`, **không** phá vỡ, **không** tăng version |

`PROTOCOL_VERSION` vẫn là **1**. `Capabilities::quote_identifier()` sống trong crate `database`, nên
`database_ui` gọi nó mà không biết engine nào tồn tại — test
`no_source_file_in_this_crate_names_an_engine` vẫn xanh.

Đây là kết quả tốt của quy trình: MySQL viết như người ngoài đã lộ ra một lỗi mà hai driver kia
không thể lộ, và chính sách version đã ghi sẵn ở phase 07 cho câu trả lời không phải bịa.

### Chỗ MySQL thật sự khác, và xử ở đâu

| Khác chỗ nào | Xử ở đâu |
|---|---|
| Quote bằng backtick | `Capabilities::identifier_quote` — driver khai, protocol quote |
| Database = schema | Ánh xạ **trong driver**, không đẻ từ thứ hai trong protocol |
| Không có materialized view | Nhánh đó đơn giản không bao giờ xuất hiện |
| Không có boolean thật (`TINYINT(1)`) | Trả về **số**. `Cell::Bool` sẽ là bịa, và grid hiện `true` cho cột chứa 7 tệ hơn hiện 7 |
| **Không có cursor client FETCH được** | Bọc `SELECT * FROM (…) LIMIT`; `EXPLAIN`/`SHOW`/`DESCRIBE`/CTE không bọc được nên chạy nguyên và cắt phía driver. **Đã ghi vào `PROTOCOL.md`** như một cảnh báo, không giấu |

Read-only: `START TRANSACTION READ ONLY` mỗi câu, đúng kết luận của phase 03 và 07.
`SET SESSION TRANSACTION READ ONLY` gỡ được từ SQL người dùng.

### Extension khai báo driver

`database_drivers` vào `ExtensionManifest` (entry thứ sáu, mẫu `DebugAdapterManifestEntry`),
`ExtensionProvides::DatabaseDrivers`, nhãn trong extensions_ui. Entry giữ `name`/`path`/`args`/`env`
— chỉ thứ Zode cần **trước khi** chạy binary; mọi thứ khác driver tự trả lời qua protocol nên không
thể lỗi thời trong manifest. `path` là đường dẫn tương đối extension chứ không phải tên trần: qua
`PATH` thì bất cứ thứ gì trên máy cũng trả lời được cái tên đó.

Test: `parse_manifest_with_a_database_driver`, `a_manifest_without_database_drivers_still_parses`.
Luật ưu tiên built-in đã có test từ phase 02
(`a_built_in_driver_survives_an_extension_claiming_its_name`).

### Còn nợ — nói rõ

*(Cả bốn khoản dưới đây **đã trả xong** — xem [phase-09](phase-09-finish-the-feature.md).
Giữ nguyên chữ để đọc lại được lịch sử.)*

- ~~**`extension_host` chưa nạp `database_drivers` vào `DriverRegistry` lúc chạy.**~~ Manifest parse
  được, `provides()` đúng, `DriverRegistry::register_extension` có và có test — nhưng **đoạn ống nối
  hai đầu chưa viết**. Nghĩa là hôm nay chỉ driver built-in dùng được; extension khai báo driver thì
  Zode đọc được manifest mà chưa khởi động nó.
- ~~Test tích hợp cần MySQL/Postgres thật vẫn chưa có~~ (như đã ghi ở phase 07). 11 + 10 test hiện tại
  là unit test cho paging, ánh xạ giá trị, mã lỗi.
