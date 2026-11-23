# Điều khiển từ xa

Điều khiển từ xa cho phép bạn xem và gõ vào các agent và terminal đang chạy trong
một Zode (gọi là **A**) từ thiết bị khác: trình duyệt tại
`zodekit.com/remote`, hoặc Zode trên máy khác. Hai thiết bị nói chuyện qua
relay zodekit.com, và mọi thứ chúng trao đổi đều được mã hoá đầu cuối;
relay chỉ chuyển tiếp các byte mà nó không đọc được. Về những gì cơ chế này bảo vệ và không bảo vệ,
xem [Bảo mật điều khiển từ xa](./remote-control-security.md).

Tính năng này **tắt theo mặc định**. Khi tắt, Zode không mở kết nối nào cho nó và
không theo dõi terminal nào.

- [Bật lên](#turn-it-on)
- [Ghép nối thiết bị](#pair-a-device)
- [Điều khiển A từ trình duyệt](#control-a-from-a-browser)
- [Điều khiển A từ một Zode khác](#control-a-from-another-zode)
- [Khi bạn đang bị điều khiển](#while-you-are-being-controlled)
- [Thu hồi thiết bị](#revoke-a-device)
- [Giới hạn](#limits)
- [Những gì không được cung cấp](#what-is-not-served)
- [Xử lý sự cố](#troubleshooting)

## Bật lên

Bạn phải đăng nhập tài khoản Zode. Sau đó bấm nút tài khoản ở
cuối thanh bên và chọn **Remote Control**, mở Settings và
bật **Allow Remote Control**, hoặc sửa file cài đặt:

```json [settings]
{
  "remote_control": {
    "enabled": true,
    "idle_timeout_minutes": 30
  }
}
```

`idle_timeout_minutes` là khoảng thời gian một thiết bị đã kết nối được phép không dùng trước khi
bị ngắt; `0` tắt giới hạn này. Gõ trong terminal, duyệt file,
liệt kê hoặc tiếp tục phiên, và các yêu cầu từ dự án mở từ một
Zode khác đều tính là đang dùng. Ping thì không, heartbeat của chính project client cũng không,
nên dự án bị bỏ mở một mình không giữ kết nối sống. Cả hai
khoá được mô tả trong [Tất cả cài đặt](./reference/all-settings.md#remote-control).

Tắt lại sẽ ngắt mọi thiết bị cùng lúc và ngừng lắng nghe.
Thoát Zode cũng làm vậy, và giữ nguyên cài đặt như cũ.

## Ghép nối thiết bị

Một thiết bị phải được duyệt một lần trên A trước khi điều khiển được. Ghép nối giúp
mỗi bên biết khoá của bên kia và buộc hai người ở hai màn hình kiểm tra
rằng không có ai chen vào giữa.

1. Trên thiết bị điều khiển, chọn Zode để ghép nối. Trong trình duyệt, mở
   `zodekit.com/remote`, chọn Zode từ danh sách (nó phải đang online, đã bật
   điều khiển từ xa và đăng nhập cùng tài khoản) rồi chọn **Pair**.
2. A hiện một lời nhắc: "wants to control this Zode", kèm sáu chữ số. Nó tự
   mở trong cửa sổ Zode đang ở trước. Nếu A đang ở nền, một thông báo hệ thống
   nhắc bạn chuyển sang Zode; bấm vào là lời nhắc mở ra. Trong lúc thiết bị chờ,
   nút **Pairing Request** ở thanh trạng thái của A, và lệnh
   `remote_control: review pairing request`, mở nó từ bất kỳ cửa sổ nào.
3. Thiết bị điều khiển cũng hiện sáu chữ số. **Nếu giống nhau**, chọn
   **The digits match** trong trình duyệt, rồi **The Digits Match: Trust It** trên A.
   Nếu khác nhau, chọn **Digits differ** (trình duyệt) hoặc **Reject** trên A.
4. Cả hai bên hoàn tất kết nối. Thiết bị giờ đã được tin cậy.

Chỉ xác nhận trên A khi chính bạn đã bắt đầu việc ghép nối. Tin cậy một thiết bị
là trao cho nó một shell trên máy của bạn, như [trang bảo mật](./remote-control-security.md#a-paired-device-has-a-shell-on-a)
giải thích.

Ghép nối có giới hạn: A nhận một lần ghép nối mỗi lúc, một yêu cầu hết hạn sau 120
giây, nó hỏi bạn tối đa ba lần mỗi giờ, trả lời tối đa mười hai yêu cầu
mỗi giờ (ba từ một thiết bị bất kỳ) kể cả khi nó không hỏi bạn, và ba lần
không khớp sẽ khoá ghép nối cho tới khi bạn mở khoá. Khi A từ chối một yêu cầu vì một trong các
lý do đó, hoặc vì nó đã tin cậy 32 thiết bị, nó sẽ báo cho bạn. Xem
[Xử lý sự cố](#troubleshooting).

Nếu thiết bị đã được tin cậy, lời nhắc sẽ nói rằng tin cậy lại sẽ thay thế những gì
đã lưu cho nó. Khi khoá của nó là mới, việc xác nhận cũng kết thúc các phiên nó có
dưới khoá cũ.

## Điều khiển A từ trình duyệt

Mở `zodekit.com/remote`, đăng nhập, chọn một Zode đã ghép nối và chọn **Connect**.

| Bạn nhận được          | Ghi chú                                                                                    |
| ---------------------- | ------------------------------------------------------------------------------------------ |
| **Agents**             | Các agent và terminal đang mở trên A, kèm trạng thái (đang chạy, chờ duyệt, rảnh, ...).    |
| **History**            | Các phiên agent trước đây của những dự án đang mở trên A. Xem [History](#history).         |
| Terminal phản chiếu    | Mở một cái để xem màn hình trực tiếp và gõ vào. Lưới của nó theo terminal của máy chủ.     |
| **Files**, **Changes** | Các tab chỉ đọc: duyệt các thư mục đang mở, đọc file, xem diff chưa commit.                |
| Thanh phím di động     | Enter, Escape, Tab, Up, Down, 1, 2, 3 và Control C, cùng một ô dòng bạn gửi bằng **Send**. |

Terminal tự chỉnh cỡ chữ để cả màn hình vừa khung, và theo
giao diện Light hoặc Dark của trang. Trên màn hình hẹp như điện thoại, terminal đang mở
phủ kín màn hình như một tờ giấy với thanh phím bên dưới; **Back to agents** đóng
nó lại.

Không có gì trong các tab Files và Changes ghi hay chạy bất cứ thứ gì. Giới hạn về những gì
chúng hiển thị được [liệt kê bên dưới](#what-is-not-served).

### Lịch sử

Tab **History** liệt kê các phiên agent trước đây của những dự án đang mở trên A,
mới nhất trước, nhóm theo agent và thư mục. Mỗi dòng cho thấy tiêu đề phiên,
lần dùng gần nhất và nhánh của nó, nếu có. Tab chỉ hiện khi A
hỗ trợ; với Zode cũ hơn thì không có và không có gì khác thay đổi.
**Show older sessions** nạp trang kế tiếp.

| Nút        | Tác dụng                                                                                                                                        |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **Resume** | Tiếp tục phiên trong một tab mới trên A, và mở terminal của tab đó trong trình duyệt.                                                           |
| **Open**   | Thay cho Resume khi phiên đã đang chạy trong một tab trên A. Nó đưa bạn tới tab đó và không khởi chạy gì.                                       |
| **Fork**   | Rẽ nhánh cuộc hội thoại thành một phiên mới, trong một tab mới, và mở nó. Có cho Claude Code và opencode. Codex và Copilot không rẽ nhánh được. |

- Resume và Fork khởi chạy một agent trên A, như thể bạn chọn phiên đó trong
  bảng lịch sử ở đó. A hiện một [toast](#while-you-are-being-controlled) cho biết
  thiết bị nào đã làm.
- A từ chối Resume hoặc Fork từ trình duyệt của bạn trong 3 giây sau khi lần trước
  đã mở một tab, và trang yêu cầu bạn thử lại.
- Một phiên chỉ xuất hiện nếu nó đã chạy trong thư mục đang mở trên A, và không nằm dưới đường dẫn
  mà A giữ riêng tư hoặc ẩn khỏi lần quét. Xem
  [Những gì không được cung cấp](#what-is-not-served).

### Thông báo đẩy

Với **Pro** hoặc **Team**, trình duyệt đã ghép nối với A nhận được thông báo đẩy
khi một agent trên A cần bạn, kể cả khi trang đã đóng hoặc điện thoại đã khoá:

| Sự kiện           | Khi nào                                                                             |
| ----------------- | ----------------------------------------------------------------------------------- |
| Đang chờ bạn      | Một tab Claude Code xin bạn duyệt quyền.                                            |
| Đã trả lời xong   | Agent trả lời xong. Với agent không phải Claude Code, A chờ 60 giây yên lặng trước. |
| Phiên đã kết thúc | CLI của agent đã thoát.                                                             |

1. Mở `zodekit.com/remote`, kết nối tới A, rồi chọn **Bật thông báo**.
   Trình duyệt sẽ xin quyền.
2. Trên iPhone hoặc iPad (iOS 16.4 trở lên), trước hết thêm trang vào Màn hình
   chính (**Chia sẻ**, rồi **Thêm vào Màn hình chính**) và mở từ đó. Safari chỉ
   gửi thông báo đẩy cho trang được mở theo cách này.
3. Bấm vào thông báo là mở đúng tab agent đó.

- Thiết bị đang mở đúng tab agent đó thì không nhận thông báo.
- Chọn sự kiện nào được đẩy bằng `remote_control.push_notifications` trong
  settings (cả ba mặc định bật). Thông báo desktop của
  `agent_finished_notification` là phần riêng và miễn phí.
- A mã hoá từng thông báo bằng khoá chỉ trình duyệt đó giữ. Server
  zodekit.com và dịch vụ push của Apple hay Google chuyển tiếp mà không
  đọc được tên tab. Chúng vẫn biết lúc nào có thông báo được gửi.
- Ở gói Free, trang báo thông báo không nằm trong gói và có nút **Xem các gói**.

## Điều khiển A từ một Zode khác

Từ máy khác, trong Zode:

1. Bấm nút tài khoản ở cuối thanh bên và chọn
   **Open a Zode on Your Account…** (cũng có trong hộp thoại
   **Remote Projects**).
2. Chọn Zode kia. Nếu chưa ghép nối, chọn **Pair…**, so sánh
   sáu chữ số, và xác nhận trên cả hai máy — **The Digits Match** ở đây,
   **The Digits Match: Trust It** ở máy kia, hoặc **Digits Differ** ở đây nếu chúng
   không giống nhau; nếu không thì chọn **Connect**. Nếu khoá của một Zode đã đổi
   từ lúc bạn ghép nối, dòng của nó đưa ra **Pair Again** thay thế, lệnh này quên
   khoá cũ và bắt đầu ghép nối mới.
3. Chọn **Open a Project on** Zode đó.

Việc này mở một workspace độc lập: bạn sửa dự án như thể nó ở máy bạn,
và terminal của nó chạy trên A, không phải máy bạn. Đó là một cửa sổ riêng biệt với
mọi thứ khác bạn đang mở.

Terminal và agent của A mà bạn mở thành tab trong Zode này dùng chung một
kết nối tới A: mở lại hộp thoại sẽ dùng lại nó khi còn sống, thay vì
chiếm thêm một trong bốn chỗ của A mỗi lần. Kết nối giữ nguyên chừng nào
còn một trong các tab đó đang mở, kể cả khi hộp thoại đã đóng. Các tab này
không được lưu cùng bố cục của bạn, nên sau khi khởi động lại chúng không quay về như shell
cục bộ.

Mỗi cửa sổ mở một dự án từ A có kết nối riêng và project
server riêng trên A, nên hai cửa sổ trên cùng một Zode chạy song song. Một
project server sống đúng bằng thời gian kết nối của nó. Nếu kết nối đứt,
hãy mở lại dự án: Zode không nối bạn lại với một server mới, vì một
server mới sẽ không biết gì về những gì cửa sổ đang giữ. Nó coi project server
cũ là đã mất.

**Cả hai Zode phải chạy cùng phiên bản.** Thông điệp của project server chỉ
tương thích giữa các bản build giống hệt nhau, nên nếu phiên bản khác nhau, Zode từ chối và
yêu cầu bạn cập nhật cả hai.

## Khi bạn đang bị điều khiển

Khi điều khiển từ xa bật và không ai kết nối, thanh trạng thái hiện một
biểu tượng màn hình nhỏ: xám khi sẵn sàng, vàng khi đang kết nối lại hoặc chưa ai
đăng nhập, đỏ khi đã dừng. Rê chuột lên để xem lý do; bấm để tắt
điều khiển từ xa hoặc quản lý thiết bị tin cậy.

- Thanh trạng thái hiện **Controlled by** tên thiết bị (hoặc "N devices"). Nó
  vẫn hiện kể cả khi `status_bar.show` ẩn thanh trạng thái, và một toast
  thông báo mỗi thiết bị nắm quyền điều khiển.
- Khi một thiết bị tiếp tục hoặc rẽ nhánh một phiên cũ, một toast nêu tên thiết bị và
  phiên, ví dụ `My phone resumed “Fix the build”`. Nó hiện ở nơi
  tab mở, và cả ở workspace phía trước của cửa sổ khi đó là một
  workspace khác, rồi biến mất sau năm giây. Mở một phiên đang
  chạy thì không có toast.
- Bấm chỉ báo để **Disconnect All Devices**, **Turn Off Remote
  Control**, hoặc **Manage Trusted Devices…**.
- Các việc đó cũng có dưới dạng action trong bảng lệnh (command palette)
  `remote_control::DisconnectAll`, `remote_control::Disable` và
  `remote_control::ManageDevices`. Menu tài khoản có **Manage Trusted
  Devices…** bất cứ lúc nào, kể cả khi điều khiển từ xa đang tắt, để bạn quên một
  thiết bị trước khi bật lại.
- Khi có thiết bị kết nối, Zode giữ màn hình luôn sáng, theo cùng điều kiện như
  [`keep_display_awake`](./reference/all-settings.md).

## Thu hồi thiết bị

| Ở đâu                                                                                   | Tác dụng                                                                                                                        |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Trên A: **Manage Trusted Devices…** rồi **Forget**                                      | Kết thúc phiên của thiết bị đó và xoá khoá của nó; nó không quay lại được nếu không ghép nối lại.                               |
| Trang thiết bị trên web, `zodekit.com/account/devices` (**Browsers**, rồi **Sign out**) | Gỡ trình duyệt đó khỏi tài khoản. Kết nối của A tới nó bị đóng và trình duyệt xoá khoá của mình.                                |
| Đăng xuất thiết bị khỏi tài khoản (**Sign out** trên cùng trang thiết bị)               | Relay đóng các kết nối của nó với lý do `device_revoked`; thiết bị tự xoá khoá và mọi ghép nối của mình, nên phải ghép nối lại. |

Nếu thiết bị bị mất hoặc bị đánh cắp, hãy thu hồi trước rồi hỏi sau.

Chỉ việc thu hồi mới xoá sạch một thiết bị, và relay báo điều đó bằng cách đóng các
kết nối với lý do `device_revoked`. Nếu dịch vụ tài khoản kết thúc
một phiên đăng nhập vì lý do khác, chẳng hạn refresh token bị dùng hai lần, relay
đóng kết nối với lý do `session_revoked`. Khi đó điều khiển từ xa
dừng lại và yêu cầu bạn đăng nhập lại, còn khoá và các ghép nối vẫn giữ.

## Giới hạn

| Giới hạn                                    | Giá trị                                                             |
| ------------------------------------------- | ------------------------------------------------------------------- |
| Zode online làm máy chủ, mỗi tài khoản      | 4                                                                   |
| Thiết bị điều khiển kết nối, mỗi tài khoản  | 8                                                                   |
| Phiên mở tới một máy chủ                    | 4 (thiết bị thứ năm bị từ chối, xem bên dưới)                       |
| Băng thông qua relay                        | Free: 5 GiB mỗi ngày cho mỗi tài khoản; Pro và Team: không giới hạn |
| Yêu cầu liệt kê file và phiên, mỗi thiết bị | 4 cùng lúc; vượt quá thì yêu cầu được trả lời `rate_limited`        |
| Tiếp tục phiên, mỗi thiết bị                | Một lần mỗi lúc, cách nhau 3 giây; sớm hơn là `rate_limited`        |
| Thông báo đẩy                               | Pro và Team; tối đa 10 mỗi phút và 60 mỗi giờ cho mỗi máy chủ       |
| Ngắt khi rảnh                               | `idle_timeout_minutes`, mặc định 30                                 |
| Terminal trong dự án từ một Zode khác       | 16 cùng lúc, tính cả những cái đang đóng                            |
| Project server đang dừng cùng lúc           | 4; dự án khác bị từ chối với `rate_limited`                         |
| Mở kết nối relay                            | Bỏ cuộc sau 20 giây, rồi thử lại với backoff                        |

Thiết bị thứ năm cố kết nối tới một Zode đã có bốn thiết bị kết nối sẽ bị
từ chối với "this Zode already has 4 devices connected; disconnect one to
connect another". Trong giao thức đây là một `error` với mã `too_large`,
theo sau là kết thúc phiên đó. Thiết bị chưa chứng minh được nó giữ
các khoá thì không chiếm một trong bốn chỗ.

Các con số của relay là mặc định và người vận hành relay có thể đổi. Khi
hết hạn mức, các kết nối đóng và kết nối mới bị từ chối cho tới khi hạn mức đặt lại lúc nửa đêm UTC.

## Những gì không được cung cấp

Duyệt là chỉ đọc và đi qua những gì A đã quét, không phải đĩa của nó.

- File bạn đã liệt kê trong `private_files`, file bị ẩn bởi
  `file_scan_exclusions`, và symlink không được cung cấp.
- Trong trình xem, file trên 1 MiB và file không phải văn bản UTF-8 không được hiển thị.
- Diff trên 2 MiB bị cắt ở ranh giới dòng và đánh dấu là bị cắt.
- Diff là của cây làm việc so với `HEAD` cho thư mục đang mở.
  File mới mà git chưa theo dõi có trong đó, tối đa 100, trừ khi chúng
  riêng tư, bị ẩn bởi `file_scan_exclusions` hoặc bị git bỏ qua. Nó cũng được đánh dấu
  bị cắt khi có hơn 100 file mới, một file không đọc được, hoặc
  các file mới không được bắt đầu hết trong 8 giây đầu. Một
  submodule đã đổi được hiển thị bằng một mục ngắn, commit cũ và mới của nó, không phải log
  các thay đổi của nó. Thư mục không phải repository git thì không có diff.
- Git có 20 giây để tạo diff. Nếu lâu hơn, yêu cầu thất bại. Diff
  không còn ai chờ nữa thì bị dừng. Đọc file mới là phần ít quan trọng nhất của diff,
  nên sau 8 giây không file nào nữa được bắt đầu và
  diff được đánh dấu là bị cắt.
- Danh sách thư mục hiện tối đa 2000 mục.
- History chỉ liệt kê các phiên đã chạy trong thư mục đang mở trên A. Phiên dưới
  đường dẫn trong `private_files` hoặc `file_scan_exclusions` không được liệt kê và không thể
  tiếp tục, phiên của các dự án không nằm trên đĩa của chính A cũng vậy,
  chẳng hạn dự án từ xa.

Quy tắc chính xác nằm ở [Giao thức điều khiển từ xa](./remote-control-protocol.md#files)
và, cho History, ở [Sessions](./remote-control-protocol.md#sessions).

## Xử lý sự cố

| Bạn thấy                                                             | Ý nghĩa và cách xử lý                                                                                                           |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| "This Zode is version X but Y runs Z. Update both Zodes..."          | Lệch phiên bản khi mở dự án. Cập nhật cả hai Zode lên cùng phiên bản.                                                           |
| "That Zode is not online right now" / Zode bị làm xám                | A đã tắt, đang ngủ, đã đăng xuất, hoặc tắt điều khiển từ xa. Hãy bật nó ở đó.                                                   |
| "This host's key is not the one expected..." / "Its key changed..."  | Khoá của thiết bị kia không phải khoá bạn đã ghép nối. Không có gì được mở. Nếu bạn đã cài lại nó, hãy quên nó và ghép nối lại. |
| "Zode did not start pairing..."                                      | A đang bận một lần ghép nối khác, ghép nối bị khoá, điều khiển từ xa đang tắt, hoặc nó đã hỏi bạn ba lần trong giờ này.         |
| "Pairing is locked after repeated mismatches"                        | Trên A: Manage Trusted Devices, rồi **Unlock Pairing**. Khoá không tự hết hạn.                                                  |
| "A device asked to pair, but Zode ..." (hiện trên A)                 | A từ chối yêu cầu mà không hiện các chữ số: đã dùng hết giới hạn ghép nối theo giờ, hoặc đã tin cậy 32 thiết bị.                |
| "The relay's limit for your account was reached" / "Zode is busy"    | Giới hạn tốc độ hoặc hạn mức. Đóng bớt tab hoặc thiết bị, chờ, rồi thử lại.                                                     |
| "earlier project servers are still being stopped; try again shortly" | A vẫn đang đóng các project server bạn đã mở rồi đóng. Chờ vài giây rồi mở lại dự án.                                           |
| "This browser was signed out of your account..."                     | Trình duyệt đã bị thu hồi. Khoá và các ghép nối của nó đã bị xoá; hãy ghép nối lại.                                             |
| "A session was just opened. Try again in a few seconds."             | A cho phép một lần Resume hoặc Fork mỗi thiết bị cứ 3 giây. Chờ một chút rồi thử lại.                                           |
| "That session is no longer on this Zode, or its folder is closed."   | Phiên đã bị xoá, thư mục của nó không còn, hoặc không cửa sổ nào trên A còn mở nó. Chọn phiên khác.                             |
| "Zode resumed the session, but its terminal has not appeared..."     | Tab đã được mở trên A nhưng terminal của nó không hiện kịp lúc. Nó sẽ được liệt kê trong **Agents** khi hiện.                   |
