# Chia sẻ nhóm — điều gì có thể và không thể xảy ra

Một nhóm dùng gói Team của Zode có thể chia sẻ file `.env` và kết nối cơ sở dữ liệu. Máy chủ
lưu chúng không phải mã nguồn mở. Trang này nói máy chủ thấy được gì,
không thấy được gì, và chỗ nào thiết kế yếu hơn vẻ ngoài. Định dạng truyền tải
nằm ở [Giao thức chia sẻ nhóm](./team-sharing-protocol.md), kèm các vector cố định
mà bộ test của chính Zode kiểm tra.

## Tóm tắt

|                                                                                  |                                                                                                  |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Máy chủ có đọc được `.env` hay kết nối đã chia sẻ không?                         | Không. Mục được niêm phong trên máy của thành viên bằng khóa nhóm mà máy chủ không bao giờ nhận. |
| Nó có phân biệt được mục nào là `.env`, mục nào là kết nối không?                | Không. Nó chỉ thấy id 32 ký tự mờ đục, độ dài làm tròn lên 4 KiB, và ai ghi mục nào lúc nào.     |
| Nó có đọc được mật khẩu cơ sở dữ liệu đã chia sẻ không?                          | Không. Và mật khẩu chỉ được chia sẻ khi người chia sẻ đánh dấu "Include password".               |
| Nó có lén thêm khóa riêng của mình vào nhóm được không?                          | Không, khi máy này đã mở nhóm. Xem [Fingerprint](#fingerprints).                                 |
| Xóa một người có chặn họ đọc mục mới không?                                      | Quyền truy cập của họ chấm dứt ngay; chỉ xoay khóa mới chặn được bản sao khóa cũ mở dữ liệu mới. |
| Zode có khôi phục được dữ liệu nhóm nếu mọi thành viên mất quyền truy cập không? | **Không.** Không ai làm được.                                                                    |

## Máy chủ thấy gì

| Thấy                                                                 | Không thấy                                          |
| -------------------------------------------------------------------- | --------------------------------------------------- |
| User id và team id của bạn                                           | Bất kỳ khóa nhóm nào, hay khóa riêng chia sẻ của ai |
| Khóa **công khai** chia sẻ của từng thành viên và fingerprint của nó | Tên dự án, tên file hay đường dẫn nào               |
| Một bản sao niêm phong của khóa nhóm cho mỗi thành viên và phiên bản | Tên hay giá trị biến nào                            |
| Id mục mờ đục, độ dài mục làm tròn lên 4 KiB                         | Chuỗi kết nối hay mật khẩu nào                      |
| Phiên bản khóa nào niêm phong mỗi mục, ai ghi, lúc nào               | Mục nào là `.env`, mục nào là kết nối               |

Khóa riêng chia sẻ của bạn cũng được lưu trên máy chủ, tại `sync/sharing-key`,
niêm phong bằng khóa khôi phục của bạn, giống hệt khóa môi trường. Máy chủ
giữ bản mã chứ không giữ khóa khôi phục, nên không mở được nó.

Không có gì được gửi lúc khởi động và không có gì gửi theo giờ. Mọi yêu cầu đều xuất phát từ
một việc bạn làm: mở tab Team của kho môi trường, bấm
tải lại ở nhóm Team của bảng cơ sở dữ liệu, mở một kết nối nhóm ở đó,
mở cửa sổ thành viên, pull, chia sẻ, cấp quyền, xóa hoặc xoay khóa. Danh sách đầy đủ các yêu cầu nằm
ở trang giao thức.

## Fingerprint

Điều duy nhất mật mã học tự nó không chặn được ở đây là máy chủ **thay thế**
một khóa: đưa admin khóa công khai do nó nắm thay vì của thành viên, để
lần cấp quyền tiếp theo bị niêm phong cho máy chủ; hoặc niêm phong cho bạn một khóa nhóm do nó tự chọn,
nhân danh một thành viên mà nó liệt kê. Zode đối phó bằng
fingerprint để chính bạn so sánh, và nói thẳng điểm xuất phát nằm ở đâu:

- Cửa sổ thành viên hiện fingerprint khóa của mọi thành viên dưới dạng tám nhóm
  bốn chữ số hex.
- Một khóa chỉ được **xác minh** trên máy này sau khi ai đó ở đây bấm
  **Trust Key…**, hộp thoại này hiện lại fingerprint và yêu cầu bạn so sánh
  với người đó trước — trực tiếp, hoặc qua một kênh mà máy chủ của Zode không
  đi qua. Mọi khóa khác hiện là **chưa được xác minh trên máy này**.
- Không có gì được niêm phong cho một khóa chưa xác minh. Admin không thể cấp
  khóa nhóm cho thành viên, và việc xoay khóa không tiến hành, cho tới khi khóa của từng người nhận
  được xác minh ở đây. Cả hai không bao giờ tự xác minh một khóa.
- Khóa nhóm bạn nhận chỉ được mở nếu thành viên niêm phong nó có một khóa
  mà máy này đã biết. Thành viên chưa từng thấy — admin mới, hoặc một
  "thành viên" do máy chủ bịa ra — sẽ bị từ chối, và tab Team nói ai đã niêm phong
  khóa rồi yêu cầu bạn so sánh fingerprint của họ trong cửa sổ thành viên.
  Tin cậy khóa ở đó thì khóa được mở.
- Khóa khác với khóa đã biết trước đó được hiện như một cảnh báo, kèm
  cả hai fingerprint. Không có gì được niêm phong cho nó và không có gì nó niêm phong được
  mở cho tới khi bạn bấm **Accept New Key…**, từng thành viên một. Máy mới
  hoặc cài lại làm khóa đổi một cách hợp lệ; việc thay thế cũng làm khóa
  đổi, vì vậy việc so sánh là do bạn làm.
- Máy chủ giữ một bản sao niêm phong của khóa nhóm cho mỗi thành viên và phiên bản. Nếu
  nó đưa bạn hai bản cho cùng một phiên bản, cả hai đều không được dùng.

**Lần đầu tiên là rủi ro còn lại.** Một máy mở nhóm lần
đầu tiên chưa biết ai, nên nó lấy khóa của người đã niêm phong khóa nhóm của bạn
từ danh sách thành viên của máy chủ, và ghi nhớ là _đã thấy_. Khóa đã thấy
mở được phong bì; nó vẫn hiện là **chưa được xác minh**, và không có gì được niêm phong
cho nó cho tới khi bạn tin cậy. Việc thay thế xảy ra trước lần xem đầu tiên đó
máy không bắt được; sau đó thì bắt được. So sánh fingerprint của người niêm phong
trong cửa sổ thành viên sau lần mở đầu tiên để đóng khoảng hở này.

**Tin cậy tính theo từng máy.** Điều bạn đã xác minh trên laptop thì máy
để bàn không biết: trên máy thứ hai, mọi thành viên lại bắt đầu ở trạng thái chưa xác minh, và
bạn phải so sánh fingerprint lần nữa trước khi cấp quyền từ máy đó.

## Khóa chia sẻ của riêng bạn

Khóa chia sẻ của bạn được niêm phong bằng khóa khôi phục và đi theo nó tới mọi
máy nơi bạn nhập. Nếu khóa chia sẻ đã lưu không mở được bằng khóa
khôi phục trên máy này — nó được niêm phong bằng khóa khôi phục mà bạn không còn
nữa — tab Team sẽ báo. Trước hết thử nhập khóa khôi phục bạn
dùng ở nơi khác. Nếu không khóa nào mở được, **Replace My Sharing Key…** tạo khóa mới.
Việc đó không hoàn tác được: mọi khóa nhóm niêm phong cho khóa cũ bị xóa, ở
mọi nhóm bạn tham gia, và admin của từng nhóm phải so sánh fingerprint mới của bạn
rồi cấp quyền lại.

## Xóa thành viên và xoay khóa

Xóa một người sẽ xóa các bản sao niêm phong khóa nhóm của họ trên máy chủ và
cắt quyền truy cập kho của họ ngay lập tức. Những gì nó không làm được:

- lấy lại những gì họ đã pull về — file trên đĩa của họ, mật khẩu trong
  keychain;
- lấy lại khóa nhóm cũ nếu trình soạn thảo của họ vẫn giữ nó trong bộ nhớ.

Đó là lý do có việc xoay khóa. Sau khi ai đó rời đi, máy chủ gắn cờ nhóm
và cửa sổ thành viên làm nổi bật **Rotate Team Key…** cho admin. Xoay khóa
tạo một phiên bản khóa mới, niêm phong cho mọi thành viên còn lại có
khóa chia sẻ, và niêm phong lại mọi mục dưới khóa đó. Mỗi khóa trong số này phải được xác minh
trên máy này: khóa nào chưa xác minh sẽ chặn việc xoay trước khi gửi bất cứ gì,
nên xoay khóa không bao giờ là lúc niềm tin bắt đầu. Bản sao của khóa cũ khi đó
không mở được gì viết sau đó. Nếu việc xoay bị gián đoạn, **Finish
Re-sealing** hoàn tất nó mà không tạo thêm phiên bản. Cờ này do máy chủ giữ:
nếu có người rời đi trong khi
việc xoay của admin khác hoàn tất, cờ vẫn được dựng và cửa sổ yêu cầu bạn xoay lại.

Hãy đổi chính các bí mật — mật khẩu cơ sở dữ liệu, khóa API — nếu người đã
rời đi không còn nên có chúng. Không cơ chế chia sẻ nào
thu hồi được một giá trị mà người ta đã nhìn thấy.

## Gói đã hết hạn

Khi gói của nhóm kết thúc, kho chuyển sang chỉ đọc: thành viên vẫn pull được
những gì đã chia sẻ, và không thể chia sẻ gì mới cho tới khi gói hoạt động lại.

## Chỗ yếu hơn bạn có thể nghĩ

**Mọi thành viên nắm khóa nhóm đều đọc được mọi mục.** Không có
quyền theo từng mục trong một nhóm. Chỉ chia sẻ `.env` production cho nhóm
nếu mọi người trong đó đều nên có nó.

**Admin được tin giao khóa nhóm.** Admin có thể cấp khóa cho bất kỳ ai trong
nhóm sau khi kiểm tra fingerprint của họ. Máy chủ ép buộc ai là admin; tài khoản
admin bị xâm phạm là cả nhóm bị xâm phạm.

**Metadata nhìn thấy được.** Máy chủ biết ai trong nhóm, nhóm có
bao nhiêu mục, mỗi mục lớn cỡ nào, và ai ghi gì lúc nào.

**File đã pull là file bình thường.** `.env` đã pull được ghi với quyền `0600` trên macOS
và Linux, kèm bản sao lưu của file bị thay đặt ngoài dự án. Bất kỳ thứ gì
đọc được file của bạn đều đọc được nó, như với đồng bộ môi trường — xem
[Bảo mật đồng bộ môi trường](./env-sync-security.md).

**Ai chia sẻ file thì quyết định nó đáp xuống đâu.** Đường dẫn mà pull ghi vào
là một phần của mục được chia sẻ. File `.env` hoặc `.env.*` được ghi ngay
(có diff trước nếu đã có file ở đó). Mọi đường dẫn khác — một cài đặt trình soạn thảo,
file task, bất cứ thứ gì trong `.git` — có thể thay đổi những gì công cụ của bạn
chạy, nên Zode hỏi trước khi ghi, kể cả khi file mới.
