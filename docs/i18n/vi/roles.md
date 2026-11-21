---
title: Roles - Zode
description: Understand Zode's organization roles and what each role can access, manage, and configure.
---

# Vai trò

Mỗi thành viên của một tổ chức Zode được gán một vai trò quyết định
những gì họ truy cập và cấu hình được.

## Các loại vai trò {#roles}

Mỗi thành viên của tổ chức được gán một trong ba vai trò:

| Vai trò    | Mô tả                                                  |
| ---------- | ------------------------------------------------------ |
| **Owner**  | Toàn quyền, gồm thanh toán và chuyển quyền sở hữu      |
| **Admin**  | Toàn quyền, trừ thanh toán                             |
| **Member** | Quyền truy cập tiêu chuẩn, không có thao tác đặc quyền |

### Owner {#role-owner}

Owner có toàn quyền với tổ chức, gồm:

- Mời và xoá thành viên
- Gán và đổi vai trò của thành viên
- Quản lý thanh toán, phương thức thanh toán và hoá đơn
- Cấu hình chính sách chia sẻ dữ liệu
- Tắt các tính năng cộng tác của Zode
- Kiểm soát việc thành viên có được dùng model do Zode host và edit prediction của Zode hay không
- Chuyển quyền sở hữu cho thành viên khác

### Admin {#role-admin}

Admin có cùng khả năng với Owner, trừ việc họ không thể:

- Truy cập hay sửa cài đặt thanh toán
- Chuyển quyền sở hữu tổ chức

Vai trò này hợp với trưởng nhóm hoặc quản lý lo việc cấp quyền thành viên
hằng ngày mà không cần xem chi tiết thanh toán.

### Member {#role-member}

Member có quyền truy cập Zode tiêu chuẩn. Họ không truy cập được thanh toán hay
cài đặt tổ chức.

## Quản lý vai trò người dùng {#managing-users}

Owner và Admin quản lý thành viên tổ chức từ dashboard của Zode
tại trang Members.

### Mời thành viên {#inviting-members}

1. Ở trang Members, chọn **+ Invite Member**.
2. Nhập email công ty của thành viên và chọn vai trò.
3. Người được mời nhận email hướng dẫn tham gia. Sau khi
   chấp nhận, họ xác thực qua GitHub.

### Đổi vai trò của thành viên {#changing-roles}

1. Ở trang Members, tìm thành viên. Bạn có thể lọc theo vai trò hoặc
   tìm theo tên.
2. Mở menu ba chấm và chọn vai trò mới.

### Xoá thành viên {#removing-members}

1. Ở trang Members, tìm thành viên.
2. Chọn **Remove** và xác nhận.

Xoá một thành viên là gỡ quyền truy cập của họ vào cài đặt tổ chức và mọi tính năng do tổ chức quản lý. Họ vẫn tiếp tục dùng Zode cho riêng mình.
