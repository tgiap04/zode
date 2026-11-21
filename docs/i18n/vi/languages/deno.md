---
title: Deno
description: "Configure Deno language support in Zode, including language servers, formatting, and debugging."
---

# Deno

Hỗ trợ Deno có sẵn qua [Deno extension](https://github.com/zed-extensions/deno).

- Language server: [Deno Language Server](https://docs.deno.com/runtime/manual/advanced/language_server/overview/)

## Cấu hình Deno

Để dùng Deno Language Server với file TypeScript và TSX, nhiều khả năng bạn sẽ muốn tắt các language server mặc định và bật Deno.

Cấu hình language server và formatter trong Settings ({#kb zode::OpenSettings}) tại Languages > JavaScript/TypeScript/TSX, hoặc thêm vào file settings:

```json [settings]
{
  "lsp": {
    "deno": {
      "settings": {
        "deno": {
          "enable": true
        }
      }
    }
  },
  "languages": {
    "JavaScript": {
      "language_servers": [
        "deno",
        "!typescript-language-server",
        "!vtsls",
        "!eslint"
      ],
      "formatter": "language_server"
    },
    "TypeScript": {
      "language_servers": [
        "deno",
        "!typescript-language-server",
        "!vtsls",
        "!eslint"
      ],
      "formatter": "language_server"
    },
    "TSX": {
      "language_servers": [
        "deno",
        "!typescript-language-server",
        "!vtsls",
        "!eslint"
      ],
      "formatter": "language_server"
    }
  }
}
```

Xem [Cấu hình các ngôn ngữ được hỗ trợ](../configuring-languages.md) trong tài liệu Zode để biết thêm.

<!--
TBD: Deno TypeScript REPL instructions [docs/repl#typescript-deno](../repl.md#typescript-deno)
-->

## Gợi ý khi cấu hình

Để có gợi ý hoàn thành cho `deno.json` hoặc `package.json`, thêm đoạn sau vào file settings ([cách sửa](../configuring-zode.md#settings-files)). Chi tiết xem [JSON](./json.md).

```json [settings]
"lsp": {
    "json-language-server": {
      "settings": {
        "json": {
          "schemas": [
            {
              "fileMatch": [
                "deno.json",
                "deno.jsonc"
              ],
              "url": "https://raw.githubusercontent.com/denoland/deno/refs/heads/main/cli/schemas/config-file.v1.json"
            },
            {
              "fileMatch": [
                "package.json"
              ],
              "url": "https://www.schemastore.org/package"
            }
          ]
        }
      }
    }
  }
```

## Hỗ trợ DAP

Để debug chương trình deno, thêm đoạn này vào `.zode/debug.json`

```json [debug]
[
  {
    "adapter": "JavaScript",
    "label": "Deno",
    "request": "launch",
    "type": "pwa-node",
    "cwd": "$ZODE_WORKTREE_ROOT",
    "program": "$ZODE_FILE",
    "runtimeExecutable": "deno",
    "runtimeArgs": ["run", "--allow-all", "--inspect-wait"],
    "attachSimplePort": 9229
  }
]
```

## Hỗ trợ chạy trực tiếp

Để chạy task deno như test từ giao diện, thêm đoạn này vào `.zode/tasks.json`

```json [tasks]
[
  {
    "label": "deno test",
    "command": "deno test -A '$ZODE_FILE'",
    "tags": ["js-test"]
  }
]
```

## Xem thêm:

- [TypeScript](./typescript.md)
- [JavaScript](./javascript.md)
