---
title: Biome
description: "Configure Biome language support in Zode, including language servers, formatting, and debugging."
---

# Biome

[Biome](https://biomejs.dev/) trong Zode được cung cấp bởi [extension Biome](https://github.com/biomejs/biome-zed) do cộng đồng duy trì.
Báo lỗi tại: [https://github.com/biomejs/biome-zed/issues](https://github.com/biomejs/biome-zed/issues)

- Language Server: [biomejs/biome](https://github.com/biomejs/biome)

## Hỗ trợ ngôn ngữ của Biome

Extension Biome hỗ trợ các ngôn ngữ sau:

- JavaScript
- TypeScript
- JSX
- TSX
- JSON
- JSONC
- Vue.js
- Astro
- Svelte
- CSS

## Cấu hình

Theo mặc định, file `biome.json` phải nằm ở thư mục gốc của workspace.

```json
{
  "$schema": "https://biomejs.dev/schemas/1.8.3/schema.json"
}
```

Danh sách đầy đủ các tuỳ chọn của `biome.json` có trong tài liệu [Biome Configuration](https://biomejs.dev/reference/configuration/).

Xem [README của extension Biome cho Zode](https://github.com/biomejs/biome-zed) để có danh sách đầy đủ các tính năng và tuỳ chọn cấu hình.
