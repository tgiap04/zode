---
title: OCaml
description: "Configure OCaml language support in Zode, including language servers, formatting, and debugging."
---

# OCaml

Hỗ trợ OCaml có sẵn qua [extension OCaml](https://github.com/zed-extensions/ocaml).

- Tree-sitter: [tree-sitter/tree-sitter-ocaml](https://github.com/tree-sitter/tree-sitter-ocaml)
- Language Server: [ocaml/ocaml-lsp](https://github.com/ocaml/ocaml-lsp)

## Hướng dẫn thiết lập

Nếu môi trường phát triển đã sẵn sàng, bạn có thể nhảy tới [Khởi chạy Zode](#launching-zed)

### Dùng Opam

Opam là trình quản lý package chính thức của OCaml và rất nên dùng khi mới bắt đầu với OCaml. Để bắt đầu với Opam, làm theo hướng dẫn [tại đây](https://ocaml.org/install).

Sau khi cài opam và tạo switch cho môi trường phát triển theo hướng dẫn, bạn có thể làm tiếp.

### Khởi chạy Zode

Đến đây bạn đã cài `ocamllsp`, có thể kiểm tra bằng cách chạy

```sh
ocamllsp --help
```

trong terminal. Nếu thấy thông báo trợ giúp là ổn. Nếu không, hãy xem lại hướng dẫn cài `ocamllsp` và chắc chắn nó đã được cài đúng.

Xong phần đó, ta có thể khởi chạy Zode. Vì cách trình quản lý package của OCaml hoạt động, bạn cần chạy Zode từ terminal, nên hãy cài [Zode cli](https://zodekit.com/docs/reference/cli) nếu chưa có.

Có cli rồi, chỉ cần vào thư mục dự án trong terminal và chạy

```sh
zode .
```

Zode sẽ chạy với hỗ trợ OCaml, không cần thiết lập thêm.
