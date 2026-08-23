# Panel đen bằng editor, bỏ xám

**Status:** ✅ done (2026-08-22), chờ commit · **Priority:** P3 · **Branch:** feat.release-v0.1.1

## Yêu cầu

Bỏ màu xám ở sidebar (rail và project panel), để nó đen giống phần code editor.

## Xám đến từ hai nguồn khác bản chất

| Chỗ | Nguồn | Dark 2026 |
|---|---|---|
| Bản thân dải rail | `title_bar_background` | `#121314` — **đã đen bằng editor rồi** |
| Item trong rail | `project_item.rs` pha **25% `panel_background`** lên trên | → xám nhẹ |
| Project panel | `panel_background` trực tiếp | `#191a1b` — xám |

Nên rail không xám vì theme: nó xám vì **một dòng code cố ý pha xám vào**. Còn project
panel thì xám vì token.

## Chốt: sửa ở theme, không sửa ở từng crate

`panel_background` là token **18 crate** dùng. Người dùng chọn sửa ở theme thay vì chỉ sửa
hai chỗ được nêu, và lý do đứng vững: chữa riêng project panel để lại **đường seam thấy
được** — cây file đen nằm cạnh git panel xám.

```
panel.background   Dark 2026:  #191a1b → #121314
                   Light 2026: #fafafd → #ffffff
```

Đen theo: project panel, git, terminal, outline, agent, debug, notification.
**Không chạm** `tab_bar.background` và `surface.background` — cái đầu là thứ tách tab khỏi
buffer, cái sau là nền của popover.

Và **bỏ dòng pha xám trong rail** dù theme đã làm nó thành no-op: hai token giờ bằng nhau
nên blend không đổi gì *trên theme này*, nhưng trên ayu/gruvbox/one thì chúng khác nhau và
sắc xám quay lại ngay.

## Đổi màu là loại thay đổi không cổng nào bắt được

Compile không bắt, clippy không bắt, test không bắt. Rủi ro thật của nó không phải sai màu
mà là **hai bề mặt cạnh nhau giờ trùng màu nên mất ranh giới**. Nên tôi quét cả hai chiều.

### Một chỗ thật sự vô hình — đã sửa

`crates/editor/src/element.rs` dùng `panel_background` làm **màu vạch của hoa văn gạch
chéo vẽ bên trong editor** (vùng spacer giữa các excerpt trong multibuffer/diff). Vẽ
absolute trên nền editor, không có nền nào khác phía sau. Hai màu trùng → **hoa văn biến
mất**.

Sửa sang `surface_background`, và nó *chính là* `#191a1b` cũ — nên hoa văn giữ y nguyên
diện mạo hôm nay. Nền của một panel không phải token đúng cho một dấu hiệu cần thấy được
trên buffer.

### Một chỗ phẳng đi — báo, không tự sửa

`git_ui::panel_editor_container` (`git_panel.rs`) vẽ ô nhập commit message bằng
`editor_background` và **không có viền**. Trước: panel `#191a1b`, ô `#121314` — thụt vào
thấy rõ. Sau: cả hai `#121314` — phẳng.

**Phẳng khác vô hình.** Ô vẫn có padding và placeholder, đọc như một phần của panel — cách
nhiều editor khác vẽ ô commit. Và "làm nó nổi lên lại" có hơn một đáp án đúng (thêm viền,
hay nền nhạt hơn), nên đó là quyết định của người dùng, không phải của tôi.

### `reviewer` tìm ra chỗ tôi bỏ sót, và chỗ bỏ sót có lý do rõ

**Vạch dòng lẻ của bảng markdown.** `crates/markdown` tô dòng lẻ bằng `panel_background`
— nhưng nó là **component render dùng chung**, không phải một panel. Nó thừa hưởng nền từ
chỗ nhúng nó, và agent chat nhúng nó trên nền `panel_background`. Nên striping tô dòng
bằng đúng màu nó đang nằm trên → **vạch biến mất**. Bảng markdown xuất hiện thường xuyên
trong câu trả lời của agent, nên đây là lỗi người dùng thấy thật.

Tôi bỏ sót vì quét theo **18 crate dùng trực tiếp** `panel_background`. `markdown` có
trong danh sách đó, nhưng tôi đọc nó như "một panel tô nền của chính nó" — loại self-
reference vô hại — thay vì hỏi *nền của nó do ai quyết định*. Đó là câu hỏi phân biệt hai
loại, và tôi không đặt nó.

Sửa 3 chỗ (reviewer nêu 1, thực tế có 2 trong `html_rendering.rs` + 1 trong `markdown.rs`)
sang `surface_background`.

**Header của bảng thì tôi không sửa, và đây là phân biệt quan trọng:** header tô bằng
`title_bar_background`, mà token đó **vốn đã** bằng `background` từ trước — nó phẳng trước
thay đổi của tôi, không phải do tôi. Phục hồi đúng cái mình làm mất trả bảng về **chính xác**
diện mạo hôm qua. Nhân dịp thiết kế lại header là việc khác, và là quyết định của người dùng.

### Reviewer xác nhận hai phán đoán, và một cái có căn cứ mạnh hơn tôi tưởng

`surface_background` cho hoa văn **không phải** vì hex tình cờ trùng: `elevation.rs`
(`ElevationIndex::EditorSurface::on_elevation_bg`) trả về đúng token đó — nó là câu trả lời
đã được hệ thống thiết kế ghi thành luật cho "một dấu hiệu nằm trên editor", và
`editor.rs` cùng `inlay_map.rs` đã dùng nó đúng cách đó. Và nó an toàn trên ayu/gruvbox/one:
ở cả ba, `surface_background == panel_background` và cả hai khác `editor_background`.

Ô commit của git panel: để nguyên là đúng, vì `elevation.rs` nói fill đúng cho thứ đặt trên
một `Surface` **là** `background` — tức container mới là thứ dịch lại cho khớp quy ước, chứ
ô không hỏng. Muốn nó nổi lại thì `border_variant` là cách nhẹ nhất.

### Không phải vấn đề

- `ui/components/diff_stat.rs:70` — nằm trong `fn preview`, là story của component preview.
- Chiều ngược lại (`editor_background` vẽ lên panel) chỉ có git_ui, và đã xét ở trên.
- `crates/ui/.../ai/thread_item.rs` có **bản sao thứ hai** của đúng dòng pha xám ấy (item
  trong danh sách thread của agent). Ngoài phạm vi được nêu, và sau khi sửa theme thì nó tự
  hết xám trên theme này. Để nguyên, ghi lại ở đây.

## File đã sửa

- `assets/themes/vscode-2026/vscode-2026.json` — `panel.background` × 2 theme
- `crates/sidebar/src/project_item.rs` — bỏ blend `panel_background × 0.25`
- `crates/editor/src/element.rs` — hoa văn spacer dùng `surface_background`
- `crates/markdown/src/html/html_rendering.rs` × 2 — vạch dòng lẻ dùng `surface_background`
- `crates/markdown/src/markdown.rs` — cùng vậy

Suite: `sidebar` 20 · `theme` 3 · `theme_settings` 5 (đọc thật file theme đã sửa) ·
`markdown` 67 · `editor` spacer 2 — tất cả xanh. Clippy 0 warning. `cargo build --bin zode` exit 0.

## Giới hạn của việc tôi kiểm được

Tôi quét được loại lỗi **vô hình** (một dấu hiệu vẽ bằng màu nền của thứ nó nằm trên). Tôi
**không** chạy được app, nên loại lỗi **phẳng** — thẻ/card mất fill vì trùng nền panel —
tôi chỉ lần được qua đọc code trên 18 crate, không xác nhận được bằng mắt. Đó đúng là chỗ
hai phase trước đã trả giá: với UI, mắt người là cổng cuối.

## Định nghĩa xong

Mở app: rail và project panel đen bằng editor, không còn sắc xám. Git/terminal/outline
panel cũng đen — không có seam nào. Vùng spacer trong diff vẫn thấy hoa văn gạch chéo.
Light 2026 trắng đều tương ứng.
