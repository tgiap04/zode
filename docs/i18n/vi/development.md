---
title: Developing Zode
description: "Guide to building and developing Zode from source."
---

# Phát triển Zode

Xem hướng dẫn riêng cho từng nền tảng để build Zode từ mã nguồn:

- [macOS](./development/macos.md)
- [Linux](./development/linux.md)
- [Windows](./development/windows.md)

## Database driver

Cột cơ sở dữ liệu giao tiếp với từng engine qua một binary riêng
(`zode-db-sqlite`, `zode-db-postgres`, `zode-db-mysql`, `zode-db-mongodb`),
do ứng dụng khởi chạy thành một tiến trình riêng. Không có gì link tới chúng, và
chúng không nằm trong `default-members`, nên cả `cargo run` lẫn `make build`
đều không build chúng.

Một checkout không có chúng hoạt động như bản release: bảng cơ sở dữ liệu đề nghị
tải từng driver vào lần đầu bạn kết nối tới engine của nó (xem bên dưới). Hãy build
chúng khi bạn đang làm việc với một driver hoặc không có đường tới API:

```sh
make drivers                             # beside `make build`
make drivers PROFILE=release-fast        # beside `make build PROFILE=release-fast`
script/build-database-drivers            # beside `cargo run`
script/build-database-drivers --release  # beside `cargo run --release`
```

Phải bật chủ động, vì driver cạnh file thực thi được dùng trước bất kỳ driver
đã tải nào. Nếu build mỗi lần `make build`, bản build phát triển sẽ bỏ qua
việc tải mãi mãi, và nút gỡ bên dưới sẽ không còn gì để gỡ.
Driver build từ lần chạy trước vẫn nằm đó và tiếp tục được ưu tiên cho tới khi bạn
xóa chúng, ví dụ `rm target/debug/zode-db-*`.

Thứ tự tìm khi bảng cần một driver: cạnh file thực thi đang chạy
trước (nơi các lệnh trên build ra), rồi kho tải về bên dưới, rồi
bất cứ thứ gì `ZODE_DB_<ID>` chỉ tới — `ZODE_DB_POSTGRES=/path/to/zode-db-postgres`, một
đường dẫn đầy đủ tới file có sẵn, không phải tên được tra trong `PATH`. Giá trị
không phải file sẽ được ghi log và bỏ qua, không bị lờ đi trong im lặng. Một checkout
vừa chạy `make drivers` luôn dùng driver nó vừa build, không bao giờ dùng
bản đã tải.

### Bản release lấy driver từ đâu

Bản release không kèm binary driver nào — các bundle release từng
build và đóng gói chúng, nhưng giờ không còn nữa. Mỗi driver được tải
vào lần đầu có người kết nối tới engine mà nó phục vụ, từ API riêng của Zode
chứ không từ GitHub release. Ứng dụng hỏi
`{ZODE_API_URL}/drivers/latest/zode-db-drivers-manifest.json` chỉ để biết
phiên bản hiện hành là gì. Sau đó nó tải manifest đã ký của phiên bản đó từ
`{ZODE_API_URL}/drivers/{version}/zode-db-manifest-signed.tar.gz`, và từng
archive từ `{ZODE_API_URL}/drivers/{version}/{asset}`. Manifest đã ký có
tên dạng archive chỉ vì backend không phục vụ gì khác ngoài tên của chính
manifest; không có gì đọc nó như một archive. Bản phát triển và bản release
tải cùng một bộ, bất kể phiên bản ứng dụng tự báo là gì, và
server chỉ giữ bộ được phát hành gần nhất — không có bộ cũ hơn nào để
lùi về.

Trước khi chạy bất cứ thứ gì, ứng dụng kiểm tra theo thứ tự này (xem
`crates/database/src/signature.rs` và `crates/database/src/install/`):

1. **Chữ ký.** Manifest đã ký gồm một dòng header,
   `zode-driver-manifest ed25519 <base64 signature>`, rồi manifest đúng như
   lúc được ký. Ứng dụng xác minh nó với khóa công khai Ed25519 được tích
   hợp sẵn trước khi tin bất kỳ dòng hay checksum nào, vì một checksum đến
   từ chính kênh phục vụ binary thì chẳng chứng minh gì. Manifest
   không ký, ký bằng khóa khác, hoặc cho phiên bản khác với phiên bản mà
   `latest` nêu đều bị từ chối.
2. **Không phải rollback.** Ứng dụng từ chối một bộ cũ hơn bộ mới nhất
   mà máy này đã cài, được ghi trong
   `<data_dir>/database_drivers/.newest-installed-version`. Chữ ký chứng minh
   ai phát hành một bộ, không chứng minh khi nào, và mọi bộ từng phát hành vẫn còn được ký.
   Cùng phiên bản thì được phép: đó là cách một driver bạn đã gỡ quay lại.
   Nếu cố ý quay lui, hãy xóa file đó. Khi không có bản ghi như vậy, như trên một
   bản cài mới, bản build từ chối mọi bộ cũ hơn
   `MINIMUM_DRIVER_SET_VERSION` (`0.1.5-0`, trong `crates/database/src/install/store.rs`),
   là bản prerelease thấp nhất của bộ đầu tiên từng được ký.
3. **Driver nói đúng giao thức của chúng ta.** Mỗi dòng mang `protocol`, phiên bản
   giao thức cơ sở dữ liệu mà driver nói. Driver mà bản build này không giao tiếp được
   sẽ bị từ chối trước khi tải: bản mới hơn thì báo hãy cập nhật Zode, bản
   cũ hơn thì báo chưa có driver nào được phát hành cho phiên bản này. Một dòng
   không có `protocol` bỏ qua bước kiểm tra này, và bước handshake vẫn có tiếng nói cuối cùng.
4. **Chính lần tải.** Nó dừng khi dữ liệu đến nhiều hơn manifest nói,
   dù manifest nói gì (không bao giờ quá 512 MiB), và bỏ cuộc khi một
   kết nối không gửi gì trong 30 giây. File dở dang bị xóa, kể cả
   khi bạn bỏ đi giữa lúc tải.
5. **Checksum.** SHA-256 của archive được so với manifest đã ký
   trước khi giải nén bất cứ thứ gì.

Khi API không có manifest để phục vụ, hoặc bộ nó phục vụ không được ký,
lần tải thất bại với một thông báo nói rõ điều đó và chỉ tới
`script/build-database-drivers` (hoặc `make drivers`), chứ không chỉ mã trạng thái HTTP. Một
manifest không có bản build của driver đó cho nền tảng của bạn sẽ nêu cách cài
bằng tay (bên dưới) như lối thoát còn lại.

Cho tới khi có một bộ đã ký được phát hành, bản build có bước kiểm tra này không thể
tải driver. Driver đã cài vẫn chạy bình thường, và các ứng dụng
cũ hơn cũng vậy, vì chúng đọc bản không ký dưới `latest`.

`ZODE_API_URL` mặc định là `https://api.zodekit.com/api`
(`zode_account::api_url()`); trỏ nó tới backend bạn chạy cục bộ để
kiểm thử luồng tải từ đầu đến cuối, ví dụ
`ZODE_API_URL=http://localhost:8000/api`. Mọi URL ứng dụng đi theo phải cùng
origin đó và là HTTPS; HTTP thường chỉ được chấp nhận cho loopback của chính máy
(`localhost`, `127.0.0.0/8` và `[::1]`). Backend bạn tự chạy
phải phát hành manifest được ký bằng khóa mà bản build của bạn tin, nếu không
lần tải bị từ chối. Đây là route công khai, không xác thực — bảng phải
chạy được cho người chưa từng đăng nhập.

`script/bundle-mac` là thứ build, ký và đóng gói các driver thành các asset
release đó, và CI chạy nó theo cách đó. `-n` bỏ qua cả build lẫn
đóng gói; `make bundle` truyền nó, vì bundle cục bộ không bao giờ kèm
driver và việc phát hành chúng là công việc của job release.

Ký mã và tải symbol chỉ chạy khi có thông tin xác thực tương ứng.
`script/bundle-mac` ký khi có `MACOS_CERTIFICATE` và các khoá notarization,
lúc đó cần thêm `MACOS_SIGNING_IDENTITY` là common name của chứng chỉ.
`script/bundle-linux` và `script/bundle-windows.ps1` chỉ tải debug symbol
khi đặt cả `SENTRY_AUTH_TOKEN` lẫn `SENTRY_ORG`.

### Phát hành và ký một bộ driver

Job release phát hành bằng
`script/publish-drivers-to-web [--force] <directory> <version>`, trong đó
thư mục chứa các archive `zode-db-*.tar.gz` và
`zode-db-drivers-manifest.json`. Script ký trước, nên khóa thiếu hoặc sai
không tốn gì. Sau đó nó tải lên các archive, tải lên manifest đã ký,
phát hành manifest sau cùng, và cuối cùng kiểm tra `latest` nêu đúng phiên bản
và backend phục vụ manifest đã ký từng byte một đúng như lúc ký.
Hai bản manifest được đưa ra: bản thường, là thứ `latest` phục vụ
và là tất cả những gì ứng dụng cũ đọc, và bản đã ký, mang giao thức của
`protocol` từng driver và là bản ứng dụng làm theo.

Khóa lấy từ môi trường, và cần một trong hai biến:

| Biến                           | Chứa                                                                                                     |
| ------------------------------ | -------------------------------------------------------------------------------------------------------- |
| `ZODE_DRIVER_SIGNING_KEY`      | Base64 của file khóa riêng PKCS#8, trên một dòng. Đây là secret của CI.                                  |
| `ZODE_DRIVER_SIGNING_KEY_FILE` | Đường dẫn của file PKCS#8 đó, cho người phát hành từ máy đang giữ nó. Nó thắng khi cả hai cùng được đặt. |

Script từ chối phiên bản cũ hơn phiên bản đang phát hành, vì
một lần phát hành thay thế bộ trước cho mọi người, và máy đã
cài bộ mới hơn dù sao cũng sẽ từ chối bộ cũ. Truyền `--force` khi
mục đích chính là quay lui.

`cargo xtask driver-manifest` là thứ script gọi:

- `generate-key --out <file>` ghi một khóa Ed25519 mới, từ chối ghi đè
  file có sẵn, và in nửa công khai để nhúng thành `DRIVER_MANIFEST_PUBLIC_KEY` trong
  `crates/database/src/signature.rs`.
- `sign --manifest <file> --out <file> [--key-file <file>]` ghi manifest
  đã ký. Nó từ chối khóa có nửa công khai không phải khóa được tích hợp trong
  checkout của bạn, vì ứng dụng sẽ từ chối những gì nó ký.
- `check-key [--key-file <file>]` thất bại trừ khi khóa ký có mặt và nửa
  công khai của nó là khóa được tích hợp trong checkout của bạn. Nó không ký gì.
- `compare-versions <version> <other>` in `older`, `same` hoặc `newer`, theo
  semver, nên `0.1.5-rc.1` cũ hơn `0.1.5`.

Trong CI, workflow release không đợi đến cuối mới phát hiện khóa
sai. Một job `check_driver_signing_key` chạy sớm build `xtask` và chạy
`xtask driver-manifest check-key` với `ZODE_DRIVER_SIGNING_KEY`, và job
tạo bản release nháp cần nó, nên secret thiếu hoặc không khớp sẽ dừng
lượt chạy trước khi tạo hay tải lên bất cứ thứ gì. Job tải asset build
`xtask` trong một bước riêng không giữ secret nào, và đưa binary đó cho
`script/publish-drivers-to-web` dưới dạng `ZODE_XTASK_BIN`, nên khóa ký và
token tải lên không bao giờ nằm trong môi trường khi cây mã đang biên dịch. Asset release
được tải lên bằng `gh release upload --clobber`, nên chạy lại job sau một
lần lỗi một phần sẽ thay thế những gì đã có.

Ứng dụng chỉ tin khóa mà nó được build cùng, nên xoay khóa cần một bản
release ứng dụng. Giữ khóa riêng ngoài repository và ngoài log. Công cụ
không bao giờ in nó ra.

Driver đã tải nằm dưới `<data_dir>/database_drivers/<id>/<version>/`,
trong đó `<version>` là phiên bản manifest nêu — phiên bản của bộ driver
đã phát hành, không phải của ứng dụng. `data_dir` là:

- **macOS**: `~/Library/Application Support/Zode`
- **Linux**: `$XDG_DATA_HOME/zode` (typically `~/.local/share/zode`)
- **Windows**: `%LOCALAPPDATA%\Zode`

Driver đã tải được giữ cho tới khi bạn gỡ: Zode không bao giờ kiểm tra bộ mới hơn,
và chỉ tải manifest khi bạn yêu cầu tải. Nếu có nhiều hơn
một thư mục phiên bản (thư mục cũ không xóa được khi
driver của nó đang chạy), bản cài gần nhất thắng. Driver có dùng được với ứng dụng hay không được quyết định lúc bạn kết nối, bằng
handshake giao thức. Driver nói giao thức khác với bản build này bị
từ chối, và thông báo bảo gỡ nó trong hộp thoại kết nối rồi tải
lại, hoặc cập nhật Zode nếu cách đó không giải quyết được.

Đây cũng là đường cài thủ công/offline: không có đường tới `api.zodekit.com`
vẫn dùng được bằng cách đặt binary tại
`<data_dir>/database_drivers/<id>/<version>/zode-db-<id>` (thêm `.exe` trên
Windows). `<version>` có thể là tên thư mục bất kỳ có dạng
`MAJOR.MINOR.PATCH[-pre]`, ví dụ `0.0.0`. Zode không chạy driver từ
thư mục có tên khác. Lần tải tiếp theo của driver đó xóa mọi
thư mục phiên bản khác dưới `<id>/`, kể cả cái bạn đặt tay, và
dọn các phần còn sót của lần tải bị gián đoạn khi chúng đã quá một giờ tuổi.

Driver đặt tay vẫn được tìm thấy và dùng như bất kỳ driver nào, và cũng được đề nghị
gỡ. Mỗi lần cài Zode thực hiện đều ghi một marker `.zode-installed` cạnh
file thực thi, và chỉ driver có marker đó mới được biết là do Zode
tải, và vì vậy quay lại được chỉ bằng một cú bấm. Driver không có marker có thể
do đặt tay, hoặc do một bản Zode cũ hơn tải mà không ghi marker; Zode
không phân biệt được.

Nút đó là biểu tượng thùng rác trên dòng engine của hộp thoại kết nối thuộc bảng cơ sở dữ liệu.
Nó xuất hiện với driver trong kho tải về, không phải với driver cạnh
file thực thi, vốn thuộc về checkout hoặc bundle, cũng không phải với driver do
`ZODE_DB_<ID>` chỉ tới, vốn thuộc về người đã chỉ. Chọn nó sẽ hỏi trước. Với
driver Zode đã tải, tooltip ghi "Remove the downloaded driver" và
câu hỏi là "Remove the downloaded driver?". Với driver không có marker, tooltip
ghi "Remove the driver", và câu hỏi nêu thư mục nó sẽ dọn trống,
`<data_dir>/database_drivers/<id>/`, và nói rằng không tải lại được driver
nếu bạn tự đặt. Dù thế nào, **Remove** xóa thư mục đó, mọi
phiên bản trong đó, và lần kết nối tiếp theo tới engine đó lại được đề nghị tải
lại. Các kết nối đang mở vẫn hoạt động cho tới khi đóng.

Driver không tìm thấy sẽ không bao giờ được khởi chạy. Kết nối tới nó thất bại ngay
với ``the `postgres` driver is not installed -- download it from the connection
dialog, or in a development build run `script/build-database-drivers` ``, và
engine hiện "Not Downloaded" trong hộp thoại (hoặc "No Driver" với engine
mà Zode không phát hành gì). Zode cũng ghi một dòng info log,
``database driver `zode-db-postgres` is not installed``, nêu nơi nó
đã tìm cạnh file thực thi.

Lưu ý lưu mật khẩu của kết nối dùng keychain, nên với bản build phát triển
xem phần bên dưới.

## Truy cập keychain

Zode lưu secret trong keychain của hệ thống.

Tuy nhiên, khi chạy bản build phát triển của Zode trên macOS (và có thể các nền tảng
khác), việc truy cập keychain gây ra rất nhiều lời nhắc keychain
yêu cầu nhập mật khẩu lặp đi lặp lại.

Trên macOS điều này do bản build phát triển không có danh tính ổn định.
Dù bạn chọn "Always Allow", hệ điều hành vẫn hỏi lại
mật khẩu lần sau khi binary có gì thay đổi.

Việc này nhanh chóng gây khó chịu và cản trở tốc độ phát triển.

Vì vậy, mặc định, khi chạy bản build phát triển của Zode, một nhà cung cấp
credential thay thế được dùng để bỏ qua keychain hệ thống.

> **Lưu ý:** Điều này **chỉ** áp dụng cho bản build phát triển. Với mọi
> kênh release không phải phát triển, luôn dùng keychain hệ thống.

Nếu cần thử một thứ bằng keychain hệ thống thật trong
bản build phát triển, chạy Zode với biến môi trường sau được đặt:

```
ZODE_DEVELOPMENT_USE_KEYCHAIN=1
```

## Đo hiệu năng

Zode có hệ thống đo thời gian khung hình, dùng để profile mỗi khung hình dựng mất bao lâu. Điều này đặc biệt hữu ích khi so hiệu năng dựng giữa các phiên bản hoặc khi tối ưu code dựng khung hình.

### Dùng ZODE_MEASUREMENTS

Để bật đo hiệu năng, đặt biến môi trường `ZODE_MEASUREMENTS`:

```sh
export ZODE_MEASUREMENTS=1
```

Khi bật, Zode sẽ in thông tin thời gian dựng khung hình ra stderr, cho biết mỗi khung hình dựng mất bao lâu.

### Quy trình so sánh hiệu năng

Đây là quy trình điển hình để so hiệu năng dựng khung hình giữa các phiên bản:

1. **Bật đo:**

   ```sh
   export ZODE_MEASUREMENTS=1
   ```

2. **Test phiên bản đầu:**

   - Checkout commit bạn muốn đo
   - Chạy Zode ở chế độ release và dùng trong 5-10 giây: `cargo run --release &> version-a`

3. **Test phiên bản thứ hai:**

   - Checkout commit khác bạn muốn so sánh
   - Chạy Zode ở chế độ release và dùng trong 5-10 giây: `cargo run --release &> version-b`

4. **Tạo bản so sánh:**

   ```sh
   script/histogram version-a version-b
   ```

Công cụ `script/histogram` nhận bao nhiêu file đo cũng được và sẽ tạo biểu đồ histogram so sánh dữ liệu hiệu năng dựng khung hình giữa các phiên bản đã cho.

### Dùng `util_macros::perf`

Để benchmark unit test, gắn thuộc tính `#[perf]` từ crate `util_macros` vào chúng. Rồi chạy `cargo
perf-test -p $CRATE` to benchmark them. See the rustdoc documentation on `crates/util_macros` and `tooling/perf` for
(tức là xem tài liệu rustdoc của hai đường dẫn này) để có ví dụ và giải thích chi tiết.

## Profile ETW trên Windows

Zode hỗ trợ profile hiệu năng bằng Event Tracing for Windows (ETW) để thu dữ liệu hiệu năng chi tiết, gồm hoạt động CPU, GPU, bộ nhớ, đĩa và I/O file. Dữ liệu được lưu vào file `.etl`, mở được bằng các công cụ profile chuẩn để phân tích.

Bản ghi ETW có thể chứa thông tin định danh cá nhân hoặc nhạy cảm về bảo mật, như đường dẫn file và khóa registry được truy cập, cũng như tên tiến trình. Hãy lưu ý điều này khi chia sẻ trace với người khác.

### Ghi một trace

Mở bảng lệnh (command palette) và chạy một trong các lệnh sau:

- `zode: record etw trace`: ghi hoạt động CPU, GPU, bộ nhớ và I/O
- `zode: record etw trace with heap tracing`: gồm cả dữ liệu cấp phát heap của tiến trình Zode

Zode sẽ nhắc bạn chọn nơi lưu file `.etl`, rồi xin quyền administrator. Khi được cấp, việc ghi bắt đầu.

### Lưu hoặc hủy

Trong khi một trace đang ghi, mở bảng lệnh và chạy một trong các lệnh sau:

- `zode: save etw trace`: dừng ghi và lưu trace xuống đĩa
- `zode: cancel etw trace`: dừng ghi mà không lưu

Bản ghi tự lưu sau 60 giây nếu không dừng thủ công.

## Liên kết cho người đóng góp

- [CONTRIBUTING.md](https://github.com/tgiap04/zode/blob/develop/CONTRIBUTING.md)
- [Gỡ lỗi crash](./development/debugging-crashes.md)
- [Quy tắc ứng xử](https://github.com/tgiap04/zode/blob/develop/CODE_OF_CONDUCT.md)
- [Đóng góp được cấp phép theo giấy phép của kho mã](https://github.com/tgiap04/zode)
