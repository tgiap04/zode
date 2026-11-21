# Quy trình phát triển tính năng của Zode

Tài liệu này dành cho tính năng cỡ vừa đến lớn: giao diện mới, thay đổi hành vi, hoặc việc đụng tới nhiều phần của Zode. Chỉnh nhỏ phím tắt hay cài đặt thì không cần làm đủ các bước.

> **Trước khi bắt đầu:** Nếu bạn là contributor bên ngoài, hãy chắc rằng nhóm thật sự muốn tính năng này trước khi bỏ nhiều công sức. Đọc [Hướng dẫn đóng góp](../../../CONTRIBUTING.md) và [Quy định đề xuất tính năng](https://github.com/zed-industries/zed/discussions/51422). Nếu chưa có GitHub issue được nhân viên xác nhận rõ ràng, hãy mở một GitHub Discussion trước. PR đề xuất tính năng bỏ qua quy trình này có tỉ lệ được merge _rất_ thấp. Làm đúng quy trình tăng đáng kể khả năng ý tưởng của bạn được nhận và xây dựng.

## 1. Vì sao việc này quan trọng?

Mọi tính năng đều bắt đầu từ một ý tưởng. Trước khi viết code, hãy có cơ sở cho nó:

- **Nó giải quyết vấn đề gì?**
- **Bằng chứng là gì?** GitHub issue, yêu cầu trên Discord, số lượt thumbs-up, bài blog.
- **Đã có tiền lệ chưa?** Nếu VS Code, JetBrains, Neovim hoặc một plugin rất phổ biến có sẵn, đó là tín hiệu mạnh. Nếu ý tưởng mới hơn, hãy nêu nó dựa trên cái gì: "Đây là X, điều chỉnh cho multi-buffer của Zode" hữu ích hơn nhiều so với "Tôi nghĩ cái này sẽ hay."

## 2. Nó là gì?

Viết một mô tả tính năng ngắn gọn, cụ thể, rồi bổ sung bối cảnh đã thu thập ở trên. Nếu không mô tả được tính năng trong vài câu, có thể nó quá lớn hoặc quá mơ hồ.

Dưới đây là một mẫu, bạn cứ điều chỉnh cho hợp với tính năng của mình:

**Feature:** Inline Git Blame

**Purpose:** Show the last commit author and message for each line directly after the editor text, so developers can understand code history without opening the git blame.

**Background:**
This is standard across all major code editors:

- \[screenshot of VSCode]
- \[screenshot of Intellij]
- \[screenshot of Neovim]
- and has 146 thumbs up on this [github issue](https://github.com).

**Decisions:**
We have to decide whether to use the git CLI or a git library. Zode uses a git library but its blame implementation is too slow for a code editor, so we should use the CLI's porcelain interface.

## 3. Nó còn ảnh hưởng tới gì nữa?

Rà danh sách này trước khi bắt tay xây dựng. Không phải mục nào cũng áp dụng:

- **Action và phím tắt.** Tính năng định nghĩa những action nào? Phím tắt mặc định có xung đột với phím hiện có không?
- **Cài đặt.** Có hành vi nào cấu hình được không? Theo người dùng, theo dự án hay theo ngôn ngữ? Đừng quên thêm cài đặt mới vào giao diện Settings.
- **Giao diện (theme) và kiểu dáng.** Có cần semantic token mới không? Hiển thị có ổn ở cả chế độ sáng lẫn tối không?
- **Chế độ Vim.** Người dùng Vim có thể kỳ vọng khác về tính năng này.
- **Phát triển từ xa.** Tính năng có chạy với dự án từ xa không? Đường dẫn file, lệnh shell và biến môi trường đều có thể hoạt động khác.
- **Lưu trạng thái qua các lần khởi động lại.** Trạng thái của tính năng có nên được giữ lại không?
- **Khả năng tiếp cận.** Có thao tác được bằng bàn phím không? Trạng thái focus có rõ không?
- **Khác biệt giữa các nền tảng.** Hành vi có khác nhau trên macOS, Linux hay Windows không?
- **Hiệu năng.** Tính năng chạy thế nào với file lớn hoặc dự án lớn? Thao tác có tức thì không?
- **Bảo mật.** Tính năng tương tác với Workspace Trust ra sao? Có mở thêm bề mặt tấn công mới trong Zode không?

Nếu tính năng chạm vào **trình soạn thảo** (editor): editor có rất nhiều tính năng cùng tồn tại như phần gutter, inline block, nhiều con trỏ, gập code, edit prediction, popover hiểu code, minimap. Hãy thử thay đổi của bạn khi bật nhiều tổ hợp trong số đó. Tính năng chạy tốt trong buffer thường có thể phải tắt trong multi-buffer.

## 4. Đưa ra

Dùng nội dung này làm nền cho GitHub Discussion, issue hoặc mô tả PR của bạn. Nghiên cứu sản phẩm tốt giúp mọi người thống nhất về mục tiêu, hiện trạng của giải pháp và các đánh đổi cần cân nhắc.
