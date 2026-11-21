---
title: Visual Customization
description: "Various aspects of Zode's visual layout can be configured in the Settings Editor or settings file."
---

# Tùy biến hiển thị

Nhiều khía cạnh của bố cục hiển thị trong Zode có thể được cấu hình trong trình chỉnh sửa Cài đặt ({#kb zode::OpenSettings}) hoặc trong file cài đặt ({#kb zode::OpenSettingsFile}).

Xem [Tất cả cài đặt](./reference/all-settings.md) để biết thêm thông tin và các cài đặt không liên quan đến hiển thị.

## Giao diện (Theme)

Bạn có thể cài nhiều [giao diện màu](./themes.md) dưới dạng extension bằng cách chạy {#action zode::Extensions} từ bảng lệnh (command palette). [Giao diện biểu tượng](./icon-themes.md) thì khác: biểu tượng file và thư mục của Zode được cố định theo Material Icon Theme và không đổi được, nên không có gì để cài ở đó.

Bạn có thể xem trước/chọn giữa các giao diện màu đã cài bằng {#action theme_selector::Toggle} ({#kb theme_selector::Toggle}), thao tác này sửa cài đặt sau:

```json [settings]
{
  "theme": "One Dark"
}
```

Nếu muốn dùng giao diện riêng cho chế độ sáng/tối, có thể đặt bằng:

```json [settings]
{
  "theme": {
    "dark": "One Dark",
    "light": "One Light",
    // Mode to use (dark, light) or "system" to follow the OS's light/dark mode (default)
    "mode": "system"
  }
}
```

Cài đặt `icon_theme` vẫn còn — mặc định là `"Material Icon Theme"` và nhận cùng dạng đối tượng `{ "mode", "light", "dark" }` như `theme` — nhưng giá trị nào ghi vào đó cũng không đổi biểu tượng được vẽ; xem [Giao diện biểu tượng](./icon-themes.md).

## Phông chữ

```json [settings]
  // UI Font. Zode's default is the bundled "Inter". Use ".SystemUIFont" for the
  // default system font (SF Pro on macOS), or ".ZodeSans" to follow whichever
  // sans Zode ships with rather than naming it
  "ui_font_family": ".SystemUIFont",
  "ui_font_weight": 400, // Font weight in standard CSS units from 100 to 900.
  "ui_font_size": 16,

  // Buffer Font - Used by editor buffers
  // use ".ZodeMono" for the bundled default monospace (currently Lilex)
  "buffer_font_family": "Berkeley Mono", // Font name for editor buffers
  "buffer_font_size": 15,                 // Font size for editor buffers
  "buffer_font_weight": 400,              // Font weight in CSS units [100-900]
  // Line height "comfortable" (1.618), "standard" (1.3) or custom: `{ "custom": 2 }`
  "buffer_line_height": "comfortable",

  // Terminal Font Settings
  "terminal": {
    "font_family": "",
    "font_size": 15,
    // Terminal line height: comfortable (1.618), standard(1.3) or `{ "custom": 2 }`
    "line_height": "standard",
  },

```

### Ligature của phông chữ

Mặc định Zode bật ligature của phông chữ, ghép một số ký tự liền kề thành một hình hiển thị.

Ví dụ `=>` sẽ hiển thị là `→` và `!=` sẽ là `≠`.
Việc này thuần túy là hiển thị, các ký tự riêng lẻ vẫn giữ nguyên.

Để tắt hành vi này, dùng:

```json [settings]
{
  "buffer_font_features": {
    "calt": false // Disable ligatures
  }
}
```

### Chỉ báo dòng và nút bảng ở dock

```json [settings]
{
  // Whether to show full labels in line indicator or short ones
  //   - `short`: "2 s, 15 l, 32 c"
  //   - `long`: "2 selections, 15 lines, 32 characters"
  "line_indicator_format": "long"

  // Individual status bar icons can be hidden:
  // "project_panel": {"button": false },
  // "outline_panel": {"button": false },
  // "git_panel": {"button": false },

  // "debugger": {"button": false },
  // "diagnostics": {"button": false },
  // "search": {"button": false },
}
```

### Thanh tiêu đề

```json [settings]
  // Control which items are shown/hidden in the title bar
  "title_bar": {
    "show_branch_status_icon": false, // Show git status on branch icon
    "show_branch_name": true,       // Show/hide branch name
    "show_project_items": true,     // Show/hide project host and name
    "show_onboarding_banner": true, // Show/hide onboarding banners
    "show_menus": false             // Show/hide menus
  },
```

## Workspace

```json [settings]
{
  // Force usage of Zode build in path prompts (file and directory pickers)
  // instead of OS native pickers (false).
  "use_system_path_prompts": true,
  // Force usage of Zode built in confirmation prompts ("Do you want to save?")
  // instead of OS native prompts (false). On linux this is ignored (always false).
  "use_system_prompts": true,

  // Active pane styling settings.
  "active_pane_modifiers": {
    // Inset border size of the active pane, in pixels.
    "border_size": 0.0,
    // Opacity of the inactive panes. 0 means transparent, 1 means opaque.
    "inactive_opacity": 1.0
  },

  // Layout mode of the bottom dock: contained, full, left_aligned, right_aligned
  "bottom_dock_layout": "contained",

  // Whether to resize all the panels in a dock when resizing the dock.
  // Can be a combination of "left", "right" and "bottom".
  "resize_all_panels_in_dock": ["left"]
}
```

<!--
TBD: Centered layout related settings
```json [settings]
    "centered_layout": {
    // The relative width of the left padding of the central pane from the
    // workspace when the centered layout is used.
    "left_padding": 0.2,
    // The relative width of the right padding of the central pane from the
    // workspace when the centered layout is used.
    "right_padding": 0.2
    },
```
-->

## Trình soạn thảo

```json [settings]
  // Whether the cursor blinks in the editor.
  "cursor_blink": true,

  // Cursor shape for the default editor: bar, block, underline, hollow
  "cursor_shape": null,

  // Highlight the current line in the editor: none, gutter, line, all
  "current_line_highlight": "all",

  // When does the mouse cursor hide: never, on_typing, on_typing_and_movement
  "hide_mouse": "on_typing_and_movement",

  // Whether to highlight all occurrences of the selected text in an editor.
  "selection_highlight": true,

  // Visually show tabs and spaces  (none, all, selection, boundary, trailing)
  "show_whitespaces": "selection",
  "whitespace_map": { // Which characters to show when `show_whitespaces` enabled
    "space": "•",
    "tab": "⟶"       // use "→", for a shorter arrow
  },

  "unnecessary_code_fade": 0.3, // How much to fade out unused code.

  // See-through window. Backgrounds fade evenly to `window_opacity` (on macOS
  // text and icons stay solid); what shows through is the background image
  // if one is set, otherwise the desktop. `window_vibrancy` blurs what shows
  // through (the desktop through the system's material, or the image) and,
  // like an image, fades the backgrounds to `window_vibrancy_opacity` of
  // `window_opacity` (lower shows more through). The image is read from its
  // path and never copied: deleting the file removes it. Settings > Appearance
  // > Window has a file picker for it.
  "window_opacity": 1.0,
  "window_vibrancy": false,
  "window_vibrancy_opacity": 0.8,
  "window_background_image": "",

  // Hide the values of in variables from visual display in private files
  "redact_private_values": false,

  // Soft-wrap and rulers
  "soft_wrap": "none",          // none, editor_width, preferred_line_length, bounded
  "preferred_line_length": 80,  // Column to soft-wrap
  "show_wrap_guides": true,     // Show/hide wrap guides (vertical rulers)
  "wrap_guides": [],            // Where to position wrap_guides (character counts)

  // Gutter Settings
  "gutter": {
    "line_numbers": true,         // Show/hide line numbers in the gutter.
    "runnables": true,            // Show/hide runnables buttons in the gutter.
    "breakpoints": true,          // Show/hide show breakpoints in the gutter.
    "folds": true,                // Show/hide show fold buttons in the gutter.
    "min_line_number_digits": 4   // Reserve space for N digit line numbers
  },
  "relative_line_numbers": "enabled", // Show relative line numbers in gutter

  // Indent guides
  "indent_guides": {
    "enabled": true,
    "line_width": 1,                  // Width of guides in pixels [1-10]
    "active_line_width": 1,           // Width of active guide in pixels [1-10]
    "coloring": "fixed",              // disabled, fixed, indent_aware
    "background_coloring": "disabled" // disabled, indent_aware
  },

  "sticky_scroll": {
    "enabled": false // Whether to stick scopes to the top of the editor. Disabled by default.
  }
```

### Git Blame {#editor-blame}

```json [settings]
  "git": {
    "inline_blame": {
      "enabled": true,             // Show/hide inline blame
      "delay_ms": 0,                  // Show after delay (ms)
      "min_column": 0,             // Minimum column to inline display blame
      "padding": 7,                // Padding between code and inline blame (em)
      "show_commit_summary": false // Show/hide commit summary
    },
    "hunk_style": "staged_hollow"  // staged_hollow, unstaged_hollow
  }
```

### Thanh công cụ của trình soạn thảo

```json [settings]
  // Editor toolbar related settings
  "toolbar": {
    "breadcrumbs": true, // Whether to show breadcrumbs.
    "quick_actions": true, // Whether to show quick action buttons.
    "selections_menu": true, // Whether to show the Selections menu
    "agent_review": true, // Whether to show agent review buttons
    "code_actions": false // Whether to show code action buttons
  }
```

### Thanh cuộn và minimap của trình soạn thảo {#editor-scrollbar}

```json [settings]
  // Scrollbar related settings
  "scrollbar": {
    // When to show the scrollbar in the editor (auto, system, always, never)
    "show": "auto",
    "cursors": true,          // Show cursor positions in the scrollbar.
    "git_diff": true,         // Show git diff indicators in the scrollbar.
    "search_results": true,   // Show buffer search results in the scrollbar.
    "selected_text": true,    // Show selected text occurrences in the scrollbar.
    "selected_symbol": true,  // Show selected symbol occurrences in the scrollbar.
    "diagnostics": "all",     // Show diagnostics (none, error, warning, information, all)
    "axes": {
      "horizontal": true,     // Show/hide the horizontal scrollbar
      "vertical": true        // Show/hide the vertical scrollbar
    }
  },

  // Minimap related settings
  "minimap": {
    "show": "never",                // When to show (auto, always, never)
    "display_in": "active_editor",  // Where to show (active_editor, all_editor)
    "thumb": "always",              // When to show thumb (always, hover)
    "thumb_border": "left_open",    // Thumb border (left_open, right_open, full, none)
    "max_width_columns": 80,        // Maximum width of minimap
    "current_line_highlight": null  // Highlight current line (null, line, gutter)
  },

  // Control Editor scroll beyond the last line: off, one_page, vertical_scroll_margin
  "scroll_beyond_last_line": "one_page",
  // Lines to keep above/below the cursor when scrolling with the keyboard
  "vertical_scroll_margin": 3,
  // The number of characters to keep on either side when scrolling with the mouse
  "horizontal_scroll_margin": 5,
  // Scroll sensitivity multiplier
  "scroll_sensitivity": 1.0,
  // Scroll sensitivity multiplier for fast scrolling (hold alt while scrolling)
  "fast_scroll_sensitivity": 4.0,
```

### Tab của trình soạn thảo

```json [settings]
  // Maximum number of tabs per pane. Unset for unlimited.
  "max_tabs": null,

  // Customize the tab bar appearance
  "tab_bar": {
    "show": true,                     // Show/hide the tab bar
    "show_nav_history_buttons": true, // Show/hide history buttons on tab bar
    "show_tab_bar_buttons": true      // Show hide buttons (new, split, zoom)
  },
  "tabs": {
    "git_status": false,              // Color to show git status
    "close_position": "right",        // Close button position (left, right, hidden)
    "show_close_button": "hover",     // Close button shown (hover, always, hidden)
    "file_icons": false,              // Icon showing file type
    // Show diagnostics in file icon (off, errors, all). Requires file_icons=true
    "show_diagnostics": "off"
  }
```

### Thanh trạng thái

`claude_usage_button` và `codex_usage_button` cũng truy cập được bằng cách bấm chuột phải vào chỉ báo mức dùng trên thanh trạng thái; `agent_usage_display` truy cập được từ nút chuyển Detailed/Compact trong bảng mức dùng (bấm chuột trái vào chỉ báo để mở) — xem [Chỉ báo mức dùng agent](./agent-usage.md) để biết bảng và menu đi kèm.

Bấm chuột phải vào phần _trống_ của thanh trạng thái (không trúng nút nào) sẽ mở menu gồm mọi mục có thể bật/tắt bên dưới, cộng thêm năm mục khác nằm ngoài `status_bar` — các nút tìm kiếm, language server, chẩn đoán, dung lượng dự án và giữ màn hình luôn sáng. Xem [Tất cả cài đặt § Thanh trạng thái](./reference/all-settings.md#status-bar) để biết danh sách đầy đủ, cách menu nhóm chúng theo phía, và việc ẩn một mục qua menu thực sự làm gì (nó bỏ hẳn entity của mục đó, không chỉ bỏ phần vẽ).

Ẩn **Keep Display Awake** theo cách này sẽ tắt cả hai công tắc của nó, `keep_display_awake` và `keep_display_awake_always`, nên nó kết thúc mọi việc giữ màn hình, kể cả việc giữ luôn bật. Biểu tượng hiện khi một trong hai công tắc đang bật. Xem [Giữ màn hình luôn sáng](./keep-display-awake.md#what-lets-it-go).

```json [settings]
  "status_bar": {
    // Show/hide the active buffer's file name.
    // Defaults to false.
    "show_active_file": false,
    // Show/hide a button that displays the active buffer's language.
    // Clicking the button brings up the language selector.
    // Defaults to true.
    "active_language_button": true,
    // Show/hide a button that displays the cursor's position.
    // Clicking the button brings up an input for jumping to a line and column.
    // Defaults to true.
    "cursor_position_button": true,
    // Show/hide a button that displays the buffer's line-ending mode.
    // Clicking the button brings up the line-ending selector.
    // Defaults to false.
    "line_endings_button": false,
    // Show/hide a button that displays the buffer's character encoding.
    // If set to "non_utf8", the button is hidden only for UTF-8 without BOM.
    // Defaults to "non_utf8".
    "active_encoding_button": "non_utf8",
    // Show/hide Claude Code's subscription quota in the status bar.
    // Defaults to true.
    "claude_usage_button": true,
    // Show/hide Codex's subscription quota in the status bar.
    // Defaults to true.
    "codex_usage_button": true,
    // How much of an agent's quota the status bar and usage panel show.
    // "detailed" shows every window the agent reports; "compact" shows only
    // the window closest to running out. Defaults to "detailed".
    "agent_usage_display": "detailed",
    // Show/hide the activity indicator (language-server status, extension
    // operations, updates and git operations).
    // Defaults to true.
    "activity_indicator": true,
    // Show/hide the active toolchain button.
    // Defaults to true.
    "active_toolchain_button": true,
    // Show/hide the Vim mode indicator.
    // Defaults to true.
    "vim_mode_button": true,
    // Show/hide image dimensions and size for the active image.
    // Defaults to true.
    "image_info_button": true,
    // Show/hide Zode's version at the right end of the status bar.
    // Clicking it opens the About window.
    // Defaults to true.
    "app_version_button": true
  },
  "global_lsp_settings": {
    // Show/hide the LSP button in the status bar.
    // Activity from the LSP is still shown.
    // Button is not shown if "enable_language_server" if false.
    "button": true
  },
```

### Multibuffer

```json [settings]
{
  // The default number of lines to expand excerpts in the multibuffer by.
  "expand_excerpt_lines": 5,
  // The default number of lines of context provided for excerpts in the multibuffer by.
  "excerpt_context_lines": 2
}
```

### Gợi ý mã, snippet, action, chẩn đoán trong trình soạn thảo {#editor-lsp}

```json [settings]
  "snippet_sort_order": "inline",        // Snippets completions: top, inline, bottom, none
  "show_completions_on_input": true,     // Show completions while typing
  "show_completion_documentation": true, // Show documentation in completions
  "auto_signature_help": false,          // Show method signatures inside parentheses

  // Whether to show the signature help after completion or a bracket pair inserted.
  // If `auto_signature_help` is enabled, this setting will be treated as enabled also.
  "show_signature_help_after_edits": false,

  // Whether to show code action button at start of buffer line.
  "inline_code_actions": true,

  // Which level to use to filter out diagnostics displayed in the editor:
  "diagnostics_max_severity": null,      // off, error, warning, info, hint, null (all)

  // How to render LSP `textDocument/documentColor` colors in the editor.
  "lsp_document_colors": "inlay",        // none, inlay, border, background
  // When to show the scrollbar in the completion menu.
  "completion_menu_scrollbar": "never", // auto, system, always, never
  // Turn on colorization of brackets in editors (configurable per language)
  "colorize_brackets": true,
```

### Gợi ý chỉnh sửa (Edit Predictions) {#editor-ai}

```json [settings]
  "edit_predictions": {
    "mode": "eager"                  // Automatically show (eager) or hold-alt (subtle)
  },
  "show_edit_predictions": true     // Show/hide predictions in editor
```

### Inlay hint của trình soạn thảo

```json [settings]
{
  "inlay_hints": {
    "enabled": false,
    // Toggle certain types of hints on and off, all switched on by default.
    "show_type_hints": true,
    "show_parameter_hints": true,
    "show_other_hints": true,

    // Whether to show a background for inlay hints (theme `hint.background`)
    "show_background": false, //

    // Time to wait after editing before requesting hints (0 to disable debounce)
    "edit_debounce_ms": 700,
    // Time to wait after scrolling before requesting hints (0 to disable debounce)
    "scroll_debounce_ms": 50,

    // A set of modifiers which, when pressed, will toggle the visibility of inlay hints.
    "toggle_on_modifiers_press": {
      "control": false,
      "shift": false,
      "alt": false,
      "platform": false,
      "function": false
    }
  }
}
```

## Trình tìm file

```json [settings]
  // File Finder Settings
  "file_finder": {
    "file_icons": true,         // Show/hide file icons
    "modal_max_width": "small", // Horizontal size: small, medium, large, xlarge, full
    "include_ignored": null     // gitignored files in results: true, false, null
  },
```

## Bảng Project

Có thể hiện/ẩn bảng Project bằng {#action project_panel::ToggleFocus} ({#kb project_panel::ToggleFocus}) hoặc bằng {#action pane::RevealInProjectPanel} ({#kb pane::RevealInProjectPanel}).

Bảng Project luôn gắn ở phía bên phải cửa sổ, đối diện thanh dự án (project rail); không có cài đặt để di chuyển nó.

Nếu thư mục dự án có vẻ lỗi thời, ví dụ thiếu file mới hoặc repository Git vừa tạo, hãy tải lại nó. Di chuột lên dòng gốc của thư mục rồi bấm nút ↻ (nút quay khi đang tải lại), hoặc bấm chuột phải vào dòng gốc và chọn **Reload**. Action {#action project_panel::ReloadWorktree} quét lại thư mục dự án chứa mục đang chọn, hoặc mọi thư mục dự án khi không chọn gì. Việc quét lại chạy trên máy sở hữu thư mục, tức là host từ xa với dự án WSL và SSH. Khách của một dự án được chia sẻ không tải lại được.

```json [settings]
  // Project Panel Settings
  "project_panel": {
    "button": true,                 // Show/hide button in the status bar
    "default_width": 240,           // Default panel width
    "entry_spacing": "comfortable", // Vertical spacing (comfortable, standard)
    "file_icons": true,             // Show/hide file icons
    "folder_icons": true,           // Show/hide folder icons
    "git_status": true,             // Indicate new/updated files
    "indent_size": 20,              // Pixels for each successive indent
    "auto_reveal_entries": true,    // Show file in panel when activating its buffer
    "auto_fold_dirs": true,         // Fold dirs with single subdir
    "bold_folder_labels": false,    // Show folder names with bold text
    "sticky_scroll": true,          // Stick parent directories at top of the project panel.
    "drag_and_drop": true,          // Whether drag and drop is enabled
    "scrollbar": {                  // Project panel scrollbar settings
      "show": null                  // Show/hide: (auto, system, always, never)
    },
    "show_diagnostics": "all",      //
    // Settings related to indent guides in the project panel.
    "indent_guides": {
      // When to show indent guides in the project panel. (always, never)
      "show": "always"
    },
    // Sort order for entries (directories_first, mixed, files_first)
    "sort_mode": "directories_first",
    // Whether to sort file and folder names case-sensitively.
    // "default": Case-insensitive natural sort, lowercase preferred in ties.
    // "upper":   Uppercase names grouped before lowercase, natural sort within.
    // "lower":   Lowercase names grouped before uppercase, natural sort within.
    // "unicode":  Pure Unicode codepoint comparison, no case folding.
    "sort_order": "default",
    // Whether to hide the root entry when only one folder is open in the window;
    // this also affects how file paths appear in the file finder history.
    "hide_root": false,
    // Whether to hide the hidden entries in the project panel.
    "hide_hidden": false
  }
```

## Bảng Terminal

Khi gắn ở phía dưới, `default_height` đặt chiều cao của toàn bộ dock dưới, không chỉ của terminal — chuyển sang bảng debug không làm dịch cạnh của nó, và kéo khi bảng debug đang hoạt động cũng đổi kích thước terminal.

```json [settings]
  // Terminal Panel Settings
  "terminal": {
    "dock": "bottom",                   // Where to dock: left, right, bottom
    "button": true,                     // Show/hide status bar icon
    "default_width": 640,               // Default width (left/right docked)
    "default_height": 320,              // Default height (bottom docked)

    // Set the cursor blinking behavior in the terminal (on, off, terminal_controlled)
    "blinking": "terminal_controlled",
    // Default cursor shape for the terminal cursor (block, bar, underline, hollow)
    "cursor_shape": "block",

    // Environment variables to add to terminal's process environment
    "env": {
      // "KEY": "value"
    },

    // Terminal scrollbar
    "scrollbar": {
      "show": null                       // Show/hide: (auto, system, always, never)
    },
    // Terminal Font Settings
    "font_family": "Fira Code",
    "font_size": 15,
    "font_weight": 400,
    // Terminal line height: comfortable (1.618), standard(1.3) or `{ "custom": 2 }`
    "line_height": "comfortable",

    "max_scroll_history_lines": 10000,   // Scrollback history (0=disable, max=100000)
  }
```

Xem [cài đặt Terminal](./reference/all-settings.md#terminal) để biết thêm các tùy chọn tùy biến không liên quan đến hiển thị.

### Các bảng khác

Ở dock trái, `default_width` dùng chung: bảng branch là bảng chính của dock trái theo mặc định, nên chuyển sang bảng git hoặc outline không đổi độ rộng của dock — kéo khi một trong hai bảng đang hoạt động cũng đổi `default_width` của bảng branch.

Các bảng xếp chồng trong cùng một dock (trái hoặc dưới) có thể được sắp xếp lại bằng cách kéo tiêu đề của một mục lên trên hoặc xuống dưới mục khác — thứ tự được nhớ qua các lần khởi động lại và không ảnh hưởng độ rộng của dock. Thanh dự án (project rail) và thanh trạng thái giữ thứ tự cố định và không kéo được.

Chiều rộng hoặc chiều cao bạn kéo được nhớ theo từng dự án chứ không dùng chung toàn cục: hai repository không liên quan giữ kích thước dock riêng, còn mọi git worktree của cùng một repository dùng chung một kích thước, nên chuyển giữa hai checkout của cùng một công việc không còn đổi kích thước thanh bên. Dự án mở lần đầu bắt đầu ở kích thước được kéo gần nhất ở bất kỳ đâu, nên không có gì nhảy trên màn hình khi bạn mở nó lần đầu. Kích thước của dự án từ xa được theo dõi riêng cho từng host, nên cùng một đường dẫn mở qua hai kết nối từ xa khác nhau được coi là hai dự án. Workspace không mở thư mục nào không bị ảnh hưởng bởi những điều này và giữ một kích thước dùng chung, như trước.

```json [settings]
  // Git Panel
  "git_panel": {
    "button": true,               // Show/hide status bar icon
    "dock": "left",               // Where to dock: left, right
    "default_width": 360,         // Default width of the git panel.
    "status_style": "icon",       // label_color, icon
    "sort_by_path": false,        // Sort by path (false) or status (true)
    "scrollbar": {
      "show": null                // Show/hide: (auto, system, always, never)
    }
  },

  // Debugger Panel
  "debugger": {
    "dock": "bottom",             // Where to dock: left, right, bottom
    "button": true                // Show/hide status bar icon
  },

  // Outline Panel
  "outline_panel": {
    "button": true,               // Show/hide status bar icon
    "default_width": 300,         // Default width of the outline panel
    "dock": "left",               // Where to dock: left, right
    "file_icons": true,           // Show/hide file_icons
    "folder_icons": true,         // Show file_icons (true), chevrons (false) for dirs
    "git_status": true,           // Show git status
    "indent_size": 20,            // Indentation for nested items (pixels)
    "indent_guides": {
      "show": "always"            // Show indent guides (always, never)
    },
    "auto_reveal_entries": true,  // Show file in panel when activating its buffer
    "auto_fold_dirs": true,       // Fold dirs with single subdir
    "scrollbar": {                // Project panel scrollbar settings
      "show": null                // Show/hide: (auto, system, always, never)
    }
  }
```
