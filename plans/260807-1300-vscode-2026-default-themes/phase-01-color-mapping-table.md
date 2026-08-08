# Phase 01 — Bảng ánh xạ màu (source-of-truth)

## Context Links

- Biên bản tư vấn đã sealed: [`plans/reports/brainstorm-260807-vscode-2026-default-themes.md`](../reports/brainstorm-260807-vscode-2026-default-themes.md)
- Schema tham chiếu: `assets/themes/one/one.json` (141 style key + 45 syntax key + 8 players)
- Định nghĩa players: `crates/theme/src/styles/players.rs`
- Nguồn VSCode: `/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/theme-defaults/themes/`

## Overview

**Priority:** P1 (chặn phase 02 và 03) · **Status:** pending · **Effort:** ~0.5d

Dựng **một** bảng ánh xạ dùng chung cho cả hai theme: mỗi key của Zed ← key nguồn VSCode,
hoặc công thức suy dẫn khi VSCode không có khái niệm tương ứng. Đây là phần phán đoán duy nhất
của cả plan; phase 02/03 chỉ áp bảng này một cách máy móc.

## Key Insights

- Importer chỉ điền **49/141** top-level key. 92 key còn lại phải suy — nhưng **gần như tất cả đều
  truy được nguồn thật** trong palette 2026, không phải bịa.
- Ba key importer điền **SAI**, bắt buộc ghi đè tay (xem bảng A).
- `editorError/Warning/Info/Hint.foreground` **không tồn tại** trong cả hai theme 2026 → phải suy
  từ nhóm `notifications*Icon.foreground` và `errorForeground`.
- `diffEditor.insertedLineBackground` / `removedLineBackground` **chỉ có ở dark**, light thiếu → light
  suy từ `*TextBackground` hạ alpha.
- `terminal.ansi*` = **0 key** trong cả chuỗi include → lấy từ bảng mặc định hardcode của VSCode.

## Requirements

**Chức năng:** Bảng phủ **đủ** 141 style key + 45 syntax key + 8 players + 24 terminal ANSI, cho
**cả** dark và light. Không ô nào để trống.

**Phi chức năng:** Mỗi dòng phải ghi rõ nguồn — hoặc là tên key VSCode, hoặc là công thức. Không hex trần
không giải thích. Người đọc sau phải kiểm chứng lại được.

## Architecture — Luật suy dẫn

### A. Ba key BẮT BUỘC ghi đè (importer sai)

| Zed key | Importer cho | Phải là | Vì sao |
|---|---|---|---|
| `border.transparent` | `#2A2B2CFF` (đục) | `#00000000` | Đúng nghĩa "trong suốt". Để đục sẽ vẽ viền ở mọi chỗ Zed cố tình giấu viền |
| `border.variant` | `= border` | `border` trộn 50% về `editor.background` | Zed dùng variant cho phân cách *nhạt hơn* border chính. Gộp = mất phân tầng |
| `border.selected` | `= border` | `focusBorder` (`#3994BCB3` / `#0069CCFF`) | Viền phần tử đang chọn phải theo accent, không phải viền trung tính |

### B. Trạng thái (status)

| Zed key | Dark ← | Light ← |
|---|---|---|
| `error` | `errorForeground` `#f48771` | `errorForeground` `#ad0707` |
| `warning` | `notificationsWarningIcon.foreground` `#CCA700` | `#B69500` |
| `info` | `notificationsInfoIcon.foreground` `#3a94bc` | `#0069CC` |
| `success` | `gitDecoration.addedResourceForeground` `#73c991` | `#587c0c` |
| `hint` | `descriptionForeground` `#8C8C8C` | `#606060` |
| `predictive` | `disabledForeground` `#555555` | `#BBBBBB` |
| `unreachable` | `disabledForeground` `#555555` | `#BBBBBB` |

**`.background` / `.border` của mỗi mục trên:** `.background` = màu gốc ở alpha `1A`;
`.border` = màu gốc ở alpha `66`. Quy tắc đồng nhất, không đặt tay từng cái.

### C. Version control / diff

| Zed key | Dark ← | Light ← |
|---|---|---|
| `created`, `version_control.added` | `gitDecoration.addedResourceForeground` `#73c991` | `#587c0c` |
| `modified`, `version_control.modified` | `gitDecoration.modifiedResourceForeground` `#e5ba7d` | `#667309` |
| `deleted`, `version_control.deleted` | `gitDecoration.deletedResourceForeground` `#f48771` | `#ad0707` |
| `conflict` | `gitDecoration.conflictingResourceForeground` `#f48771` | `#ad0707` |
| `ignored`, `hidden` | `gitDecoration.ignoredResourceForeground` `#8C8C8C` | `#8E8E90` |
| `renamed` | `textLink.foreground` `#48A0C7` | `#0069CC` |
| `version_control.word_added` | `diffEditor.insertedTextBackground` `#57ab5a4d` | `#587c0c26` |
| `version_control.word_deleted` | `diffEditor.removedTextBackground` `#f470674d` | `#ad070726` |
| `version_control.conflict_marker.ours` | `diffEditor.insertedLineBackground` `#347d3926` | **suy:** `#587c0c` @ alpha `26` |
| `version_control.conflict_marker.theirs` | `diffEditor.removedLineBackground` `#c93c3726` | **suy:** `#ad0707` @ alpha `26` |

`.background`/`.border` của `created/modified/deleted/conflict/ignored/hidden/renamed`: cùng luật alpha `1A`/`66` như bảng B.

### D. Icon & text

| Zed key | ← |
|---|---|
| `icon` | `icon.foreground` (`#8C8C8C` / `#606060`) |
| `icon.muted` | `editorCodeLens.foreground` (`#8C8C8C` / `#606060`) |
| `icon.disabled` | `disabledForeground` (`#555555` / `#BBBBBB`) |
| `icon.placeholder` | `input.placeholderForeground` (`#555555` / `#999999`) |
| `icon.accent` | `textLink.foreground` (`#48A0C7` / `#0069CC`) |
| `text.accent` | `textLink.foreground` |
| `text.disabled` | `disabledForeground` |
| `text.placeholder` | `input.placeholderForeground` |

### E. Element & ghost_element

`ghost_element.*` = phần tử **không** có nền khi nghỉ (nút toolbar, item cây thư mục).

| Zed key | ← |
|---|---|
| `ghost_element.background` | `#00000000` (theo định nghĩa) |
| `ghost_element.active` | `toolbar.activeBackground` (`#FFFFFF33` / `#D6D6D8`) |
| `ghost_element.disabled` | `#00000000` |
| `element.active` | `toolbar.activeBackground` |
| `element.disabled` | `element.background` (giữ nền, chỉ text/icon mờ đi) |

### F. Editor bổ sung

| Zed key | Dark ← | Light ← |
|---|---|---|
| `editor.document_highlight.read_background` | `editor.wordHighlightBackground` `#27678250` | `#0069CC26` |
| `editor.document_highlight.write_background` | `editor.wordHighlightStrongBackground` `#27678280` | `#0069CC26` |
| `editor.highlighted_line.background` | `editor.rangeHighlightBackground` `#242526` | `#EAEAEA` |
| `editor.invisible` | `editorWhitespace.foreground` `#8C8C8C4D` | `#60606040` |
| `editor.hover_line_number` | `editorLineNumber.activeForeground` `#BBBEBF` | `#202020` |
| `editor.subheader.background` | `editorGroupHeader.tabsBackground` `#191A1B` | `#FAFAFD` |
| `search.active_match_background` | `editor.findMatchBackground` `#27678290` | `#0069CC40` |
| `pane.focused_border`, `panel.focused_border` | `focusBorder` `#3994BCB3` | `#0069CCFF` |
| `title_bar.inactive_background` | `titleBar.inactiveBackground` `#191A1B` | `#FAFAFD` |

### G. Terminal

`terminal.foreground` ← `foreground` (`#bfbfbf` / `#202020`).
`terminal.bright_foreground` ← `editor.foreground`. `terminal.dim_foreground` ← `descriptionForeground`.

**24 key ANSI** ← bảng mặc định hardcode của VSCode (trích ở bước 2 bên dưới). 16 màu base/bright
map 1-1. **8 key `dim_*`**: VSCode không có khái niệm dim → dùng màu base tương ứng ở alpha `B3` (70%).

### H. Players — 8 entry (KHÔNG phải trang trí collab)

> **Đọc kỹ.** `crates/theme/src/styles/players.rs` cho thấy `players[0]` = **local player**, điều khiển
> con trỏ editor (`editor.rs:26414`, `:29359`, `:29861`), con trỏ terminal (`terminal_element.rs:956`,
> `:1177`, `:1655`), search bar (`search_bar.rs:125`), và `read_only()` = grayscale của nó
> (`editor.rs:10314`). `agent()`/`absent()` = **phần tử cuối** (`players[7]`).
> Bỏ trống ⇒ `merge_player_colors` (`theme_settings.rs:342`) **return sớm**, không lỗi, không cảnh báo —
> con trỏ âm thầm giữ màu xanh built-in của Zed.

| Index | cursor ← | background ← | selection ← |
|---|---|---|---|
| **0 (local)** | `editorCursor.foreground` (`#BBBEBF` / `#202020`) | cursor @ alpha `4D` | `editor.selectionBackground` (`#276782dd` / `#0069CC40`) |
| 1 | `charts.orange` (`#CD861A` / `#d18616`) | @ alpha `4D` | @ alpha `3D` |
| 2 | `charts.red` (`#EF8773` / `#ad0707`) | @ alpha `4D` | @ alpha `3D` |
| 3 | `charts.green` (`#86CF86` / `#388A34`) | @ alpha `4D` | @ alpha `3D` |
| 4 | `charts.purple` (`#AD80D7` / `#652D90`) | @ alpha `4D` | @ alpha `3D` |
| 5 | `charts.yellow` (`#E0B97F` / `#667309`) | @ alpha `4D` | @ alpha `3D` |
| 6 | `charts.blue` (`#57A3F8` / `#1A5CFF`) | @ alpha `4D` | @ alpha `3D` |
| **7 (agent/absent)** | `descriptionForeground` (`#8C8C8C` / `#606060`) | @ alpha `4D` | @ alpha `3D` |

→ Kể cả players **cũng có nguồn thật** trong theme 2026 (`charts.*`), không phải sáng tác.

### I. Syntax — 11 key importer bỏ sót

| Zed syntax key | ← tokenColor scope |
|---|---|
| `enum` | `entity.name` (dark `#ffa657`) |
| `namespace` | `entity.name` |
| `selector`, `selector.pseudo` | `entity.name.tag` (`#7ee787`) |
| `punctuation.markup` | `punctuation.definition.list.begin.markdown` (`#ffa657`) |
| `diff.plus` | `markup.inserted` (`#7ee787`) |
| `diff.minus` | `markup.deleted` (`#ffa198`) |
| `primary` | `editor.foreground` (không có scope tương ứng) |
| `variant` | `support` (`#79c0ff`) |
| `hint` | key `hint` ở bảng B |
| `predictive` | key `predictive` ở bảng B |

## Related Code Files

**Đọc:** `assets/themes/one/one.json`, `crates/theme/src/styles/players.rs`, `crates/theme_settings/src/theme_settings.rs:338-370`
**Tạo:** `plans/260807-1300-vscode-2026-default-themes/reference/color-mapping.md`
**Không sửa:** `crates/theme_importer/**` (quyết định đã sealed)

## Implementation Steps

1. **Flatten chuỗi include** (công thức nằm ở đây, KHÔNG tạo script trong repo):

```bash
python3 - <<'PY'
import json, os
SRC="/Applications/Visual Studio Code.app/Contents/Resources/app/extensions/theme-defaults/themes"
def load(name):
    d=json.load(open(os.path.join(SRC,name)))
    colors, tokens = {}, []
    if d.get("include"):
        pc, pt = load(os.path.basename(d["include"])); colors.update(pc); tokens.extend(pt)
    colors.update(d.get("colors",{})); tokens.extend(d.get("tokenColors",[]))
    return colors, tokens
for base, out in [("2026-dark.json","flat-2026-dark.json"),("2026-light.json","flat-2026-light.json")]:
    d=json.load(open(os.path.join(SRC,base))); c,t=load(base)
    json.dump({"name":d["name"],"type":d["type"],"colors":c,"tokenColors":t,"semanticHighlighting":True}, open(out,"w"), indent=2)
PY
```
   Kỳ vọng: dark **328 màu / 118 tokenColors**, light **342 / 113**. Lệch số ⇒ VSCode đã cập nhật, dừng lại rà soát.

2. **Trích bảng ANSI** từ bundle VSCode:

```bash
python3 - <<'PY'
import re
p="/Applications/Visual Studio Code.app/Contents/Resources/app/out/vs/workbench/workbench.desktop.main.js"
s=open(p,encoding='utf-8',errors='replace').read()
i=s.find('"terminal.ansiBlack":{index:0')
for n,idx,l,d in re.findall(r'"(terminal\.ansi\w+)":\{index:(\d+),defaults:\{light:"(#\w+)",dark:"(#\w+)"', s[i:i+4000]):
    print(f"{idx:>2} {n:<28} light={l}  dark={d}")
PY
```
   Kỳ vọng **đúng 16 dòng**. Ít hơn ⇒ VSCode đổi cấu trúc bundle, dừng lại.

3. Chạy `cargo run -q -p theme_importer -- flat-2026-dark.json -o imported-dark.json` (và bản light)
   để lấy 49 key map được. **Chỉ dùng làm nguyên liệu** — không phải kết quả.
4. Viết `reference/color-mapping.md`: mỗi dòng = `zed_key | dark_hex | light_hex | nguồn`.
   Áp bảng A–I ở trên. Bảng A ghi đè output importer **sau cùng**, không để bị ghi ngược lại.
5. **Đối soát phủ sóng:** so danh sách key trong bảng với danh sách key của `assets/themes/one/one.json`.
   Chênh lệch phải bằng **0** ở cả hai chiều.

## Todo List

- [ ] Flatten hai theme, xác nhận đúng 328/118 và 342/113
- [ ] Trích 16 màu ANSI, xác nhận đủ 16 dòng
- [ ] Chạy importer cho cả hai, lấy 49 key nền
- [ ] Áp bảng A (3 key ghi đè) — kiểm `border.transparent = #00000000`
- [ ] Áp bảng B–G (status, VC, icon/text, element, editor, terminal)
- [ ] Áp bảng H (8 players) — đối chiếu lại `players.rs` xem index 0 và 7 đúng vai
- [ ] Áp bảng I (11 syntax key)
- [ ] Đối soát phủ sóng với `one.json`: chênh lệch = 0
- [ ] Ghi `reference/color-mapping.md`

## Success Criteria

- `reference/color-mapping.md` có **141 + 45 + 8 + 24** dòng, không ô trống
- Mỗi dòng ghi được nguồn (key VSCode hoặc công thức) — không hex trần
- Đối soát với `one.json`: không key thừa, không key thiếu
- `border.transparent = #00000000`; `border` ≠ `border.variant` ≠ `border.selected`

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| VSCode cập nhật theme 2026 ⇒ số màu lệch | Bước 1 và 2 đều có kỳ vọng số cụ thể để dừng sớm |
| Luật alpha (`1A`/`66`/`4D`/`3D`) làm màu chìm trên nền sáng | Phase 03 có bước soi riêng cho light |
| Suy sai vai `players[0]` / `players[7]` | Bước todo bắt buộc đối chiếu lại `players.rs:126-145` |

## Security Considerations

Không có. Đây là dữ liệu màu tĩnh, không có đầu vào người dùng, không có bề mặt mạng.

## Next Steps

Mở khoá phase 02 (Dark) và phase 03 (Light) — **chạy song song được**, hai file khác nhau.
