---
title: Go
description: "Configure Go language support in Zode, including language servers, formatting, and debugging."
---

# Go

Zode hỗ trợ Go sẵn, không cần cài thêm.

- Tree-sitter: [tree-sitter/tree-sitter-go](https://github.com/tree-sitter/tree-sitter-go)
- Language server: [golang/tools/tree/master/gopls](https://github.com/golang/tools/tree/master/gopls)
- Debug adapter: [delve](https://github.com/go-delve/delve)

## Thiết lập

Chúng tôi khuyên cài gopls qua trình quản lý gói của go, không qua Homebrew hay trình quản lý gói của bản phân phối Linux.

1. Hãy chắc bạn đã gỡ mọi phiên bản gopls cài qua trình quản lý gói:

```sh
# MacOS homebrew
brew remove gopls
# Ubuntu
sudo apt-get remove gopls
sudo snap remove gopls
# Arch
sudo pacman -R gopls
```

2. Cài/Cập nhật `gopls` lên phiên bản mới nhất bằng công cụ module của go:

```sh
go install golang.org/x/tools/gopls@latest
```

3. Đảm bảo `gopls` nằm trong path của bạn:

```sh
which gopls
gopls version
```

Nếu không tìm thấy `gopls`, nhiều khả năng bạn cần thêm `export PATH="$PATH:$HOME/go/bin"` vào `.zshrc` / `.bash_profile`

## Inlay Hint

Zode đặt các tuỳ chọn khởi tạo sau cho inlay hint:

```json
"hints": {
    "assignVariableTypes": true,
    "compositeLiteralFields": true,
    "compositeLiteralTypes": true,
    "constantValues": true,
    "functionTypeParameters": true,
    "parameterNames": true,
    "rangeVariableTypes": true
}
```

để language server gửi lại inlay hint khi Zode bật chúng trong cài đặt.

Dùng

```json
"lsp": {
    "gopls": {
        "initialization_options": {
            "hints": {
                // ....
            }
        }
    }
}
```

để ghi đè các cài đặt này.

Xem [tài liệu inlayHints của gopls](https://github.com/golang/tools/blob/master/gopls/doc/inlayHints.md) để biết thêm.

## Gỡ lỗi

Zode hỗ trợ gỡ lỗi không cần cấu hình cho test Go và điểm vào (`func main`) bằng Delve. Chạy {#action debugger::Start} ({#kb debugger::Start}) để xem danh sách theo ngữ cảnh của các task debug cấu hình sẵn này.

Để kiểm soát nhiều hơn, bạn thêm cấu hình debug vào `.zode/debug.json`. Xem ví dụ bên dưới.

- [Tài liệu cấu hình Delve](https://github.com/go-delve/delve/blob/master/Documentation/api/dap/README.md#launch-and-attach-configurations)

### Debug package Go

Để debug một package cụ thể, đặt chế độ Delve là "debug". Khi đó "program" nên đặt là tên package.

```json [debug]
[
  {
    "label": "Go (Delve)",
    "adapter": "Delve",
    "program": "$ZODE_FILE",
    "request": "launch",
    "mode": "debug"
  },
  {
    "label": "Run server",
    "adapter": "Delve",
    "request": "launch",
    "mode": "debug",
    // For Delve, the program can be a package name
    "program": "./cmd/server"
    // "args": [],
    // "buildFlags": [],
  }
]
```

### Debug test Go

Để debug test của một package, đặt chế độ Delve là "test".
"program" vẫn là tên package, và bạn dùng "buildFlags" để làm những việc như đặt tag, còn "args" để đặt đối số cho binary test. (Xem `go help testflags` để biết thêm.)

```json [debug]
[
  {
    "label": "Run integration tests",
    "adapter": "Delve",
    "request": "launch",
    "mode": "test",
    "program": ".",
    "buildFlags": ["-tags", "integration"]
    // To filter down to just the test your cursor is in:
    // "args": ["-test.run", "$ZODE_SYMBOL"]
  }
]
```

### Build và debug riêng

Nếu cần build ứng dụng bằng một lệnh cụ thể, bạn dùng chế độ "exec" của Delve. Khi đó "program" nên trỏ tới một file thực thi,
và lệnh "build" nên build ra file đó.

```json [debug]
[
  {
    "label": "Debug Prebuilt Unit Tests",
    "adapter": "Delve",
    "request": "launch",
    "mode": "exec",
    "program": "${ZODE_WORKTREE_ROOT}/__debug_unit",
    "args": ["-test.v", "-test.run=${ZODE_SYMBOL}"],
    "build": {
      "command": "go",
      "args": [
        "test",
        "-c",
        "-tags",
        "unit",
        "-gcflags\"all=-N -l\"",
        "-o",
        "__debug_unit",
        "./pkg/..."
      ]
    }
  }
]
```

### Attach vào một instance Delve có sẵn

Có thể bạn cần kết nối tới một instance Delve có sẵn, không nhất thiết chạy trên máy bạn; khi đó bạn dùng `tcp_arguments` để điều khiển kết nối của Zode tới Delve.

```json [debug]
[
  {
    "adapter": "Delve",
    "label": "Connect to a running Delve instance",
    "program": "/Users/zed/Projects/language_repositories/golang/hello/hello",
    "cwd": "/Users/zed/Projects/language_repositories/golang/hello",
    "args": [],
    "env": {},
    "request": "launch",
    "mode": "exec",
    "stopOnEntry": false,
    "tcp_connection": { "host": "127.0.0.1", "port": 53412 }
  }
]
```

Khi đó Zode không khởi chạy instance Delve mới mà dùng cái có sẵn. Hệ quả là _trong Zode sẽ không có terminal_; bạn phải tương tác trực tiếp với instance Delve, vì nó xử lý stdin/stdout của chương trình bị debug.

## Go Mod

- Tree-sitter: [camdencheek/tree-sitter-go-mod](https://github.com/camdencheek/tree-sitter-go-mod)
- Language server: N/A

## Go Sum

- Tree-sitter: [amaanq/tree-sitter-go-sum](https://github.com/amaanq/tree-sitter-go-sum)
- Language server: N/A

## Go Work

- Tree-sitter:
  [tree-sitter-go-work](https://github.com/d1y/tree-sitter-go-work)
- Language server: N/A
