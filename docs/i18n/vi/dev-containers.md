---
title: Dev Containers - Zode
description: Open projects in dev containers with Zode. Reproducible development environments using devcontainer.json configuration.
---

# Dev Containers

Dev Containers cho bạn một môi trường phát triển nhất quán, tái lập được, bằng cách khai báo dependency, công cụ và cài đặt của dự án trong một cấu hình container.

Nếu repository có file `.devcontainer/devcontainer.json`, Zode có thể mở dự án bên trong dev container.

## Yêu cầu

- Máy phải cài Docker và sẵn sàng trong `PATH`. Zode cần có lệnh `docker`. Nếu dùng Podman, hãy đặt alias thành `docker`, ví dụ tạo symlink: `sudo ln -s $(which podman) {some_known_path}/docker`.
- Dự án phải có thư mục/file `.devcontainer/devcontainer.json`.

## Dùng Dev Containers trong Zode

### Hộp thoại tự động

Khi bạn mở một dự án có thư mục/file `.devcontainer/devcontainer.json`, Zode hiện hộp thoại hỏi có mở dự án trong dev container hay không. Chọn "Open in Container" sẽ:

1. Build image của dev container (nếu cần).
2. Khởi chạy container.
3. Mở lại dự án, kết nối vào môi trường container.

### Mở thủ công

Nếu bạn đã đóng hộp thoại hoặc muốn mở lại dự án trong container sau này, hãy dùng bảng lệnh (command palette) của Zode để chạy lệnh "Project: Open Remote" và chọn mục mở dự án trong dev container.
Hoặc bạn có thể mở modal Remote Projects (bằng phím tắt {#kb projects::OpenRemote}) và chọn "Connect Dev Container".

## Sửa cấu hình dev container

Nếu bạn sửa `.devcontainer/devcontainer.json`, Zode hiện chưa tự rebuild hay tải lại container. Sau khi đổi cấu hình:

- Dừng hoặc kill container đang chạy bằng tay (ví dụ `docker kill <container>`).
- Mở lại dự án trong container.

## Làm việc trong Dev Container

Sau khi kết nối, Zode chạy task, terminal và language server bên trong môi trường container.
File từ workspace của bạn được liên kết vào container theo đặc tả dev container.

## Hạn chế đã biết

> **Lưu ý:** Tính năng này vẫn đang phát triển.

- **Extension:** Zode chưa quản lý extension riêng cho môi trường container. Extension của máy host được dùng nguyên trạng.
- **Chuyển tiếp cổng:** Chỉ hỗ trợ trường `appPort`. `forwardPorts` và các tính năng chuyển tiếp cổng nâng cao khác chưa được triển khai.
- **Thay đổi cấu hình:** Cập nhật `devcontainer.json` không kích hoạt rebuild hay tải lại tự động; phải khởi động lại container bằng tay.

## Xem thêm

- [Remote Development](./remote-development.md) để kết nối tới server từ xa qua SSH.
- [Task](./tasks.md) để chạy lệnh trong terminal tích hợp.
