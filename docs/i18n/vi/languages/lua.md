---
title: Lua
description: "Configure Lua language support in Zode, including language servers, formatting, and debugging."
---

# Lua

Hỗ trợ Lua có sẵn qua [extension Lua](https://github.com/zed-extensions/lua).

- Tree-sitter: [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua)
- Language server: [LuaLS/lua-language-server](https://github.com/LuaLS/lua-language-server)

## luarc.json

Để cấu hình LuaLS, bạn có thể tạo file `.luarc.json` ở thư mục gốc của dự án.

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "format.enable": true,
  "workspace.library": ["../somedir/library"]
}
```

Xem [tài liệu cài đặt LuaLS](https://luals.github.io/wiki/settings/) để biết mọi tùy chọn cấu hình. Khi sửa file này trong Zode, các tùy chọn cài đặt sẽ được gợi ý tự động (ví dụ `runtime.version` sẽ hiện `"Lua 5.1"`, `"Lua 5.2"`, `"Lua 5.3"`, `"Lua 5.4"` và `"LuaJIT"` là các giá trị hợp lệ). Lưu ý khi chuyển các tùy chọn cài đặt từ VS Code sang, hãy bỏ tiền tố `Lua.` (ví dụ dùng `runtime.version` thay vì `Lua.runtime.version`).

### Định nghĩa LuaCATS

LuaLS có thể đưa ra gợi ý tự động LSP phong phú hơn và kiểm tra kiểu nhờ các định nghĩa LuaCATS (Lua Comment and Type System). Các định nghĩa này có sẵn cho nhiều thư viện Lua phổ biến, và có thể chỉ định đường dẫn cục bộ chứa chúng qua `workspace.library` trong `luarc.json`. Bạn có thể dùng đường dẫn tương đối nếu checkout các định nghĩa vào cùng thư mục cha với dự án (`../playdate-luacats`, `../love2d`, v.v.). Hoặc tạo submodule trong dự án cho từng repo định nghĩa LuaCATS.

### LÖVE (Love2D) {#love2d}

Để dùng [LÖVE (Love2D)](https://love2d.org/) trong Zode, checkout [LuaCATS/love2d](https://github.com/LuaCATS/love2d) vào thư mục tên `love2d-luacats` nằm trong thư mục cha của dự án:

```sh
cd .. && git clone https://github.com/LuaCATS/love2d love2d-luacats
```

Sau đó trong `.luarc.json`:

```
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "workspace.library": ["../love2d-luacats"],
  "runtime.special": {
    "love.filesystem.load": "loadfile"
  }
}
```

### PlaydateSDK

Để dùng [Playdate Lua SDK](https://play.date/dev/) trong Zode, checkout [playdate-luacats](https://github.com/notpeter/playdate-luacats) vào thư mục cha của dự án:

```sh
cd .. && git clone https://github.com/notpeter/playdate-luacats
```

Sau đó trong `.luarc.json`:

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.4",
  "runtime.nonstandardSymbol": [
    "+=",
    "-=",
    "*=",
    "/=",
    "//=",
    "%=",
    "<<=",
    ">>=",
    "&=",
    "|=",
    "^="
  ],
  "diagnostics.severity": { "duplicate-set-field": "Hint" },
  "diagnostics.globals": ["import"],
  "workspace.library": ["../playdate-luacats"],
  "format.defaultConfig": {
    "indent_style": "space",
    "indent_size": "4"
  },
  "format.enable": true,
  "runtime.builtin": { "io": "disable", "os": "disable", "package": "disable" }
}
```

### Inlay Hint

Để bật [Inlay Hint](../configuring-languages.md#inlay-hints) cho LuaLS trong Zode

1. Cấu hình inlay hint trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Lua, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Lua": {
      "inlay_hints": {
        "enabled": true,
        "show_type_hints": true,
        "show_parameter_hints": true,
        "show_other_hints": true
      }
    }
  }
}
```

2. Thêm `"hint.enable": true` vào `.luarc.json` của bạn.

## Định dạng mã

### Định dạng bằng LuaLS

Để bật tự động định dạng bằng LuaLS (do [CppCXY/EmmyLuaCodeStyle](https://github.com/CppCXY/EmmyLuaCodeStyle) cung cấp), đảm bảo trong .luarc.json có `"format.enable": true,`:

```json
{
  "$schema": "https://raw.githubusercontent.com/sumneko/vscode-lua/master/setting/schema.json",
  "format.enable": true
}
```

Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Lua, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Lua": {
      "format_on_save": "on",
      "formatter": "language_server"
    }
  }
}
```

Bạn có thể tùy chỉnh nhiều tùy chọn kiểu của EmmyLuaCodeStyle qua `.editorconfig`, xem [lua.template.editorconfig](https://github.com/CppCXY/EmmyLuaCodeStyle/blob/master/lua.template.editorconfig) để biết mọi tùy chọn.

### Định dạng bằng StyLua

Hoặc dùng [StyLua](https://github.com/JohnnyMorganz/StyLua) để tự động định dạng:

1. Cài [StyLua](https://github.com/JohnnyMorganz/StyLua): `brew install stylua` hoặc `cargo install stylua --features lua52,lua53,lua54,luau,luajit` (cứ bỏ bớt phiên bản Lua nào bạn không cần).
2. Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Lua, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Lua": {
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "stylua",
          "arguments": [
            "--syntax=Lua54",
            "--respect-ignores",
            "--stdin-filepath",
            "{buffer_path}",
            "-"
          ]
        }
      }
    }
  }
}
```

Bạn có thể chỉ định nhiều tùy chọn cho StyLua ngay trên dòng lệnh ở trên (như `--syntax=Lua54`) hoặc trong `stylua.toml` của dự án:

```toml
syntax = "Lua54"
column_width = 100
line_endings = "Unix"
indent_type = "Spaces"
indent_width = 4
quote_style = "AutoPreferDouble"
call_parentheses = "Always"
collapse_simple_statement = "All"

[sort_requires]
enabled = true
```

Danh sách đầy đủ các tùy chọn xem tại: [Tùy chọn StyLua](https://github.com/JohnnyMorganz/StyLua?tab=readme-ov-file#options).
