# Phiên agent qua các lần khởi động lại

Đóng Zode khi còn các tab agent đang mở rồi mở lại, các tab sẽ quay về: cùng
agent, cùng khung, cùng tên. Trang này nói về
phần nằm bên dưới — tab quay về đúng **cuộc hội thoại** nó đang ở,
hay chỉ là một tab trống.

## Những gì được ghi nhớ

Theo từng tab, trong từng dự án:

- nó chạy agent nào
- chế độ của nó
- tên bạn đặt, nếu bạn đã đổi tên
- phiên mà nó thuộc về, cùng tiêu đề của phiên đó tại thời điểm
  tab mở vào phiên. Đây là ảnh chụp, không phải liên kết sống — đó là tên
  của tab khi bạn chưa từng đổi tên, hoặc sau khi bạn xoá tên đã đổi, nhưng
  nó sẽ không đổi theo tiêu đề của phiên nếu bản ghi hội thoại được tóm tắt lại
  về sau. Tab tự khởi tạo phiên của mình vẫn mang tên agent,
  kể cả khi bản ghi của phiên đó đã có tiêu đề.

Zode không bao giờ lưu chính cuộc hội thoại. Nó thuộc về
CLI của agent, nằm trong thư mục của agent, và dựng lại ở đây nghĩa là
tự bịa ra. Zode chỉ giữ **id** của phiên, rồi trao lại cho
CLI để CLI tự tiếp tục.

## Agent nào quay về đúng cuộc hội thoại

Không phải agent nào cũng giống nhau, và lý do nằm ở một cờ:

| Agent       | Tab bạn mở từ lịch sử     | Tab bạn mở mới                |
| ----------- | ------------------------- | ----------------------------- |
| Claude Code | quay về đúng phiên của nó | **quay về đúng phiên của nó** |
| Codex       | quay về đúng phiên của nó | quay về trống                 |
| Copilot     | quay về đúng phiên của nó | quay về trống                 |
| opencode    | quay về đúng phiên của nó | quay về trống                 |

Claude Code nhận `--session-id`, nên Zode chọn id **trước** khi chạy
CLI và biết nó ngay từ lúc đó. Codex, Copilot và opencode tự sinh
id và không báo lại, nên tab bạn mở mới không có id nào để Zode
ghi lại — mà ghi đại một id đoán còn tệ hơn không ghi gì,
vì tab sẽ quay về nhầm cuộc hội thoại của người khác.

Mở một phiên Codex, Copilot hoặc opencode từ bảng lịch sử thì nó được
theo dõi ngay từ đầu, vì id lấy từ bảng đó.

## Hai trường hợp nên biết

**Tab bạn mở rồi chưa gõ gì.** Claude chỉ ghi bản ghi hội thoại sau
tin nhắn đầu tiên, nên tab bạn mở hôm qua mà chưa dùng thì có id nhưng không có
phiên nào trên đĩa. Nó quay về như một tab mới dùng được và vẫn giữ id đó —
gõ vào là id trở thành thật.

**Phiên bạn đã xoá.** Xoá một phiên từ bảng lịch sử là xoá
cuộc hội thoại, không xoá tab đang ở trong đó. Tab Claude bắt đầu lại từ đầu dưới
id của chính nó. Tab Codex, Copilot hoặc opencode báo phiên không còn trên
máy này và đề nghị mở phiên mới, thay vì lặng lẽ mở một phiên trống
trông như phần tiếp theo.

Tuy nhiên, cách xoá không giống nhau ở cả bốn agent. Bản ghi của Claude, Codex và
Copilot được chuyển vào thùng rác của hệ điều hành và có thể khôi phục từ đó.
opencode giữ phiên trong kho riêng, không phải file mà trình soạn thảo này chuyển vào
thùng rác được, nên xoá một phiên là nhờ chính opencode xoá (`opencode session
delete`). **Không thể hoàn tác.** Xoá hàng loạt sẽ nói rõ điều này mỗi khi
đợt xoá có một phiên opencode.

Xoá không dừng agent đang chạy trong tab đang mở. Nếu có tab
đang chạy phiên bạn sắp xoá, hộp xác nhận sẽ báo và gợi ý đóng tab trước.
Agent có thể vẫn ghi vào bản ghi hội thoại sau khi nó bị chuyển đi, nên tab
có thể ngừng báo các lượt đã xong và phiên có thể hiện lại trong danh sách lịch sử.
Zode không từ chối việc xoá:
đóng tab hay không là do bạn quyết.

## Phiên rẽ nhánh không được theo dõi

**Continue in New Session…** trong bảng lịch sử sẽ rẽ nhánh một phiên. Tab
chạy CLI trong thư mục mà phiên đã chạy, với cờ rẽ nhánh riêng của agent
(`--resume <id> --fork-session` cho Claude Code, `-s <id> --fork` cho opencode),
nên tab mới bắt đầu từ cuộc hội thoại cho tới thời điểm đó. Codex và Copilot không có
rẽ nhánh, nên mục này bị vô hiệu với chúng. Nếu bản gốc không còn trong
kho của agent, tab báo phiên không còn thay vì mở một
phiên trống.

CLI tạo id phiên **mới** cho nhánh và không nói đó là id nào, nên
Zode không ghi lại được. Vì vậy tab rẽ nhánh không được theo dõi: sau khi
khởi động lại Zode, nó quay về trống thay vì tiếp tục nhánh, đó là
điều duy nhất sẽ gây hiểu lầm thật sự. Chạy lại agent trong
tab đó, sau khi CLI của nó đã kết thúc, sẽ rẽ nhánh từ bản gốc thêm lần nữa và cho ra một
nhánh mới.

## Lúc khởi động trông thế nào

Các tab được khôi phục chạy CLI ngay thay vì chờ bạn bấm vào, để
agent sẵn sàng khi cửa sổ hiện lên. Mở nhiều tab nghĩa là lúc khởi động có
nhiều tiến trình CLI, mỗi tiến trình nạp bản ghi hội thoại và các MCP server
mà nó được cấu hình. [Chỉ báo
dung lượng dự án](./project-footprint-indicator.md) trên thanh trạng thái là nơi
thấy được điều đó.

## Bản ghi của Claude Code được đọc từ đâu

Zode đọc bản ghi của Claude Code từ thư mục `projects` trong
`CLAUDE_CONFIG_DIR` khi biến này được đặt là đường dẫn tuyệt đối, và từ
`~/.claude/projects` trong các trường hợp còn lại. Zode đọc biến này từ môi trường của chính nó.
Giá trị rỗng và đường dẫn tương đối đều bị bỏ qua: đường dẫn tương đối tính theo
thư mục làm việc của CLI, thứ mà Zode không có, nên Zode đọc
thư mục mặc định.

Chỉ những bản ghi có tên là UUID của phiên mới được liệt kê. File `.jsonl` có tên
khác bị bỏ qua, vì tên của nó là thứ được truyền cho `--resume`, và
Claude Code không thể tiếp tục một bản ghi không được đặt tên theo cách đó.

## Dự án từ xa

Trong cửa sổ WSL, SSH hoặc dev container, CLI của agent chạy trên máy chủ, bản ghi hội thoại
cũng nằm ở đó. Bảng lịch sử liệt kê, tiếp tục, rẽ nhánh và xoá các
phiên lưu trên máy chủ đó, và tab được khôi phục tiếp tục trên đó. Không có gì
được chép về máy của bạn, và máy chủ không kết nối được thì được báo ra
chứ không bị thay bằng các phiên cục bộ của bạn. Nếu Zode server trên máy chủ cũ hơn Zode này, bảng sẽ báo; hãy kết nối lại cửa sổ rồi làm mới.

## Lưu ở đâu

Trong cơ sở dữ liệu của chính Zode, cạnh phần bố cục workspace còn lại — không nằm trong
file cài đặt của bạn, cũng không nằm trong thư mục của agent. Xoá lịch sử
của một dự án thì id phiên của các tab trong đó cũng bị xoá theo.
