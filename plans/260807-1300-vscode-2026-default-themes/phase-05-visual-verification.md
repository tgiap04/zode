# Phase 05 — Đối chiếu thị giác với VSCode

## Context Links

- [phase 04](phase-04-assemble-and-wire-defaults.md) — file theme đã lắp và mặc định đã đổi
- Danh sách chờ alpha từ [phase 03](phase-03-build-light-theme.md) bước 8

## Overview

**Priority:** P2 · **Status:** pending · **Depends:** phase 04 · **Effort:** ~0.5d

Chạy zode cạnh VSCode, đối chiếu từng vùng, đóng các sai lệch còn lại. Đây là phase **duy nhất**
được phép sửa màu dựa trên mắt thay vì bảng.

## Key Insights

- Zed và VSCode **không có cùng bộ khung UI**. VSCode có activity bar, Zed không. Zed vẽ viền ở
  nhiều chỗ hơn. Nên mục tiêu là **cùng một cảm giác và cùng hệ màu**, không phải trùng khít pixel —
  đòi trùng khít là đòi sai thứ.
- `players[0]` chi phối con trỏ editor **và** con trỏ terminal. Nếu bỏ sót, hai chỗ này sẽ giữ màu
  xanh built-in của Zed mà **không có lỗi nào báo** — chỉ nhìn mới thấy. Đây là mục kiểm quan trọng nhất.

## Requirements

**Chức năng:** 7 vùng dưới đây được đối chiếu và ghi nhận bằng ảnh chụp.
**Phi chức năng:** Mọi thay đổi màu ở phase này phải ghi ngược lại vào `reference/color-mapping.md`
kèm lý do — bảng ánh xạ không được để lệch khỏi file theme đã ship.

## Architecture

Không có thay đổi kiến trúc. Chỉ tinh chỉnh giá trị trong `assets/themes/vscode-2026/vscode-2026.json`.

## Related Code Files

**Sửa (nếu cần):** `assets/themes/vscode-2026/vscode-2026.json`, `reference/color-mapping.md`
**Không sửa:** mọi thứ dưới `crates/`

## Implementation Steps

1. Mở cùng một repo trong zode và VSCode, cùng appearance (dark trước, light sau).
2. Chụp và đối chiếu **7 vùng**:

   | # | Vùng | Chú ý riêng |
   |---|---|---|
   | 1 | Editor + gutter | **con trỏ text** (← `players[0].cursor`), số dòng, active line, selection |
   | 2 | Sidebar / project tree | nền, hover, selected, icon trạng thái git |
   | 3 | Tab bar | tab active vs inactive, viền trên tab active, hover |
   | 4 | Status bar | nền, chữ, item hover |
   | 5 | **Terminal** | nền, **con trỏ** (← `players[0].cursor`), 16 màu ANSI |
   | 6 | Command palette / quick input | nền, item được chọn, chữ highlight |
   | 7 | Popover / menu / suggest widget | nền, viền, separator, item selected |

3. Chạy `ls --color=always` hoặc tương đương trong terminal của **cả hai** để so trực tiếp 16 màu ANSI.
4. Duyệt **danh sách chờ alpha** từ phase 03 bước 8: giữ nguyên số VSCode, hay nâng tương phản?
   Quyết định ở đây, sau khi đã nhìn.
5. Lặp lại toàn bộ cho light mode.
6. Mọi giá trị đã sửa → ghi ngược vào `reference/color-mapping.md` kèm lý do.
7. `./script/clippy` lần cuối.

## Todo List

- [ ] Đối chiếu 7 vùng ở **dark**, chụp ảnh từng vùng
- [ ] **Kiểm riêng con trỏ editor và con trỏ terminal** — không được là xanh built-in của Zed
- [ ] So 16 màu ANSI bằng output terminal thật ở cả hai IDE
- [ ] Đối chiếu 7 vùng ở **light**
- [ ] Duyệt danh sách chờ alpha từ phase 03, ra quyết định giữ/nâng
- [ ] Ghi ngược mọi thay đổi vào `reference/color-mapping.md`
- [ ] `./script/clippy` sạch
- [ ] Cập nhật `docs/project-changelog.md` — ghi rõ đây là thay đổi nhìn thấy được với người dùng

## Success Criteria

- Cả 7 vùng ở **cả hai** appearance được đối chiếu và có ảnh chụp
- Con trỏ editor và con trỏ terminal mang màu 2026, không phải xanh built-in
- 16 màu ANSI khớp bảng mặc định VSCode
- `reference/color-mapping.md` khớp 1-1 với file theme đã ship
- `docs/project-changelog.md` có mục cho thay đổi này

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Sa vào chỉnh pixel vô tận vì Zed ≠ VSCode về cấu trúc UI | Chốt trước: mục tiêu là cùng hệ màu, không phải trùng khít. Giới hạn ở 7 vùng đã liệt kê |
| Sửa màu ở đây rồi quên ghi ngược vào bảng | Todo và Success Criteria đều bắt buộc mục này |
| Bỏ sót con trỏ vì nó không báo lỗi | Có mục kiểm riêng, tách khỏi mục đối chiếu vùng |

## Security Considerations

Không có.

## Next Steps

Xong plan. Chạy `/tkm:write-journal` ghi lại phiên làm việc, rồi mở PR theo `docs/` conventions.
