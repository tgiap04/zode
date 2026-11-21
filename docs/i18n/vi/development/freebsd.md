---
title: Building Zode for FreeBSD
description: "Guide to building Zode for freebsd for Zode development."
---

# Build Zode cho FreeBSD

FreeBSD hiện chưa là nền tảng được hỗ trợ chính thức, nên hướng dẫn này vẫn đang hoàn thiện.

## Repository

Clone [repository của Zode](https://github.com/tgiap04/zode).

## Phụ thuộc

- Cài các gói hệ thống cần thiết và rustup:

  ```sh
  script/freebsd
  ```

  Nếu muốn, bạn có thể đọc [`script/freebsd`](https://github.com/tgiap04/zode/blob/develop/script/freebsd) và tự làm từng bước.

## Build từ mã nguồn

Cài xong các phụ thuộc là bạn build được Zode bằng [Cargo](https://doc.rust-lang.org/cargo/).

Để build bản debug của trình soạn thảo:

```sh
cargo run
```

Và để chạy test:

```sh
cargo test --workspace
```

Ở chế độ release, giao diện chính là crate `cli`. Bạn có thể chạy nó khi phát triển bằng:

```sh
cargo run -p cli
```

### Lưu ý về WebRTC

Build `webrtc-sys` trên FreeBSD hiện bị lỗi vì upstream chưa hỗ trợ và không có binary dựng sẵn. Vì vậy các tính năng cộng tác phụ thuộc WebRTC (gọi thoại và chia sẻ màn hình) tạm thời bị tắt.

Xem thêm [Issue #15309: FreeBSD Support] và [Discussion #29550: Unofficial FreeBSD port for Zode].

## Xử lý sự cố

### Cargo báo lỗi một phụ thuộc đang dùng tính năng unstable

Thử `cargo clean` rồi `cargo build`.
