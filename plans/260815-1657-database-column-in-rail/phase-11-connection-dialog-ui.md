# Phase 11 — Hộp thoại kết nối theo mẫu TablePro

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-16) · **Blocked by:** 10

Người dùng gửi hai ảnh TablePro. Hai ảnh là hai thứ khác nhau, và chỉ một trong hai làm được **thật**.

## Ảnh 1 — "Choose a Database": làm đầy đủ

Danh sách engine có mô tả, search, nhãn **Not Installed**, footer Import from URL / Cancel / Continue.

### Chỗ khó: bất biến "không biết engine nào tồn tại"

Phase 07 đặt luật `database_ui` không được gọi tên engine, có test canh. Nhưng picker **phải** liệt kê
cả engine chưa có driver — một engine vắng mặt khỏi picker là engine không ai biết là mình đang thiếu.

Giải: `CATALOGUE` sống trong `driver_registry.rs` — file duy nhất được miễn trừ — và tên engine ở đó
là **dữ liệu hiển thị cho người**, không bao giờ là nhánh rẽ trong hành vi. Mọi thứ một connection làm
vẫn đến từ câu trả lời của driver.

### Alias không phải là mẹo

| Engine | Driver | Vì sao |
|---|---|---|
| CockroachDB | `postgres` | Dùng đúng wire protocol của PostgreSQL |
| PGlite | `postgres` | Socket server của nó tồn tại chính để nói PG protocol |
| MariaDB | `mysql` | Tương thích protocol MySQL |
| Oracle, SQL Server | — | Không chia protocol với thứ gì đang có → **Not Installed**, chờ extension |

6 trong 8 dùng được ngay, 2 nói thật là chưa. Không cái nào là lời hứa suông.

### Chi tiết

- [x] Search khớp **cả mô tả**: gõ "postgres" ra CockroachDB — đó là lý do các dòng mô tả tồn tại
- [x] Sắp xếp: cài rồi lên trước, rồi theo alphabet. Thứ hôm nay với tới được không nên nằm dưới thứ không
- [x] Bấm = **chọn**, Continue = đi tiếp (double-click cũng đi tiếp). Engine chọn nhầm bởi một cú bấm lạc là engine không ai muốn chọn
- [x] Continue trên engine chưa cài **nói lý do**, không im lặng — nút không làm gì là câu trả lời tệ nhất
- [x] Extension thêm driver Zode chưa từng nghe tên vẫn được nối vào cuối danh sách
- [x] "Import from URL…" chuyển thẳng sang form một ô URL — chính là `fallback_form` đã có

## Ảnh 2 — form: làm phần thật, **không vẽ sidebar**

Tiêu đề "New PostgreSQL Connection", nhóm Connection / Authentication, hàng label-trái/ô-phải trên một
mặt phẳng có kẻ ngăn, Cancel / Save / Save & Connect, và hàng Status + Test Connection.

- [x] `ConnectionField.group` — trường optional có mặc định dè dặt, **không tăng version**, cùng khuôn
      `identifier_quote` và `connection_form`. Chỉ driver mới biết ô nào là *tới được server* và ô nào
      là *được cho vào*
- [x] **Test Connection thật**: mở connection, báo tên driver đã trả lời, rồi đóng. Mọi thứ khác trên
      form là phỏng đoán cho tới khi đầu kia đồng ý — và biết sau khi lưu nghĩa là phải sửa file settings
- [x] **Save & Connect** thật: ghi settings rồi mở connection trong cây. Deferred, vì node chưa tồn tại
      lúc bấm — chính cú ghi settings mới tạo ra nó
- [x] Nút Back về lại danh sách, giữ tên đã gõ

### Sidebar 9 mục: cố tình không vẽ

Ảnh có SSH Tunnel, Cloudflare Tunnel, Cloud SQL Auth Proxy, SOCKS Proxy, SSL/TLS, Customization,
Advanced, AI Rules. Trong tám mục đó, thứ có thật hôm nay là **không mục nào**.

Vẽ một sidebar 9 mục mà 8 mục là vỏ rỗng là UI nói dối — tệ hơn không có sidebar. Mỗi mục là một tính
năng thật (SSH tunnel một mình đã là cả một implementation) và đáng có phase riêng nếu cần.

## Test

`database_ui` 36 → **39**. Bốn test cũ của modal bị mất khi ghi đè file và đã khôi phục.

| Test mới | Canh điều gì |
|---|---|
| `the_picker_lists_engines_with_no_driver_and_marks_them` | Engine chưa cài vẫn hiện, và thứ cài rồi nằm trên |
| `continuing_on_an_uninstalled_engine_explains_itself` | Không im lặng, và không nhảy sang form không điền được |
| `the_search_matches_descriptions_as_well_as_names` | Lý do các dòng mô tả tồn tại |

`no_source_file_in_this_crate_names_an_engine` **vẫn xanh** — `CATALOGUE` nằm trong file được miễn trừ.

## Một lỗi bắt được lúc đang viết

Bản đầu của `catalogue()` dùng `String::leak` cho driver từ extension. Hàm này chạy **mỗi lần mở
picker**, nên đó là rò rỉ không chặn — đúng thứ `CLAUDE.md` cấm. Đổi sang `SharedString`.

## Còn lại

- Chưa sửa/xoá connection từ hộp thoại. Chỉ thêm.
- Icon mỗi engine dùng chung `IconName::Database`. Logo riêng như TablePro cần asset thật cho từng
  engine, không phải thứ bịa ra được.
