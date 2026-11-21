---
title: Release Notes
description: "Guide to release notes for Zode development."
---

# Ghi chú phát hành

Mỗi khi bạn mở pull request, phần nội dung được điền sẵn theo [mẫu pull request](https://github.com/tgiap04/zode/blob/develop/.github/pull_request_template.md) này.

```md
...

Release Notes:

- N/A _or_ Added/Fixed/Improved ...
```

Vào thứ Tư hằng tuần, chúng tôi chạy [`get-preview-channel-changes`](https://github.com/zed-industries/zed/blob/main/script/get-preview-channel-changes). Script này gom các dòng `Release Notes` từ những pull request vào kênh preview, như mô tả trong tài liệu [Release](https://zodekit.com/docs/development/release-notes).

Script xuất toàn bộ nội dung bên dưới dòng `Release Notes`, kèm metadata như tác giả pull request (nếu không phải thành viên nhóm Zode) và đường dẫn tới pull request.
Nếu bạn ghi `N/A`, script bỏ qua hẳn pull request đó.

## Hướng dẫn viết dòng `Release Notes`

- Chỉ viết dòng `Release Notes` khi người dùng nhìn thấy hoặc cảm nhận được sự khác biệt trong Zode.
- Viết dòng `Release Notes` sao cho người dùng Zode hiểu thay đổi là gì.
  Đừng giả định người dùng rành thuật ngữ của dân làm trình soạn thảo; hãy diễn đạt bằng ngôn ngữ của người dùng trình soạn thảo văn bản.
- Muốn đưa chi tiết kỹ thuật của pull request cho các thành viên khác xem, hãy viết phía trên dòng `Release Notes`.
- Thay đổi tài liệu ghi là `N/A`.
- Nếu pull request thêm hoặc đổi một cài đặt hay phím tắt, luôn nêu rõ cài đặt hay phím tắt đó.
  Đừng bắt người dùng đi lục tài liệu hay pull request để tìm (dù thông tin này cũng phải có trong tài liệu).
- Với pull request revert:
  - Nếu thay đổi bị revert **đã phát hành**, thêm dòng `Release Notes` giải thích lý do revert, vì đây là thay đổi gây phá vỡ.
- Nếu thay đổi bị revert **chưa phát hành**, sửa dòng `Release Notes` của PR gốc thành `N/A`; nếu không, dòng đó vẫn bị đưa vào và công cụ biên soạn ghi chú phát hành có thể không biết để bỏ qua.
