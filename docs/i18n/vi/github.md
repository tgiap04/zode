---
title: GitHub
description: Làm việc với issue, epic, project và pull request của repo đang mở, trong một tab GitHub nói chuyện thẳng với GitHub từ máy của bạn.
---

# GitHub

Tab GitHub hiện issue, pull request và project của repo mà git remote của dự án trỏ tới. Tab gọi thẳng GitHub từ máy bạn bằng chính tài khoản của bạn; không có gì đi qua server của zodekit, và không tốn tiền.

## Mở tab

Bấm icon GitHub trên thanh rail của sidebar, hoặc chạy `github: toggle` trong bảng lệnh. Tab mở ở giữa cửa sổ như một file. Bấm nút lần nữa khi tab đang ở trước thì quay về tab bạn vừa đọc; tab GitHub vẫn mở và vẫn đăng nhập.

Tab không bao giờ được khôi phục khi Zode khởi động, nên mở Zode không gửi gì tới GitHub. Zode chỉ tìm token khi bạn mở tab.

## Đăng nhập

Có hai cách, và tab ghi rõ đang dùng cách nào ngay cạnh `@login` của bạn.

- **GitHub CLI.** Nếu `gh` đã cài và đã đăng nhập, mỗi lần tab mở hoặc tải lại, Zode xin token của nó (`gh auth token --hostname github.com`). Zode không lưu token này. Khi `gh` có nhiều tài khoản, Zode dùng tài khoản đang active; đổi bằng `gh auth switch` rồi bấm **Reload**. Biến `GH_TOKEN` hoặc `GITHUB_TOKEN` trong môi trường của Zode được ưu tiên hơn tài khoản đăng nhập của `gh`, và tab ghi rõ điều đó.
- **Sign in with GitHub.** Zode xin GitHub một mã ngắn; bạn nhập mã ở `github.com/login/device` rồi chấp thuận. Nút này chỉ có trong bản build mang app GitHub của zodekit; không có thì dùng GitHub CLI.

Sign in with GitHub xin ba quyền (scope):

| Scope      | Vì sao                                                     |
| ---------- | ---------------------------------------------------------- |
| `repo`     | Đọc và sửa issue, pull request, kể cả trong repo riêng tư. |
| `read:org` | Đọc bạn thuộc tổ chức (organization) và team nào.          |
| `project`  | Đọc và sửa GitHub Projects.                                |

Token chỉ nằm trên máy này. Zode giữ nó trong keychain của hệ thống và chỉ gửi nó tới `api.github.com`; token không bao giờ tới server của zodekit, và tài khoản Zode của bạn không cần là tài khoản GitHub. Bản build dev chỉ giữ token trong bộ nhớ, nên mỗi lần khởi động lại phải đăng nhập lại.

**Disconnect** (cạnh tên đăng nhập, hoặc mục **GitHub Account** ở trang Panels của Settings) xoá token mà Zode đã đăng nhập, đăng xuất mọi cửa sổ đang dùng tài khoản đó, và mở trang của GitHub để thu hồi quyền của Zode. Zode không tự thu hồi token được, nên hãy thu hồi ở trang đó để chấm dứt hẳn. Token của GitHub CLI thì giữ nguyên; muốn đăng xuất nó, chạy `gh auth logout`. Cũng trang đó trong Settings cho biết bạn đã đăng nhập chưa, và nút **Sign in with GitHub** ở đó mở tab rồi bắt đầu đăng nhập.

Nếu GitHub không nhận token nữa, tab báo như vậy và chờ bạn quyết. Tab không bao giờ tự chuyển sang token khác, vì làm vậy có thể âm thầm thao tác bằng một tài khoản khác.

### Projects cần scope `project`

Lần đăng nhập thông thường của GitHub CLI không có scope này. Thiếu nó, mục Projects giải thích còn thiếu gì và không gửi gì cả. Để thêm, chạy:

```sh
gh auth refresh -s project
```

Rồi bấm **Check again**. Đăng nhập bằng Sign in with GitHub thì đã xin scope này ngay từ đầu.

### Tổ chức có giới hạn

- **Giới hạn truy cập của OAuth App.** Tổ chức có thể chặn các app mà chủ tổ chức chưa duyệt. Tab nêu tên tổ chức và có nút **Request access**; sau đó một chủ tổ chức duyệt app trên GitHub.
- **SAML single sign-on.** Token phải được cấp phép cho tổ chức đó. Tab có nút **Authorize on GitHub**, mở đúng trang của GitHub để làm việc này; làm xong thì tải lại.

## Chọn repo

Zode đọc remote `origin` và `upstream` của từng git repo trong dự án và liệt kê mọi repo trên `github.com` tìm thấy, kèm tên remote và repo đó công khai hay riêng tư. Zode không bao giờ tự ưu tiên upstream hơn fork của bạn; repo tìm thấy đầu tiên được chọn, và bạn đổi bằng hàng nút ở trên cùng.

- Remote đi qua alias host SSH, như `git@github.com-work:owner/repo`, không được nhận ra. Hãy dùng `git@github.com:` hoặc địa chỉ `https://github.com/`.
- Chỉ hỗ trợ `github.com`, không hỗ trợ GitHub Enterprise Server.
- Với dự án từ xa, remote được đọc trên máy từ xa, nhưng `gh`, keychain và mọi request tới GitHub vẫn ở máy này. Trên Windows, WSL cũng vậy: `gh` chỉ đăng nhập bên trong một bản phân phối WSL sẽ không được tìm thấy, nên hãy đăng nhập `gh` trên Windows hoặc dùng Sign in with GitHub.
- Với dự án do người cộng tác chia sẻ, không thao tác nào được đổi file của máy chủ: checkout pull request bị tắt.

## Issue

Mục Issues liệt kê issue của repo. Mỗi dòng có biểu tượng trạng thái (đang mở, đóng vì đã xong, hoặc đóng vì không làm, mỗi loại một hình riêng), số, tiêu đề và label, ảnh đại diện của người được giao, số bình luận và lần đổi gần nhất. Pull request không nằm trong danh sách này.

- **Bộ lọc**: Open, Closed hoặc All; Assigned to me, Created by me hoặc Mentioning me; một label; một milestone; và issue type khi repo có type.
- **Tìm kiếm**: gõ vào ô sẽ lọc các issue đã tải. Bấm Enter để tìm trên toàn repo qua GitHub. Tìm kiếm có hạn mức chặt hơn, nên chỉ chạy khi bấm Enter.
- **Load more** tải trang tiếp theo; cuộn xuống không tự tải thêm.

Bấm vào một issue để mở nó trong tab riêng: tiêu đề, trạng thái, tác giả, label, người được giao, milestone, nội dung và bình luận, kèm **Open on GitHub** và **Copy link**.

### Sửa issue

Mọi thay đổi đều đến từ một nút bạn bấm; không có gì được ghi ngầm.

- **New issue** mở hộp thoại gồm tiêu đề, nội dung Markdown, label, người được giao, milestone và, khi repo có issue type, một type. Nút **Create issue** chỉ bấm được khi đã có tiêu đề, và ⌘↩ (Ctrl-↩ trên Linux và Windows) tạo issue bằng bàn phím. Hộp thoại luôn hiện tài khoản và repo sẽ nhận issue, cùng việc repo đó công khai hay riêng tư; tạo thất bại thì mọi thứ bạn đã nhập vẫn còn nguyên.
- Trong tab issue, **Edit** sửa tiêu đề và nội dung. Nếu ai đó đã đổi issue trên GitHub sau khi bạn bắt đầu sửa, Zode báo lại chứ không ghi đè, và giữ nguyên chữ bạn đang gõ. Ô nội dung, ở đây và trong **New issue**, cùng ô bình luận của issue và pull request, có hai tab **Write** và **Preview** cùng thanh công cụ cho tiêu đề, chữ đậm, chữ nghiêng, trích dẫn, code, link và danh sách gạch đầu dòng, đánh số, việc cần làm; bấm lại một nút là bỏ định dạng đó. Khi đang gõ, ⌘B, ⌘I và ⌘E (Ctrl-B, Ctrl-I và Ctrl-E trên Linux và Windows) đổi vùng chọn thành chữ đậm, chữ nghiêng hoặc code.
- **Close** với lý do completed, not planned hoặc duplicate; **Reopen** để mở lại.
- Thêm và bỏ từng label, từng người được giao (có cả **Assign me**), đặt hoặc bỏ milestone. Muốn giao cho ai, gõ một phần login hoặc tên của họ vào ô tìm người, ở đây và trong hộp thoại New issue: khi bạn ngừng gõ, Zode hỏi GitHub những người có thể được giao trong repo (20 người đầu khi ô để trống) và liệt kê từng người với ảnh, tên và `@login`. ↑ và ↓ để chọn, Enter để giao, Esc đóng danh sách, và chọn lại người đã được giao thì bỏ giao người đó. Zode chỉ gửi đúng label hay người đó, nên thay đổi người khác làm cùng lúc vẫn được giữ.
- **Comment** gửi bình luận; ⌘↩ (Ctrl-↩ trên Linux và Windows) gửi bằng bàn phím.
- Không có quyền triage thì GitHub lặng lẽ bỏ qua label, người được giao và milestone. Zode đọc lại issue và báo cho bạn phần nào chưa được áp dụng.

Muốn đổi nhiều issue một lúc, đánh dấu chúng trong danh sách rồi chọn **Close…** hoặc **Add label…**. Hộp thoại liệt kê từng issue và hỏi xác nhận trước; sau đó thanh tiến độ chạy, **Cancel** dừng trước issue kế tiếp, và từng issue báo thành công hay thất bại.

## Epic: sub-issue và issue type

Issue có sub-issue hiện tiến độ, ví dụ _2/5 done_, và mở ra các issue con ngay trong danh sách. Trong tab issue, phần **Sub-issues** liệt kê chúng và cho phép thêm một issue có sẵn, tạo issue mới làm con, đưa lên hoặc xuống, hay bỏ ra sau khi xác nhận. **Parent** đổi hoặc bỏ issue cha.

GitHub cho tối đa 100 sub-issue dưới một issue cha và lồng sâu tối đa 8 cấp; vượt quá thì Zode hiện lời từ chối của GitHub. Ô chọn **Type** và bộ lọc type chỉ xuất hiện khi GitHub báo repo có issue type.

## Projects

Mục Projects hiện các GitHub Project gắn với repo. Chọn một project và một view của nó:

- View **board** có một cột cho mỗi lựa chọn của field nhóm (mặc định là Status), thêm một cột cho các mục chưa có giá trị. Mỗi thẻ cho biết mục đó là issue đang mở hay đã đóng, pull request hay bản nháp, kèm repo và số, tiêu đề, label, ảnh và tên người được giao, và giá trị các field khác mà view đang hiện, như Priority hay iteration.
- View **table** hiện các field của view, sắp xếp theo cách view đó sắp. View roadmap được hiện dưới dạng table. Cột người được giao hiện ảnh kèm tên đăng nhập, **Linked pull requests** hiện số pull request đã liên kết (rê chuột để xem số hiệu), và **Sub-issues progress** hiện số sub-issue đã xong, như 1/3; thẻ trên board cũng hiện như vậy.

Zode tải 500 mục đầu tiên của project và báo khi còn nữa; **Load 500 more** tải các mục tiếp theo. GitHub không áp bộ lọc của view cho app khác, nên Zode hiện bộ lọc dưới dạng chữ và liệt kê mọi mục. Mục bạn không được xem hiện là _Hidden item_.

### Sửa project

- **Chuyển** thẻ sang cột khác bằng menu chuột phải, bằng kéo thả, hoặc bằng bàn phím: chọn thẻ bằng ↑ và ↓, rồi bấm ⌘↩ (Ctrl-↩ trên Linux và Windows) để mở menu. Thẻ chuyển ngay, và quay về chỗ cũ kèm thông báo nếu GitHub từ chối.
- **Sửa field** trong table: text, number, date, single select và iteration. Enter để lưu, Escape để huỷ. Người được giao, label và milestone thuộc về issue, nên hãy sửa chúng trong tab issue.
- **Thêm** một issue hoặc pull request có sẵn của repo, và tuỳ chọn đặt luôn Status.
- **Remove from project…** hỏi trước. Lệnh này không xoá issue hay pull request; draft issue chỉ sống trong project nên bị xoá theo.

Sửa project cần scope `project`, chỉ `read:project` là không đủ; mục này sẽ báo khi token thiếu.

### Tạo project

**New project** cạnh ô chọn project (hoặc **Create project** khi repo chưa có project nào) mở một hộp thoại. Chọn chủ sở hữu: **@me** (tài khoản của bạn) hoặc, khi repo thuộc một tổ chức, chính tổ chức đó. Gõ tiêu đề (tối đa 256 ký tự) rồi bấm **Create project**. Project mới được gắn với repo và được chọn sẵn trong mục này; GitHub tự thêm field Status với Todo, In progress và Done. Khi GitHub từ chối (chẳng hạn tổ chức không cho thành viên tạo project), hộp thoại hiện nguyên lời của GitHub và giữ lại tiêu đề. Tạo project cũng cần scope `project`.

## Pull request

Mục Pull requests liệt kê pull request kèm trạng thái check (đạt, lỗi hay đang chạy), trạng thái review, nhánh và nhãn _Draft_. Lọc theo Open, Closed, Merged hoặc All, và theo Created by me hoặc Review requested. Hai bộ lọc sau chỉ lọc các trang đã tải; bấm **Load more** để xem xa hơn.

Pull request mở trong tab riêng với mô tả, bình luận, các check kèm link tới lần chạy, review, và các file đổi kèm số dòng thêm, bớt. Bạn bình luận được ngay ở đó.

### Checkout pull request

**Check out** lấy nhánh của pull request từ git remote trỏ tới repo đang chọn rồi chuyển sang nó, trong một nhánh cục bộ mặc định tên `pr-<number>-<branch>`. Bạn sửa được tên; tên git không nhận, tên bắt đầu bằng `-` và tên có ký tự ẩn đều bị từ chối. Nếu nhánh đã có, Zode hỏi chuyển sang nhánh đó, giữ nguyên như cũ, hay dùng tên khác. Khi còn thay đổi chưa commit, Zode dừng và nhờ bạn commit hoặc stash trước.

Pull request từ **fork** được lấy từ `refs/pull/<number>/head`. Trước đó, Zode cảnh báo ai viết và nó đến từ đâu: checkout xong, mã này chạy trong một worktree đã tin cậy, nơi settings dự án, task, build script và language server của nó có thể chạy mà không hỏi. Chỉ checkout khi bạn tin nó. Cảnh báo hiện ra mọi lần, và sau đó Zode không tự chạy gì cho bạn.

### Merge

**Merge…** mở hộp thoại hiện tài khoản của bạn, pull request, nhánh đích và chỉ những cách merge mà repo cho phép. Lệnh merge gửi kèm commit head bạn đã thấy; nếu trong lúc đó có commit mới được push, không gì được merge và pull request được tải lại.

- Zode không bao giờ merge vượt qua branch protection, kể cả với quản trị viên. Pull request bị chặn, tụt sau nhánh đích, có xung đột hay đang là draft đều ghi rõ lý do.
- Khi GitHub còn đang tính xem pull request có merge được không, tab nhờ bạn tải lại sau giây lát.
- Repo dùng merge queue thì merge trên GitHub: tab có nút **Merge on GitHub**.
- **Delete `<branch>` after merging** chỉ hiện với nhánh cùng repo, không phải nhánh mặc định, không được bảo vệ, và vẫn đứng ở đúng commit được merge. Khi repo tự xoá nhánh đã merge, hộp thoại ghi rõ điều đó thay cho tuỳ chọn này.

## An toàn

Nội dung issue và pull request do người khác viết, nên Zode hiển thị cẩn thận:

- Markdown hiện không kèm HTML, và ảnh không bao giờ được tải về; chỉ hiện chữ alt của ảnh. Ảnh duy nhất tab tải về là ảnh đại diện từ `https://avatars.githubusercontent.com/`; địa chỉ khác, hoặc ảnh không tải được, thì hiện chữ cái đầu tên người đó.
- Chỉ link `http` và `https` hoạt động. Link có chữ hiển thị một địa chỉ nhưng dẫn tới địa chỉ khác sẽ nêu host thật và hỏi trước khi mở, kể cả khi chữ có khoảng trắng quanh địa chỉ. Link `http://` thường cũng được hỏi trước, và link Zode từ chối mở (như `javascript:` hay `file:`) sẽ có thông báo. Link chỉ mở bên trong Zode khi nó đúng là issue hoặc pull request trên `github.com`.
- Ký tự ẩn, như ký tự đảo chiều phải-sang-trái hay khoảng trắng độ rộng 0, hiện thành `<U+…>` trong tiêu đề, label, tên người và tên nhánh.

Mọi lần ghi đều đến từ một nút bạn bấm, còn merge, thao tác hàng loạt và gỡ bỏ đều hỏi trước. Các lần ghi được gửi lần lượt từng cái một.

## Hạn mức và tải lại

Zode không dò định kỳ. Zode đọc khi bạn mở tab hoặc một mục, và đọc lại khi bạn bấm **Reload**. Danh sách issue không đổi được GitHub trả từ cache và không tính vào hạn mức. Đầu tab hiện số request REST và GraphQL còn lại và lúc hạn mức được làm mới. Chạm hạn mức thì tab báo như vậy, và các lần ghi chờ một lúc rồi mới chạy tiếp.

## Giới hạn

- Chỉ `github.com`; chưa hỗ trợ GitHub Enterprise Server.
- Không review code trong editor: xem file đổi trong tab pull request và review trên GitHub.
- Chưa có template issue; **New issue** bắt đầu từ một form trống.
- Created by me và Review requested của pull request chỉ lọc các trang đã tải.
- Không có thông báo, không có GitHub Actions.

## Cài đặt

| Cài đặt               | Mặc định | Tác dụng                                     |
| --------------------- | -------- | -------------------------------------------- |
| `github_panel.button` | `true`   | Hiện nút GitHub trên thanh rail của sidebar. |

Xem [Tất cả cài đặt](./reference/all-settings.md) để biết cách đổi.
