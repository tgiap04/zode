---
title: Ruby
description: "Configure Ruby language support in Zode, including language servers, formatting, and debugging."
---

# Ruby

Hỗ trợ Ruby có sẵn qua [extension Ruby](https://github.com/zed-extensions/ruby).

- Tree-sitter:
  - [tree-sitter-ruby](https://github.com/tree-sitter/tree-sitter-ruby)
  - [tree-sitter-embedded-template](https://github.com/tree-sitter/tree-sitter-embedded-template)
- Language Server:
  - [ruby-lsp](https://github.com/Shopify/ruby-lsp)
  - [solargraph](https://github.com/castwide/solargraph)
  - [rubocop](https://github.com/rubocop/rubocop)
  - [Herb](https://herb-tools.dev)
- Debug Adapter: [`rdbg`](https://github.com/ruby/debug)

Extension Ruby cũng hỗ trợ file ERB.

## Language server

Ruby có nhiều language server. Zode hỗ trợ hai cái sau:

- [solargraph](https://github.com/castwide/solargraph)
- [ruby-lsp](https://github.com/Shopify/ruby-lsp)

Cả hai có bộ tính năng chồng nhau gồm tự hoàn thành, chẩn đoán, code action, v.v., bạn tự quyết định dùng cái nào. Lưu ý là không dùng cả hai cùng lúc được.

Ngoài hai language server này, Zode còn hỗ trợ:

- [rubocop](https://github.com/rubocop/rubocop) là công cụ phân tích code tĩnh và linter cho Ruby. Bên trong, Zode cũng dùng nó như một language server, nhưng chức năng của nó bổ sung cho solargraph và ruby-lsp.
- [sorbet](https://sorbet.org/) là công cụ kiểm tra kiểu tĩnh cho Ruby với hệ thống kiểu tăng dần riêng.
- [steep](https://github.com/soutaro/steep) là công cụ kiểm tra kiểu tĩnh cho Ruby dùng Ruby Signature (RBS).
- [Herb](https://herb-tools.dev) là language server cho file ERB.

Khi cấu hình language server, nên mở cửa sổ LSP Logs bằng lệnh 'dev: Open Language Server Logs'. Sau đó chọn instance ngôn ngữ tương ứng để xem thông tin đã log.

## Cấu hình language server

[Extension Ruby](https://github.com/zed-extensions/ruby) hỗ trợ cả hai language server `solargraph` và `ruby-lsp`.

### Kích hoạt language server

Với mọi language server Ruby được hỗ trợ (`solargraph`, `ruby-lsp`, `rubocop`, `sorbet` và `steep`), extension Ruby kích hoạt theo thứ tự sau:

1. Nếu language server có trong `Gemfile` của dự án, nó được dùng qua `bundle exec`.
2. Nếu không có trong `Gemfile`, extension Ruby tìm file thực thi trong `PATH` của hệ thống.
3. Nếu language server không có ở cả hai nơi, extension Ruby tự cài nó dưới dạng gem toàn cục (lưu ý: sẽ không cài vào gemset Ruby hiện tại của bạn).

Bạn có thể bỏ qua bước 1 và buộc dùng file thực thi của hệ thống bằng cách đặt `use_bundler` thành `false` trong cài đặt:

```json [settings]
{
  "lsp": {
    "<SERVER_NAME>": {
      "settings": {
        "use_bundler": false
      }
    }
  }
}
```

### Dùng `solargraph`

`solargraph` được bật mặc định trong extension Ruby.

### Dùng `ruby-lsp`

Cấu hình language server trong Settings ({#kb zode::OpenSettings}) tại Languages > Ruby, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": ["ruby-lsp", "!solargraph", "!rubocop", "..."]
    },
    // Enable herb and ruby-lsp for *.html.erb files
    "HTML+ERB": {
      "language_servers": ["herb", "ruby-lsp", "..."]
    },
    // Enable ruby-lsp for *.js.erb files
    "JS+ERB": {
      "language_servers": ["ruby-lsp", "..."]
    },
    // Enable ruby-lsp for *.yaml.erb files
    "YAML+ERB": {
      "language_servers": ["ruby-lsp", "..."]
    }
  }
}
```

Cấu hình đó tắt `solargraph` và `rubocop` và dùng `ruby-lsp`.

### Dùng `rubocop`

Extension Ruby cũng hỗ trợ language server `rubocop` để phát hiện vi phạm và tự sửa.

Cấu hình language server trong Settings ({#kb zode::OpenSettings}) tại Languages > Ruby, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": ["ruby-lsp", "rubocop", "!solargraph", "..."]
    }
  }
}
```

Hoặc ngược lại, bạn có thể tắt `ruby-lsp` và bật `solargraph` và `rubocop`:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": ["solargraph", "rubocop", "!ruby-lsp", "..."]
    }
  }
}
```

## Thiết lập `solargraph`

Solargraph tắt format và chẩn đoán theo mặc định. Ta có thể bảo Zode bật chúng bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "solargraph": {
      "initialization_options": {
        "diagnostics": true,
        "formatting": true
      }
    }
  }
}
```

### Cấu hình

Solargraph đọc cấu hình từ file `.solargraph.yml` ở thư mục gốc của dự án. Để biết thêm về file này, xem [tài liệu cấu hình Solargraph](https://solargraph.org/guides/configuration).

## Thiết lập `ruby-lsp`

Bạn có thể truyền cấu hình Ruby LSP vào `initialization_options`, ví dụ:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": ["ruby-lsp", "!solargraph", "..."]
    }
  },
  "lsp": {
    "ruby-lsp": {
      "initialization_options": {
        "enabledFeatures": {
          // "someFeature": false
        }
      }
    }
  }
}
```

Để xem đầy đủ tùy chọn cấu hình, xem [website Ruby LSP](https://shopify.github.io/ruby-lsp/editors.html).

`settings` và `initialization_options` của LSP cũng có thể đặt riêng cho từng dự án. Ví dụ để dùng [standardrb/standard](https://github.com/standardrb/standard) làm formatter và linter cho một dự án cụ thể, thêm đoạn này vào `.zode/settings.json` trong repo dự án:

```json [settings]
{
  "lsp": {
    "ruby-lsp": {
      "initialization_options": {
        "formatter": "standard",
        "linters": ["standard"]
      }
    }
  }
}
```

## Thiết lập `rubocop` LSP

Rubocop tắt tự sửa không an toàn theo mặc định. Ta có thể bảo Zode bật nó bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "languages": {
    "Ruby": {
      // Use ruby-lsp as the primary language server and rubocop as the secondary.
      "language_servers": ["ruby-lsp", "rubocop", "!solargraph", "..."]
    }
  },
  "lsp": {
    "rubocop": {
      "initialization_options": {
        "safeAutocorrect": false
      }
    },
    "ruby-lsp": {
      "initialization_options": {
        "enabledFeatures": {
          "diagnostics": false
        }
      }
    }
  }
}
```

## Thiết lập Sorbet

[Sorbet](https://sorbet.org/) là công cụ kiểm tra kiểu tĩnh phổ biến cho Ruby, có kèm language server.

Để bật Sorbet, thêm `\"sorbet\"` vào danh sách `language_servers` của Ruby. Bạn có thể muốn tắt các language server khác nếu Sorbet là LSP chính, hoặc nếu bạn định dùng nó cùng một LSP khác cho các tính năng riêng như kiểm tra kiểu.

Cấu hình language server trong Settings ({#kb zode::OpenSettings}) tại Languages > Ruby, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": [
        "ruby-lsp",
        "sorbet",
        "!rubocop",
        "!solargraph",
        "..."
      ]
    }
  }
}
```

Về mọi khía cạnh của việc cài Sorbet, thiết lập trong dự án và cấu hình hành vi của nó, xem [tài liệu chính thức của Sorbet](https://sorbet.org/docs/overview).

## Thiết lập Steep

[Steep](https://github.com/soutaro/steep) là công cụ kiểm tra kiểu tĩnh cho Ruby, dùng file RBS để định nghĩa kiểu.

Để bật Steep, thêm `\"steep\"` vào danh sách `language_servers` của Ruby. Bạn có thể cần chỉnh thứ tự hoặc tắt các LSP khác tùy theo thiết lập mong muốn.

Cấu hình language server trong Settings ({#kb zode::OpenSettings}) tại Languages > Ruby, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Ruby": {
      "language_servers": [
        "ruby-lsp",
        "steep",
        "!solargraph",
        "!rubocop",
        "..."
      ]
    }
  }
}
```

## Thiết lập Herb

`Herb` được bật mặc định cho ngôn ngữ `HTML+ERB`.

## Dùng Tailwind CSS Language Server với Ruby

Để có đủ tính năng (tự hoàn thành, lint, v.v.) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file Ruby/ERB, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "experimental": {
          "classRegex": ["\\bclass:\\s*['\"]([^'\"]*)['\"]"]
        }
      }
    }
  }
}
```

Với các cài đặt này, bạn sẽ có gợi ý cho class Tailwind CSS trong thuộc tính HTML bên trong file ERB và trong string Ruby/ERB đứng sau khóa `class:`. Ví dụ:

```rb
# Ruby file:
def method
  div(class: "pl-2 <completion here>") do
    p(class: "mt-2 <completion here>") { "Hello World" }
  end
end

# ERB file:
<%= link_to "Hello", "/hello", class: "pl-2 <completion here>" %>
<a href="/hello" class="pl-2 <completion here>">Hello</a>
```

## Chạy test

Để chạy test trong dự án Ruby, bạn có thể thiết lập task tùy chỉnh trong file cấu hình `.zode/tasks.json` cục bộ. Các task này có thể định nghĩa để dùng với nhiều framework test như Minitest, RSpec, quickdraw và tldr. Dưới đây là một số ví dụ về cách thiết lập task để chạy test ngay trong trình soạn thảo.

### Minitest với Rails

```json [tasks]
[
  {
    "label": "test $ZODE_RELATIVE_FILE -n /$ZODE_CUSTOM_RUBY_TEST_NAME/",
    "command": "bin/rails",
    "args": [
      "test",
      "$ZODE_RELATIVE_FILE",
      "-n",
      "\"$ZODE_CUSTOM_RUBY_TEST_NAME\""
    ],
    "cwd": "$ZODE_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

### Minitest

Minitest thuần không hỗ trợ chạy test theo số dòng, chỉ theo tên, nên ta phải dùng `$ZODE_CUSTOM_RUBY_TEST_NAME`:

```json [tasks]
[
  {
    "label": "-Itest $ZODE_RELATIVE_FILE -n /$ZODE_CUSTOM_RUBY_TEST_NAME/",
    "command": "bundle",
    "args": [
      "exec",
      "ruby",
      "-Itest",
      "$ZODE_RELATIVE_FILE",
      "-n",
      "\"$ZODE_CUSTOM_RUBY_TEST_NAME\""
    ],
    "cwd": "$ZODE_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

### RSpec

```json [tasks]
[
  {
    "label": "test $ZODE_RELATIVE_FILE:$ZODE_ROW",
    "command": "bundle",
    "args": ["exec", "rspec", "\"$ZODE_RELATIVE_FILE:$ZODE_ROW\""],
    "cwd": "$ZODE_WORKTREE_ROOT",
    "tags": ["ruby-test"]
  }
]
```

Có thể dùng cú pháp task tương tự cho các framework test khác như `quickdraw` hoặc `tldr`.

## Gỡ lỗi

Extension Ruby cung cấp debug adapter để gỡ lỗi code Ruby. Tên Zode dùng cho adapter (trong giao diện và `debug.json`) là `rdbg`, và bên trong nó dùng gem [`debug`](https://github.com/ruby/debug). Extension dùng [cùng logic kích hoạt](#language-server-activation) như các language server.

### Ví dụ

#### Gỡ lỗi một script Ruby

```json [debug]
[
  {
    "label": "Debug current file",
    "adapter": "rdbg",
    "request": "launch",
    "script": "$ZODE_FILE",
    "cwd": "$ZODE_WORKTREE_ROOT"
  }
]
```

#### Gỡ lỗi Rails server

```json [debug]
[
  {
    "label": "Debug Rails server",
    "adapter": "rdbg",
    "request": "launch",
    "command": "./bin/rails",
    "args": ["server"],
    "cwd": "$ZODE_WORKTREE_ROOT",
    "env": {
      "RUBY_DEBUG_OPEN": "true"
    }
  }
]
```

## Formatter

### `erb-formatter`

Để format template ERB, bạn có thể dùng formatter `erb-formatter`. Formatter này dùng gem [`erb-formatter`](https://rubygems.org/gems/erb-formatter) để format template ERB.

Cấu hình format trong Settings ({#kb zode::OpenSettings}) tại Languages > HTML+ERB, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "HTML+ERB": {
      "formatter": {
        "external": {
          "command": "erb-formatter",
          "arguments": ["--stdin-filename", "{buffer_path}"]
        }
      }
    }
  }
}
```
