---
title: Language Extensions
description: "Overview of programming language support in Zode, including built-in and extension-based languages."
---

# Extension ngôn ngữ

Hỗ trợ ngôn ngữ trong Zode gồm nhiều thành phần:

- Metadata và cấu hình ngôn ngữ
- Grammar
- Query
- Language server

## Metadata ngôn ngữ

Mỗi ngôn ngữ Zode hỗ trợ phải được định nghĩa trong một thư mục con bên trong thư mục `languages` của extension.

Thư mục con này phải chứa một file tên `config.toml` với cấu trúc sau:

```toml
name = "My Language"
grammar = "my-language"
path_suffixes = ["myl"]
line_comments = ["# "]
```

- `name` (bắt buộc) là tên dễ đọc sẽ hiện trong danh sách thả xuống Select Language.
- `grammar` (bắt buộc) là tên của một grammar. Grammar được đăng ký riêng, mô tả bên dưới.
- `path_suffixes` là mảng các hậu tố file nên gắn với ngôn ngữ này. Khác với `file_types` trong settings, mục này không hỗ trợ glob pattern.
- `line_comments` là mảng các chuỗi dùng để nhận biết comment dòng trong ngôn ngữ. Nó được dùng cho phím tắt `editor::ToggleComments`: {#kb editor::ToggleComments} để bật tắt comment các dòng code.
- `tab_size` xác định kích thước thụt lề/tab dùng cho ngôn ngữ này (mặc định là `4`).
- `hard_tabs` quyết định thụt lề bằng tab (`true`) hay dấu cách (`false`, mặc định).
- `first_line_pattern` là một biểu thức chính quy dùng cùng `path_suffixes` (ở trên) hoặc `file_types` trong settings để khớp các file nên dùng ngôn ngữ này. Ví dụ, Zode dùng nó để nhận ra Shell Script bằng cách khớp [dòng shebang](https://github.com/zed-industries/zed/blob/main/crates/languages/src/bash/config.toml) ở dòng đầu của script.
- `debuggers` là mảng các chuỗi dùng để nhận biết trình gỡ lỗi (debugger) trong ngôn ngữ. Khi mở `New Process Modal` của một debugger, Zode sẽ sắp xếp các debugger khả dụng theo thứ tự các mục trong mảng này.

<!--
TBD: Document `language_name/config.toml` keys

- autoclose_before
- brackets (start, end, close, newline, not_in: ["comment", "string"])
- word_characters
- prettier_parser_name
- opt_into_language_servers
- code_fence_block_name
- scope_opt_in_language_servers
- increase_indent_pattern, decrease_indent_pattern
- collapsed_placeholder
- auto_indent_on_paste, auto_indent_using_last_non_empty_line
- overrides: `[overrides.element]`, `[overrides.string]`
-->

## Grammar

Zode dùng thư viện phân tích cú pháp [Tree-sitter](https://tree-sitter.github.io) để cung cấp các tính năng riêng cho từng ngôn ngữ. Có sẵn grammar cho nhiều ngôn ngữ, và bạn cũng có thể [tự phát triển grammar](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html). Danh sách tính năng của Zode xây trên việc khớp mẫu qua cây cú pháp bằng query Tree-sitter ngày càng dài. Như đã nói ở trên, mọi ngôn ngữ được định nghĩa trong một extension phải nêu tên một grammar Tree-sitter dùng để phân tích cú pháp. Các grammar này sau đó được đăng ký riêng trong file `extension.toml` của extension, như sau:

```toml
[grammars.gleam]
repository = "https://github.com/gleam-lang/tree-sitter-gleam"
rev = "58b7cac8fc14c92b0677c542610d8738c373fa81"
```

Trường `repository` phải nêu repository để tải grammar Tree-sitter, và trường `rev` phải chứa một revision Git để dùng, như SHA của một commit Git. Nếu bạn đang phát triển extension cục bộ và muốn tải grammar từ hệ thống file cục bộ, bạn có thể dùng URL `file://` cho `repository`. Một extension có thể cung cấp nhiều grammar bằng cách tham chiếu nhiều repository tree-sitter.

## Query Tree-sitter

Zode dùng cây cú pháp do ngôn ngữ query [Tree-sitter](https://tree-sitter.github.io) tạo ra để triển khai
nhiều tính năng:

- Tô sáng cú pháp
- Khớp dấu ngoặc
- Outline/cấu trúc code
- Tự động thụt lề
- Code injection
- Ghi đè cú pháp
- Che giấu văn bản
- Nhận diện code chạy được
- Chọn class, hàm, v.v.

Các phần sau giải thích cách [query Tree-sitter](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/index.html) hiện thực hóa
các tính năng này trong Zode, lấy [cú pháp JSON](https://www.json.org/json-en.html) làm ví dụ dẫn dắt.

### Tô sáng cú pháp

Trong Tree-sitter, file `highlights.scm` định nghĩa quy tắc tô sáng cú pháp cho một cú pháp cụ thể.

Đây là ví dụ từ `highlights.scm` cho JSON:

```scheme
(string) @string

(pair
  key: (string) @property.json_key)

(number) @number
```

Query này đánh dấu chuỗi, khóa đối tượng và số để tô sáng. Sau đây là danh sách đầy đủ các capture mà giao diện (theme) hỗ trợ:

| Capture                  | Mô tả                          |
| ------------------------ | ------------------------------ |
| @attribute               | Bắt các attribute              |
| @boolean                 | Bắt các giá trị boolean        |
| @comment                 | Bắt comment                    |
| @comment.doc             | Bắt comment tài liệu           |
| @constant                | Bắt hằng số                    |
| @constant.builtin        | Bắt hằng số dựng sẵn           |
| @constructor             | Bắt constructor                |
| @embedded                | Bắt nội dung nhúng             |
| @emphasis                | Bắt văn bản nhấn mạnh          |
| @emphasis.strong         | Bắt văn bản nhấn mạnh mạnh     |
| @enum                    | Bắt enum                       |
| @function                | Bắt hàm                        |
| @hint                    | Bắt gợi ý                      |
| @keyword                 | Bắt từ khóa                    |
| @label                   | Bắt nhãn                       |
| @link_text               | Bắt chữ của liên kết           |
| @link_uri                | Bắt URI của liên kết           |
| @number                  | Bắt giá trị số                 |
| @operator                | Bắt toán tử                    |
| @predictive              | Bắt văn bản dự đoán            |
| @preproc                 | Bắt chỉ thị tiền xử lý         |
| @primary                 | Bắt các phần tử chính          |
| @property                | Bắt property                   |
| @punctuation             | Bắt dấu câu                    |
| @punctuation.bracket     | Bắt dấu ngoặc                  |
| @punctuation.delimiter   | Bắt dấu phân cách              |
| @punctuation.list_marker | Bắt ký hiệu đầu danh sách      |
| @punctuation.special     | Bắt dấu câu đặc biệt           |
| @string                  | Bắt chuỗi literal              |
| @string.escape           | Bắt ký tự escape trong chuỗi   |
| @string.regex            | Bắt biểu thức chính quy        |
| @string.special          | Bắt chuỗi đặc biệt             |
| @string.special.symbol   | Bắt symbol đặc biệt            |
| @tag                     | Bắt tag                        |
| @tag.doctype             | Bắt doctype (ví dụ trong HTML) |
| @text.literal            | Bắt văn bản literal            |
| @title                   | Bắt tiêu đề                    |
| @type                    | Bắt kiểu dữ liệu               |
| @type.builtin            | Bắt kiểu dữ liệu dựng sẵn      |
| @variable                | Bắt biến                       |
| @variable.special        | Bắt biến đặc biệt              |
| @variable.parameter      | Bắt tham số hàm/phương thức    |
| @variant                 | Bắt variant                    |

#### Capture dự phòng

Một pattern Tree-sitter có thể chỉ định nhiều capture trên cùng một node để định nghĩa tô sáng dự phòng.
Zode phân giải chúng từ phải sang trái: trước tiên thử capture ngoài cùng bên phải, và nếu giao diện (theme) hiện tại không có kiểu cho nó thì lùi về capture kế bên trái, cứ thế tiếp tục.

Ví dụ:

```scheme
(type_identifier) @type @variable
```

Ở đây Zode sẽ thử phân giải `@variable` từ giao diện trước. Nếu giao diện định nghĩa kiểu cho `@variable`, kiểu đó được dùng. Nếu không, Zode lùi về `@type`.

Cách này hữu ích khi một ngôn ngữ muốn đưa ra kiểu tô sáng ưu tiên mà không phải giao diện nào cũng hỗ trợ, đồng thời vẫn lùi về một capture phổ biến hơn mà hầu hết giao diện đều định nghĩa.

### Khớp dấu ngoặc

File `brackets.scm` định nghĩa các cặp dấu ngoặc khớp nhau.

Đây là ví dụ từ file `brackets.scm` cho JSON:

```scheme
("[" @open "]" @close)
("{" @open "}" @close)
("\"" @open "\"" @close)
```

Query này nhận diện dấu ngoặc mở và đóng, ngoặc nhọn và dấu nháy.

| Capture | Mô tả                                  |
| ------- | -------------------------------------- |
| @open   | Bắt dấu ngoặc, ngoặc nhọn và nháy mở   |
| @close  | Bắt dấu ngoặc, ngoặc nhọn và nháy đóng |

Zode dùng chúng để tô sáng các dấu ngoặc khớp nhau: tô mỗi cặp dấu ngoặc một màu khác nhau ("rainbow brackets") và tô sáng các dấu ngoặc nếu con trỏ nằm trong cặp.

Để tắt việc tô màu rainbow brackets, thêm đoạn sau vào mục `brackets.scm` tương ứng:

```scheme
(("\"" @open "\"" @close) (#set! rainbow.exclude))
```

### Outline/cấu trúc code

File `outline.scm` định nghĩa cấu trúc cho outline của code.

Đây là ví dụ từ file `outline.scm` cho JSON:

```scheme
(pair
  key: (string (string_content) @name)) @item
```

Query này bắt các khóa đối tượng cho cấu trúc outline.

| Capture        | Mô tả                                                                           |
| -------------- | ------------------------------------------------------------------------------- |
| @name          | Bắt nội dung của khóa đối tượng                                                 |
| @item          | Bắt toàn bộ cặp khóa-giá trị                                                    |
| @context       | Bắt các phần tử cung cấp ngữ cảnh cho mục outline                               |
| @context.extra | Bắt thêm thông tin ngữ cảnh cho mục outline                                     |
| @annotation    | Bắt các node chú thích mục outline (comment tài liệu, attribute, decorator)[^1] |

[^1]: Các chú thích này được Assistant dùng khi tạo các bước sửa code.

### Tự động thụt lề

File `indents.scm` định nghĩa quy tắc thụt lề.

Đây là ví dụ từ file `indents.scm` cho JSON:

```scheme
(array "]" @end) @indent
(object "}" @end) @indent
```

Query này đánh dấu phần cuối của mảng và đối tượng cho mục đích thụt lề.

| Capture | Mô tả                                    |
| ------- | ---------------------------------------- |
| @end    | Bắt dấu ngoặc và ngoặc nhọn đóng         |
| @indent | Bắt toàn bộ mảng và đối tượng để thụt lề |

### Code injection

File `injections.scm` định nghĩa quy tắc nhúng một ngôn ngữ vào ngôn ngữ khác, như khối code trong Markdown hoặc query SQL trong chuỗi Python.

Đây là ví dụ từ file `injections.scm` cho Markdown:

```scheme
(fenced_code_block
  (info_string
    (language) @injection.language)
  (code_fence_content) @injection.content)

((inline) @content
 (#set! injection.language "markdown-inline"))
```

Query này nhận diện các khối code có rào, bắt ngôn ngữ nêu trong info string và nội dung trong khối. Nó cũng bắt nội dung inline và đặt ngôn ngữ của nó là "markdown-inline".

| Capture             | Mô tả                                         |
| ------------------- | --------------------------------------------- |
| @injection.language | Bắt định danh ngôn ngữ của một khối code      |
| @injection.content  | Bắt nội dung được xử lý như một ngôn ngữ khác |

Lưu ý chúng tôi không dùng JSON làm ví dụ ở đây vì nó không hỗ trợ language injection.

### Ghi đè cú pháp

File `overrides.scm` định nghĩa các _scope_ cú pháp dùng để ghi đè một số setting của trình soạn thảo trong các cấu trúc ngôn ngữ cụ thể.

Ví dụ, có một setting riêng cho ngôn ngữ tên `word_characters` kiểm soát những ký tự không phải chữ cái nào được coi là một phần của từ, chẳng hạn khi bạn nhấp đúp để chọn một biến. Trong JavaScript, "$" và "#" được coi là ký tự của từ.

Cũng có một setting riêng cho ngôn ngữ tên `completion_query_characters` kiểm soát ký tự nào kích hoạt gợi ý autocomplete. Trong JavaScript, khi con trỏ nằm trong một _chuỗi_, `-` nên được coi là ký tự query của completion. Để làm được vậy, file `overrides.scm` của JavaScript chứa pattern sau:

```scheme
[
  (string)
  (template_string)
] @string
```

Và `config.toml` của JavaScript chứa setting này:

```toml
word_characters = ["#", "$"]

[overrides.string]
completion_query_characters = ["-"]
```

Bạn cũng có thể tắt một số dấu ngoặc tự đóng trong một scope cụ thể. Ví dụ, để ngăn tự đóng `'` trong chuỗi, bạn có thể đặt đoạn sau vào `config.toml` của JavaScript:

```toml
brackets = [
  { start = "'", end = "'", close = true, newline = false, not_in = ["string"] },
  # other pairs...
]
```

#### Tính bao gồm của khoảng

Mặc định, các khoảng định nghĩa trong `overrides.scm` là _loại trừ_. Nên trong trường hợp trên, nếu con trỏ của bạn nằm _ngoài_ dấu nháy bao quanh chuỗi, scope `string` sẽ không có hiệu lực. Đôi khi bạn muốn khoảng đó _bao gồm_. Bạn làm được bằng cách thêm hậu tố `.inclusive` vào tên capture trong query.

Ví dụ, trong JavaScript, chúng tôi cũng tắt tự đóng nháy đơn trong comment. Và scope comment phải kéo dài đến tận ký tự xuống dòng sau một comment dòng. Để làm được vậy, `overrides.scm` của JavaScript chứa pattern sau:

```scheme
(comment) @comment.inclusive
```

### Text object

File `textobjects.scm` định nghĩa quy tắc điều hướng theo text object. Tính năng này được thêm ở Zode v0.165 và hiện chỉ dùng trong chế độ Vim.

Vim có hai mức độ chi tiết để điều hướng trong file. Từng phần với `[]` v.v., và từng phương thức với `]m` v.v. Ngay cả những ngôn ngữ không hỗ trợ hàm và class cũng chạy tốt nếu định nghĩa các khái niệm tương tự. Ví dụ CSS định nghĩa một rule-set là phương thức, và một media-query là class.

Với các ngôn ngữ có closure, chúng thường không nên được tính là hàm trong Zode. Tuy nhiên đây chỉ là cố gắng tối đa, vì các ngôn ngữ như JavaScript không phân biệt closure với khai báo hàm cấp cao nhất về mặt cú pháp.

Với các ngôn ngữ có khai báo như C, hãy cung cấp query khớp `@class.around` hoặc `@function.around`. Text object `if` và `ic` sẽ mặc định dùng chúng nếu không có phần bên trong.

Nếu bạn không chắc nên đặt gì vào textobjects.scm, cả [nvim-treesitter-textobjects](https://github.com/nvim-treesitter/nvim-treesitter-textobjects) lẫn [trình soạn thảo Helix](https://github.com/helix-editor/helix) đều có query cho nhiều ngôn ngữ. Bạn có thể tham khảo [các ngôn ngữ dựng sẵn](https://github.com/tgiap04/zode/tree/develop/crates/languages/src) của Zode để xem cách điều chỉnh.

| Capture          | Mô tả                                                                       | Chế độ Vim                                       |
| ---------------- | --------------------------------------------------------------------------- | ------------------------------------------------ |
| @function.around | Toàn bộ định nghĩa hàm hoặc một đoạn nhỏ tương đương của file.              | `[m`, `]m`, `[M`,`]M` motions. `af` text object  |
| @function.inside | Thân hàm (phần nằm trong ngoặc nhọn).                                       | `if` text object                                 |
| @class.around    | Toàn bộ định nghĩa class hoặc một đoạn lớn tương đương của file.            | `[[`, `]]`, `[]`, `][` motions. `ac` text object |
| @class.inside    | Nội dung của một định nghĩa class.                                          | `ic` text object                                 |
| @comment.around  | Toàn bộ một comment (ví dụ mọi comment dòng liền kề, hoặc một comment khối) | `gc` text object                                 |
| @comment.inside  | Nội dung của một comment                                                    | `igc` text object (ít được hỗ trợ)               |

Ví dụ:

```scheme
; include only the content of the method in the function
(method_definition
    body: (_
        "{"
        (_)* @function.inside
        "}")) @function.around

; match function.around for declarations with no body
(function_signature_item) @function.around

; join all adjacent comments into one
(comment)+ @comment.around
```

### Che giấu văn bản

File `redactions.scm` định nghĩa quy tắc che giấu văn bản. Khi cộng tác và chia sẻ màn hình, nó bảo đảm một số node cú pháp được hiển thị ở chế độ che để tránh bị lộ.

Đây là ví dụ từ file `redactions.scm` cho JSON:

```scheme
(pair value: (number) @redact)
(pair value: (string) @redact)
(array (number) @redact)
(array (string) @redact)
```

Query này đánh dấu giá trị số và chuỗi trong các cặp khóa-giá trị và mảng để che giấu.

| Capture | Mô tả                        |
| ------- | ---------------------------- |
| @redact | Bắt các giá trị cần che giấu |

### Nhận diện code chạy được

File `runnables.scm` định nghĩa quy tắc nhận diện code chạy được.

Đây là ví dụ từ file `runnables.scm` cho JSON:

```scheme
(
    (document
        (object
            (pair
                key: (string
                    (string_content) @_name
                    (#eq? @_name "scripts")
                )
                value: (object
                    (pair
                        key: (string (string_content) @run @script)
                    )
                )
            )
        )
    )
    (#set! tag package-script)
    (#set! tag composer-script)
)
```

Query này nhận diện các script chạy được trong file package.json và composer.json.

Capture `@run` chỉ định nơi nút chạy xuất hiện trong trình soạn thảo. Các capture khác, trừ những capture có tiền tố gạch dưới, được đưa ra dưới dạng biến môi trường với tiền tố `ZODE_CUSTOM_$(capture_name)` khi chạy code (template viết cho Zed cũng có thể dùng `ZED_CUSTOM_`).

| Capture | Mô tả                                   |
| ------- | --------------------------------------- |
| @\_name | Bắt khóa "scripts"                      |
| @run    | Bắt tên script                          |
| @script | Cũng bắt tên script (cho mục đích khác) |

<!--
TBD: `#set! tag`
-->

## Language server

Zode dùng [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) để cung cấp hỗ trợ ngôn ngữ nâng cao.

Một extension có thể cung cấp bao nhiêu language server cũng được. Để cung cấp một language server từ extension, thêm một mục vào `extension.toml` với tên language server và (các) ngôn ngữ nó áp dụng. Mục trong danh sách `languages` phải khớp với trường `name` trong file `config.toml` của ngôn ngữ đó:

```toml
[language_servers.my-language-server]
name = "My Language LSP"
languages = ["My Language"]
```

Sau đó, trong code Rust của extension, hiện thực phương thức `language_server_command` trên extension của bạn:

```rust
impl zode::Extension for MyExtension {
    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zode::Worktree,
    ) -> Result<zode::Command> {
        Ok(zode::Command {
            command: get_path_to_language_server_executable()?,
            args: get_args_for_language_server()?,
            env: get_env_for_language_server()?,
        })
    }
}
```

Bạn có thể tùy chỉnh cách xử lý language server bằng vài phương thức tùy chọn trong trait `Extension`. Ví dụ, bạn kiểm soát cách gợi ý hoàn thành được tạo kiểu bằng phương thức `label_for_completion`. Để xem danh sách đầy đủ các phương thức, xem [tài liệu API của extension API Zode](https://docs.rs/zed_extension_api).

### Tô sáng cú pháp bằng Semantic Token

Zode hỗ trợ tô sáng cú pháp bằng semantic token từ các language server đang gắn. Hiện mặc định tắt, nhưng bật được trong file settings:

```json [settings]
{
  // Enable semantic tokens globally, backed by tree-sitter highlights for each language:
  "semantic_tokens": "combined",
  // Or, specify per-language:
  "languages": {
    "Rust": {
      // No tree-sitter, only LSP semantic tokens:
      "semantic_tokens": "full"
    }
  }
}
```

Setting `semantic_tokens` nhận các giá trị sau:

- `"off"` (mặc định): Không yêu cầu semantic token từ language server.
- `"combined"`: Dùng semantic token của LSP cùng với tô sáng tree-sitter.
- `"full"`: Chỉ dùng semantic token của LSP, thay thế tô sáng tree-sitter.

#### Quy tắc semantic token do extension cung cấp

Extension ngôn ngữ có thể kèm quy tắc semantic token mặc định cho các kiểu token riêng của language server. Để làm vậy, đặt file `semantic_token_rules.json` trong thư mục ngôn ngữ cạnh `config.toml`:

```
my-extension/
  languages/
    my-language/
      config.toml
      highlights.scm
      semantic_token_rules.json
```

File dùng cùng định dạng với mảng `semantic_token_rules` trong settings của người dùng — một mảng JSON các đối tượng quy tắc:

```json
[
  {
    "token_type": "lifetime",
    "style": ["lifetime"]
  },
  {
    "token_type": "builtinType",
    "style": ["type"]
  },
  {
    "token_type": "selfKeyword",
    "style": ["variable.special"]
  }
]
```

Cách này hữu ích khi một language server báo kiểu semantic token riêng (không chuẩn) mà các quy tắc mặc định dựng sẵn của Zode không phủ. Quy tắc do extension cung cấp đóng vai trò mặc định hợp lý cho ngôn ngữ đó — người dùng luôn ghi đè được qua `semantic_token_rules` trong file settings, và quy tắc mặc định dựng sẵn chỉ được dùng khi cả quy tắc của người dùng lẫn của extension đều không khớp.

#### Tùy chỉnh kiểu semantic token

Zode hỗ trợ tùy chỉnh kiểu dùng cho semantic token. Bạn định nghĩa quy tắc trong file settings, để tùy chỉnh cách semantic token được ánh xạ sang kiểu trong giao diện (theme) của bạn.

```json [settings]
{
  "global_lsp_settings": {
    "semantic_token_rules": [
      {
        // Highlight macros as keywords.
        "token_type": "macro",
        "style": ["syntax.keyword"]
      },
      {
        // Highlight unresolved references in bold red.
        "token_type": "unresolvedReference",
        "foreground_color": "#c93f3f",
        "font_weight": "bold"
      },
      {
        // Underline all mutable variables/references/etc.
        "token_modifiers": ["mutable"],
        "underline": true
      }
    ]
  }
}
```

Mọi quy tắc khớp với một `token_type` và `token_modifiers` đều được áp dụng. Quy tắc đứng trước được ưu tiên. Nếu không quy tắc nào khớp, token không được tô sáng.

Quy tắc được áp dụng theo thứ tự ưu tiên sau (cao đến thấp):

1. **Settings của người dùng** — quy tắc từ `semantic_token_rules` trong file settings của bạn.
2. **Quy tắc của extension** — quy tắc từ `semantic_token_rules.json` trong thư mục ngôn ngữ của extension.
3. **Quy tắc mặc định** — quy tắc dựng sẵn của Zode cho các kiểu token LSP chuẩn.

Mỗi quy tắc trong mảng `semantic_token_rules` được định nghĩa như sau:

- `token_type`: Kiểu semantic token như định nghĩa trong [đặc tả LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_semanticTokens). Nếu bỏ qua, quy tắc khớp mọi kiểu token.
- `token_modifiers`: Danh sách semantic token modifier cần khớp. Phải có đủ mọi modifier mới khớp.
- `style`: Danh sách các kiểu từ giao diện cú pháp hiện tại để dùng. Kiểu đầu tiên tìm thấy sẽ được dùng. Mọi setting bên dưới ghi đè kiểu đó.
- `foreground_color`: Màu chữ dùng cho kiểu token, ở dạng hex (ví dụ `"#ff0000"`).
- `background_color`: Màu nền dùng cho kiểu token, ở dạng hex (ví dụ `"#ff0000"`).
- `underline`: Boolean hoặc màu để gạch chân, ở dạng hex. Nếu là `true`, token sẽ được gạch chân bằng màu chữ.
- `strikethrough`: Boolean hoặc màu để gạch ngang, ở dạng hex. Nếu là `true`, token sẽ có gạch ngang bằng màu chữ.
- `font_weight`: Một trong `"normal"`, `"bold"`.
- `font_style`: Một trong `"normal"`, `"italic"`.

### Hỗ trợ nhiều ngôn ngữ

Nếu language server của bạn hỗ trợ thêm ngôn ngữ, bạn có thể dùng `language_ids` để ánh xạ `languages` của Zode sang các định danh [`languageId` riêng của LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocumentItem) mong muốn:

```toml

[language-servers.my-language-server]
name = "Whatever LSP"
languages = ["JavaScript", "HTML", "CSS"]

[language-servers.my-language-server.language_ids]
"JavaScript" = "javascript"
"TSX" = "typescriptreact"
"HTML" = "html"
"CSS" = "css"
```
