# Giữ màn hình luôn sáng

Khi một terminal đang liên tục xuất output, Zode yêu cầu hệ điều hành
giữ màn hình sáng. Điều này áp dụng cho CLI của agent, terminal chạy task từ
dock terminal, và terminal shell thông thường — một đợt refactor dài, một lượt
chạy test hay một loạt subagent có thể chạy nhiều phút mà không in gì,
và máy tối màn hình rồi khoá giữa chừng thì bạn lại phải
quay lại đánh thức nó.

Tính năng này bật theo mặc định:

```json
{
  "keep_display_awake": true
}
```

Bên cạnh có một công tắc thủ công thứ hai, giữ màn hình chừng nào còn một
cửa sổ Zode đang mở, bất kể các terminal đang làm gì. Nó tắt theo mặc định —
xem [Công tắc luôn bật](#the-always-on-switch).

## Điều gì giữ màn hình

Quy tắc tự động dựa trên **tốc độ**, không phải việc tiến trình còn sống: terminal xuất output
đủ nhanh thì giữ màn hình; terminal đã im thì không, dù
tiến trình của nó có còn chạy hay không.

- Output đến nhanh hơn khoảng tám lần ghi mỗi giây thì giữ
  màn hình. Tốc độ đó phải được duy trì — màn hình được nhả sau khoảng một phút kể từ
  đợt output dồn dập cuối cùng, nên một bản build tạm nghỉ giữa các bước vẫn giữ màn hình
  sáng, còn bản build đã xong thì không.
- Dev server đã khởi động xong và đang nhàn rỗi — hoặc `tail -f`
  không có gì mới để in — **sẽ** để màn hình ngủ khi hết một phút
  đó. Đây là cố ý, không phải lỗi.
- Việc vẽ lại chậm — `top`, `htop`, đồng hồ trong prompt, thanh trạng thái `tmux` —
  chỉ vẽ lại một hai lần mỗi giây, dưới ngưỡng, nên cũng không giữ
  màn hình.
- Shell đứng ở prompt không ghi gì và không giữ gì.
- Gõ phím ở prompt có thể vượt ngưỡng, nhưng thực tế chẳng đổi gì:
  hệ điều hành vốn đã đặt lại bộ đếm nhàn rỗi của nó mỗi khi có
  phím bấm.
- Trình soạn thảo nhàn rỗi có mở terminal không tốn gì để theo dõi: tiến trình thăm dò phía sau
  chỉ bắt đầu khi terminal ghi ra gì đó, và tự dừng
  khi mọi terminal đã im một phút.

Subagent chạy nền trong tab agent cũng giữ màn hình. Trong lúc agent
chờ nó, agent chỉ vẽ lại một spinner, thấp xa dưới ngưỡng, nên một
subagent còn đang chạy được tính là tab đang phản hồi dù tab
im lặng. Việc giữ chấm dứt khi thông báo task của subagent đến,
và bắt đầu lại nếu subagent được tiếp tục.

## Công tắc luôn bật

Công tắc tự động quyết định thay bạn: nó giữ màn hình khi có gì đó
đang xuất output và nhả khi không còn gì. Nếu muốn tự quyết,
công tắc thủ công thứ hai giữ màn hình chừng nào còn một cửa sổ Zode
đang mở:

```json [settings]
{
  "keep_display_awake_always": true
}
```

Khi bật, màn hình được giữ bất kể terminal đang làm gì —
kể cả khi không có terminal nào mở — và bất kể máy có chạy pin hay không.
Nó độc lập với `keep_display_awake`, vẫn giữ ý nghĩa tự động của mình,
và không có giới hạn thời gian. Nó tắt theo mặc định, và là một cài đặt bình thường,
nên cửa sổ mở ra khi nó đang bật sẽ giữ màn hình ngay.

Có ba cách bật/tắt, tất cả đều quy về cùng một cài đặt đó, nên mọi cửa sổ đang mở
đều theo ngay:

- Bấm biểu tượng con mắt và chọn **Always Keep Display Awake**, dưới **Keep
  Display Awake While a Tab Is Producing Output**. Mục này được đánh dấu khi
  công tắc đang bật.
- Chạy `keep_awake::ToggleAlwaysOn` từ bảng lệnh (command palette). Nó không có phím tắt
  mặc định; [gán một phím](./key-bindings.md#user-keymaps) của riêng bạn và phím
  sẽ hiện cạnh mục trong menu.
- Sửa `keep_display_awake_always` trong `settings.json`.

Nếu hệ điều hành từ chối yêu cầu, dòng trong menu ghi "The system
refused the request" và Zode hỏi lại khoảng một phút một lần cho tới khi được
chấp nhận hoặc bạn tắt công tắc, nên session bus chưa trả lời
lúc đăng nhập vẫn được bắt kịp mà không cần khởi động lại. Cảnh báo trong log chỉ được ghi
ở lần từ chối đầu tiên.

## Công tắc trên thanh trạng thái

Có một biểu tượng con mắt trên thanh trạng thái. Nó sáng khi màn hình đang
được giữ và mờ khi không, và bấm vào sẽ mở một menu với hai
công tắc cùng một dòng cho biết điều gì đang thật sự diễn ra:

| Menu ghi                        | Ý nghĩa                                                   |
| ------------------------------- | --------------------------------------------------------- |
| The display is being held awake | Đang giữ màn hình, vì có output hoặc vì công tắc luôn bật |
| Nothing is producing output     | Không có gì để giữ                                        |
| Paused - running on battery     | Có output, nhưng máy đang chạy pin                        |
| The system refused the request  | Mọi điều kiện đều đạt mà hệ điều hành vẫn từ chối         |

Dòng này tồn tại vì ba trong bốn trạng thái đó cùng là biểu tượng mờ.
Tooltip nêu tên tab chịu trách nhiệm khi có một tab như vậy — nhãn của agent hoặc
tiêu đề của terminal. Khi công tắc luôn bật đang bật, tooltip ghi "Keeping
the display awake until turned off", vì đóng hết các tab
cũng không chấm dứt việc giữ đó. Khi công tắc đó bật mà không có việc giữ nào, dòng luôn là
"The system refused the request": công tắc tự nó đã đủ lý do, nên
chỉ có việc bị từ chối mới làm nó thiếu.

Trạng thái đứng sau dòng đó thật ra có năm giá trị, không phải bốn: giá trị thứ năm là
"Turned off in settings", chỉ hiện khi cả hai công tắc đều tắt. Nó không có dòng
trong bảng trên vì khi cả hai công tắc tắt, biểu tượng không bị mờ mà
bị gỡ hẳn — xem [Điều gì khiến nó nhả](#what-lets-it-go).

## Điều gì khiến nó nhả

Điều chấm dứt việc giữ tuỳ thuộc công tắc nào đã giữ. Output im đi và
pin chỉ chấm dứt việc giữ tự động; việc giữ do công tắc luôn bật không gắn với cái nào.

- Mọi terminal đang xuất output đều im — lệnh của nó kết thúc, hoặc tốc độ
  output tụt dưới ngưỡng suốt một phút (xem [Điều gì giữ
  màn hình](#what-holds-it)) — hoặc bạn đóng tab của nó. Chỉ với việc giữ tự động.
- Máy bắt đầu chạy pin. Việc giữ trở lại khi bạn cắm sạc lại;
  nguồn điện được kiểm tra lại khoảng một phút một lần, nên có thể trễ tới một phút
  theo cả hai chiều. Chỉ với việc giữ tự động.
- Bạn đặt công tắc đang giữ về `false`: `keep_display_awake` với việc giữ
  tự động, `keep_display_awake_always` với việc giữ luôn bật. Sửa tay
  `settings.json`, hoặc bỏ đánh dấu mục của công tắc trong menu biểu tượng con mắt; với
  công tắc luôn bật bạn cũng có thể chạy `keep_awake::ToggleAlwaysOn`. Điều này có
  hiệu lực ngay, kể cả với việc giữ đang có — không cần khởi động lại. Hai
  công tắc không bù cho nhau: đặt `keep_display_awake` thành `false`
  trong khi `keep_display_awake_always` đang bật thì không đổi gì, và màn hình
  vẫn được giữ.
- Bạn bấm chuột phải vào vùng trống của thanh trạng thái và bỏ đánh dấu **Keep Display
  Awake** trong menu hiện ra (xem [Cài đặt thanh
  trạng thái](./visual-customization.md#status-bar)). Thao tác này tắt **cả hai**
  công tắc, nên mọi việc giữ chấm dứt ngay, và nó gỡ biểu tượng con mắt khỏi
  chính thanh, thay vì để mờ: ẩn một mục thanh trạng thái theo cách này
  sẽ bỏ entity của mục đó, mà biểu tượng chính là entity ấy. Đánh dấu lại chỉ
  bật lại `keep_display_awake`.

Biểu tượng nằm trên thanh khi một trong hai công tắc đang bật. Khi cái cuối cùng tắt,
bằng bất kỳ cách nào ở trên, biểu tượng biến mất theo; bật công tắc luôn bật
sẽ đưa nó trở lại.

## Cái giá phải trả

**Màn hình sẽ không khoá khi nhàn rỗi trong lúc đang có việc giữ.** Đó là
tính năng chạy đúng như yêu cầu, và cũng là rủi ro: trình soạn thảo để lại ở văn phòng
chung hay quán cà phê thì ai đi ngang cũng đọc được. Cơ chế bảo vệ pin lo cho
laptop khi di chuyển, nhưng máy bàn không có pin, nên trên máy bàn không gì
chấm dứt việc giữ tự động ngoài việc terminal đang xuất output im đi, tab
đóng, hoặc cài đặt bị tắt. Không có giới hạn thời gian.

**Công tắc luôn bật gỡ bỏ mọi lối thoát đó.** Nó giữ
màn hình, và do đó vô hiệu khoá khi nhàn rỗi, chừng nào còn một cửa sổ Zode mở —
dù mọi terminal đều im, dù không có terminal nào mở, và cả khi chạy pin. Không
có cơ chế bảo vệ pin và không có giới hạn thời gian. Laptop để đó với công tắc này bật sẽ giữ màn hình
sáng cả khi chạy pin, ai đi ngang cũng đọc được, cho tới khi bạn tắt
công tắc hoặc đóng mọi cửa sổ Zode.

Ẩn thanh trạng thái không nhả việc giữ. Đặt `experimental.show` trong
`status_bar` thành `false` sẽ gỡ cả thanh, kèm biểu tượng con mắt, trong khi việc giữ luôn bật
vẫn tiếp tục khuất mắt — như việc giữ tự động vốn đã vậy. Bảng lệnh
vẫn với tới được công tắc: chạy `keep_awake::ToggleAlwaysOn` để
bật/tắt, hoặc xem `keep_display_awake_always` trong `settings.json`.

Nếu sự đánh đổi đó không hợp với nơi bạn làm việc, đặt `keep_display_awake` thành `false`
và để `keep_display_awake_always` tắt, rồi tự khoá máy,
hoặc khoá tay trước khi rời đi — việc giữ chỉ ngăn khoá do _nhàn rỗi_, không ngăn
khoá do bạn yêu cầu.

## Nền tảng được hỗ trợ

Cần hai việc riêng biệt, và chúng được xây riêng: yêu cầu hệ điều hành
giữ màn hình, và nhận biết máy đang chạy pin.

| Nền tảng | Giữ màn hình                                                                                             | Phát hiện pin |
| -------- | -------------------------------------------------------------------------------------------------------- | ------------- |
| macOS    | Có — một IOKit assertion, thấy được trong `pmset -g assertions` và đặt theo tên tab đang xuất output     | Có            |
| Windows  | Có — `SetThreadExecutionState` với `ES_DISPLAY_REQUIRED`                                                 | Có            |
| Linux    | Có — một inhibit `org.freedesktop.ScreenSaver` trên session bus, cùng một lệnh gọi cho cả X11 và Wayland | Có            |

Cả ba đều mới, và chưa cái nào được chạy thử đầu cuối trong một trình soạn thảo
đang chạy. Bằng chứng phía sau không ngang nhau, nên cần nói thẳng: trên
macOS, lệnh gọi assertion chính xác đã được kiểm chứng riêng và hiện trong
`pmset -g assertions`; trên Windows và Linux, code mới chỉ được biên dịch với
các API thật của những target đó, chưa hơn. Nếu màn hình không giữ sáng
với bạn, hoặc công tắc báo đang giữ mà màn hình không tuân theo, đó chính là
loại vấn đề nên tạo issue.

Việc giữ cho công tắc luôn bật được đặt tên "the display is kept awake until
turned off" ở nơi hệ điều hành ghi lý do, thay vì theo tên tab.

Nền tảng hoàn toàn không thể gửi yêu cầu thì không có gì trong số này: không có
biểu tượng, không công tắc, không action `keep_awake::ToggleAlwaysOn`, không việc chạy nền — một
điều khiển mờ mở ra công tắc không bao giờ làm được gì còn tệ hơn
không có gì. Đó là một đường code có thật
(nền tảng không có bản triển khai sẽ nhận nó theo mặc định), nhưng
cả ba nền tảng trên đều không còn đi đường đó: mỗi nền tảng nay đều trả lời có, nên trên
mọi nền tảng được hỗ trợ biểu tượng luôn hiện.

### Mỗi nền tảng không cho bạn biết điều gì

- **Windows** không có chỗ ghi _lý do_ màn hình được giữ —
  `SetThreadExecutionState` chỉ nhận cờ và không gì khác. Công tắc trên thanh trạng thái
  là nơi duy nhất để biết.
- **Linux** đi qua quy ước session-bus `org.freedesktop.ScreenSaver`,
  nên hoạt động như nhau dưới X11 và Wayland — nhưng chỉ ở nơi
  có thứ gì đó trả lời tên đó. GNOME và KDE có. **Các compositor dựa trên wlroots —
  sway, Hyprland, river — mặc định thì không**: chúng triển khai
  chặn nhàn rỗi qua một giao thức Wayland, mà cơ chế này chưa dùng, nên công tắc
  ở đó sẽ ghi "The system refused the request". Còn hai trường hợp nữa: một số
  môi trường trả lời lệnh gọi rồi lại không thật sự giữ màn hình, điều này
  không phát hiện được từ phía này, nên công tắc có thể báo đang giữ trong khi
  màn hình vẫn ngủ; và khi hoàn toàn không có session bus — trong container, hoặc qua
  SSH — yêu cầu đơn giản là thất bại và menu cho biết điều đó.
