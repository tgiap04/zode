---
title: C
description: "Configure C language support in Zode, including language servers, formatting, and debugging."
---

# C

Zode hỗ trợ C sẵn, không cần cài thêm.

- Tree-sitter: [tree-sitter/tree-sitter-c](https://github.com/tree-sitter/tree-sitter-c)
- Language server: [clangd/clangd](https://github.com/clangd/clangd)
- Debug adapter: [CodeLLDB](https://github.com/vadimcn) (chính), [GDB](https://sourceware.org/gdb/) (phụ, không có trên Apple silicon)

## Clangd: Ép nhận diện là C

Clangd mặc định giả định dự án trộn lẫn C++/C. Nếu dự án của bạn chỉ có C, bạn có thể muốn bảo clangd coi mọi file là C bằng cờ `-xc`. Để làm vậy, tạo file `.clangd` ở gốc dự án với nội dung sau:

```yaml
# yaml-language-server: $schema=https://json.schemastore.org/clangd.json
CompileFlags:
  Add: [-xc]
```

Mặc định clang và gcc nhận `*.C` và `*.H` (đuôi viết hoa) là C++ chứ không phải C, nên Zode cũng theo quy ước này. Nếu bạn làm việc với dự án chỉ có C (có thể có cách đặt tên viết hoa kiểu cũ như `FILENAME.C`), bạn ghi đè hành vi này bằng cách thêm đoạn sau vào cài đặt:

```json [settings]
{
  "file_types": {
    "C": ["C", "H"]
  }
}
```

## Định dạng

Mặc định Zode dùng language server `clangd` để định dạng mã C như công cụ CLI `clang-format`. Để cấu hình, bạn thêm file `.clang-format`. Ví dụ:

```yaml
# yaml-language-server: $schema=https://json.schemastore.org/clang-format-21.x.json
---
BasedOnStyle: GNU
IndentWidth: 2
---
```

Xem [Clang-Format Style Options](https://clang.llvm.org/docs/ClangFormatStyleOptions.html) để biết danh sách đầy đủ các tuỳ chọn.

Bạn kích hoạt định dạng bằng {#kb editor::Format} hoặc action `editor: format` từ bảng lệnh (command palette), hoặc bật định dạng khi lưu.

Cấu hình định dạng trong Settings ({#kb zode::OpenSettings}) tại Languages > C, hoặc thêm vào file settings:

```json [settings]
  "languages": {
    "C": {
      "format_on_save": "on",
      "tab_size": 2
    }
  }
```

## Compile Commands

Với một số dự án, Clangd cần file `compile_commands.json` để phân tích dự án đúng. File này chứa cơ sở dữ liệu biên dịch cho clangd biết dự án được build thế nào.

### Compile Commands của CMake

Với CMake, bạn tạo `compile_commands.json` tự động bằng cách thêm dòng sau vào `CMakeLists.txt`:

```cmake
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
```

Sau khi build dự án, CMake sẽ tạo file `compile_commands.json` trong thư mục build và clangd tự động nhận nó.

## Gỡ lỗi

Bạn dùng CodeLLDB hoặc GDB để debug binary native. (Hãy chắc quy trình build truyền `-g` cho trình biên dịch C, để thông tin debug nằm trong binary tạo ra.) Xem bên dưới các ví dụ cấu hình debug có thể thêm vào `.zode/debug.json`.

- [Tài liệu cấu hình CodeLLDB](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md#starting-a-new-debug-session)
- [Tài liệu cấu hình GDB](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Debugger-Adapter-Protocol.html)

### Build và debug binary

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
