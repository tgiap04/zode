---
title: Elixir
description: "Configure Elixir language support in Zode, including language servers, formatting, and debugging."
---

# Elixir

Hỗ trợ Elixir có sẵn qua [extension Elixir](https://github.com/zed-extensions/elixir).

- Tree-sitter Grammars:
  - [elixir-lang/tree-sitter-elixir](https://github.com/elixir-lang/tree-sitter-elixir)
  - [phoenixframework/tree-sitter-heex](https://github.com/phoenixframework/tree-sitter-heex)
- Language Servers:
  - [elixir-lang/expert](https://github.com/elixir-lang/expert)
  - [elixir-lsp/elixir-ls](https://github.com/elixir-lsp/elixir-ls)
  - [elixir-tools/next-ls](https://github.com/elixir-tools/next-ls)
  - [lexical-lsp/lexical](https://github.com/lexical-lsp/lexical)
  - [remoteoss/dexter](https://github.com/remoteoss/dexter)

Ngoài ra, extension còn hỗ trợ template [EEx](https://hexdocs.pm/eex/EEx.html) (Embedded Elixir) và [HEEx](https://hexdocs.pm/phoenix/components.html#heex), một dạng kết hợp HTML với EEx mà ứng dụng Phoenix LiveView dùng.

## Language Server

Extension Elixir hỗ trợ các language server ElixirLS, Expert, Dexter, Next LS và Lexical. Mặc định chỉ bật ElixirLS. Bạn có thể đổi hoặc tắt các language server đang bật trong cài đặt ({#kb zode::OpenSettings}) ở Languages > Elixir/EEx/HEEx, hoặc sửa thẳng file cài đặt.

Một số language server còn nhận tuỳ chọn khởi tạo hoặc cấu hình workspace. Xem các mục bên dưới để biết mỗi server hỗ trợ gì. Cấu hình truyền qua file cài đặt, lần lượt bằng `lsp.{language-server-id}.initialization_options` và `lsp.{language-server-id}.settings`.

Xem hướng dẫn [Cấu hình Zode](../configuring-zode.md#settings-files) để biết cách sửa file cài đặt.

### Dùng ElixirLS

ElixirLS nhận các tuỳ chọn cấu hình workspace.

Ví dụ sau tắt [Dialyzer](https://github.com/elixir-lsp/elixir-ls#dialyzer-integration):

```json [settings]
  "lsp": {
    "elixir-ls": {
      "settings": {
        "dialyzerEnabled": false
      }
    }
  }
```

Xem [danh sách cài đặt cấu hình ElixirLS](https://github.com/elixir-lsp/elixir-ls#elixirls-configuration-settings) chính thức để biết mọi tuỳ chọn.

### Dùng Expert

Bật Expert bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "languages": {
    "Elixir": {
      "language_servers": ["expert", "!elixir-ls", "!dexter", "!next-ls", "!lexical", "..."]
    },
    "EEx": {
      "language_servers": ["expert", "!elixir-ls", "!dexter", "!next-ls", "!lexical", "..."]
    },
    "HEEx": {
      "language_servers": ["expert", "!elixir-ls", "!dexter", "!next-ls", "!lexical", "..."]
    }
  }
```

Expert nhận các tuỳ chọn cấu hình workspace.

Ví dụ sau đặt số ký tự tối thiểu để tìm symbol trong dự án trả về kết quả:

```json [settings]
  "lsp": {
    "expert": {
      "settings": {
        "workspaceSymbols": {
          "minQueryLength": 0
        }
      }
    }
  }
```

Xem trang [cấu hình Expert](https://expert-lsp.org/docs/configuration/) để biết mọi tuỳ chọn.

Để dùng bản Expert tự build, thêm đoạn sau vào file cài đặt:

```json [settings]
  "lsp": {
    "expert": {
      "binary": {
        "path": "/path/to/expert",
        "arguments": ["--stdio"]
      }
    }
  }
```

### Dùng Dexter

[Dexter](https://github.com/remoteoss/dexter) là language server Elixir nhanh, đầy đủ tính năng, tối ưu cho codebase lớn. Nó đọc thẳng file nguồn, không cần biên dịch. Hỗ trợ go-to-definition, references, hover docs, autocompletion, rename và format khi lưu.

Bật Dexter bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "languages": {
    "Elixir": {
      "language_servers": ["dexter", "!expert", "!elixir-ls", "!next-ls", "!lexical", "..."]
    },
    "EEx": {
      "language_servers": ["dexter", "!expert", "!elixir-ls", "!next-ls", "!lexical", "..."]
    },
    "HEEx": {
      "language_servers": ["dexter", "!expert", "!elixir-ls", "!next-ls", "!lexical", "..."]
    }
  }
```

Dexter nhận các tuỳ chọn khởi tạo.

Ví dụ sau tắt việc đi theo `defdelegate` tới hàm đích:

```json [settings]
  "lsp": {
    "dexter": {
      "initialization_options": {
        "followDelegates": false
      }
    }
  }
```

Để dùng binary Dexter tuỳ chọn, thêm đoạn sau vào file cài đặt:

```json [settings]
  "lsp": {
    "dexter": {
      "binary": {
        "path": "/path/to/dexter",
        "arguments": ["lsp"]
      }
    }
  }
```

Xem [tài liệu Dexter](https://github.com/remoteoss/dexter) để biết thêm chi tiết.

### Dùng Next LS

Bật Next LS bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "languages": {
    "Elixir": {
      "language_servers": ["next-ls", "!expert", "!elixir-ls", "!dexter", "!lexical", "..."]
    },
    "EEx": {
      "language_servers": ["next-ls", "!expert", "!elixir-ls", "!dexter", "!lexical", "..."]
    },
    "HEEx": {
      "language_servers": ["next-ls", "!expert", "!elixir-ls", "!dexter", "!lexical", "..."]
    }
  }
```

Next LS nhận các tuỳ chọn khởi tạo.

Completions là tính năng thử nghiệm trong Next LS, và được bật mặc định trong Zode. Tắt bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "lsp": {
    "next-ls": {
      "initialization_options": {
        "experimental": {
          "completions": {
            "enable": false
          }
        }
      }
    }
  }
```

Next LS còn có extension tích hợp [Credo](https://hexdocs.pm/credo/overview.html), được bật mặc định. Bạn có thể tắt bằng cách thêm mục sau vào file cài đặt:

```json [settings]
  "lsp": {
    "next-ls": {
      "initialization_options": {
        "extensions": {
          "credo": {
            "enable": false
          }
        }
      }
    }
  }
```

Next LS cũng có thể truyền thẳng tuỳ chọn CLI cho Credo. Ví dụ sau truyền `--min-priority high` cho nó:

```json [settings]
  "lsp": {
    "next-ls": {
      "initialization_options": {
        "extensions": {
          "credo": {
            "cli_options": ["--min-priority high"]
          }
        }
      }
    }
  }
```

Xem trang [Credo Command Line Switches](https://hexdocs.pm/credo/suggest_command.html#command-line-switches) để biết thêm tuỳ chọn CLI.

### Dùng Lexical

Bật Lexical bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "languages": {
    "Elixir": {
      "language_servers": ["lexical", "!expert", "!elixir-ls", "!dexter", "!next-ls", "..."]
    },
    "EEx": {
      "language_servers": ["lexical", "!expert", "!elixir-ls", "!dexter", "!next-ls", "..."]
    },
    "HEEx": {
      "language_servers": ["lexical", "!expert", "!elixir-ls", "!dexter", "!next-ls", "..."]
    }
  }
```

## Định dạng code khi không dùng language server

Nếu bạn thích làm việc không có language server nhưng vẫn muốn định dạng code bằng [Mix](https://hexdocs.pm/mix/Mix.html), có thể cấu hình nó làm formatter bên ngoài bằng cách thêm đoạn sau vào file cài đặt:

```json [settings]
  "languages": {
    "Elixir": {
      "enable_language_server": false,
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "mix",
          "arguments": ["format", "--stdin-filename", "{buffer_path}", "-"]
        }
      }
    },
    "EEx": {
      "enable_language_server": false,
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "mix",
          "arguments": ["format", "--stdin-filename", "{buffer_path}", "-"]
        }
      }
    },
    "HEEx": {
      "enable_language_server": false,
      "format_on_save": "on",
      "formatter": {
        "external": {
          "command": "mix",
          "arguments": ["format", "--stdin-filename", "{buffer_path}", "-"]
        }
      }
    }
  }
```

## Dùng Tailwind CSS Language Server với template HEEx

Để có đủ tính năng (autocomplete, linting và hover docs) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong template HEEx, thêm đoạn sau vào file cài đặt:

```json [settings]
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "elixir": "html",
          "heex": "html"
        },
        "experimental": {
          "classRegex": ["class=\"([^\"]*)\"", "class='([^']*)'"]
        }
      }
    }
  }
```

Với các cài đặt này, bạn sẽ có gợi ý cho class Tailwind CSS trong template HEEx. Ví dụ:

```heex
<%!-- Standard class attribute --%>
<div class="flex items-center <completion here>">
  <p class="text-lg font-bold <completion here>">Hello World</p>
</div>

<%!-- With Elixir expression --%>
<div class={"flex #{@custom_class} <completion here>"}>
  Content
</div>

<%!-- With Phoenix function --%>
<div class={class_list(["flex", "items-center", "<completion here>"])}>
  Content
</div>
```

## Xem thêm

- [Erlang](./erlang.md)
- [Gleam](./gleam.md)
