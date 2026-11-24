# Tài khoản Zode của bạn

Đăng nhập là **tuỳ chọn**. Zode dùng đầy đủ khi chưa đăng nhập, và lúc đó nó
không gửi một request mạng nào — không lúc khởi động, không theo chu
kỳ, không để kiểm tra bất cứ thứ gì. Mọi thứ dưới đây chỉ xảy ra khi bạn yêu
cầu.

## Đăng nhập

Bấm biểu tượng hình người ở cuối thanh bên trái. Zode hiện một mã tám ký tự và
đề nghị mở trình duyệt với mã đã điền sẵn; bạn xác nhận ở đó, vài giây sau
trình soạn thảo nhận được phiên đăng nhập.

Nếu không có trình duyệt để mở — ví dụ phiên remote hoặc SSH — cửa
sổ sẽ in luôn địa chỉ và mã để bạn nhập từ một máy khác. Đó là lý do
Zode dùng cách này thay vì chuyển hướng (redirect): đây là cách duy
nhất chạy được ở cả hai nơi.

Phiên đăng nhập được lưu trong keychain của hệ điều hành (Keychain Access trên
macOS, libsecret trên Linux, Credential Manager trên Windows) và vẫn còn sau khi
khởi động lại.

## Đồng bộ cài đặt

**Account → Sync Settings…** trong menu của thanh bên. Ba thứ có thể đồng bộ:

|              | Thứ được chuyển                                                   |
| ------------ | ----------------------------------------------------------------- |
| Settings     | toàn bộ `settings.json` của bạn, kể cả comment và định dạng       |
| Key bindings | toàn bộ `keymap.json` của bạn                                     |
| Extensions   | danh sách ID extension đã cài — không bao giờ gồm chính extension |

Không có gì tự đồng bộ. Không có đối chiếu nền và không đồng bộ lúc khởi động;
mỗi lần chuyển là một lần bấm nút, và bạn được xem chính xác thứ sẽ thay đổi
trước khi có gì được ghi xuống.

### Recovery key của bạn

Cài đặt của bạn được mã hoá **ngay trên máy bạn**, trước khi rời máy. Máy chủ
chỉ lưu bản mã và không có khoá — nó không đọc được cài đặt của bạn, người nào
lấy được cơ sở dữ liệu của nó cũng không đọc được.

Điều đó khả thi chỉ vì bạn giữ khoá. Lần đồng bộ đầu tiên, Zode tạo một recovery
key và chỉ hiện cho bạn một lần:

```
ZODE-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXX
```

**Hãy ghi lại.** Mất khoá thì dữ liệu đã đồng bộ không khôi phục được.
Không phải khó khôi phục — mà là không khôi phục được. Không có yêu cầu hỗ
trợ nào hoàn tác được, vì phía chúng tôi không có gì để hoàn tác. Đó
là ý nghĩa của mã hoá đầu cuối, và cũng là lý do `settings.json` mới đồng bộ
được: file đó có thể chứa `language_models.*.api_key`, `terminal.env` và
`remote_env`.

Bạn xem lại khoá bất cứ lúc nào từ **Account → Show Recovery Key**.

Trên máy thứ hai, đăng nhập rồi chọn **Account → Enter Recovery Key…**.

### Khi cả hai phía đều đã thay đổi

Pull không bao giờ ghi đè file của bạn một cách âm thầm. Zode hiện phần khác
biệt và ba lựa chọn: **Keep This Machine's Copy**, **Take the Server's Copy**
hoặc **Cancel**.
Bạn chọn gì thì file bị thay thế cũng được chép sang `settings_backup.json`
(hoặc `keymap_backup.json`) trước.

Nếu hai máy push cùng lúc, máy thứ hai bị từ chối thay vì được phép phá công
sức của máy thứ nhất — bạn nhận cùng bản diff và cùng lựa chọn đó.

### Extension chỉ được liệt kê, không bao giờ được cài

Pull danh sách extension cho bạn thấy những gì đã cài ở nơi khác mà máy này còn
thiếu. Nó không cài gì cả. Cài là một nút riêng bạn bấm sau khi đọc danh sách;
một payload đồng bộ mà cài được mã sẽ là rủi ro chuỗi cung ứng, với chính tài
khoản của bạn làm chìa khoá.

## Quản lý các máy của bạn

**Account → Account on the Web** mở danh sách các trình soạn thảo đã đăng nhập
vào tài khoản của bạn, tại đó bạn đổi tên hoặc đăng xuất bất kỳ máy nào.

Trỏ trình soạn thảo sang backend khác bằng `ZODE_API_URL` thì liên kết này đi
theo. Nếu ứng dụng web không đơn giản là host API bỏ tiền tố `api.`, hãy đặt
thêm `ZODE_WEB_URL`.

Đăng xuất một thiết bị là kết thúc phiên của nó. Việc này **không** đổi
recovery key, nên một máy bạn không còn kiểm soát vẫn đọc được dữ
liệu đồng bộ nó đã tải về. Để cắt đường đó, dùng **Account → Rotate
Recovery Key…**, lệnh này thay khoá và mã hoá lại mọi thứ lưu dưới khoá đó.
Mọi máy khác sau đó sẽ hỏi khoá mới.

"Mọi thứ" gồm hai khoá được niêm phong dưới recovery key của bạn: khoá đồng bộ `.env`
(`env-key`) và khoá chia sẻ nhóm (`sharing-key`). Chúng được niêm phong lại nguyên
trạng, không bị thay, nên các file `.env` và mọi nhóm bạn tham gia
vẫn đọc được bằng recovery key mới. Mỗi mục chỉ được ghi lại nếu không ai khác
đã đổi nó kể từ lúc được đọc.

Thứ tự được chọn để không có gì bị bỏ rơi:

- Cả hai khoá, cũ và mới, được giữ trong keychain trước khi ghi lại bất cứ
  thứ gì, và khoá cũ chỉ bị quên khi mọi thứ đã nằm dưới
  khoá mới.
- Hai ô khoá đi trước, cùng nhau. Nếu một trong hai không ghi lại được
  (mạng rớt, máy khác ghi cùng lúc, hoặc dịch vụ từ chối), ô đã ghi lại
  được trả về dưới khoá cũ và máy này tiếp tục
  dùng khoá cũ.
- Chỉ khi cả hai ô khoá đã nằm dưới khoá mới thì nó mới thành recovery key của
  máy này. Cài đặt, keymap và danh sách extension theo sau.

Nếu dừng giữa chừng, cửa sổ cho biết còn lại gì, và hiện recovery key mới
để bạn ghi lại nếu nó đã được dùng. Chọn **Rotate Recovery Key…** lần
nữa sẽ tiếp tục với đúng khoá mới đó thay vì tạo khoá thứ ba, và hoàn
tất công việc.

## Gói của bạn

Đồng bộ, đồng bộ `.env` và remote control chạy trên dịch vụ do Zode host, và một
tài khoản dùng được bao nhiêu phụ thuộc vào gói của nó. Menu tài khoản ghi tên
gói — ví dụ _Plan: Free · 2 of 2 synced devices_ hoặc _Plan: Pro (renews 3 Dec)_ —
kèm **Upgrade…** hoặc **Manage Plan…** bên cạnh. Cả hai mở website trong trình
duyệt của bạn. Thanh toán diễn ra ở đó, qua Polar; trình soạn thảo không bao giờ
thấy thẻ hay khoản thanh toán.

Trình soạn thảo đọc gói từ `api.zodekit.com` chỉ ở bốn thời điểm: khi bạn đăng nhập,
một lần khi trình soạn thảo khởi động với phiên đăng nhập đã lưu, khi bạn mở menu tài
khoản hoặc cửa sổ đồng bộ, và ngay sau một request bị gói của bạn từ chối. Không bao giờ
theo chu kỳ, và không bao giờ trên máy chưa đăng nhập. Câu trả lời chứa bậc
gói, giới hạn của nó và tên nhóm của bạn, và chỉ được giữ trong bộ nhớ. Không có thông tin
nào về cách bạn dùng trình soạn thảo được gửi đi để lấy nó.

Khi gói không bao phủ một việc nào đó, cửa sổ nói rõ chuyện gì đã xảy ra và nên
làm gì thay vì báo lỗi bằng một mã:

- **Thiết bị thứ ba trên Free** không đồng bộ, và cửa sổ nói rõ như vậy. Thu hồi
  một thiết bị trên web, hoặc nâng cấp.
- **Đồng bộ `.env` trên Free** không tải lên gì. File đã lưu vẫn đọc
  và xoá được.
- **Remote control** trên Free tạm dừng sau 5 GiB qua relay trong một ngày. Nó
  đặt lại lúc 00:00 UTC; Pro và Team không giới hạn mỗi ngày.
- **Thông báo đẩy** của remote control có ở mọi gói, kể cả Free.
- **File dùng chung của nhóm** chỉ còn đọc khi gói của nhóm hết hạn. Thành viên
  vượt quá số seat nhóm trả tiền được nhắc nhờ chủ nhóm thêm seat.

Không có gì trong trình soạn thảo bị tắt vì gói. Máy chủ quyết định, nên gói bạn
vừa đổi một phút trước có hiệu lực ngay.

## Những gì không bao giờ được gửi

- Telemetry. Các crate tài khoản thậm chí không với tới được mã telemetry — điều
  này được ép theo đồ thị phụ thuộc trong CI, không để cho khâu review.
- Bất cứ thứ gì khi bạn chưa đăng nhập.
- Bất cứ thứ gì bạn chưa bấm nút để gửi.
