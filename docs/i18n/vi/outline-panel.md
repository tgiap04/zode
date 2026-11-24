---
title: Outline Panel - Zode
description: Navigate code structure with Zode's outline panel. View symbols, jump to definitions, and browse file outlines.
---

# Bảng outline

Ngoài outline dạng modal (`cmd-shift-o`), Zode còn có bảng outline. Mở bảng outline bằng `cmd-shift-b` (`outline panel: toggle focus` trong bảng lệnh), hoặc bấm nút `Outline Panel` trên thanh trạng thái.

Với buffer "singleton" (tức một file đơn lẻ trên một tab), bảng outline hoạt động giống outline modal－hiển thị outline các symbol của buffer hiện tại. Mỗi mục có tiền tố loại (như "struct", "fn", "mod", "impl") đứng trước tên symbol, giúp bạn nhận ra ngay đó là symbol gì. Bấm vào một mục để nhảy tới đoạn tương ứng trong file. Outline cũng tự cuộn tới đoạn chứa vị trí con trỏ hiện tại.

![Dùng bảng outline trong buffer singleton](https://zed.dev/img/outline-panel/singleton.png)

## Dùng với multibuffer

Bảng outline phát huy tác dụng rõ nhất với multibuffer. Vài ví dụ:

### Kết quả tìm kiếm dự án

Xem tổng quan kết quả tìm kiếm trên toàn dự án.

![Dùng bảng outline trong multibuffer kết quả tìm kiếm dự án](https://zed.dev/img/outline-panel/project-search.png)

### Chẩn đoán dự án

Xem tóm tắt mọi lỗi và cảnh báo mà language server báo về.

![Dùng bảng outline khi xem multibuffer chẩn đoán dự án](https://zed.dev/img/outline-panel/project-diagnostics.png)

### Tìm mọi tham chiếu

Duyệt nhanh qua mọi tham chiếu khi dùng action `editor: find all references`.

![Dùng bảng outline khi xem multibuffer `find all references`](https://zed.dev/img/outline-panel/find-all-references.png)

Outline giúp nhảy nhanh tới từng phần của code và giữ ngữ cảnh khi làm việc với tập kết quả lớn trong multibuffer.
