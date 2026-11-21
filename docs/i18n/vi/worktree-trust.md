---
title: Zode and trusted worktrees
description: "Configure which folders Zode trusts for running code and extensions."
---

# Zode và worktree tin cậy

Worktree trong Zode là một thư mục hoặc một file đơn lẻ mà Zode mở như một "dự án" độc lập.
Zode mở một worktree mỗi khi bạn chạy `zode some/path`, kéo thả file hoặc thư mục vào Zode, hoặc mở file cài đặt người dùng.

Mỗi worktree được mở có thể chứa file `.zode/settings.json` với các tùy chọn cấu hình bổ sung, có thể đòi hỏi cài đặt và chạy language server hoặc MCP server.
Để người dùng tự chọn theo mô hình mối đe dọa và mức chấp nhận rủi ro của mình, mọi worktree đều khởi đầu ở Chế độ hạn chế (Restricted Mode). Chế độ hạn chế chặn việc tải về và chạy các thành phần liên quan từ `.zode/settings.json`. Chừng nào worktree chưa được tin cậy, Zode không chạy các hành động chưa tin cậy đó mà chờ người dùng xác nhận. Nhờ vậy người dùng có cơ hội xem lại cài đặt dự án, MCP server và language server.

Zode vẫn tin các công cụ do chính nó cài ở phạm vi toàn cục. MCP server toàn cục và language server toàn cục như Prettier và Copilot vẫn được cài và khởi động bình thường, không phụ thuộc vào việc worktree có được tin cậy hay không.

Nếu worktree chưa được tin cậy, Zode hiện biểu tượng dấu chấm than trên thanh tiêu đề. Bấm vào biểu tượng này hoặc dùng action `workspace::ToggleWorktreeSecurity` sẽ mở hộp thoại bảo mật để người dùng tin cậy worktree.

Quyết định tin cậy một worktree được giữ lại giữa các lần khởi động lại. Bạn có thể xóa mọi worktree đã tin cậy bằng lệnh `workspace::ClearTrustedWorktrees`.
Lệnh này khởi động lại Zode để chắc chắn không còn cài đặt, language server hay MCP server chưa tin cậy nào tồn tại.

Tính năng này hoạt động cả ở máy cục bộ lẫn trên host từ xa qua SSH và WSL. Trong các trường hợp đó, Zode theo dõi thông tin tin cậy riêng cho từng host.

## Những gì bị hạn chế

Chế độ hạn chế chặn:

- Cài đặt dự án (`.zode/settings.json`) không được phân tích và áp dụng
- Language server không được cài và chạy
- MCP server không được cài và chạy

## Cấu hình tin cậy rộng cho worktree

Mặc định, Zode không tin cậy worktree mới. Người dùng phải tin cậy từng worktree mới một. Dù không khuyến khích, người dùng có thể tin cậy mọi worktree bằng cài đặt này ([cách chỉnh sửa](./configuring-zode.md#settings-files)):

```json [settings]
"session": {
  "trust_all_worktrees": true
}
```

Worktree được tự động tin cậy không được giữ lại giữa các lần khởi động lại; chỉ worktree được tin cậy thủ công mới được giữ. Nhờ vậy người dùng phải ra quyết định tin cậy mới nếu sau này tắt `trust_all_worktrees`.

## Phân cấp tin cậy

Phần này chủ yếu là chi tiết nội bộ và có thể thay đổi, nhưng giúp giải thích vì sao nhiều yêu cầu tin cậy có thể được duyệt cùng lúc.
Zode có nhiều tầng tin cậy tùy theo yêu cầu, từ thấp đến cao nhất:

- "worktree file đơn"

Sau khi mở một cửa sổ Zode trống, bạn có thể mở một file đơn lẻ. Bạn cũng có thể mở file nằm ngoài thư mục hiện tại sau khi đã mở một thư mục.
Ví dụ thường gặp là `zode: open settings file`, có thể khởi động language server cho file đó và tạo một worktree file đơn mới.

Chạy language server tiềm ẩn rủi ro nếu language server bị tấn công chuỗi cung ứng; vì vậy Zode mặc định hạn chế việc này. Mỗi worktree file đơn cần được cấp quyền tin cậy riêng, trừ khi thư mục chứa nó đã được tin cậy hoặc mọi worktree đã được tin cậy.

- "worktree thư mục"

Nếu một thư mục đang mở trong Zode, nó là một worktree đầy đủ. Nó có thể chạy nhiều language server và MCP server khai báo trong cài đặt dự án. Do đó mỗi worktree thư mục cần được cấp quyền tin cậy riêng, trừ khi đã có quyền tin cậy ở thư mục cha (xem bên dưới).

Khi một worktree thư mục được tin cậy, language server và MCP server được phép tải về và khởi động, nên khi đó chúng tôi cũng tự động bật tin cậy worktree file đơn cho host tương ứng: điều này hữu ích khi mở các file đơn lẻ và dùng tính năng language server trong worktree thư mục đã tin cậy.

- "worktree thư mục cha"

Để ra quyết định tin cậy cho nhiều worktree thư mục cùng lúc, bạn có thể tin cậy mọi thư mục con của một worktree thư mục cha đang mở trong Zode bằng cách đánh dấu hộp kiểm tương ứng. Việc này cấp quyền tin cậy cho mọi thư mục con của nó, gồm cả các worktree thư mục hiện có lẫn sau này.
