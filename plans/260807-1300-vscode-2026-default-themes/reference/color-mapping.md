# Bảng ánh xạ màu — VSCode 2026 → Zed

> **Sinh tự động** từ `build/dark-2026.json` + `build/light-2026.json`. Đây là bảng đã **resolve**;
> luật suy dẫn (bảng A–I) nằm trong [`../phase-01-color-mapping-table.md`](../phase-01-color-mapping-table.md).
> Sửa màu ở phase 05 phải cập nhật lại file này.

**Phủ sóng:** 147 style key (one.json cần 141) · 45 syntax · 8 players · 24 terminal ANSI · 6 accents.
Không key nào rơi về fallback.

## Style keys

| Zed key | Dark 2026 | Light 2026 | Nguồn |
|---|---|---|---|
| `border` | `#2a2b2cff` | `#f0f1f2ff` | `theme_importer` (map tự động từ key VSCode) |
| `border.variant` | `#1e1f20ff` | `#f8f8f8ff` | **Bảng A** — ghi đè: `border` trộn 50% về `editor.background` |
| `border.focused` | `#3994bcb3` | `#0069ccff` | `theme_importer` (map tự động từ key VSCode) |
| `border.selected` | `#3994bcb3` | `#0069ccff` | **Bảng A** — ghi đè: `focusBorder` |
| `border.transparent` | `#00000000` | `#00000000` | **Bảng A** — ghi đè: theo định nghĩa `#00000000` (importer trả về màu đục, sai) |
| `border.disabled` | `#2a2b2cff` | `#f0f1f2ff` | `theme_importer` (map tự động từ key VSCode) |
| `elevated_surface.background` | `#202122ff` | `#fafafdff` | **F** ← `editorWidget.background` |
| `surface.background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `element.background` | `#297aa0ff` | `#0069ccff` | `theme_importer` (map tự động từ key VSCode) |
| `element.hover` | `#ffffff14` | `#00000014` | `theme_importer` (map tự động từ key VSCode) |
| `element.active` | `#ffffff33` | `#d6d6d8ff` | **E** ← `toolbar.activeBackground` |
| `element.selected` | `#ffffff22` | `#00000025` | `theme_importer` (map tự động từ key VSCode) |
| `element.disabled` | `#297aa0ff` | `#0069ccff` | **E** ← `element.background` |
| `drop_target.background` | `#3994bc1a` | `#0069cc15` | `theme_importer` (map tự động từ key VSCode) |
| `ghost_element.background` | `#00000000` | `#00000000` | **E** — theo định nghĩa `#00000000` |
| `ghost_element.hover` | `#ffffff14` | `#00000014` | `theme_importer` (map tự động từ key VSCode) |
| `ghost_element.active` | `#ffffff33` | `#d6d6d8ff` | **E** ← `toolbar.activeBackground` |
| `ghost_element.selected` | `#ffffff22` | `#00000025` | `theme_importer` (map tự động từ key VSCode) |
| `ghost_element.disabled` | `#00000000` | `#00000000` | **E** — theo định nghĩa `#00000000` |
| `text` | `#bfbfbfff` | `#202020ff` | `theme_importer` (map tự động từ key VSCode) |
| `text.muted` | `#8c8c8cff` | `#606060ff` | `theme_importer` (map tự động từ key VSCode) |
| `text.placeholder` | `#555555ff` | `#999999ff` | **D** ← `input.placeholderForeground` |
| `text.disabled` | `#555555ff` | `#bbbbbbff` | **D** ← `disabledForeground` |
| `text.accent` | `#48a0c7ff` | `#0069ccff` | **D** ← `textLink.foreground` |
| `icon` | `#8c8c8cff` | `#606060ff` | **D** ← `icon.foreground` |
| `icon.muted` | `#8c8c8cff` | `#606060ff` | **D** ← `editorCodeLens.foreground` |
| `icon.disabled` | `#555555ff` | `#bbbbbbff` | **D** ← `disabledForeground` |
| `icon.placeholder` | `#555555ff` | `#999999ff` | **D** ← `input.placeholderForeground` |
| `icon.accent` | `#48a0c7ff` | `#0069ccff` | **D** ← `textLink.foreground` |
| `status_bar.background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `title_bar.background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `title_bar.inactive_background` | `#191a1bff` | `#fafafdff` | **F** ← `titleBar.inactiveBackground` |
| `toolbar.background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `tab_bar.background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `tab.inactive_background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `tab.active_background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `search.match_background` | `#27678290` | `#0069cc40` | `theme_importer` (map tự động từ key VSCode) |
| `search.active_match_background` | `#27678290` | `#0069cc40` | **F** ← `editor.findMatchBackground` |
| `panel.background` | `#191a1bff` | `#fafafdff` | `theme_importer` (map tự động từ key VSCode) |
| `panel.focused_border` | `#3994bcb3` | `#0069ccff` | **F** ← `focusBorder` |
| `pane.focused_border` | `#3994bcb3` | `#0069ccff` | **F** ← `focusBorder` |
| `scrollbar.thumb.background` | `#a8a9aa85` | `#646464c0` | `theme_importer` (map tự động từ key VSCode) |
| `scrollbar.thumb.hover_background` | `#a8a9aa90` | `#646464d0` | `theme_importer` (map tự động từ key VSCode) |
| `scrollbar.thumb.border` | `#a8a9aa85` | `#646464c0` | `theme_importer` (map tự động từ key VSCode) |
| `scrollbar.track.background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `scrollbar.track.border` | `#2a2b2cff` | `#f0f1f2ff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.foreground` | `#bbbebfff` | `#202020ff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.gutter.background` | `#121314ff` | `#ffffffff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.subheader.background` | `#191a1bff` | `#fafafdff` | **F** ← `editorGroupHeader.tabsBackground` |
| `editor.active_line.background` | `#242526ff` | `#eaeaea40` | `theme_importer` (map tự động từ key VSCode) |
| `editor.highlighted_line.background` | `#242526ff` | `#eaeaeaff` | **F** ← `editor.rangeHighlightBackground` |
| `editor.line_number` | `#858889ff` | `#606060ff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.active_line_number` | `#bbbebfff` | `#202020ff` | `theme_importer` (map tự động từ key VSCode) |
| `editor.hover_line_number` | `#bbbebfff` | `#202020ff` | **F** ← `editorLineNumber.activeForeground` |
| `editor.invisible` | `#8c8c8c4d` | `#60606040` | **F** ← `editorWhitespace.foreground` |
| `editor.wrap_guide` | `#8384854d` | `#f7f7f740` | **F** ← `editorIndentGuide.background` |
| `editor.active_wrap_guide` | `#838485ff` | `#eeeeeeff` | **F** ← `editorIndentGuide.activeBackground` |
| `editor.document_highlight.read_background` | `#27678250` | `#0069cc26` | **F** ← `editor.wordHighlightBackground` |
| `editor.document_highlight.write_background` | `#27678280` | `#0069cc26` | **F** ← `editor.wordHighlightStrongBackground` |
| `terminal.background` | `#191a1bff` | `#fafafdff` | `theme_importer`; **light thiếu nguồn** → `panel.background` (VSCode để terminal thừa hưởng nền panel) |
| `terminal.foreground` | `#bfbfbfff` | `#202020ff` | **G** ← `foreground` |
| `terminal.bright_foreground` | `#bbbebfff` | `#202020ff` | **G** ← `editor.foreground` |
| `terminal.dim_foreground` | `#8c8c8cff` | `#606060ff` | **G** ← `descriptionForeground` |
| `terminal.ansi.black` | `#000000ff` | `#000000ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_black` | `#666666ff` | `#666666ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_black` | `#000000b3` | `#000000b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.red` | `#cd3131ff` | `#cd3131ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_red` | `#f14c4cff` | `#cd3131ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_red` | `#cd3131b3` | `#cd3131b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.green` | `#0dbc79ff` | `#107c10ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_green` | `#23d18bff` | `#14ce14ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_green` | `#0dbc79b3` | `#107c10b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.yellow` | `#e5e510ff` | `#949800ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_yellow` | `#f5f543ff` | `#b5ba00ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_yellow` | `#e5e510b3` | `#949800b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.blue` | `#2472c8ff` | `#0451a5ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_blue` | `#3b8eeaff` | `#0451a5ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_blue` | `#2472c8b3` | `#0451a5b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.magenta` | `#bc3fbcff` | `#bc05bcff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_magenta` | `#d670d6ff` | `#bc05bcff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_magenta` | `#bc3fbcb3` | `#bc05bcb3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.cyan` | `#11a8cdff` | `#0598bcff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_cyan` | `#29b8dbff` | `#0598bcff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_cyan` | `#11a8cdb3` | `#0598bcb3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `terminal.ansi.white` | `#e5e5e5ff` | `#555555ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.bright_white` | `#e5e5e5ff` | `#a5a5a5ff` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) |
| `terminal.ansi.dim_white` | `#e5e5e5b3` | `#555555b3` | **G** ← bảng ANSI mặc định hardcode của VSCode (`workbench.desktop.main.js`) @ alpha `B3` |
| `link_text.hover` | `#53a5caff` | `#0069ccff` | `theme_importer` (map tự động từ key VSCode) |
| `version_control.added` | `#73c991ff` | `#587c0cff` | **C** ← `gitDecoration.addedResourceForeground` |
| `version_control.modified` | `#e5ba7dff` | `#667309ff` | **C** ← `gitDecoration.modifiedResourceForeground` |
| `version_control.word_added` | `#57ab5a4d` | `#587c0c26` | **C** ← `diffEditor.insertedTextBackground` |
| `version_control.word_deleted` | `#f470674d` | `#ad070726` | **C** ← `diffEditor.removedTextBackground` |
| `version_control.deleted` | `#f48771ff` | `#ad0707ff` | **C** ← `gitDecoration.deletedResourceForeground` |
| `version_control.conflict_marker.ours` | `#347d3926` | `#587c0c26` | **C** ← `diffEditor.insertedLineBackground`; light thiếu nguồn → `created` @ alpha `26` |
| `version_control.conflict_marker.theirs` | `#c93c3726` | `#ad070726` | **C** ← `diffEditor.removedLineBackground`; light thiếu nguồn → `deleted` @ alpha `26` |
| `conflict` | `#f48771ff` | `#ad0707ff` | **B/C** ← `gitDecoration.conflictingResourceForeground` |
| `conflict.background` | `#f487711a` | `#ad07071a` | **B/C** ← `conflict` @ alpha `1A` |
| `conflict.border` | `#f4877166` | `#ad070766` | **B/C** ← `conflict` @ alpha `66` |
| `created` | `#73c991ff` | `#587c0cff` | **B/C** ← `gitDecoration.addedResourceForeground` |
| `created.background` | `#73c9911a` | `#587c0c1a` | **B/C** ← `created` @ alpha `1A` |
| `created.border` | `#73c99166` | `#587c0c66` | **B/C** ← `created` @ alpha `66` |
| `deleted` | `#f48771ff` | `#ad0707ff` | **B/C** ← `gitDecoration.deletedResourceForeground` |
| `deleted.background` | `#f487711a` | `#ad07071a` | **B/C** ← `deleted` @ alpha `1A` |
| `deleted.border` | `#f4877166` | `#ad070766` | **B/C** ← `deleted` @ alpha `66` |
| `error` | `#f48771ff` | `#ad0707ff` | **B/C** ← `errorForeground` |
| `error.background` | `#f487711a` | `#ad07071a` | **B/C** ← `error` @ alpha `1A` |
| `error.border` | `#f4877166` | `#ad070766` | **B/C** ← `error` @ alpha `66` |
| `hidden` | `#8c8c8cff` | `#8e8e90ff` | **B/C** ← `gitDecoration.ignoredResourceForeground` |
| `hidden.background` | `#8c8c8c1a` | `#8e8e901a` | **B/C** ← `hidden` @ alpha `1A` |
| `hidden.border` | `#8c8c8c66` | `#8e8e9066` | **B/C** ← `hidden` @ alpha `66` |
| `hint` | `#969696ff` | `#969696ff` | **B/C** ← `editorInlayHint.foreground (qua importer)` |
| `hint.background` | `#9696961a` | `#9696961a` | **B/C** ← `hint` @ alpha `1A` |
| `hint.border` | `#96969666` | `#96969666` | **B/C** ← `hint` @ alpha `66` |
| `ignored` | `#8c8c8cff` | `#8e8e90ff` | **B/C** ← `gitDecoration.ignoredResourceForeground` |
| `ignored.background` | `#8c8c8c1a` | `#8e8e901a` | **B/C** ← `ignored` @ alpha `1A` |
| `ignored.border` | `#8c8c8c66` | `#8e8e9066` | **B/C** ← `ignored` @ alpha `66` |
| `info` | `#3a94bcff` | `#0069ccff` | **B/C** ← `notificationsInfoIcon.foreground` |
| `info.background` | `#3a94bc1a` | `#0069cc1a` | **B/C** ← `info` @ alpha `1A` |
| `info.border` | `#3a94bc66` | `#0069cc66` | **B/C** ← `info` @ alpha `66` |
| `modified` | `#e5ba7dff` | `#667309ff` | **B/C** ← `gitDecoration.modifiedResourceForeground` |
| `modified.background` | `#e5ba7d1a` | `#6673091a` | **B/C** ← `modified` @ alpha `1A` |
| `modified.border` | `#e5ba7d66` | `#66730966` | **B/C** ← `modified` @ alpha `66` |
| `predictive` | `#555555ff` | `#bbbbbbff` | **B/C** ← `disabledForeground` |
| `predictive.background` | `#5555551a` | `#bbbbbb1a` | **B/C** ← `predictive` @ alpha `1A` |
| `predictive.border` | `#55555566` | `#bbbbbb66` | **B/C** ← `predictive` @ alpha `66` |
| `renamed` | `#48a0c7ff` | `#0069ccff` | **B/C** ← `textLink.foreground` |
| `renamed.background` | `#48a0c71a` | `#0069cc1a` | **B/C** ← `renamed` @ alpha `1A` |
| `renamed.border` | `#48a0c766` | `#0069cc66` | **B/C** ← `renamed` @ alpha `66` |
| `success` | `#73c991ff` | `#587c0cff` | **B/C** ← `gitDecoration.addedResourceForeground` |
| `success.background` | `#73c9911a` | `#587c0c1a` | **B/C** ← `success` @ alpha `1A` |
| `success.border` | `#73c99166` | `#587c0c66` | **B/C** ← `success` @ alpha `66` |
| `unreachable` | `#555555ff` | `#bbbbbbff` | **B/C** ← `disabledForeground` |
| `unreachable.background` | `#5555551a` | `#bbbbbb1a` | **B/C** ← `unreachable` @ alpha `1A` |
| `unreachable.border` | `#55555566` | `#bbbbbb66` | **B/C** ← `unreachable` @ alpha `66` |
| `warning` | `#cca700ff` | `#b69500ff` | **B/C** ← `notificationsWarningIcon.foreground` |
| `warning.background` | `#cca7001a` | `#b695001a` | **B/C** ← `warning` @ alpha `1A` |
| `warning.border` | `#cca70066` | `#b6950066` | **B/C** ← `warning` @ alpha `66` |
| `background.appearance` | `opaque` | `opaque` | `theme_importer` (map tự động từ key VSCode) |
| `pane_group.border` | `#2a2b2cff` | `#f0f1f2ff` | **F** ← `panel.border` |
| `scrollbar.thumb.active_background` | `#a8a9aa9c` | `#646464e0` | `theme_importer` (map tự động từ key VSCode) |
| `minimap.thumb.background` | `#a8a9aa85` | `#646464c0` | `theme_importer` (map tự động từ key VSCode) |
| `minimap.thumb.hover_background` | `#a8a9aa90` | `#646464d0` | `theme_importer` (map tự động từ key VSCode) |
| `minimap.thumb.active_background` | `#a8a9aa9c` | `#646464e0` | `theme_importer` (map tự động từ key VSCode) |
| `editor.document_highlight.bracket_background` | `#3994bc55` | `#0069cc40` | `theme_importer` (map tự động từ key VSCode) |
| `vim.yank.background` | `#242526ff` | `#eaeaeaff` | `theme_importer` (map tự động từ key VSCode) |

## Players — 8 entry

> `players[0]` = **local**: con trỏ editor (`editor.rs:26414`, `:29359`, `:29861`), con trỏ terminal
> (`terminal_element.rs:956`, `:1177`, `:1655`), search bar (`search_bar.rs:125`), và `read_only()` là
> grayscale của nó (`editor.rs:10314`). `players[7]` = `agent()`/`absent()`. **Không phải trang trí collab.**

| # | Vai | cursor (dark / light) | background | selection | Nguồn cursor |
|---|---|---|---|---|---|
| 0 | **local** — con trỏ editor + terminal | `#bbbebfff` / `#202020ff` | `#bbbebf4d` / `#2020204d` | `#276782dd` / `#0069cc40` | `editorCursor.foreground` |
| 1 | participant | `#cd861aff` / `#d18616ff` | `#cd861a4d` / `#d186164d` | `#cd861a3d` / `#d186163d` | `charts.orange` |
| 2 | participant | `#ef8773ff` / `#ad0707ff` | `#ef87734d` / `#ad07074d` | `#ef87733d` / `#ad07073d` | `charts.red` |
| 3 | participant | `#86cf86ff` / `#388a34ff` | `#86cf864d` / `#388a344d` | `#86cf863d` / `#388a343d` | `charts.green` |
| 4 | participant | `#ad80d7ff` / `#652d90ff` | `#ad80d74d` / `#652d904d` | `#ad80d73d` / `#652d903d` | `charts.purple` |
| 5 | participant | `#e0b97fff` / `#667309ff` | `#e0b97f4d` / `#6673094d` | `#e0b97f3d` / `#6673093d` | `charts.yellow` |
| 6 | participant | `#57a3f8ff` / `#1a5cffff` | `#57a3f84d` / `#1a5cff4d` | `#57a3f83d` / `#1a5cff3d` | `charts.blue` |
| 7 | **agent/absent** | `#8c8c8cff` / `#606060ff` | `#8c8c8c4d` / `#6060604d` | `#8c8c8c3d` / `#6060603d` | `descriptionForeground` |

`players[0].selection` ← `editor.selectionBackground`; các entry khác ← cursor @ alpha `3D`. background ← cursor @ alpha `4D`.

## Syntax — 45 key

| Zed syntax key | Dark 2026 | Light 2026 |
|---|---|---|
| `attribute` | `#9cdcfeff` | `#e50000ff` |
| `boolean` | `#569cd6ff` | `#0000ffff` |
| `comment` | `#8b949eff` | `#6e7781ff` |
| `comment.doc` | `#8b949eff` | `#6e7781ff` |
| `constant` | `#79c0ffff` | `#0550aeff` |
| `constructor` | `#7ee787ff` | `#116329ff` |
| `embedded` | `#d4d4d4ff` | `#000000ff` |
| `emphasis` | `#c9d1d9ff` | `#1f2328ff` |
| `emphasis.strong` | `#c9d1d9ff` | `#1f2328ff` |
| `function` | `#d2a8ffff` | `#8250dfff` |
| `keyword` | `#ff7b72ff` | `#cf222eff` |
| `label` | `#ffa657ff` | `#953800ff` |
| `link_text` | `#a5d6ffff` | `#0a3069ff` |
| `link_uri` | `#a5d6ffff` | `#0a3069ff` |
| `number` | `#b5cea8ff` | `#098658ff` |
| `operator` | `#d4d4d4ff` | `#000000ff` |
| `preproc` | `#569cd6ff` | `#0000ffff` |
| `property` | `#9cdcfeff` | `#e50000ff` |
| `punctuation` | `#808080ff` | `#800000ff` |
| `punctuation.bracket` | `#808080ff` | `#800000ff` |
| `punctuation.delimiter` | `#808080ff` | `#800000ff` |
| `punctuation.list_marker` | `#808080ff` | `#800000ff` |
| `punctuation.special` | `#808080ff` | `#800000ff` |
| `string` | `#a5d6ffff` | `#0a3069ff` |
| `string.escape` | `#ff7b72ff` | `#cf222eff` |
| `string.regex` | `#a5d6ffff` | `#0a3069ff` |
| `string.special` | `#a5d6ffff` | `#0a3069ff` |
| `string.special.symbol` | `#a5d6ffff` | `#0a3069ff` |
| `tag` | `#7ee787ff` | `#116329ff` |
| `text.literal` | `#a5d6ffff` | `#0a3069ff` |
| `title` | `#ffa657ff` | `#953800ff` |
| `type` | `#4ec9b0ff` | `#267f99ff` |
| `variable` | `#ffa657ff` | `#953800ff` |
| `variable.special` | `#79c0ffff` | `#0550aeff` |
| `enum` | `#ffa657ff` | `#953800ff` |
| `namespace` | `#ffa657ff` | `#953800ff` |
| `selector` | `#7ee787ff` | `#116329ff` |
| `selector.pseudo` | `#7ee787ff` | `#116329ff` |
| `punctuation.markup` | `#ffa657ff` | `#953800ff` |
| `diff.plus` | `#7ee787ff` | `#116329ff` |
| `diff.minus` | `#ffa198ff` | `#82071eff` |
| `primary` | `#bbbebfff` | `#202020ff` |
| `variant` | `#79c0ffff` | `#0550aeff` |
| `hint` | `#969696ff` | `#969696ff` |
| `predictive` | `#555555ff` | `#bbbbbbff` |

34 key do `theme_importer` map từ `tokenColors`; 11 key còn lại (`enum`, `namespace`, `selector`,
`selector.pseudo`, `punctuation.markup`, `diff.plus`, `diff.minus`, `primary`, `variant`, `hint`,
`predictive`) suy theo **bảng I**.

## Accents — 6 màu

Dark: `#cd861aff`, `#ef8773ff`, `#86cf86ff`, `#ad80d7ff`, `#e0b97fff`, `#57a3f8ff`

Light: `#d18616ff`, `#ad0707ff`, `#388a34ff`, `#652d90ff`, `#667309ff`, `#1a5cffff`

← `charts.orange/red/green/purple/yellow/blue`. Bỏ trống sẽ âm thầm rơi về palette built-in của Zed.
