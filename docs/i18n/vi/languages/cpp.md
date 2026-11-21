---
title: C++
description: "Configure C++ language support in Zode, including language servers, formatting, and debugging."
---

# C++

C++ được hỗ trợ sẵn trong Zode.

- Tree-sitter: [tree-sitter/tree-sitter-cpp](https://github.com/tree-sitter/tree-sitter-cpp)
- Language Server: [clangd/clangd](https://github.com/clangd/clangd)

## Binary

Bạn có thể cấu hình Zode dùng binary `clangd` nào.

Mặc định, Zode tìm `clangd` trong `$PATH` và dùng nó. Nếu binary đó chạy được thì dùng luôn. Nếu không, Zode tự cài phiên bản `clangd` của riêng nó rồi dùng bản đó.

Nếu muốn cài bản `clangd` pre-release, bạn có thể bảo Zode làm vậy bằng cách đặt `pre_release` thành `true` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "clangd": {
      "fetch": {
        "pre_release": true
      }
    }
  }
}
```

Nếu muốn Zode không tìm binary `clangd` trong hệ thống, đặt `ignore_system_version` thành `true` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "clangd": {
      "binary": {
        "ignore_system_version": true
      }
    }
  }
}
```

Nếu muốn dùng binary ở vị trí tuỳ chọn, chỉ định `path` và `arguments` (không bắt buộc):

```json [settings]
{
  "lsp": {
    "clangd": {
      "binary": {
        "path": "/path/to/clangd",
        "arguments": []
      }
    }
  }
}
```

`"path"` này phải là đường dẫn tuyệt đối.

## Đối số

Bạn có thể truyền bao nhiêu đối số tuỳ ý cho clangd. Để xem đầy đủ các tuỳ chọn, chạy `clangd --help` trong dòng lệnh. Ví dụ với `--function-arg-placeholders=0`, phần gợi ý cho lời gọi hàm chỉ có cặp ngoặc đơn, còn mặc định (`--function-arg-placeholders=1`) gợi ý còn kèm placeholder cho tham số của method.

```json [settings]
{
  "lsp": {
    "clangd": {
      "binary": {
        "path": "/path/to/clangd",
        "arguments": ["--function-arg-placeholders=0"]
      }
    }
  }
}
```

## Định dạng code

Mặc định Zode dùng language server `clangd` để định dạng code C++. Clangd tương đương công cụ CLI `clang-format`. Để cấu hình, bạn có thể thêm file `.clang-format`. Ví dụ:

```yaml
# yaml-language-server: $schema=https://json.schemastore.org/clang-format-21.x.json
---
BasedOnStyle: LLVM
IndentWidth: 4
---
Language: Cpp
# Force pointers to the type for C++.
DerivePointerAlignment: false
PointerAlignment: Left
---
```

Xem [Tuỳ chọn kiểu của Clang-Format](https://clang.llvm.org/docs/ClangFormatStyleOptions.html) để có danh sách đầy đủ.

Bạn có thể kích hoạt định dạng bằng {#kb editor::Format} hoặc action `editor: format` từ bảng lệnh (command palette), hoặc bật format khi lưu.

Cấu hình định dạng trong Settings ({#kb zode::OpenSettings}) ở Languages > C++, hoặc thêm vào file cài đặt:

```json [settings]
  "languages": {
    "C++": {
      "format_on_save": "on",
      "tab_size": 2
    }
  }
```

## Cấu hình server thêm

Ở thư mục gốc dự án, thường người ta tạo file `.clangd` để đặt thêm cấu hình.

```yaml
# yaml-language-server: $schema=https://json.schemastore.org/clangd.json
CompileFlags:
  Add:
    - "--include-directory=/path/to/include"
Diagnostics:
  MissingIncludes: Strict
  UnusedIncludes: Strict
```

Về cách dùng nâng cao của file cấu hình clangd, xem [trang chính thức](https://clangd.llvm.org/config.html).

## Compile Commands

Với một số dự án, Clangd cần file `compile_commands.json` để phân tích dự án đúng. File này chứa cơ sở dữ liệu biên dịch, cho clangd biết dự án được build thế nào.

### Compile Commands của CMake

Với CMake, bạn có thể tự sinh `compile_commands.json` bằng cách thêm dòng sau vào `CMakeLists.txt`:

```cmake
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
```

Sau khi build dự án, CMake sẽ sinh file `compile_commands.json` trong thư mục build và clangd tự động nhận file đó.

## Gỡ lỗi

Bạn có thể dùng CodeLLDB hoặc GDB để gỡ lỗi binary native. (Đảm bảo quy trình build truyền `-g` cho trình biên dịch C++ để binary tạo ra có thông tin gỡ lỗi.) Bên dưới là các ví dụ cấu hình gỡ lỗi bạn có thể thêm vào `.zode/debug.json`.

- [Tài liệu cấu hình CodeLLDB](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md#starting-a-new-debug-session)
- [Tài liệu cấu hình GDB](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Debugger-Adapter-Protocol.html)
  - GDB cần phiên bản từ v14.1 trở lên

### Build và gỡ lỗi binary

```json [debug]
[
  {
    "label": "Debug native binary",
    "build": {
      "command": "make",
      "args": ["-j8"],
      "cwd": "$ZODE_WORKTREE_ROOT"
    },
    "program": "$ZODE_WORKTREE_ROOT/build/prog",
    "request": "launch",
    "adapter": "CodeLLDB"
  }
]
```

## Mở rộng giao thức

Hiện Zode triển khai các [extension](https://clangd.llvm.org/extensions) sau của `clangd`:

### Vùng không hoạt động

Tự động làm mờ các đoạn code không hoạt động do chỉ thị tiền xử lý, như các khối `#if`, `#ifdef` hay `#ifndef` có điều kiện sai.

### Chuyển giữa file nguồn và file header

Cho phép chuyển giữa file nguồn C++ (ví dụ `.cpp`) và file header (ví dụ `.h`) tương ứng
bằng cách chạy lệnh {#action editor::SwitchSourceHeader} từ bảng lệnh hoặc đặt
phím tắt cho action `editor::SwitchSourceHeader`.

```json [keymap]
{
  "context": "Editor",
  "bindings": {
    "alt-enter": "editor::SwitchSourceHeader"
  }
}
```
