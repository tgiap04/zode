# Brainstorm — Activity Bar kiểu VSCode cho zode

**Ngày:** 2026-08-08 · **Lens:** CTO · **Trạng thái:** thiết kế đã chốt, chưa triển khai

## Commission

Thêm một dải icon dọc hẹp cạnh file explorer, chứa các mục chuyển panel — tương đương
**Activity Bar** của VSCode.

## Khảo sát

### Máy móc đã có sẵn một nửa

Trait `Panel` (`crates/workspace/src/dock.rs:62-65`) đã buộc mỗi panel khai báo `icon()`,
`icon_tooltip()`, `icon_label()`. `PanelButtons` (`dock.rs:1184`) đã render đúng danh sách đó,
chỉ khác là **nằm ngang trong status bar**. Dựng activity bar phần lớn là đổi cách trình bày.

API điều khiển cũng đủ: `Dock::activate_panel` (`:796`), `visible_panel` (`:811`),
`active_panel_index` (`:510`), `is_open` (`:454`), `panels_len` (`:786`).

### Nhưng zode chỉ có 5 panel, và 2 mục quan trọng nhất không phải panel

| VSCode | zode |
|---|---|
| Explorer | `project_panel` ✅ |
| Source Control | `git_panel` ✅ |
| Run & Debug | `debug_panel` ✅ |
| Outline | `outline_panel` ✅ |
| Search | ❌ pane item, không phải panel |
| Extensions | ❌ pane item, không phải panel |

### Va chạm với project rail

Fork đã có dải dọc luôn hiện ở mép — `crates/sidebar/src/rail.rs`, `div().id("sidebar-container")`
(`multi_workspace.rs:2688`), là **anh em ruột của toàn bộ Workspace**. Nó chiếm đúng vị trí
activity bar của VSCode.

### Ràng buộc cứng

`Dock::panel_entries` (`dock.rs:259`) là **field private**. Module activity bar buộc phải nằm
**trong** crate `workspace`, hoặc phải thêm accessor public vào `dock.rs` — tức là sửa code upstream.

### Căng thẳng về triết lý — cần nói thẳng

Zed **cố ý** cho mọi panel nằm ở dock bất kỳ và gom nút bật/tắt vào status bar. Activity bar giả
định "sidebar chính có một danh sách cố định". Hai mô hình khác nhau. Càng bám sát VSCode càng
phải bảo trì phần lệch đó qua mỗi lần rebase upstream.

## Quyết định đã chốt

| # | Quyết định | Lý do |
|---|---|---|
| 1 | **Hai dải riêng cạnh nhau** — rail project rồi đến activity bar | Giữ tính năng multi-project vừa xây. Tốn ~96px chiều ngang, chấp nhận |
| 2 | **Chỉ 4 panel có sẵn**, không Search/Extensions | Chạy tự động qua `Panel::icon()`; panel mới tự xuất hiện, không đấu dây thủ công. Search dạng panel là việc lớn riêng |
| 3 | **Giữ nguyên nút panel ở status bar** | Không đụng `PanelButtons` (upstream). Status bar còn lo dock dưới/phải mà activity bar không phụ trách |

## Hướng dựng

**Module mới `crates/workspace/src/activity_bar.rs`** — nằm trong crate `workspace` để đọc được
`panel_entries` mà không phải thêm API public vào `dock.rs`.

Điểm chèn: **`Workspace::render_dock`** (`workspace.rs:7583`) khi `position == Left` — một chỗ duy
nhất phủ cả 4 nhánh `BottomDockLayout`, tránh lặp lại như các thay đổi layout trước.

Hành vi theo VSCode:
- Click icon panel đang ẩn → mở dock và kích hoạt panel đó
- Click icon panel đang hiện → đóng dock
- Icon của panel đang hiện được tô sáng

### Các hướng đã cân nhắc và loại

| Hướng | Vì sao loại |
|---|---|
| Crate `activity_bar` riêng | `panel_entries` private → phải thêm accessor public vào `dock.rs`, tức sửa upstream. Đổi lấy sự cô lập không đáng |
| Thêm chế độ dọc cho `PanelButtons` | Sửa thẳng `dock.rs` upstream → điểm conflict vĩnh viễn, và gộp hai UI khác mục đích vào một component |
| Đặt activity bar trong crate `sidebar` | `sidebar-container` ở tầng MultiWorkspace nên sẽ trải cả cạnh title bar — khác VSCode. Và từ đó khó với tới left dock của Workspace |

## Rủi ro cần canh

| Rủi ro | Mức | Đối phó |
|---|---|---|
| `render_dock` trả `Option<Div>`, dùng qua `.children()`. Chèn activity bar thành **surface riêng** cạnh dock cần tái cấu trúc chỗ này | Trung bình | Bọc `h_flex[activity_bar, dock]`, mỗi cái tự gọi `workspace_surface` |
| Thêm ~96px chiều ngang cố định (rail 48 + activity bar ~48) | Trung bình | Đã là quyết định có ý thức của người dùng |
| Trùng lặp với nút ở status bar | Thấp | Chấp nhận có ý thức, đổi lấy việc không đụng upstream |
| Không có test nào phủ layout — lớp lỗi đã lọt 2 lần trong phiên trước | **Cao** | Viết test cho *hành vi* (click → panel nào active, dock mở/đóng) thay vì hình dạng |

## Đo thành công

- Bốn icon hiện đúng thứ tự panel của dock trái, lấy từ `Panel::icon()`
- Click panel đang ẩn → dock mở, panel đó active; click panel đang hiện → dock đóng
- Icon panel đang hiện được tô sáng
- Panel mới đăng ký sau này tự xuất hiện, không cần sửa activity bar
- `./script/clippy` sạch; test hành vi xanh
- Chỉ **một** dòng sửa file upstream có sẵn (`mod activity_bar;`) + một điểm chèn trong `render_dock`

## Chưa giải quyết

- Search và Extensions vắng mặt — muốn đủ như VSCode thì phải viết search panel mới, việc riêng
- VSCode cho kéo thả sắp xếp lại icon activity bar; bản này không có

---

## Đảo hướng (2026-08-08, sau khi bản dải-riêng đã dựng xong)

Người dùng nhìn lại và chốt: **gộp activity bar vào chính project rail**, không để hai dải cạnh nhau.
Quyết định #1 ở bảng trên bị thay thế.

Lý do đứng về phía họ: rủi ro "~96px chiều ngang cho một hàng icon" đã ghi ở quyết định #1 là **có
thật**, và gộp lại thì mất hẳn — một dải 48px gánh cả hai việc.

### Hai lý do loại crate `sidebar` — soi lại

| Lý do đã ghi | Thực tế |
|---|---|
| "`sidebar-container` trải cả cạnh title bar → khác VSCode" | **Không còn liên quan.** Icon panel nằm ở *đáy* dải; `Sidebar::top_inset` đã chừa sẵn dải traffic-light macOS ở đỉnh |
| "Từ đó khó với tới left dock của Workspace" | **Sai.** `MultiWorkspace::workspace()` (`multi_workspace.rs:1590`) cho ra `Entity<Workspace>`, từ đó `left_dock()` — ba dòng, không có rào cản nào |

### Cái vẫn đúng

Ràng buộc `Dock::panel_entries` private **vẫn buộc** thêm accessor `Dock::panels()` vào `dock.rs`.
Bản dựng đầu tiên cũng đã phải thêm, dù biên bản khi đó dự đoán là không cần — private theo
**module**, không theo crate.

### Hình dạng cuối

`crates/sidebar/src/rail_panels.rs`. Bố cục dải, từ trên xuống:
ô project (cuộn) → đường kẻ → icon panel (khối cố định) → đường kẻ → footer (toggle danh sách, +).
`Workspace::render_dock` trả về `Option<Div>` như nguyên bản — không còn chèn gì vào đó.
