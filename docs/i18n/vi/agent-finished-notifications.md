# Thông báo khi agent hoàn tất

Zode có thể nhờ hệ điều hành đăng một thông báo desktop thật -- giao diện thông
báo của chính hệ điều hành, không phải toast trong ứng dụng -- khi một agent
hoàn tất. Tính năng này dành cho lúc bạn đã chuyển sang việc khác và muốn biết
mà không phải nhìn tab.

Tính năng này bật mặc định:

```json
{
  "agent_finished_notification": {
    "enabled": true,
    "quiet_period_ms": 12000
  }
}
```

## Các điều kiện kích hoạt

- **Agent trả lời xong.** Cách phát hiện phụ thuộc vào loại tab;
  xem bên dưới.
- **Agent đang chờ bạn duyệt.** Chỉ với tab Claude mà Zode đang theo dõi phiên
  -- xem bên dưới.
- **Tiến trình CLI của agent thoát.** Độc lập với các điều kiện khác -- gồm cả
  agent bị crash, bị kill, hoặc kết thúc hẳn phiên của nó.

Mỗi điều kiện có thể kích hoạt riêng; không cái nào phụ thuộc cái nào.

## Tab Claude: đọc từ transcript

Với tab Claude mà Zode đang theo dõi phiên, "trả lời xong" lấy từ chính
transcript của Claude, nơi ghi lại lúc một lượt kết thúc. Zode chờ ba giây sau
dòng đó rồi mới thông báo, nên một lượt lập tức bắt đầu lại sẽ không thông báo.
Lượt bạn ngắt giữa chừng thì không thông báo.

Một lượt kết thúc trong khi các agent nền do nó khởi chạy vẫn còn chờ thì bị giữ
lại. Khi kết quả của chúng mở ra một lượt tiếp theo, lượt đó sẽ thông báo khi
kết thúc. Nếu không có lượt nào theo sau, Zode thông báo muộn: sau khoảng mười
phút không có hoạt động subagent, hoặc sau khoảng ba mươi phút nếu hoàn toàn
không thấy được các subagent.

`quiet_period_ms` không áp dụng ở đây, và các lần gọi công cụ lâu hay chờ model
không gây thông báo sớm. Ngoại lệ duy nhất là một Stop hook chạy quá khoảng tám
giây trên phiên bản Claude không bao giờ ghi dòng turn-duration: Zode có thể
báo lượt kết thúc trước khi hook xong.

Nếu transcript không bao giờ hiện dấu lượt trong khi CLI ghi output
khoảng một phút, Zode ngừng tin nó cho tab đó. Có thể nó nằm ở nơi Zode
không tìm, hoặc định dạng đã đổi. Khi đó tab quay về [khoảng
im lặng](#every-other-agent-the-quiet-period), giống mọi agent khác,
và thông báo "trả lời xong" đến từ đó. Dấu lượt đầu tiên xuất
hiện sẽ đưa tab về lại transcript. Thông báo "phiên kết thúc" thì
kích hoạt trong cả hai trường hợp.

Nếu bạn chạy `/clear`, Claude bắt đầu một transcript mới. Tab đi theo nó, nên
các lượt vẫn được báo.

### Chờ bạn duyệt

Khi hộp thoại cấp quyền của Claude đang hiện và terminal im lặng vài
giây, Zode đăng một thông báo riêng "Waiting for your approval".
Zode nhận ra hộp thoại bằng cách đọc terminal, nên việc này có thể
hỏng: hộp thoại nó không nhận ra chỉ làm mất đúng thông báo đó. Không gì
khác bị ảnh hưởng. Thông báo không bị thu hồi khi bạn trả lời hộp
thoại.

## Mọi agent khác: khoảng im lặng

Mọi agent khác, tab Claude không có phiên được theo dõi, và tab Claude mà Zode
đã ngừng tin transcript (xem trên) đều dùng cách ước lượng mô tả ở đây. Một
tab agent có cạnh tín hiệu "đang tạo phản hồi", và khi
cạnh đó chuyển từ true sang false rồi giữ false trong
`quiet_period_ms`, một thông báo được kích hoạt.

`quiet_period_ms` (mặc định `12000`, sàn `2000`) tồn tại vì không có tín hiệu nào
tốt hơn cho các agent này. Chúng chạy dưới dạng CLI trong một pty, không phải
thứ Zode nói chuyện qua giao thức có cấu trúc -- không có sự kiện "trả lời xong"
ở mức giao thức để đọc. Việc agent có "còn đang trả lời" hay không được suy ra
từ tần suất terminal của nó bị ghi, và chính tín hiệu đó cũng im đi trong một
lần gọi công cụ lâu hoặc khi agent chờ phản hồi của model, không chỉ khi nó thực
sự xong.

**Nghĩa là một thông báo có thể và sẽ kích hoạt khi agent vẫn
đang làm việc**, mỗi khi nó dừng lâu hơn khoảng im lặng đã cấu hình
-- model phản hồi chậm, một lần build dài, một lần đọc
file lớn. Tăng `quiet_period_ms` là đổi sự nhanh nhạy lấy ít thông báo sớm
hơn; không thể tăng để loại bỏ hoàn toàn, vì tín hiệu gốc không mang
nhiều thông tin hơn thế. Nếu bạn thấy thông báo trong khi agent rõ
ràng vẫn đang chạy, hãy tăng giá trị này thay vì coi đó là bug.

## Không bị chặn khi Zode đang được focus

Thông báo được đăng bất kể focus cửa sổ, kể cả khi cửa sổ Zode là
ứng dụng ở trước nhất. Đây là chủ ý, không phải sơ suất -- không
có setting chặn theo focus. Nếu bạn chỉ muốn được báo về agent
đã xong khi đang ở chỗ khác, hãy tự đưa cửa sổ xuống nền; Zode sẽ
không phán đoán thay bạn.

## Thông báo mang gì

Tiêu đề là nhãn của chính tab agent. Nội dung là một cụm cố định -- không bao giờ
là nội dung transcript, và không bao giờ là bất cứ thứ gì từ cuộc hội thoại. Lịch
sử thông báo hiện trên màn hình khoá ở cả ba nền tảng, đó là lý do không có gì từ
cuộc hội thoại đến được tầng hệ điều hành.

## Hỗ trợ nền tảng

| Nền tảng | Cơ chế gửi                                                                                       | Kiểm chứng                                                                                                                                                                                                                           |
| -------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| macOS    | `UNUserNotificationCenter`                                                                       | Hành vi về bundle identity, vị trí bundle và thời điểm cấp quyền được đo trực tiếp trên framework thật (bên dưới) bằng một probe độc lập. **Chưa có người nào xác nhận bằng mắt một banner** -- API chấp nhận không phải là quan sát |
| Linux    | `org.freedesktop.Notifications` over the session bus (zbus), the same call under X11 and Wayland | **Chỉ kiểm chứng ở mức biên dịch.** Đã type-check với các crate thật cho target Linux và build trong CI. Chưa ai chạy và nhìn thấy một thông báo hiện ra                                                                             |
| Windows  | WinRT `ToastNotification`                                                                        | **Chỉ kiểm chứng ở mức biên dịch,** như Linux. Chưa người nào xây dựng tính năng này thấy một toast từ nó                                                                                                                            |

Chưa có người nào quan sát được thông báo từ tính năng này trên cả ba nền tảng.

Nền tảng không có triển khai thì không có gì được dựng -- không timer, không
subscription, không gì theo dõi tab agent -- cùng quy tắc "nơi đáp án là không
thì không dựng gì" mà [Keeping the Display
Awake](./keep-display-awake.md#platform-support) tuân theo.

### macOS: cần app bundle thật, đặt đúng chỗ

`UNUserNotificationCenter` cần bundle identifier khác nil. Binary
`cargo run` thông thường không có, nên trên macOS tính năng này âm thầm không hoạt động
ngoài bản build đóng bundle -- và phép kiểm tra quyết định điều này không phải
là cẩn thận cho có: gọi vào notification center khi không có bundle identifier
làm sập tiến trình ngay lập tức, theo cách không exception handler nào ở hai
phía của ranh giới FFI bắt được. Zode kiểm tra bundle identity trước, vô điều
kiện, trước khi bất cứ thứ gì khác chạm vào notification center.

Bundle cũng phải nằm ở nơi hệ điều hành chịu tin. Chữ ký ad-hoc là đủ,
nhưng một `.app` chạy từ thư mục tạm bị từ chối cấp quyền thẳng thừng; đúng
bundle đó trong `~/Applications` thì được chấp nhận. Bản build đóng bundle
cục bộ (`script/bundle-mac`) phải nằm trong `~/Applications` hoặc
`/Applications`, không phải nơi nháp hay thư mục tạm, nếu không thông báo sẽ bị
từ chối âm thầm.

Lần đầu tính năng này thử thông báo, macOS hiện hộp thoại
xin quyền thông báo quen thuộc. Nếu bạn đã bỏ qua, hoặc
sau này muốn cấp lại, vào **System Settings → Notifications →
Zode**. Yêu cầu cấp quyền là bất đồng bộ và không đảm bảo xong
nhanh, nên đừng ngạc nhiên nếu thông báo đầu tiên sau khi cài
không hiện ngay.

### Windows: cần bản đã cài, mở từ shortcut

Toast notification dựa vào AppUserModelID (AUMID), mà bản cài của Zode lấy từ
shortcut trong Start Menu. Mở kiểu không đóng gói hoặc không có shortcut thì
không có AUMID đã đăng ký, và khi đó API toast của Windows trả về thành công mà
không vẽ gì -- không có lỗi nào để bắt. API thành công không phải bằng chứng
rằng toast đã hiện.

### Tắt tính năng

```json
{
  "agent_finished_notification": {
    "enabled": false
  }
}
```
