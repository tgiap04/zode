# Phase 09 — Trả nốt bốn khoản nợ

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 08

Tám phase đầu dựng xong hình. Phase này đóng bốn chỗ đã ghi rõ là còn dở ở cuối phase 07 và 08 —
hai trong số đó là **chặn dùng thật**, không phải đánh bóng.

## A — Extension khai báo driver thì driver phải chạy được

Manifest parse được từ phase 08 nhưng không ai đọc nó lúc chạy. Hôm nay chỉ driver built-in dùng
được, nên lời hứa "plugin ngay từ đầu" mới là một nửa.

Đi theo đúng mẫu năm proxy đã có (`ExtensionContextServerProxy`, `ExtensionDebugAdapterProviderProxy`):

- `ExtensionDatabaseDriver` + `ExtensionDatabaseDriverProxy` trong `extension_host_proxy.rs`
- `extension_host` gọi register lúc load, unregister lúc unload
- `database_ui::driver_registry` cài proxy đó và ghi vào registry

### Chỗ suýt hỏng, và nó thật

`extension_host` đăng ký mọi thứ trong vòng lặp trên `wasm_extensions`, mà vòng lặp đó mở đầu bằng:

```rust
if extension.manifest.lib.kind.is_none() { continue; }
```

**Một extension chỉ khai báo driver database thì không có wasm.** Đặt đăng ký vào đó là viết một
đoạn code không bao giờ chạy — và test thì vẫn xanh, vì test nào cũng dựng extension có wasm.
Driver được gom từ `extension_entries` **trước** vòng lặp ấy.

### Registry thành global

Trước đây mỗi panel tự dựng `Arc<DriverRegistry>` riêng. Extension nạp driver sau khi khởi động, nên
bản sao riêng nghĩa là panel mở trước mãi mãi trả lời theo danh sách cũ. Đổi thành một
`Entity<DriverRegistry>` toàn cục; `Session::open` nhận thẳng `DriverDescriptor` thay vì cả registry.

### `allow_exec` — không có đường tắt

Driver phải được chính manifest đó cấp `process:exec`, đối chiếu với **đường dẫn tương đối như đã
viết trong manifest** — đường tuyệt đối khác nhau theo máy nên tác giả extension không thể viết ra
được. Không cấp thì bỏ qua kèm cảnh báo. Nếu không, gọi một binary là "database driver" sẽ trở thành
cách vòng qua danh sách capability.

- [x] `database_drivers_to_register()` — thuần, có 3 test: giải đường dẫn, thiếu grant, grant sai dạng

## B — Ghi mật khẩu vào keychain

Trước phase này chỉ có `read_secret`. Nghĩa là **không có cách nào** đưa mật khẩu vào keychain từ
trong app, nên mọi server có mật khẩu đều không kết nối được. Đây là khoản nợ nặng nhất.

- [x] `write_secret` / `delete_secret` trong `connection_store`
- [x] `PasswordPrompt` — modal, `Editor::set_masked(true)`, dismiss khi blur
- [x] Menu chuột phải trên connection: Pin/Unpin + **Set Password…**
- [x] Rỗng = **quên** mật khẩu đã lưu, không phải lưu chuỗi rỗng — đây là cử chỉ duy nhất ai cũng
      tìm tới để sửa một lần gõ nhầm
- [x] Lỗi ghi keychain **hiện lên**, không nuốt: keychain khoá thì đó là thứ duy nhất giải thích vì
      sao mật khẩu không dính

### Tự mở hộp mật khẩu khi driver từ chối

`PROTOCOL.md` viết sẵn: `authentication` là **lỗi duy nhất đáng mời nhập mật khẩu**. Nay đúng lúc đó
hộp tự mở — là lúc người dùng còn nhớ mật khẩu trong đầu. Mọi mã lỗi khác không mở, vì host không
tới được hay driver không chạy thì hỏi mật khẩu là hỏi nhầm thứ.

Phân loại tách thành `Outcome::for_error()` để test được: 6 mã lỗi + trường hợp lỗi trần không mang
mã nào.

**Đổi hành vi:** chuột phải trước đây pin/unpin ngay. Giờ mở menu. Một cử chỉ âm thầm làm một trong
nhiều việc là cử chỉ không ai tin.

## C — Test chạy driver thật

10 + 11 test của Postgres/MySQL đều là unit test cho paging script, ánh xạ giá trị, mã lỗi. Không
test nào **khởi động binary** và nói protocol với nó. Một driver có thể xanh hết mà vẫn không dùng
được.

`database::driver_test_suite` — thuần std, không gpui, không async (một `#[test]` không có executor
để nhường việc, và chặn luồng ở đây chính là điều muốn):

- [x] `DriverProcess` — spawn, một dòng gửi một dòng nhận, kill khi drop
- [x] `shared_suite` — initialize/connect/list_schemas · write bị từ chối là `read_only` **không phải
      `syntax`** · null ≠ chuỗi rỗng · trang 2 khác trang 1 và trang đầy có `truncated` ·
      `unknown_connection`
- [x] Postgres/MySQL: đọc `ZODE_TEST_{POSTGRES,MYSQL}_URL`, **bỏ qua có in lý do** khi vắng server

> **SQLite chạy cùng bộ đó, và đấy là điểm chính.** Hai bộ kia bị bỏ qua ở mọi máy không có server;
> một harness chỉ chạy trong những lần đó thì hỏng cũng không ai biết. SQLite không cần server nên
> nó giữ cho chính cái harness còn thật.

## D — Tay kéo giữa ba vùng

Ghi ở phase 06 là nợ vì `pane_group::pane_axis` là `pub(crate)` của `workspace`. Mở rộng nó ra là
đổi code dùng chung nhiều hơn thứ nhận lại, nên làm cục bộ.

- [x] Hai tay kéo: cây↔SQL, SQL↔kết quả. Nháy đúp trả về mặc định
- [x] Kéo theo **quãng đường đi được**, không theo toạ độ tuyệt đối — vùng không tự biết bounds của
      mình, mà theo dõi bounds mỗi frame chỉ để trả lời một câu hỏi chỉ hỏi lúc đang giữ tay kéo
- [x] Lần di chuyển **đầu tiên** của một lần kéo chỉ đặt mốc, không di chuyển gì. Sai chỗ này thì mỗi
      lần nắm tay kéo là vùng nhảy đúng bằng khoảng cách tới chỗ chuột vừa ở
- [x] Chặn dưới 56px: một vùng mỏng hơn thế thì không nắm lại được để kéo ra
- [x] Lưu theo project qua KVP, cùng mẫu với pin. Giá trị hỏng (0, âm, NaN) bị bỏ qua — KVP không
      phải schema và không gì kiểm tra thứ rơi vào đó

## E — Driver phải có mặt lúc chạy (tìm ra khi chuẩn bị chạy thử)

Bốn khoản trên xong rồi mà tính năng **vẫn không chạy thử được**, vì không gì build ba binary driver
cùng app:

- `default-members = ["crates/zed"]` → `cargo run` không build driver. Checkout mới không có cái nào
- Ba script đóng gói build đích danh `--package zode --package cli`, không có driver → **bản đóng gói
  cũng thiếu**

Triệu chứng giống hệt nhau ở cả hai: mọi kết nối chết ở "could not start the driver". Không test nào
bắt được, vì test gọi thẳng `CARGO_BIN_EXE_*` — cargo tự build binary cho test target.

- [x] `bundle-mac` / `bundle-linux` / `bundle-windows.ps1`: build, copy **cạnh executable của app**
      (`Contents/MacOS/`, `libexec/`, cạnh `Zode.exe`), và **ký** trên macOS/Windows
- [x] Linux vào `libexec` chứ không `bin`: `bin` vừa sai chỗ `driver_path` tìm, vừa nằm trên `PATH`
      người dùng
- [x] `script/build-database-drivers` cho bản dev
- [x] `docs/src/development.md` — mục riêng
- [x] Thông báo lỗi tự nói cách sửa: `(in a development build, run script/build-database-drivers first)`

`bundle-freebsd` không đụng: dòng build app ở đó vốn đã bị comment, script ấy chỉ dựng remote_server.

## Xong (2026-08-15)

`cargo check -p zode` sạch · clippy sạch trên mọi crate đã chạm · fmt không đụng file ngoài phạm vi.

| Crate | Test |
|---|---|
| `database_ui` | 20 → **31** |
| `extension_host` | +3 (`database_driver_tests`) |
| `zode-db-sqlite` | 15 + **1 suite chạy thật** |
| `zode-db-postgres` | 10 + 1 (bỏ qua có lý do) |
| `zode-db-mysql` | 11 + 1 (bỏ qua có lý do) |
| `database`, `extension`, `sidebar`, `workspace` | không đổi, vẫn xanh |

`PROTOCOL_VERSION` vẫn là **1**. Không method mới, không trường mới — phase này không chạm protocol,
chỉ thêm một mục *Conformance* vào `PROTOCOL.md` mô tả bộ suite cho người viết driver bên thứ ba.

### Ngoại lệ lint, nói rõ

`std::process::Command` bị cấm toàn workspace vì chặn luồng gọi nó. Trong `driver_test_suite` đó
đúng là thứ cần: `#[allow]` có phạm vi một hàm, kèm lý do.

### Còn lại

- `extension_host` khởi động driver của extension nhưng **chưa có extension thật nào để thử** —
  đường đi có test đơn vị cho từng đoạn, chưa có test đầu-cuối với một extension đóng gói thật.
- Ô quá dài vẫn không có popover; export vẫn chỉ trang đang hiện.
