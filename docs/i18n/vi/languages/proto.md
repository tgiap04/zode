---
title: Proto
description: "Configure Proto language support in Zode, including language servers, formatting, and debugging."
---

# Proto

Hỗ trợ Proto/proto3 (ngôn ngữ định nghĩa Protocol Buffers) có sẵn qua [extension Proto](https://github.com/tgiap04/zode/tree/develop/extensions/proto).

- Tree-sitter: [coder3101/tree-sitter-proto](https://github.com/coder3101/tree-sitter-proto)
- Language Server: [protobuf-language-server](https://github.com/lasorda/protobuf-language-server)

<!--
TBD: Clarify which language server(s) to use / Feature support.

## Setup

### Install protobuf-language-server

Install protobuf-language-server and make sure it's in your PATH:

```
go install github.com/lasorda/protobuf-language-server@latest
which protobuf-language-server
```

### Install ProtoLS

Install protols and make sure it's in your PATH:

```
cargo install protols
which protols
```

## Configuration

```json [settings]
"lsp": {
  "protobuf-language-server": {
    "binary": {
      "path": "protols"
    }
  }
}
```

## Formatting

ProtoLS supports formatting if you have `clang-format` installed.

```sh
# MacOS:
brew install clang-format
# Ubuntu
sudo apt-get install clang-format
# Fedora
sudo dnf install clang-tools-extra
```

To customize your formatting preferences, create a `.clang-format` file, e.g.:

```clang-format
IndentWidth: 4
ColumnLimit: 120
```

Hoặc bạn có thể để Zode gọi thẳng `clang-format` bằng cách đặt nó làm [formatter](https://zodekit.com/docs/reference/all-settings#formatter) trong cài đặt:

```json [settings]
  "languages": {
    "Proto": {
      "format_on_save": "on",
      "tab_size": 4,
      "formatter": {
        "external": {
          "command": "clang-format",
          "arguments": ["-style={IndentWidth: 4, ColumnLimit: 0}"]
        }
      }
    },
  }
```
-->
