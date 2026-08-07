# Phase 03 — Dựng "Light 2026"

## Context Links

- Bảng ánh xạ: [`phase-01-color-mapping-table.md`](phase-01-color-mapping-table.md) → `reference/color-mapping.md`
- Chạy **song song được** với [phase 02](phase-02-build-dark-theme.md) — file khác nhau

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** phase 01 · **Effort:** ~0.5d

Giống phase 02 nhưng dùng **cột light** của bảng ánh xạ, xuất ra `build/light-2026.json`.
Phase này có thêm **một bước riêng** mà phase 02 không có: soi alpha trên nền sáng.

## Key Insights

- **Light 2026 dùng nhiều màu có alpha trên nền trắng**, và đó là chỗ dễ hỏng nhất:
  `list.activeSelectionBackground #00000025`, `input.border #D8D8D866`, `list.hoverBackground #00000014`,
  `editorIndentGuide.background #F7F7F740`. Alpha thấp trên nền `#FFFFFF` cho tương phản rất mỏng.
  Zed hợp nhất alpha ở một số chỗ khác VSCode ⇒ **không suy ra được, phải nhìn**.
- **Hai key light thiếu hẳn nguồn:** `diffEditor.insertedLineBackground` và `removedLineBackground`
  (dark có, light `None`). Bảng C của phase 01 đã cho công thức suy — dùng đúng công thức đó.
- Light 2026 dùng bảng `charts.*` **bão hoà hơn hẳn** bản dark (`#ad0707`, `#652D90`, `#1A5CFF`)
  vì phải bám trên nền trắng. Players lấy đúng cột light, **không** tái dùng hex của dark.

## Requirements

**Chức năng:** `build/light-2026.json` chứa `{"name": "Light 2026", "appearance": "light", "style": {...}}`,
đủ 141 style + 45 syntax + 8 players + 24 ANSI.
**Phi chức năng:** Các trạng thái selected/hover phải **phân biệt được bằng mắt** trên nền `#FFFFFF`
và `#FAFAFD` — không chỉ đúng về mặt số.

## Architecture

Giống phase 02, khác `name` / `appearance` và toàn bộ giá trị lấy từ cột light.

## Related Code Files

**Đọc:** `reference/color-mapping.md`, `assets/themes/one/one.json`
**Tạo:** `plans/260807-1300-vscode-2026-default-themes/build/light-2026.json`
**Không sửa:** bất kỳ file nào dưới `crates/` hoặc `assets/`

## Implementation Steps

1–7. Như phase 02, nhưng dùng **cột light**. `border.selected` → `#0069CCFF`.
   Players index 0: cursor `#202020`, selection `#0069CC40`; index 7: `#606060`.

8. **Bước riêng của phase này — soi alpha trên nền sáng.** Với mỗi cặp dưới đây, tính màu hợp nhất
   trên nền tương ứng và kiểm tương phản có đủ tách bạch không:

   | Key | Nền hợp nhất trên | Ghi chú |
   |---|---|---|
   | `element.selected` ← `list.activeSelectionBackground #00000025` | `#FAFAFD` (sidebar) | alpha 14% — mỏng nhất, rủi ro cao nhất |
   | `element.hover` ← `list.hoverBackground #00000014` | `#FAFAFD` | alpha 8% |
   | `ghost_element.hover` | `#FFFFFF` (editor) | phải thấy được cả trên nền trắng |
   | `border.focused` ← `focusBorder #0069CCFF` | `#FFFFFF` | đục, an toàn |
   | `editor.active_line.background` | `#FFFFFF` | |
   | `search.match_background` ← `#0069CC1A` | `#FFFFFF` | alpha 10% trên trắng |

   Chênh lệch độ sáng so với nền **dưới 3%** ⇒ ghi nhận vào danh sách chờ, để phase 05 quyết định
   sau khi nhìn thực tế. **Không tự nâng alpha ở phase này** — sealed decision là bê nguyên số VSCode.

## Todo List

- [ ] Dựng khung từ `imported-light.json`
- [ ] Điền 92 key thiếu theo bảng B–G (cột light)
- [ ] Suy hai key `conflict_marker.ours/theirs` theo công thức bảng C (light thiếu nguồn trực tiếp)
- [ ] Điền 8 players theo cột light — **không** tái dùng hex của dark
- [ ] Điền 11 syntax key + 24 terminal ANSI (cột light)
- [ ] Áp bảng A sau cùng — `border.selected` = `#0069CCFF`
- [ ] Chạy bước 8: soi alpha 6 cặp, ghi danh sách chờ cho phase 05
- [ ] Đối soát phủ sóng ra `0 0 0`

## Success Criteria

- Cùng script đối soát của phase 02 (đổi đường dẫn sang `build/light-2026.json`): `0 0 0` · `8 45` · `24`
- `border.transparent == "#00000000"`, ba giá trị border khác nhau
- Có **danh sách chờ** từ bước 8 (kể cả khi rỗng) bàn giao cho phase 05

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Trạng thái selected/hover chìm vào nền trắng | Bước 8 đo trước, phase 05 nhìn sau; không sửa mù |
| Tái dùng hex của dark cho players/charts | Todo có mục kiểm riêng |
| Tự ý nâng alpha để "cho dễ nhìn" ⇒ lệch khỏi VSCode | Bước 8 nói rõ: chỉ ghi nhận, không sửa |

## Security Considerations

Không có.

## Next Steps

Cùng phase 02 mở khoá phase 04.
