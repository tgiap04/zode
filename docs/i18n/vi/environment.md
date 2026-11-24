---
title: Environment Variables - Zode
description: How Zode detects and uses environment variables. Shell integration, dotenv support, and troubleshooting.
---

# Biến môi trường

_**Lưu ý**: Nội dung dưới đây chỉ áp dụng cho Zode 0.152.0 trở lên._

Nhiều tính năng trong Zode chịu ảnh hưởng của biến môi trường:

- [Task](./tasks.md)
- [Terminal tích hợp](./terminal.md)
- Tìm language server
- Language server

Để dùng tốt các tính năng này, bạn nên hiểu Zode lấy biến môi trường từ đâu và dùng chúng ra sao.

## Zode lấy biến môi trường từ đâu?

Cách Zode khởi động quyết định nó dùng được những biến môi trường nào. Gồm cả việc mở từ Dock của macOS, từ trình quản lý cửa sổ trên Linux, hoặc từ CLI `zode`.

### Mở từ CLI

Nếu Zode được mở qua CLI (`zode`), nó kế thừa biến môi trường từ phiên shell xung quanh.

Nghĩa là nếu bạn chạy

```
$ export MY_ENV_VAR=hello
$ zode .
```

thì biến môi trường `MY_ENV_VAR` có sẵn bên trong Zode. Ví dụ, trong terminal tích hợp.

Từ Zode 0.152.0, CLI `zode` _luôn_ truyền môi trường của nó cho Zode, bất kể trước đó đã có tiến trình Zode nào đang chạy hay chưa. Trước Zode 0.152.0 không như vậy và chỉ tiến trình Zode đầu tiên kế thừa biến môi trường.

### Mở từ trình quản lý cửa sổ, Dock hoặc launcher

Khi Zode được mở từ Dock của macOS, từ biểu tượng GNOME hoặc KDE trên Linux, hay từ trình khởi chạy ứng dụng như Alfred hoặc Raycast, nó không có môi trường shell xung quanh để kế thừa biến môi trường.

Để vẫn có một môi trường dùng được, Zode tạo một login shell trong thư mục home của người dùng và đọc môi trường của shell đó. Môi trường này sau đó được đặt lên _tiến trình_ Zode, nên mọi cửa sổ và dự án của Zode đều kế thừa nó.

Vì cách đó gây vấn đề cho người cần biến môi trường khác nhau theo từng dự án (ví dụ với `direnv`, `asdf` hoặc `mise`), Zode tạo thêm một login shell nữa khi mở dự án. Shell thứ hai này chạy trong thư mục của dự án. Môi trường từ shell đó _không_ được đặt lên tiến trình, vì nếu không thì mở một dự án mới sẽ đổi môi trường của mọi cửa sổ Zode. Thay vào đó, môi trường này được lưu lại và truyền đi khi chạy task, mở terminal hoặc khởi động language server.

## Biến môi trường được dùng ở đâu và thế nào?

Có hai nhóm biến môi trường:

1. Biến môi trường của tiến trình Zode
2. Biến môi trường lưu theo từng dự án

Biến ở nhóm (1) luôn được dùng, vì chúng nằm ngay trên tiến trình và mọi tiến trình con (task, terminal, language server, ...) kế thừa chúng theo mặc định.

Biến ở nhóm (2) được dùng một cách tường minh, tuỳ tính năng.

### Task

Task được khởi chạy với môi trường gộp. Theo thứ tự ưu tiên (thấp đến cao, giá trị sau ghi đè giá trị trước):

- môi trường của tiến trình Zode
- nếu dự án được mở từ CLI: môi trường của CLI
- nếu dự án không được mở từ CLI: biến môi trường của dự án, lấy bằng cách chạy login shell trong thư mục gốc của dự án
- tuỳ chọn, môi trường được cấu hình tường minh trong cài đặt

### Terminal tích hợp

Terminal tích hợp, giống task, được khởi chạy với môi trường gộp. Theo thứ tự ưu tiên (thấp đến cao):

- môi trường của tiến trình Zode
- nếu dự án được mở từ CLI: môi trường của CLI
- nếu dự án không được mở từ CLI: biến môi trường của dự án, lấy bằng cách chạy login shell trong thư mục gốc của dự án
- tuỳ chọn, môi trường được cấu hình tường minh trong cài đặt

### Tìm language server

Với một số ngôn ngữ, adapter của language server tìm file thực thi trong `$PATH` của người dùng. Ví dụ:

- Go
- Zig
- Rust (nếu [được cấu hình như vậy](./languages/rust.md#binary))
- C
- TypeScript

Cho việc tìm này, Zode dùng môi trường sau:

- nếu dự án được mở từ CLI: môi trường của CLI
- nếu dự án không được mở từ CLI: biến môi trường của dự án, lấy bằng cách chạy login shell trong thư mục gốc của dự án

### Language server

Sau khi tìm được language server, Zode khởi chạy chúng.

Các tiến trình language server này luôn kế thừa môi trường tiến trình của Zode. Nhưng tuỳ cách tìm language server, có thể có thêm biến môi trường được đặt hoặc ghi đè lên môi trường tiến trình.

- Nếu language server được tìm thấy trong `$PATH` của môi trường dự án, thì môi trường dự án đó được truyền cho tiến trình language server. Môi trường dự án đến từ đâu tuỳ vào cách dự án được mở (qua CLI hay không). Xem mục trước về việc tìm language server.
- Nếu không tìm thấy language server trong môi trường dự án, Zode thử cài và chạy nó ở cấp toàn cục. Khi đó tiến trình kế thừa môi trường tiến trình của Zode và, nếu dự án được mở qua CLI, cả môi trường của CLI.
