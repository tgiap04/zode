---
title: Toolchains
description: "Zode projects offer a dedicated UI for toolchain selection, which lets you pick a set of tools for working with a given language in a current project."
---

# Toolchain

Dự án Zode có bộ chọn toolchain để bạn chọn các công cụ dùng cho một ngôn ngữ trong dự án hiện tại.

Ví dụ, trong dự án Python, virtual environment định nghĩa dependency và đường dẫn interpreter. Language server cần môi trường đó để phân tích code đúng.
Với bộ chọn toolchain, bạn chọn virtual environment phù hợp từ danh sách thả xuống thay vì cấu hình đường dẫn language server bằng tay.

Bạn thậm chí có thể chọn toolchain khác nhau cho các dự án con khác nhau trong dự án Zode. Thế nào là dự án con tùy theo từng ngôn ngữ.
Khi cộng tác, chỉ chủ dự án mới thấy và sửa được toolchain đang hoạt động.

Trong [dự án từ xa](./remote-development.md), bạn có thể dùng bộ chọn toolchain để điều khiển toolchain đang hoạt động trên SSH host.

## Vì sao cần toolchain?

Toolchain đang hoạt động được dùng khi khởi chạy language server. Không có toolchain đúng, language server có thể không resolve được dependency, và các tính năng như "Go to Definition" hay "Code Completions" có thể không chạy.

Toolchain đang hoạt động cũng liên quan khi mở shell trong bảng terminal: một số toolchain cung cấp "activation script" cho shell, giúp toolchain đó dùng được ngay trong môi trường shell. Zode tự chạy các activation script này khi bạn tạo terminal mới.

Điều này cũng áp dụng cho [task](./tasks.md). Zode chạy task như thể bạn mở một tab terminal mới và tự chạy lệnh của task, nên việc chạy task cũng chịu ảnh hưởng của toolchain đang hoạt động và activation script của nó.

## Chọn toolchain

Toolchain đang hoạt động (nếu có) hiện ở thanh trạng thái bên phải. Nhấn vào đó để mở bộ chọn toolchain, hoặc chạy action trong bảng lệnh ({#action toolchain::Select}).

Zode tự suy ra một tập toolchain để bạn chọn dựa trên dự án đang làm. Một toolchain mặc định cũng được chọn giúp bạn theo kiểu cố gắng hết sức (best-effort) khi bạn mở dự án lần đầu.

Việc chọn toolchain áp dụng cho dự án con hiện tại, có thể là cả dự án hoặc chỉ một phần của nó. Ví dụ trong monorepo, bạn có thể chọn toolchain khác nhau cho từng dự án con.

## Thêm toolchain thủ công

Nếu tự phát hiện chưa đủ, bạn có thể thêm toolchain thủ công. Nhấn nút "Add toolchain" trong bộ chọn toolchain. Ở đó bạn nhập đường dẫn tới toolchain và đặt tên tùy ý.
