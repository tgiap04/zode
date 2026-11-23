---
title: How to Migrate from VS Code to Zode
description: "Guide for migrating from VS Code to Zode, including settings and keybindings."
---

# Cách chuyển từ VS Code sang Zode

Hướng dẫn này giúp bạn chuyển từ VS Code sang Zode mà không phải dựng lại cách làm việc của mình.

Nội dung gồm: setting nào được nhập tự động, phím tắt nào ánh xạ gọn gàng, và hành vi nào khác nhau để bạn điều chỉnh nhanh.

## Cài Zode

Zode có trên macOS, Windows và Linux.

Với macOS và Windows, tải trình cài đặt (`Zode-<arch>.dmg` hoặc `Zode-<arch>.exe`) từ [trang releases](https://github.com/tgiap04/zode/releases).

Với đa số người dùng Linux, cách cài Zode dễ nhất là dùng script cài đặt của chúng tôi:
`curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh`

Sau khi cài, bạn mở Zode từ thư mục Applications (macOS) hoặc trực tiếp từ terminal (Linux) bằng:
`zode .`
Lệnh này mở thư mục hiện tại trong Zode.

## Nhập cài đặt từ VS Code

Zode không nhập cài đặt từ VS Code. Các bảng dưới đây đối chiếu cài đặt VS Code phổ biến với cài đặt tương ứng của Zode:

### Cài đặt được nhập từ VS Code

Chép những cài đặt bạn dùng vào file cài đặt của Zode:

**Trình soạn thảo**

| Setting VS Code                             | Setting Zode                                   |
| ------------------------------------------- | ---------------------------------------------- |
| `editor.fontFamily`                         | `buffer_font_family`                           |
| `editor.fontSize`                           | `buffer_font_size`                             |
| `editor.fontWeight`                         | `buffer_font_weight`                           |
| `editor.tabSize`                            | `tab_size`                                     |
| `editor.insertSpaces`                       | `hard_tabs` (inverted)                         |
| `editor.wordWrap`                           | `soft_wrap`                                    |
| `editor.wordWrapColumn`                     | `preferred_line_length`                        |
| `editor.cursorStyle`                        | `cursor_shape`                                 |
| `editor.cursorBlinking`                     | `cursor_blink`                                 |
| `editor.renderLineHighlight`                | `current_line_highlight`                       |
| `editor.lineNumbers`                        | `gutter.line_numbers`, `relative_line_numbers` |
| `editor.showFoldingControls`                | `gutter.folds`                                 |
| `editor.minimap.enabled`                    | `minimap.show`                                 |
| `editor.minimap.autohide`                   | `minimap.show`                                 |
| `editor.minimap.showSlider`                 | `minimap.thumb`                                |
| `editor.minimap.maxColumn`                  | `minimap.max_width_columns`                    |
| `editor.stickyScroll.enabled`               | `sticky_scroll.enabled`                        |
| `editor.scrollbar.horizontal`               | `scrollbar.axes.horizontal`                    |
| `editor.scrollbar.vertical`                 | `scrollbar.axes.vertical`                      |
| `editor.mouseWheelScrollSensitivity`        | `scroll_sensitivity`                           |
| `editor.fastScrollSensitivity`              | `fast_scroll_sensitivity`                      |
| `editor.cursorSurroundingLines`             | `vertical_scroll_margin`                       |
| `editor.hover.enabled`                      | `hover_popover_enabled`                        |
| `editor.hover.delay`                        | `hover_popover_delay`                          |
| `editor.hover.sticky`                       | `hover_popover_sticky`                         |
| `editor.hover.hidingDelay`                  | `hover_popover_hiding_delay`                   |
| `editor.parameterHints.enabled`             | `auto_signature_help`                          |
| `editor.multiCursorModifier`                | `multi_cursor_modifier`                        |
| `editor.selectionHighlight`                 | `selection_highlight`                          |
| `editor.roundedSelection`                   | `rounded_selection`                            |
| `editor.find.seedSearchStringFromSelection` | `seed_search_query_from_cursor`                |
| `editor.rulers`                             | `wrap_guides`                                  |
| `editor.renderWhitespace`                   | `show_whitespaces`                             |
| `editor.guides.indentation`                 | `indent_guides.enabled`                        |
| `editor.linkedEditing`                      | `linked_edits`                                 |
| `editor.autoSurround`                       | `use_auto_surround`                            |
| `editor.formatOnSave`                       | `format_on_save`                               |
| `editor.formatOnPaste`                      | `auto_indent_on_paste`                         |
| `editor.formatOnType`                       | `use_on_type_format`                           |
| `editor.trimAutoWhitespace`                 | `remove_trailing_whitespace_on_save`           |
| `editor.suggestOnTriggerCharacters`         | `show_completions_on_input`                    |
| `editor.suggest.showWords`                  | `completions.words`                            |
| `editor.inlineSuggest.enabled`              | `show_edit_predictions`                        |

**File & Workspace**

| Setting VS Code             | Setting Zode                   |
| --------------------------- | ------------------------------ |
| `files.autoSave`            | `autosave`                     |
| `files.autoSaveDelay`       | `autosave.milliseconds`        |
| `files.insertFinalNewline`  | `ensure_final_newline_on_save` |
| `files.associations`        | `file_types`                   |
| `files.watcherExclude`      | `file_scan_exclusions`         |
| `files.watcherInclude`      | `file_scan_inclusions`         |
| `files.simpleDialog.enable` | `use_system_path_prompts`      |
| `search.smartCase`          | `use_smartcase_search`         |
| `search.useIgnoreFiles`     | `search.include_ignored`       |

**Terminal**

| Setting VS Code                       | Setting Zode                        |
| ------------------------------------- | ----------------------------------- |
| `terminal.integrated.fontFamily`      | `terminal.font_family`              |
| `terminal.integrated.fontSize`        | `terminal.font_size`                |
| `terminal.integrated.lineHeight`      | `terminal.line_height`              |
| `terminal.integrated.cursorStyle`     | `terminal.cursor_shape`             |
| `terminal.integrated.cursorBlinking`  | `terminal.blinking`                 |
| `terminal.integrated.copyOnSelection` | `terminal.copy_on_select`           |
| `terminal.integrated.scrollback`      | `terminal.max_scroll_history_lines` |
| `terminal.integrated.macOptionIsMeta` | `terminal.option_as_meta`           |
| `terminal.integrated.{platform}Exec`  | `terminal.shell`                    |
| `terminal.integrated.env.{platform}`  | `terminal.env`                      |

**Tab & Bảng**

| Setting VS Code                                    | Setting Zode                                       |
| -------------------------------------------------- | -------------------------------------------------- |
| `workbench.editor.showTabs`                        | `tab_bar.show`                                     |
| `workbench.editor.showIcons`                       | `tabs.file_icons`                                  |
| `workbench.editor.tabActionLocation`               | `tabs.close_position`                              |
| `workbench.editor.tabActionCloseVisibility`        | `tabs.show_close_button`                           |
| `workbench.editor.focusRecentEditorAfterClose`     | `tabs.activate_on_close`                           |
| `workbench.editor.enablePreview`                   | `preview_tabs.enabled`                             |
| `workbench.editor.enablePreviewFromQuickOpen`      | `preview_tabs.enable_preview_from_file_finder`     |
| `workbench.editor.enablePreviewFromCodeNavigation` | `preview_tabs.enable_preview_from_code_navigation` |
| `workbench.editor.editorActionsLocation`           | `tab_bar.show_tab_bar_buttons`                     |
| `workbench.editor.limit.enabled` / `value`         | `max_tabs`                                         |
| `workbench.editor.restoreViewState`                | `restore_on_file_reopen`                           |
| `workbench.statusBar.visible`                      | `status_bar.show`                                  |

**Bảng Project (File Explorer)**

| Setting VS Code                | Setting Zode                        |
| ------------------------------ | ----------------------------------- |
| `explorer.compactFolders`      | `project_panel.auto_fold_dirs`      |
| `explorer.autoReveal`          | `project_panel.auto_reveal_entries` |
| `explorer.excludeGitIgnore`    | `project_panel.hide_gitignore`      |
| `problems.decorations.enabled` | `project_panel.show_diagnostics`    |
| `explorer.decorations.badges`  | `project_panel.git_status`          |

**Git**

| Setting VS Code                      | Setting Zode                                   |
| ------------------------------------ | ---------------------------------------------- |
| `git.enabled`                        | `git_panel.button`                             |
| `git.defaultBranchName`              | `git_panel.fallback_branch_name`               |
| `git.decorations.enabled`            | `git.inline_blame`, `project_panel.git_status` |
| `git.blame.editorDecoration.enabled` | `git.inline_blame.enabled`                     |

**Cửa sổ & Hành vi**

| Setting VS Code                                  | Setting Zode                             |
| ------------------------------------------------ | ---------------------------------------- |
| `window.confirmBeforeClose`                      | `confirm_quit`                           |
| `window.nativeTabs`                              | `use_system_window_tabs`                 |
| `window.closeWhenEmpty`                          | `when_closing_with_no_tabs`              |
| `accessibility.dimUnfocused.enabled` / `opacity` | `active_pane_modifiers.inactive_opacity` |

**Khác**

| Setting VS Code            | Setting Zode                                             |
| -------------------------- | -------------------------------------------------------- |
| `http.proxy`               | `proxy`                                                  |
| `npm.packageManager`       | `node.npm_path`                                          |
| `telemetry.telemetryLevel` | `telemetry.metrics`, `telemetry.diagnostics`             |
| `outline.icons`            | `outline_panel.file_icons`, `outline_panel.folder_icons` |
| `chat.agent.enabled`       | `agent.enabled`                                          |
| `mcp`                      | `context_servers`                                        |

Zode không nhập cài đặt, extension hay phím tắt từ VS Code. Để chép các cài đặt ở trên, mở file cài đặt từ bảng lệnh:

`Cmd+Shift+P → zode: open settings file`

## Thiết lập tùy chọn trình soạn thảo

Bạn cấu hình được phần lớn cài đặt trong Settings Editor ({#kb zode::OpenSettings}). Với các cài đặt nâng cao, chạy `zode: open settings file` từ bảng lệnh để sửa trực tiếp file settings.

Dưới đây là cách các cài đặt VS Code thường dùng được chuyển sang Zode:

| VS Code             | Zode               | Ghi chú                                                          |
| ------------------- | ------------------ | ---------------------------------------------------------------- |
| editor.fontFamily   | buffer_font_family | Zode mặc định dùng SF Mono trên macOS, JetBrains Mono ở nơi khác |
| editor.fontSize     | buffer_font_size   | Đặt theo pixel                                                   |
| editor.tabSize      | tab_size           | Có thể ghi đè theo từng ngôn ngữ                                 |
| editor.insertSpaces | insert_spaces      | Boolean                                                          |
| editor.formatOnSave | format_on_save     | Chạy khi đã bật formatter                                        |
| editor.wordWrap     | soft_wrap          | Hỗ trợ cột ngắt dòng tùy chọn                                    |

Zode cũng hỗ trợ cài đặt theo từng dự án. Bạn cũng tìm thấy chúng trong Settings Editor.

## Mở hoặc tạo dự án

Sau khi thiết lập, nhấn `Cmd+O` (`Ctrl+O` trên Linux) để mở một thư mục. Thư mục này trở thành workspace của bạn trong Zode. Zode không hỗ trợ workspace nhiều thư mục gốc hay file `.code-workspace` như VS Code. Zode giữ mọi thứ đơn giản: một thư mục, một workspace.

Để bắt đầu dự án mới, tạo một thư mục bằng terminal hoặc trình quản lý file, rồi mở nó trong Zode. Trình soạn thảo sẽ coi thư mục đó là gốc của dự án.

Bạn cũng có thể mở Zode từ terminal bên trong bất kỳ thư mục nào bằng:
`zode .`

Khi đã vào dự án, dùng `Cmd+P` để nhảy nhanh giữa các file. `Cmd+Shift+P` (`Ctrl+Shift+P` trên Linux) mở bảng lệnh để chạy action / task, bật tắt cài đặt, hoặc bắt đầu một phiên cộng tác.

Các buffer đang mở hiện thành tab ở phía trên. Bảng Project hiển thị cây file và trạng thái Git. Thu gọn nó bằng `Cmd+B` để có không gian làm việc gọn hơn.

## Khác biệt về phím tắt

Nếu bạn chọn keymap VS Code lúc làm quen, đa số phím tắt sẽ thấy quen thuộc ngay.
Dưới đây là bảng tra nhanh chỗ nào phím tắt giống nhau và chỗ nào khác.

### Phím tắt chung (Zode <> VS Code)

| Hành động                | Phím tắt               |
| ------------------------ | ---------------------- |
| Tìm file                 | `Cmd + P`              |
| Chạy một lệnh            | `Cmd + Shift + P`      |
| Tìm văn bản (toàn dự án) | `Cmd + Shift + F`      |
| Tìm symbol (toàn dự án)  | `Cmd + T`              |
| Tìm symbol (trong file)  | `Cmd + Shift + O`      |
| Bật/tắt dock trái        | `Cmd + B`              |
| Bật/tắt dock dưới        | `Cmd + J`              |
| Mở terminal              | `Ctrl + ~`             |
| Mở cây thư mục file      | `Cmd + Shift + E`      |
| Đóng buffer hiện tại     | `Cmd + W`              |
| Đóng cả dự án            | `Cmd + Shift + W`      |
| Refactor: đổi tên symbol | `F2`                   |
| Đổi giao diện (theme)    | `Cmd + K, Cmd + T`     |
| Ngắt dòng                | `Opt + Z`              |
| Duyệt các tab đang mở    | `Cmd + Opt + Arrow`    |
| Gập / mở theo cú pháp    | `Cmd + Opt + {` or `}` |

### Phím tắt khác nhau (Zode <> VS Code)

| Hành động                | VS Code               | Zode                   |
| ------------------------ | --------------------- | ---------------------- |
| Mở dự án gần đây         | `Ctrl + R`            | `Cmd + Opt + O`        |
| Di chuyển dòng lên/xuống | `Opt + Up/Down`       | `Cmd + Ctrl + Up/Down` |
| Chia khung               | `Cmd + \`             | `Cmd + K, Arrow Keys`  |
| Mở rộng vùng chọn        | `Shift + Alt + Right` | `Opt + Up`             |

### Chỉ có ở Zode

| Hành động         | Phím tắt                     | Ghi chú                                                    |
| ----------------- | ---------------------------- | ---------------------------------------------------------- |
| Bật/tắt dock phải | `Cmd + R` or `Cmd + Alt + B` |                                                            |
| Chọn theo cú pháp | `Opt + Up/Down`              | Chọn code theo cấu trúc (ví dụ: bên trong dấu ngoặc nhọn). |

### Cách tùy chỉnh phím tắt

Để sửa phím tắt:

- Mở bảng lệnh (`Cmd+Shift+P`)
- Chạy `zode: open keymap`

Lệnh này mở danh sách mọi phím tắt hiện có. Bạn có thể ghi đè từng phím, gỡ xung đột, hoặc dựng bố cục phù hợp hơn với mình.

Zode cũng hỗ trợ chord (chuỗi nhiều phím) như `Cmd+K Cmd+C`, giống VS Code.

## Khác biệt về giao diện người dùng

### Không có Workspace

VS Code dùng khái niệm Workspace riêng, với nhiều thư mục gốc, file `.code-workspace`, và phân biệt rõ giữa “một cửa sổ” và “một workspace”.
Zode đơn giản hóa mô hình này.

Trong Zode:

- Không có định dạng file workspace. Mở một thư mục chính là ngữ cảnh dự án của bạn.

- Zode không hỗ trợ workspace nhiều thư mục gốc. Mỗi cửa sổ chỉ mở được một thư mục tại một thời điểm.

- Hầu hết hành vi cấp dự án gắn với thư mục bạn mở. Tìm kiếm, tích hợp Git, task và nhận diện môi trường đều coi thư mục đã mở là gốc dự án.

- Cài đặt theo dự án là tùy chọn. Bạn có thể thêm file `.zode/settings.json` trong dự án để ghi đè cài đặt toàn cục, nhưng Zode không dùng file `.code-workspace` và sẽ không nhập chúng.

- Bạn có thể bắt đầu từ một file đơn lẻ hoặc một cửa sổ trống. Zode không bắt bạn mở thư mục mới được soạn thảo.

Kết quả là một mô hình đơn giản hơn:
Mở một thư mục → làm việc trong thư mục đó → không có thêm tầng workspace nào.

### Điều hướng trong dự án

Trong VS Code, điểm vào chuẩn là mở một thư mục. Từ đó, bảng bên trái là trung tâm điều hướng.
Zode theo hướng khác:

- Bạn vẫn mở được thư mục, nhưng không bắt buộc. Mở một file đơn lẻ, hoặc thậm chí bắt đầu với workspace trống đều hợp lệ.
- Bảng lệnh (`Cmd+Shift+P`) và trình tìm file (`Cmd+P`) là công cụ điều hướng chính. Trình tìm file tìm file, symbol và lệnh trên toàn workspace.
- Thay vì một bảng luôn hiện, Zode khuyến khích bạn:
  - Tìm mờ file theo tên (`Cmd+P`)
  - Nhảy thẳng tới symbol (`Cmd+Shift+O`)
  - Dùng khung chia và tab để giữ ngữ cảnh, thay vì mở một cây file lớn (dù bạn vẫn làm được vậy với bảng Project nếu thích).

Giao diện giữ các bảng phụ ở ngoài tầm mắt để việc điều hướng luôn tập trung vào code.

### Extension so với Marketplace

Zode có ít extension hơn VS Code. Các extension hiện có tập trung vào hỗ trợ ngôn ngữ, giao diện (theme), tô sáng cú pháp và các cải tiến chỉnh sửa cốt lõi khác.

Nhiều tính năng thường cần extension trong VS Code đã được tích hợp sẵn trong Zode:

- Cộng tác thời gian thực có tiếng nói và chia sẻ con trỏ (không cần Live Share)
- Hỗ trợ viết code bằng AI (không cần extension Copilot)
- Bảng terminal tích hợp
- Tìm mờ trên toàn dự án
- Task runner cấu hình bằng JSON
- Chẩn đoán inline và code action qua LSP

Bạn sẽ không tìm được sản phẩm thay thế một-một cho mọi extension của VS Code, nhất là nếu bạn dựa vào công cụ cho DevOps, container hay test runner. Danh mục extension của Zode vẫn đang lớn dần và còn nhỏ hơn.

### Cộng tác trong Zode so với VS Code

Khác VS Code, Zode không cần extension để cộng tác. Tính năng này nằm sẵn trong lõi.

- Mở bảng Collab ở dock trái.
- Tạo một channel và [mời cộng tác viên](https://zodekit.com/docs/collaboration#inviting-a-collaborator) tham gia.
- [Chia sẻ màn hình hoặc codebase](https://zodekit.com/docs/collaboration#share-a-project) trực tiếp.

Khi đã kết nối, bạn thấy con trỏ, vùng chọn và chỉnh sửa của nhau theo thời gian thực. Có sẵn chat thoại để vừa làm vừa nói chuyện. Không cần công cụ riêng hay đăng nhập bên thứ ba.

Tìm hiểu [cách Zed dùng Zed](https://zed.dev/blog/zed-is-our-office) để lên kế hoạch và cộng tác.

### Dùng AI trong Zode

Nếu bạn quen GitHub Copilot trong VS Code, bạn làm được điều tương tự trong Zode. Bạn cũng có thể dùng key của riêng mình hoặc một agent bên ngoài, không cần tài khoản. Bạn có thể tắt hẳn các tính năng AI nếu muốn.

#### Cấu hình GitHub Copilot

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Vào **AI → Edit Predictions**
3. Bấm **Configure** cạnh "Configure Providers"
4. Dưới **GitHub Copilot**, bấm **Sign in to GitHub**

Khi đã đăng nhập, cứ bắt đầu gõ. Zode sẽ đưa gợi ý inline để bạn chấp nhận.

#### Các tùy chọn AI khác

Để dùng các mô hình AI khác trong Zode, bạn có vài lựa chọn:

- Zode không có mô hình lưu trữ sẵn hay gói đăng ký: tính năng AI dùng key hoặc agent của riêng bạn.
- Dùng [API key](https://zodekit.com/docs/ai/llm-providers.html) của riêng bạn, không cần xác thực
- Dùng [agent bên ngoài như Claude Agent](https://zodekit.com/docs/ai/external-agents.html).

### Cấu hình nâng cao và mẹo tăng năng suất

Zode có các cài đặt nâng cao cho người dùng thành thạo muốn tinh chỉnh môi trường.

Dưới đây là vài mẹo hữu ích:

**Format on Save:**

```json
"format_on_save": "on"
```

**Bật hỗ trợ direnv:**

```json
"load_direnv": "shell_hook"
```

**Task tùy chỉnh**: Định nghĩa lệnh build hoặc chạy trong `tasks.json` của bạn (mở bằng bảng lệnh: `zode: open tasks`):

```json
[
  {
    "label": "build",
    "command": "cargo build"
  }
]
```

**Mang snippet tùy chỉnh sang**
Chép thẳng JSON snippet VS Code của bạn vào thư mục snippet của Zode (`zode: configure snippets`).
