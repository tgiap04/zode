---
title: 'Theme mặc định: port VSCode "Dark 2026" / "Light 2026" sang zode'
description: >-
  Thêm assets/themes/vscode-2026/vscode-2026.json chứa hai theme "Dark 2026" +
  "Light 2026" dựng đủ 141 style key + 45 syntax key + 8 players + 24 terminal
  ANSI, rồi đổi mặc định của app sang chúng. Giữ nguyên One Dark / One Light.
status: pending
priority: P2
effort: 2-3d
branch: feat/vscode-2026-default-themes
tags:
  - theme
  - ui
  - assets
  - fork
blockedBy: []
blocks: []
work_type: deliverable
spec_waived: >-
  SDD mode tắt trong project này (theo tiền lệ plan 260805-1913). Đây là port
  một artifact màu có nguồn xác định, không phải capability mới — yêu cầu đã
  được chốt trọn trong plans/reports/brainstorm-260807-vscode-2026-default-themes.md
created: 2026-08-07
---

# Theme mặc định: port VSCode "Dark 2026" / "Light 2026"

**Thiết kế đã sealed** tại [`plans/reports/brainstorm-260807-vscode-2026-default-themes.md`](../reports/brainstorm-260807-vscode-2026-default-themes.md).
Đọc file đó trước — nó chứa khảo sát đã kiểm chứng, số đo thật, và các đường đã loại kèm lý do.
**Không re-litigate** các quyết định đã chốt.

## Phạm vi

Thêm **một file mới** `assets/themes/vscode-2026/vscode-2026.json` + sửa **hai điểm** cấu hình.
Không đụng `crates/theme_importer`, không đụng border radius, không giữ script generator trong repo.

## Các phase

| # | Phase | Trạng thái | Phụ thuộc | Sở hữu file |
|---|-------|-----------|-----------|-------------|
| 01 | [Bảng ánh xạ màu](phase-01-color-mapping-table.md) | ✅ done | — | `reference/color-mapping.md` |
| 02 | [Dựng Dark 2026](phase-02-build-dark-theme.md) | ✅ done | 01 | `build/dark-2026.json` |
| 03 | [Dựng Light 2026](phase-03-build-light-theme.md) | ✅ done | 01 | `build/light-2026.json` |
| 04 | [Lắp ráp + đổi mặc định](phase-04-assemble-and-wire-defaults.md) | ✅ done | 02, 03 | `assets/themes/vscode-2026/`, `assets/settings/default.json`, `crates/settings_content/src/theme.rs` |
| 05 | [Đối chiếu thị giác](phase-05-visual-verification.md) | ⏸ chờ người dùng | 04 | như phase 04 |

Phase 02 và 03 **chạy song song được** — hai file khác nhau, cùng ăn output của phase 01.

## Phụ thuộc chéo giữa các plan

`assets/settings/default.json` bị **cả ba plan đang mở** đụng tới, nhưng ở các key khác nhau:

| Plan | Key trong default.json |
|---|---|
| Plan này (phase 04) | `theme.light` / `theme.dark` (`:11-12`) |
| `260726-1531-remove-auth-cloud-hard-fork` (phase 09) | 21 key thuộc nhóm auth/cloud/agent |
| `260805-1913-multi-project-window-switching` (phase 01/02/05/06) | `multi_project.*`, `hibernate_after_ms` |

→ **Không có phụ thuộc ngữ nghĩa** (không plan nào cần output của plan kia), nên `blockedBy`/`blocks`
để trống. Chỉ là rủi ro merge-conflict cùng file, xử lý ở mức git.

**Một điểm cần đồng bộ:** `260726-1531/phase-09:97` ghi *"`players` must stay in all three theme files"*
kèm 10 call site non-collab. Sau plan này sẽ có **bốn** file theme — file mới cũng bắt buộc có `players`.
Xem [Rủi ro](#rủi-ro) bên dưới.

## Rủi ro

| Rủi ro | Mức | Đối phó |
|---|---|---|
| `players` rỗng ⇒ **âm thầm rơi về palette xanh built-in của Zed**, không lỗi, không cảnh báo. Mà `players[0]` điều khiển **con trỏ editor, con trỏ terminal, search bar, read-only cursor** — không phải trang trí collab | **Cao** | Phase 02/03 bắt buộc điền đủ 8 entry; phase 05 kiểm bằng mắt con trỏ editor + terminal |
| Sửa `DEFAULT_DARK_THEME` chạm code upstream Zed | Trung bình | Một hằng số duy nhất; ghi vào changelog để nhớ khi rebase |
| Light 2026 dùng nhiều màu có alpha (`#00000025`, `#D8D8D866`) — Zed hợp nhất alpha khác VSCode | Trung bình | Phase 03 có bước soi riêng trạng thái selected/hover trên nền sáng |
| Đổi theme mặc định là thay đổi **nhìn thấy ngay với mọi người dùng** zode | Trung bình | Giữ nguyên One Dark/One Light trong selector để rollback tức thì |
| Output `theme_importer` có lỗi thật (`border.transparent` đục, `border.variant`/`border.selected` bị gộp) | Trung bình | Phase 01 ghi rõ 3 key này là ngoại lệ bắt buộc ghi đè tay |

## Kết quả thực thi (2026-08-07)

Phase 01–04 đã xong. Ba điều chệch khỏi bản vẽ, đều có lý do:

1. **Điểm sửa mặc định đã hiệu chỉnh** — bản vẽ ghi `crates/theme/src/theme.rs:45`; trace call site
   cho thấy đó là hằng số **fallback khẩn cấp**, không phải mặc định. Đã sửa
   `crates/settings_content/src/theme.rs:272-273` thay thế; `theme.rs:45` giữ nguyên `"One Dark"`.
   Chi tiết trong [phase 04](phase-04-assemble-and-wire-defaults.md) § Key Insight.
2. **`terminal.background` của Light 2026 không có nguồn** — VSCode để terminal thừa hưởng
   `panel.background` (terminal nằm trong panel). Dùng đúng luật đó: `#fafafdff`.
3. **Thêm 3 test vào `crates/theme_settings`** — ngoài phạm vi bản vẽ, nhưng bắt buộc:
   `load_bundled_themes` (`theme_settings.rs:193-202`) nuốt lỗi parse bằng `.log_err()` rồi `continue`,
   nên theme hỏng chỉ **âm thầm vắng mặt**. clippy xanh không chứng minh được gì. Test đã được
   mutation-test (đổi tên theme thành `"Dark 2027"` → fail đúng như mong đợi).

**Bổ sung ngoài yêu cầu tối thiểu:** `accents` (6 màu) cũng được điền từ `charts.*` — bỏ trống sẽ
âm thầm rơi về palette built-in giống hệt vấn đề của `players`.

**Danh sách chờ alpha** từ phase 03 bước 8: **rỗng** — cả 6 cặp đều vượt ngưỡng tương phản 3%
(thấp nhất là `editor.active_line.background` ở 4.40%). Phase 05 không phải quyết định gì về alpha.

## Định nghĩa hoàn thành

- `./script/clippy` sạch (**không** dùng `cargo clippy`)
- zode khởi động; "Dark 2026" + "Light 2026" xuất hiện trong theme selector; One Dark/One Light còn nguyên
- **141/141** style key, **45/45** syntax key, **8/8** players, **24/24** terminal ANSI đều có giá trị — không key nào rơi về fallback
- `border.transparent = "#00000000"`; `border`, `border.variant`, `border.selected` là **ba giá trị khác nhau**
- Đối chiếu ảnh chụp zode vs VSCode trên: editor, sidebar, tab bar, status bar, terminal, command palette, popover/menu
