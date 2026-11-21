---
title: R
description: "Configure R language support in Zode, including language servers, formatting, and debugging."
---

# R

R được hỗ trợ qua nhiều extension R cho Zode:

- [ocsmit/zed-r](https://github.com/ocsmit/zed-r)

  - Tree-sitter: [r-lib/tree-sitter-r](https://github.com/r-lib/tree-sitter-r)
  - Language-Server: [REditorSupport/languageserver](https://github.com/REditorSupport/languageserver)

- [posit-dev/air](https://github.com/posit-dev/air/tree/main/editors/zed)
  - Formatter: [posit-dev/air](https://posit-dev.github.io/air/)

## Installation

1. [Tải và cài R](https://cloud.r-project.org/).
2. Cài các gói R `languageserver` và `lintr`:

```R
install.packages("languageserver")
install.packages("lintr")
```

3. Cài extension [R](https://github.com/ocsmit/zed-r) qua trình quản lý extension của Zode để có hỗ trợ ngôn ngữ R cơ bản (tô sáng cú pháp, hỗ trợ tree-sitter) và hỗ trợ [REditorSupport/languageserver](https://github.com/REditorSupport/languageserver).

4. Cài extension [Air](https://posit-dev.github.io/air/) qua trình quản lý extension của Zode để định dạng code R bằng Air.

## Linting

`REditorSupport/languageserver` tích hợp sẵn [r-lib/lintr](https://github.com/r-lib/lintr) làm linter. Bạn cấu hình nó bằng file `.lintr` trong dự án (hoặc trong thư mục home để đặt mặc định toàn cục).

```r
linters: linters_with_defaults(
    line_length_linter(120),
    commented_code_linter = NULL
  )
exclusions: list(
    "inst/doc/creating_linters.R" = 1,
    "inst/example/bad.R",
    "tests/testthat/exclusions-test"
  )
```

Hoặc loại trừ để không lint gì cả,

```r
exclusions: list(".")
```

Xem [Using lintr](https://lintr.r-lib.org/articles/lintr.html) để có danh sách đầy đủ các tuỳ chọn,

## Định dạng

### Air

[Air](https://posit-dev.github.io/air/) định dạng code R, gồm cả định dạng khi lưu. [Tài liệu Air cho Zode](https://posit-dev.github.io/air/editor-zed.html) có hướng dẫn mới nhất để chạy Air trong Zode.

Hãy chắc chắn bạn đã cài cả extension [ocsmit/zed-r](https://github.com/ocsmit/zed-r) (để Zode nhận biết ngôn ngữ R nói chung) và extension [Air](https://posit-dev.github.io/air/).

Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > R, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "R": {
      "language_servers": ["air"]
    }
  }
}
```

Nếu bạn dùng `"r_language_server"` từ `REditorSupport/languageserver` nhưng vẫn muốn dùng Air để định dạng, hãy cấu hình trong Cài đặt ({#kb zode::OpenSettings}) ở mục Languages > R, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "R": {
      "language_servers": ["air", "r_language_server"],
      "use_on_type_format": false
    }
  }
}
```

Lưu ý `"air"` phải đứng đầu danh sách này, nếu không [r-lib/styler](https://github.com/r-lib/styler) sẽ được gọi qua `"r_language_server"`.

`"r_language_server"` định dạng khi gõ theo quy tắc khác với Air. Để tránh hoàn toàn và để Air toàn quyền định dạng file R của bạn, hãy đặt thêm `"use_on_type_format": false` như trên.

#### Cấu hình Air

Air chỉ cấu hình được ở mức tối thiểu qua file `air.toml` đặt ở thư mục gốc của dự án:

```toml
[format]
line-width = 80
indent-width = 2
```

Chi tiết hơn, xem tài liệu Air về [cấu hình](https://posit-dev.github.io/air/configuration.html).

### Styler

`REditorSupport/languageserver` tích hợp sẵn [r-lib/styler](https://github.com/r-lib/styler) làm công cụ định dạng. Xem [Customizing Styler](https://cran.r-project.org/web/packages/styler/vignettes/customizing_styler.html) để biết cách tuỳ chỉnh hành vi của nó.

<!--
TBD: Get this working

### REditorSupport/languageserver Configuration

You can configure the [R languageserver settings](https://github.com/REditorSupport/languageserver#settings) via Zode Project Settings `.zode/settings.json` or Zode User Settings `~/.config/zode/settings.json`:

For example to disable Lintr linting and suppress code snippet suggestions (both enabled by default):

```json [settings]
{
  "lsp": {
    "r_language_server": {
      "settings": {
        "r": {
          "lsp": {
            "diagnostics": false,
            "snippet_support": false
          }
        }
      }
    }
  }
}
```

-->

<!--
TBD: R REPL Docs

## REPL

### Ark Installation

To use the Zode REPL with R you need to install [Ark](https://github.com/posit-dev/ark), an R Kernel for Jupyter applications.
You can down the latest version from the [Ark GitHub Releases](https://github.com/posit-dev/ark/releases) and then extract the `ark` binary to a directory in your `PATH`.

For example to install the latest non-debug build:

```sh
# macOS
cd /tmp
curl -L -o ark-latest-darwin.zip \
    $(curl -s "https://api.github.com/repos/posit-dev/ark/releases/latest" | \
    jq -r '.assets[] | select(.name | contains("darwin-universal") and (contains("debug") | not)) | .browser_download_url')
unzip ark-latest-darwin.zip ark
sudo mv /tmp/ark /usr/local/bin/
```

```sh
# Linux X86_64
cd /tmp
curl -L -o ark-latest-linux.zip \
    $(curl -s "https://api.github.com/repos/posit-dev/ark/releases/latest" \
        | jq -r '.assets[] | select(.name | contains("linux-x64") and (contains("debug") | not)) | .browser_download_url'
    )
unzip ark-latest-linux.zip ark
sudo mv /tmp/ark /usr/local/bin/
```

-->
