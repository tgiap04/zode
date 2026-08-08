# Phase 02 — Dựng "Dark 2026"

## Context Links

- Bảng ánh xạ: [`phase-01-color-mapping-table.md`](phase-01-color-mapping-table.md) → `reference/color-mapping.md`
- Schema tham chiếu: `assets/themes/one/one.json`
- Chạy **song song được** với [phase 03](phase-03-build-light-theme.md) — file khác nhau

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** phase 01 · **Effort:** ~0.5d

Áp bảng ánh xạ lên `flat-2026-dark.json`, xuất ra `build/dark-2026.json` — một object theme
đúng schema Zed, đủ 141 style + 45 syntax + 8 players + 24 ANSI.

## Key Insights

- Phase này **không phán đoán**. Mọi quyết định màu đã nằm trong bảng phase 01. Gặp ô trống ⇒
  dừng và quay lại phase 01, **không** tự chế màu tại chỗ.
- Output importer là **nguyên liệu**, không phải kết quả. Bảng A (3 key ghi đè) áp **sau cùng**.

## Requirements

**Chức năng:** `build/dark-2026.json` chứa `{"name": "Dark 2026", "appearance": "dark", "style": {...}}`.
**Phi chức năng:** JSON hợp lệ, key sắp cùng thứ tự với `one.json` để diff giữa các theme đọc được.

## Architecture

Cấu trúc object (khớp `assets/themes/one/one.json` → `themes[]`):

```jsonc
{
  "name": "Dark 2026",
  "appearance": "dark",
  "style": {
    /* 141 top-level key — trong đó: */
    "players": [ /* 8 object {cursor, background, selection} */ ],
    "syntax":  { /* 45 key {color, font_style, font_weight} */ }
  }
}
```

## Related Code Files

**Đọc:** `reference/color-mapping.md`, `assets/themes/one/one.json`
**Tạo:** `plans/260807-1300-vscode-2026-default-themes/build/dark-2026.json`
**Không sửa:** bất kỳ file nào dưới `crates/` hoặc `assets/`

## Implementation Steps

1. Lấy `imported-dark.json` (49 key nền) làm điểm khởi đầu.
2. Áp bảng B–G của phase 01 cho 92 key còn thiếu, dùng **cột dark**.
3. Áp bảng H: điền đủ **8** entry `players`. Index 0 = `editorCursor.foreground` `#BBBEBF`,
   selection = `editor.selectionBackground` `#276782dd`. Index 7 = `descriptionForeground` `#8C8C8C`.
4. Áp bảng I: bổ sung **11** syntax key còn thiếu.
5. Điền **24** key `terminal.ansi.*` từ cột `dark` của bảng ANSI (16 base/bright + 8 `dim_*` ở alpha `B3`).
6. **Áp bảng A sau cùng** — ghi đè `border.transparent` → `#00000000`,
   `border.variant` → `border` trộn 50% về `editor.background`, `border.selected` → `#3994BCB3`.
7. Chạy đối soát phủ sóng (script ở Success Criteria). Phải ra `0 0 0`.

## Todo List

- [ ] Dựng khung từ `imported-dark.json`
- [ ] Điền 92 key thiếu theo bảng B–G (cột dark)
- [ ] Điền 8 players theo bảng H — kiểm index 0 và 7
- [ ] Điền 11 syntax key theo bảng I
- [ ] Điền 24 terminal ANSI key
- [ ] Áp bảng A sau cùng, xác nhận không bị ghi ngược
- [ ] Đối soát phủ sóng ra `0 0 0`
- [ ] JSON parse sạch

## Success Criteria

```bash
python3 - <<'PY'
import json
t=json.load(open('build/dark-2026.json'))['style']
r=json.load(open('assets/themes/one/one.json'))['themes'][0]['style']
missing=[k for k in r if k not in t]
extra=[k for k in t if k not in r]
empty=[k for k,v in t.items() if v is None or v=='']
print(len(missing), len(extra), len(empty))          # phải là: 0 0 0
print(len(t['players']), len(t['syntax']))            # phải là: 8 45
print(len([k for k in t if k.startswith('terminal.ansi.')]))  # phải là: 24
print(t['border.transparent'],
      t['border'] != t['border.variant'] != t['border.selected'])  # #00000000 True
PY
```

- Ba dòng đầu ra đúng: `0 0 0` · `8 45` · `24`
- `border.transparent == "#00000000"` và ba giá trị border khác nhau
- Không key nào rơi về fallback

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Áp bảng A trước rồi bị output importer ghi đè ngược | Bước 6 đặt bảng A **sau cùng**; todo có mục kiểm riêng |
| `players` để rỗng ⇒ âm thầm rơi về palette xanh của Zed, không lỗi | Script đối soát kiểm `len(players) == 8` |
| Tự chế màu tại chỗ khi gặp ô trống | Quy tắc rõ: dừng, quay lại phase 01 |

## Security Considerations

Không có. Dữ liệu màu tĩnh.

## Next Steps

Cùng phase 03 mở khoá phase 04 (lắp ráp + đổi mặc định).
