---
title: Swift
description: "Configure Swift language support in Zode, including language servers, formatting, and debugging."
---

# Swift

Hỗ trợ ngôn ngữ Swift trong Zode do [extension Swift](https://github.com/zed-extensions/swift) của cộng đồng cung cấp.
Báo lỗi tại: [https://github.com/zed-extensions/swift/issues](https://github.com/zed-extensions/swift/issues)

- Tree-sitter: [alex-pinkus/tree-sitter-swift](https://github.com/alex-pinkus/tree-sitter-swift)
- Language Server: [swiftlang/sourcekit-lsp](https://github.com/swiftlang/sourcekit-lsp)
- Debug Adapter: [`lldb-dap`](https://github.com/swiftlang/llvm-project/blob/next/lldb/tools/lldb-dap/README.md)

## Cấu hình language server

Bạn có thể chỉnh hành vi của SourceKit LSP bằng cách tạo `.sourcekit-lsp/config.json` trong thư mục home hoặc thư mục gốc dự án. Xem [file cấu hình SourceKit-LSP](https://github.com/swiftlang/sourcekit-lsp/blob/main/Documentation/Configuration%20File.md) để có tài liệu đầy đủ.

## Gỡ lỗi

Extension Swift cung cấp một debug adapter để gỡ lỗi code Swift.
Tên Zode đặt cho adapter này (trên giao diện và trong `debug.json`) là `Swift`, bên dưới nó dùng [`lldb-dap`](https://github.com/swiftlang/llvm-project/blob/next/lldb/tools/lldb-dap/README.md) đi kèm Swift toolchain.
Extension tìm binary `lldb-dap` bằng `swiftly`, rồi `xcrun`, rồi tìm trong `$PATH`, theo thứ tự ưu tiên đó.
Extension không tự tải `lldb-dap` nếu không tìm thấy.

- [Tài liệu cấu hình lldb-dap](https://github.com/llvm/llvm-project/blob/main/lldb/tools/lldb-dap/README.md#configuration-settings-reference)

### Ví dụ

#### Build và gỡ lỗi một binary Swift

```json [debug]
[
  {
    "label": "Debug Swift",
    "build": {
      "command": "swift",
      "args": ["build"]
    },
    "program": "$ZODE_WORKTREE_ROOT/swift-app/.build/arm64-apple-macosx/debug/swift-app",
    "request": "launch",
    "adapter": "Swift"
  }
]
```
