# Đồng bộ môi trường — điều gì có thể và không thể xảy ra

Zode có thể lưu các file `.env` của bạn để máy khác lấy về. Máy chủ
giữ chúng không phải mã nguồn mở. Trang này tồn tại vì lo ngại đó là hợp lý,
và vì "cứ tin chúng tôi" không phải là câu trả lời.

Mọi điều ở đây hoặc bạn tự kiểm tra được, hoặc có một test trong repository
này giữ. Chỗ nào một khẳng định yếu hơn vẻ ngoài của nó, trang sẽ nói rõ.

## Tóm tắt ngắn

|                                                                  |                                                                              |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| Máy chủ có đọc được file môi trường của bạn không?               | Không. Nó không có khoá và không bao giờ nhận khoá.                          |
| Nó có biết bạn có những dự án nào không?                         | Không. Nó chỉ thấy 32 ký tự thập lục phân ngẫu nhiên cho mỗi file.           |
| Nó có biết một file chứa bao nhiêu biến không?                   | Không. Mọi blob đều được đệm tới ranh giới 4 KiB.                            |
| Nó có thể trả lại cho bạn một file cũ không?                     | Nó có thể thử. Client phát hiện và không ghi gì.                             |
| Nó có thể đưa cho bạn file của người khác không?                 | Không. Bản mã gắn với tài khoản của bạn và định danh của file đó.            |
| Zode có khôi phục được dữ liệu nếu bạn mất khoá khôi phục không? | **Không.** Không ai làm được. Đó là điều phần còn lại của trang này đảm bảo. |

## Mô hình mối đe doạ, nói thẳng

**Coi là thù địch:** máy chủ, cơ sở dữ liệu, bản sao lưu, người vận hành nó,
bất kỳ ai có được những thứ đó, và bất kỳ ai trên mạng giữa bạn và nó.

**Coi là đáng tin:** máy của bạn, keychain của hệ điều hành, và binary
Zode bạn đang chạy. Nếu một trong số đó bị xâm phạm, tính năng này không
cứu được bạn. Không gì ở tầng đó cứu được, vì đó là nơi file được
đọc dạng văn bản thuần để sử dụng.

**Ngoài phạm vi:** người đã có sẵn laptop đang mở khoá của bạn. Giá trị bị che
và bước kiểm tra Touch ID tuỳ chọn chỉ làm việc nhìn trộm qua vai khó hơn. Chúng
không phải lớp phòng thủ trước việc truy cập kéo dài vào máy đã mở khoá, và
cũng không được quảng cáo như vậy.

## Những gì rời khỏi máy của bạn

Mỗi file một request, chứa một trường:

```json
{ "blob": "<base64 of an encrypted envelope>" }
```

Envelope, dữ liệu xác thực bổ sung (additional authenticated data), phần đệm và bộ đếm phiên bản
đều được đặc tả trong [Giao thức đồng bộ môi trường](./env-sync-protocol.md),
kèm các vector cố định mà bộ test của chính Zode dùng để đối chiếu tài liệu này.
Đổi một chữ số trên trang đó là bản build đỏ.

## Hai cách tự kiểm tra

**1. Xem các byte trước khi chúng đi.** Zode hiển thị đúng phần thân request trước
khi push. Đó không phải bản dựng lại, mà là cùng một giá trị, dựng một lần rồi mới gửi.
`crates/zode_env_sync/tests/env_pull_decision.rs` sẽ lỗi nếu hai thứ đó khác nhau, và phải
như vậy: nonce là mới cho mỗi lần mã hoá,
nên một bảng mã hoá lại file của bạn để hiển thị
sẽ hiện ra thứ khác nhau mỗi lần và chẳng chứng minh được gì.

Trỏ một proxy như `mitmproxy` vào trình soạn thảo rồi so thứ nó bắt được
với thứ bảng đã hiển thị. Chúng là cùng một chuỗi.

**2. Đọc client.** `crates/zode_env_sync/` dài khoảng hai nghìn dòng,
và lớp mã hoá của nó hoàn toàn không có code mạng. Cách phân lớp đó là cố ý: nhờ vậy
có thể kiểm tra mã hoá với các vector cố định
công bố ở [trang giao thức](./env-sync-protocol.md) thay vì với một
máy chủ đang chạy.

_Lệnh `zode env inspect` độc lập chưa được phát hành. CLI `zode` là một
trình khởi chạy nhỏ, không link các thư viện của trình soạn thảo, và để chạm tới
phần mã hoá từ đó thì phải kéo cả framework UI vào một binary
cố ý giữ gọn. Bảng và proxy trả lời cùng một câu hỏi ở thời điểm
hiện tại._

## Máy chủ vẫn có thể làm gì, và chuyện gì xảy ra khi nó thử

Kho lưu trữ mã hoá đầu cuối không ngăn máy chủ thù địch
_cư xử sai_. Nó chỉ ngăn máy chủ thành công trong âm thầm.

| Nó thử                                 | Chuyện gì xảy ra                                                                                                                                      |
| -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Đưa blob của file A vào chỗ của file B | Bị từ chối. Định danh của file nằm trong dữ liệu xác thực, nên tag không khớp.                                                                        |
| Đưa blob của tài khoản khác            | Bị từ chối. User id của bạn cũng nằm trong đó.                                                                                                        |
| Đưa một bản sao từ tháng trước         | Bị phát hiện. Mỗi blob mang một bộ đếm chỉ tăng, và máy bạn nhớ giá trị cao nhất đã áp dụng. Bạn được báo, và **không có gì được ghi**.               |
| Xoá file của bạn                       | Thấy được, và không phá huỷ cục bộ: file đã có trên máy bạn không bị đụng tới, và Zode không bao giờ xoá file cục bộ chỉ vì máy chủ ngừng liệt kê nó. |
| Từ chối phục vụ mọi thứ                | Là tấn công từ chối dịch vụ. Có thật, và câu trả lời trung thực là: hãy tự sao lưu.                                                                   |
| Sửa bản mã                             | Bị từ chối. AES-256-GCM xác thực nó.                                                                                                                  |

Dòng cuối của bảng đầu tiên là sự đánh đổi: không có đường khôi phục,
vì đường khôi phục là một lối vào thứ hai, và lối vào thứ hai là lối vào cho
bất kỳ ai nắm cơ sở dữ liệu.

## Chỗ Zode yếu hơn bạn tưởng

Những điều này được nêu ra vì một trang bảo mật chỉ liệt kê điểm mạnh
thì chỉ là quảng cáo.

**Các AI agent của Zode là chương trình riêng, và chúng đọc thẳng file của bạn.**
Bảng agent khởi chạy Claude Code, opencode và các công cụ tương tự như tiến trình
bên ngoài. Chúng thấy hệ thống file của bạn, kể cả `.env`, giống như mọi
chương trình bạn chạy. Cài đặt `private_files` giữ các đường dẫn đó khỏi việc
chia sẻ cộng tác; nó **không** sandbox một agent bên ngoài,
và không cài đặt nào trong Zode làm được. Nếu điều đó quan trọng với bạn, đừng trỏ agent vào
một checkout chứa thông tin xác thực production. Đồng bộ môi trường không gây ra
cũng không khắc phục điều này.

**Thời điểm và kích thước không được giấu hoàn toàn.** Đệm tới 4 KiB che đi số
biến trong một file. Nó không che việc bạn push thứ gì đó lúc 02:14, hay
việc bạn có mười một file thay vì hai.

**"Xác minh là bạn" không có ở mọi nơi.** macOS dùng Touch ID hoặc mật khẩu
đăng nhập. Windows và Linux hiện chưa hỏi được, và Zode nói rõ điều đó
trên nút thay vì hiện một hộp thoại chẳng kiểm tra gì.

**Quyền file trên Windows yếu hơn.** Trên macOS và Linux, `.env` kéo về được
ghi với quyền `0600` (chỉ mình bạn đọc được), bản sao lưu của nó cũng vậy. Windows không có
cơ chế tương đương; file thừa hưởng quyền của thư mục nó nằm trong.

**Nguồn gốc bản phát hành, và ở dạng yếu hơn bạn nghĩ.** Các bản phát hành của Zode
đi kèm một chứng thực (attestation) có chữ ký mà bạn có thể kiểm tra:

```
gh attestation verify zode.dmg --repo tgiap04/zode
```

Hãy đọc kỹ phần chữ nhỏ, vì nó quan trọng. Chứng thực này do
`.github/workflows/attest_release.yml` tạo ra (công khai, trong repository này), chạy
**sau khi** bản phát hành được công bố, trên các asset nó tải về. Nó chứng minh
đúng những byte đó đã được workflow công khai kia thấy, tại commit đó, trong repository
này.

Nó **không** ràng buộc bản build với đầu ra của nó như một chứng thực được cấp
bên trong job build. Dạng mạnh hơn cần quyền `attestations: write`,
và công cụ sinh workflow mà repository này dùng không biểu diễn được; file
nêu trên có ghi điều này ở đầu. Cả hai dạng đều không chứng minh trình biên dịch trung thực,
chỉ có bản build tái lập từng bit mới làm được, và Zode chưa có.

## Nếu bạn mất khoá khôi phục

Các file đã lưu mất luôn. Không phải khó khôi phục, mà là mất. Không có
yêu cầu hỗ trợ nào, vì phía máy chủ không có gì để khôi phục chúng.

File trên các máy của bạn không bị đụng tới, vì Zode không bao giờ xoá bản
cục bộ. Hãy ghi khoá ra giấy.
