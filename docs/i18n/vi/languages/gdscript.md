---
title: GDScript
description: "Configure GDScript language support in Zode, including language servers, formatting, and debugging."
---

# GDScript

Hỗ trợ ngôn ngữ [GDScript](https://gdscript.com/) của Godot trong Zode do [extension GDScript](https://github.com/GDQuest/zed-gdscript) do cộng đồng duy trì cung cấp.
Báo lỗi tại: [https://github.com/GDQuest/zed-gdscript/issues](https://github.com/GDQuest/zed-gdscript/issues)

- Tree-sitter: [PrestonKnopp/tree-sitter-gdscript](https://github.com/PrestonKnopp/tree-sitter-gdscript) và [PrestonKnopp/tree-sitter-godot-resource](https://github.com/PrestonKnopp/tree-sitter-godot-resource)
- Language Server: [gdscript-language-server](https://github.com/godotengine/godot)

## Điều kiện cần

Bạn cần có:

- [Godot](https://godotengine.org/download/).
- netcat (`nc` hoặc `ncat`) trong PATH của hệ thống.

## Cài đặt

1. Trong Godot editor, mở Editor Settings, tìm `Text Editor -> External` và đặt các tuỳ chọn sau:
   - Exec Path: `/path/to/zode`
   - Exec Flags: `{project} {file}:{line}:{col}`
   - Use External Editor: "✅ On"
2. Mở bất kỳ file \*.gd nào từ Godot và Zode sẽ khởi chạy.

## Cách dùng

Khi Godot đang chạy, extension GDScript kết nối tới language server do Godot runtime cung cấp, mang lại `jump to definition`, hover khi giữ Ctrl/cmd và các tính năng khác của language server.
