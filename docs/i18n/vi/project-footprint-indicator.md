# Chỉ báo mức dùng tài nguyên của dự án

Một huy hiệu trên thanh trạng thái hiển thị tổng CPU và RAM của mọi tiến trình con
thuộc các dự án đang theo dõi — language server và terminal của chúng (các agent
CLI chạy trong đó), cùng toàn bộ cây tiến trình con cháu. Bấm vào huy hiệu sẽ mở
một popover với số liệu chia theo từng dự án.

Tính năng này bật mặc định:

```json
{
  "project_footprint_indicator": true
}
```

## Những gì được đếm, và những gì cố ý không đếm

"Tiến trình con" của một dự án gồm language server, terminal của nó, và
mọi thứ các terminal đó sinh ra — ví dụ một shell sinh ra một agent CLI, agent đó
lại sinh ra một lần build, tất cả đều tính vào dự án đó khi đã duyệt
cây tiến trình.

Có hai thứ **không** được đếm, và là cố ý:

- **Heap và CPU của chính Zode.** Zode chạy mọi dự án đang mở trong một
  tiến trình hệ điều hành, nên bộ nhớ và CPU của chính nó không thể quy cho một
  dự án nào và bị loại ra hoàn toàn.
- **Phần còn lại của máy.** Đây không phải công cụ giám sát hệ thống. Một dự án
  không chạy gì thì không hiện gì, và tổng của huy hiệu sẽ không khớp với
  số liệu toàn máy trong Activity Monitor, Task Manager hay `top`. Mỗi
  tiến trình được tính theo cách công cụ giám sát của chính nền tảng đó tính;
  huy hiệu chỉ cộng lại một tập nhỏ hơn nhiều.

Một PID truy cập được từ nhiều hơn một dự án (có thể xảy ra nếu một tiến trình bị
gắn lại cha vào một shell vốn là terminal đang theo dõi) được ghi cho
dự án nào nhận trước, không bao giờ cho cả hai — tổng có thể đếm thiếu
trong trường hợp biên như vậy, nhưng không bao giờ đếm trùng.

## Đọc huy hiệu và popover

Huy hiệu nằm ở mép trái của nhóm bên phải thanh trạng thái, trước
các điều khiển riêng của buffer. Nó không vẽ gì và không chiếm chỗ khi
không có gì để báo — một cửa sổ vừa mở, chưa chạy terminal hay
language server nào thì không hiện biểu tượng nào. Khi một dự án đang theo dõi có
tiến trình con, huy hiệu hiển thị tổng bộ nhớ và CPU của mọi dự án
đang theo dõi, mỗi thứ sau một biểu tượng riêng: hình các chữ số nhị phân cho bộ nhớ,
hình tia chớp cho CPU.

Hai biểu tượng đó chỉ là tạm thay thế. Repository này không có biểu tượng CPU hay
mô-đun bộ nhớ làm riêng, nên tooltip của huy hiệu và mọi dòng trong popover đều ghi rõ
"RAM" và "CPU" bằng chữ, thay vì để biểu tượng tự gánh
toàn bộ ý nghĩa.

Bấm vào sẽ mở một popover phía trên huy hiệu, căn theo mép phải, mỗi dự án
một dòng: tên hiển thị, bộ nhớ và CPU. Giá trị chưa
đo được — ví dụ CPU ở
lần lấy mẫu đầu tiên, khi chưa có số đọc trước đó để lấy hiệu
— hiện "not measured", không bao giờ là "0%", vì hai thứ này có nghĩa khác
nhau: 0 nghĩa là dự án đang rảnh, not measured nghĩa là chưa biết gì.

### "Bộ nhớ" ở đây nghĩa là gì

Bất cứ thứ gì công cụ giám sát hệ thống của nền tảng gọi là bộ nhớ, để huy hiệu có thể
đối chiếu với `htop`, Activity Monitor hay Task Manager thay vì lặng lẽ
lệch với chúng:

| Nền tảng | Nguồn                                                         | So sánh được với                                                                                                                                                           |
| -------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Linux    | proportional set size, từ `/proc/<pid>/smaps_rollup`          | các trường `Pss:` cộng lại của chính kernel, là thứ nó đã được đối chiếu                                                                                                   |
| macOS    | physical footprint, qua `proc_pid_rusage`                     | cột **Memory** của Activity Monitor, và `top -l 1 -stats mem`, cùng đọc một nguồn                                                                                          |
| Windows  | private commit, từ `PROCESS_MEMORY_COUNTERS_EX::PrivateUsage` | private working set của Task Manager, nhưng đọc cao hơn một chút: commit tính cả các page đã bị đẩy ra đĩa. Chưa từng đối chiếu, xem [Hỗ trợ nền tảng](#platform-support). |

Cố ý không dùng resident set size. Một page chia sẻ giữa một tiến trình và
bốn tiến trình con nó fork ra xuất hiện trong cả năm số RSS, nên cộng
lại là tính cùng một vùng nhớ năm lần: đo được cao hơn 33% trên Linux
và 90% trên macOS, nơi mọi tiến trình đều map bộ đệm thư viện dùng chung.
Mỗi nền tảng lùi về RSS nếu không đọc được số liệu riêng của nó — một tiến trình
không đọc được không phải là tiến trình không dùng bộ nhớ.

### "CPU" ở đây nghĩa là gì

Một phần trong số CPU mà ứng dụng này thực sự được phép chạy
(`std::thread::available_parallelism()`, nên hạn mức cgroup hay mặt nạ CPU affinity
đều được tôn trọng), chặn ở 100%. Một dự án bận trên nhiều lõi hiện
là "bận" thay vì một con số vượt 100 trả lời một câu hỏi khác
với câu huy hiệu này phục vụ. Con số tổng hợp của các dự án cũng bị chặn
như vậy, vì mỗi phần đóng góp vốn đã là một phần của tổng thể.

CPU logic, không phải lõi vật lý. Hai con số này bằng nhau chỉ trên phần cứng
không có SMT, và khi chúng khác nhau, chia cho số nhỏ hơn làm huy hiệu
đọc gấp đôi. Số lượng được đọc lại theo nhịp chậm bên dưới thay vì lúc
khởi động, nên khi nâng hạn mức CPU của container hay đổi mặt nạ affinity thì trong
khoảng 30 giây là nhận được, thay vì để huy hiệu sai suốt
vòng đời tiến trình.

## Việc thăm dò này tốn bao nhiêu

Hai nhịp dùng chung một bộ hẹn giờ, nên lượt tốn kém và lượt rẻ không bao giờ
chồng lên nhau:

- Mỗi **~3 giây**, một lượt làm mới hẹp chỉ với các PID đã biết là
  thuộc một dự án đang theo dõi.
- Mỗi **~30 giây** (cứ mười nhịp hẹp một lần), một lượt liệt kê toàn bộ cây tiến trình,
  chạy một lần để tìm những PID nào giờ là hậu duệ của gốc
  mỗi dự án, rồi bỏ đi.

Cả hai dừng hẳn khi cửa sổ không phải cửa sổ đang hoạt động, và khi
setting tắt. Nếu không dự án đang theo dõi nào sở hữu tiến trình con nào — trường hợp
thường gặp với cửa sổ vừa mở — một nhịp không làm việc nền
nào cả: không có gì để một lượt quét phát hiện.

### Các con số phía sau, và chúng từ đâu ra

Mọi con số dưới đây lấy từ một probe độc lập gọi các hàm `sysinfo` mà tính năng
này dùng, không phải từ việc xem tính năng chạy trong trình soạn thảo, vì
chưa ai làm vậy. Chúng đến từ hai máy khác nhau và không
áp dụng chéo được, nên mỗi số đều ghi rõ lấy từ máy nào.

Trên một máy macOS 10 lõi không SMT (Apple Silicon), có 817 tiến trình
đang chạy:

- Liệt kê toàn bộ tiến trình: 12–15 ms, trên một `sysinfo::System` dùng một lần
  và bị hủy ngay khi lượt đó kết thúc.
- Làm mới hẹp các PID đã biết: ~165 µs, rẻ hơn khoảng 80 lần.
- Một `System` chỉ được cho các PID đã biết để làm mới thì chỉ giữ lại các PID đó (đo được 10
  bản ghi, không phải 819), nên thứ nằm lại trong bộ nhớ giữa các nhịp là một
  nhúm bản ghi tiến trình, không phải toàn bộ bảng tiến trình của máy.

Cả hai số đo đều có trước các bản sửa tính đúng đắn tháng 2026-09. Số liệt kê
vẫn giữ nguyên sau đó, vì macOS chưa bao giờ liệt kê thread như tiến trình và
việc liệt kê đó chính là công việc bản sửa đã bỏ. Số làm mới hẹp giờ là một
mức sàn chứ không phải toàn bộ chi phí của nhịp đó: mỗi tiến trình được lấy mẫu còn
phải đọc bộ nhớ theo nền tảng, và phần đó không nằm trong những gì đã đo
ở đây. Trên Linux, việc liệt kê từng làm nhiều việc hơn hiện tại,
duyệt mọi thread của mọi tiến trình và đọc nội dung
`/proc/<pid>/task` để làm vậy. Việc đó đã bỏ, nhưng nó từng tốn bao nhiêu thì
chưa từng đo, nên không có số liệt kê trên Linux để trích dẫn.

Trên một container aarch64 Linux, chi phí duy nhất nền tảng đó thêm vào: đọc
proportional set size của một tiến trình mở `/proc/<pid>/smaps_rollup`, thứ duyệt
mọi vùng map bộ nhớ mà tiến trình sở hữu, trong khi số resident set mà nó
thay thế là một lần đọc thời gian hằng. Đo được 35 µs cho tiến trình có ~74
vùng map, 171 µs ở ~2,000, và 746 µs ở ~8,000, nên một nhúm
language server lớn có thể thêm vài mili giây mỗi nhịp. Việc đó chạy trên
một thread nền mỗi ba giây, nên không ảnh hưởng thời gian dựng khung hình.

## Quyền riêng tư

Lượt phát hiện hỏi `sysinfo` dữ liệu tiến trình bằng
`ProcessRefreshKind::nothing().without_tasks()`. Lệnh đó báo PID cha của mỗi tiến trình, là
tất cả những gì lượt phát hiện cần, và nó **không** đọc dòng lệnh,
đường dẫn thực thi hay môi trường của bất kỳ tiến trình nào trên máy. Tính năng này chỉ giữ PID,
PID cha, số liệu bộ nhớ và CPU, và
không ghi log thứ nào trong đó.

Trên Linux, việc đọc bộ nhớ còn mở `/proc/<pid>/smaps_rollup`,
mà kernel chỉ cho phép với tiến trình thuộc người dùng này; trên macOS
`proc_pid_rusage` cũng bị từ chối với tiến trình ngoài tầm với của người dùng này.
Cả hai lỗi đều lùi về số đọc mà `sysinfo` đã có sẵn. Ranh giới đó
là của kernel, và dù sao tính năng này chỉ hỏi về các tiến trình là hậu duệ của
chính các tiến trình con của nó.

## Hỗ trợ nền tảng

Huy hiệu chạy trên Linux, macOS và Windows. Mức độ kiểm tra kỹ của từng nền tảng
khác nhau, và sự khác biệt đó đáng biết: bốn lỗi được tìm thấy
vào 2026-09 mà macOS, máy duy nhất tính năng từng được đo, không thể thấy.
Bản tường trình đầy đủ nằm trong nhật ký kỹ thuật của repository,
dưới `2026-09-13-four-defects-that-only-apple-silicon-could-hide`.

- **Linux** — bộ lấy mẫu và test của chính nó được biên dịch và chạy trong một
  container, cách đó chứng minh được lỗi đếm thread. Số liệu bộ nhớ
  được đối chiếu với tổng các trường `Pss:` của kernel cho cùng
  năm tiến trình, lệch 0.04%. Số CPU được kiểm tra trong một
  container `--cpuset-cpus=0-1`, nơi nó trả 2 còn số
  lõi vật lý mà nó thay thế vẫn trả 10.
- **macOS** — số liệu bộ nhớ được đối chiếu với `top -l 1 -stats mem`,
  cùng nguồn với cột Memory của Activity Monitor: 646.9 MB
  so với 647M cho một tiến trình đang chạy.
- **Windows** — CI build crate và chạy test trên Windows, nên
  số đọc trên Windows biên dịch được, trả về số dương và nằm trong khoảng tám
  lần so với resident set size. Đó là toàn bộ những gì đã biết.
  **Chưa ai đặt nó cạnh Task Manager.** Độ chính xác ở đó
  chưa được xác nhận, và điều duy nhất kỳ vọng là về hướng: private commit
  tính cả các page đã bị đẩy ra đĩa, nên nó nên đọc cao hơn đôi chút
  private working set của Task Manager.

Các số đo thời gian ở mục trên đều ghi rõ máy lấy số, và
chưa số nào được đo lại trên máy khác. Hãy đọc chúng như hình dạng của khác biệt
giữa hai nhịp, không phải con số đúng ở mọi nơi.

Chưa ai xem huy hiệu hiển thị trong trình soạn thảo trên bất kỳ nền tảng nào.

## Tắt tính năng

Đặt `project_footprint_indicator` thành `false` thì huy hiệu, popover và
vòng thăm dò phía sau chúng đều dừng — không có trạng thái nửa vời nào mà
vòng lặp vẫn chạy trong khi không có gì để hiển thị.

Cũng có thể đổi setting này bằng cách bấm chuột phải vào một vùng trống của
thanh trạng thái và tắt **Project Footprint** trong menu mở ra —
xem [cài đặt thanh trạng thái](./visual-customization.md#status-bar). Menu đó
ghi đúng setting mà sửa tay sẽ ghi, nên hiệu quả như nhau
trong cả hai cách: huy hiệu biến mất và vòng thăm dò dừng theo.
