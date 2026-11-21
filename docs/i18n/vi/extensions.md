---
title: Extensions
description: "Extend Zode with themes, language support, AI tools, and more through the extension system."
---

# Extension

Zode cho phép bạn thêm chức năng mới bằng các extension do người dùng định nghĩa.

- [Cài đặt extension](./extensions/installing-extensions.md)
- [Khả năng của extension](./extensions/capabilities.md)
- [Phát triển extension](./extensions/developing-extensions.md)
  - [Phát triển extension ngôn ngữ](./extensions/languages.md)
  - [Phát triển extension trình gỡ lỗi](./extensions/debugger-extensions.md)
  - [Phát triển giao diện (theme)](./extensions/themes.md)
  - [Phát triển bộ biểu tượng](./extensions/icon-themes.md)
  - [Phát triển snippet](./extensions/snippets.md)

## Extension agent-server không được hỗ trợ

Một extension có thể khai báo agent server trong `extension.toml`:

```toml
[agent_servers.myagent]
name = "My Agent"
```

Bản build này không chạy loại đó. Trường `agent_servers` đã bị gỡ cùng với
ACP registry mà nó được tạo ra để nuôi, và không có gì thay thế — agent ở đây là
năm CLI tích hợp sẵn trên thanh bên (`claude`, `codex`, `agy`, `copilot`,
`opencode`), mỗi cái chạy như một phiên terminal, cộng với bất kỳ lệnh nào bạn tự
đặt tên trong
**cài đặt** `agent_servers` (khác với trường trong manifest;
xem [Cấu hình Zode](./configuring-zode.md)).

Trong thực tế điều này có nghĩa là:

- Extension có manifest chứa mục `agent_servers` **vẫn cài được**,
  và mọi khả năng khác của nó vẫn hoạt động. Mục đó bị bỏ qua, không bị từ chối
  — trình phân tích manifest của bản build này không cấm trường lạ.
- Vì bị bỏ qua chứ không bị từ chối, lúc cài đặt không có gì báo cho bạn.
  Khi extension registry có báo khả năng này, trang extension gắn nhãn
  **"Agent Servers (unsupported)"**, đó là cảnh báo duy nhất bạn nhận được.
- Biểu ngữ của Zed gốc từng nói extension agent-server đang bị loại bỏ dần
  "in favor of the ACP registry". Ở đây không xảy ra như vậy: registry cũng bị
  gỡ. Không có đường chuyển đổi, và biểu ngữ đã biến mất.

Bản build này không tải agent nào từ URL hay kho gói. Nếu bạn muốn dùng một
agent, hãy tự cài CLI của nó; nút trên thanh bên sẽ đưa lệnh cài của nhà cung cấp
khi thiếu CLI.
