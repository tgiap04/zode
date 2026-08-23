# Consultation — menu chuột phải trên avatar project + kéo thả đổi vị trí

**Ngày:** 2026-08-23 · **Nhánh:** `feat.release-v0.1.1` · **Lens:** CTO (default)

## Commission

Chuột phải vào avatar project trên rail → hiện menu tùy chọn (bắt đầu từ "gỡ khỏi
phiên"), và kéo thả avatar để đổi vị trí.

## Đọc xưởng trước khi bàn — ba dữ kiện đổi hình dạng công việc

**1. Menu này đã tồn tại một nửa.** `crates/sidebar/src/context_menu.rs` đã có menu
project với đúng hai mục: `Open Project in New Window` và `Remove Project`. Nhưng nó
gắn vào nút **ellipsis của hàng project trong panel rộng**, không phải avatar trên
rail. `render_rail_item` (`rail.rs:212`) chỉ có `on_click`, không có `right_click_menu`
nào. Nên phần lớn công việc là *đưa cái đã có sang rail và mở rộng*, không phải dựng
mới.

**2. Kéo thả rẻ hơn dự đoán.** Thứ tự project là `Vec<ProjectGroupState>`
(`multi_workspace.rs:966`) và đã persist thành `Vec<SerializedProjectGroup>` trong KVP
(`persistence/model.rs:114`). Đổi vị trí = đổi chỗ trong Vec + ghi lại state. Không cần
cột thứ tự, không cần schema mới.

**3. Activate KHÔNG sắp xếp lại rail.** `ensure_project_group_state`
(`multi_workspace.rs:796-798`) return sớm khi group đã tồn tại — chỉ project **mới** mới
được `insert(0)`. Điều này quan trọng: nó có nghĩa thứ tự sắp bằng tay sống sót qua mọi
lần đổi project, chỉ bị chèn thêm ở đầu khi mở project mới. Tôi đã định phản đối lựa
chọn "project mới vào đầu rail" vì sợ nó phá thứ tự tay — **đọc code thì thấy lo đó sai**,
và đã nói lại ngay trong buổi.

Chỗ `project_groups.push` ở `multi_workspace.rs:2373` là test helper (`#[cfg(test)]`),
không phải đường production thứ hai. Đáng ghi vì test dựng thứ tự **khác** production:
test kéo thả không được dựa vào `test_add_project_group` để mô phỏng thứ tự thật.

## Quyết định đã chốt

| # | Quyết định | Lý do |
|---|-----------|-------|
| 1 | Menu rail có 6 mục: Remove Project · Open in New Window · Reveal in Finder · Copy Project Path · Đổi chữ viết tắt… · Đổi màu… | Bốn mục đầu dùng API đã có; hai mục sau cần state mới nhưng nhỏ |
| 2 | **Hai** menu riêng: rail đầy đủ, panel giữ ellipsis 2 mục | Người dùng chọn (tôi khuyến nghị một menu — xem "Cái tôi không đồng ý") |
| 3 | Project mới vào **đầu** rail, như hiện tại | Giữ hành vi hôm nay; thứ tự tay không bị xáo (dữ kiện 3) |
| 4 | Remove Project **luôn** hỏi xác nhận | Người dùng chọn |
| 5 | "Rename" chỉ đổi **initials trên avatar**, không đổi tên ở panel/title | Phạm vi nhỏ nhất; nhưng phải đặt tên mục đúng — xem dưới |
| 6 | Kéo: đổi thứ tự trong rail **+ kéo ra ngoài = mở cửa sổ mới** | Người dùng chọn |
| 7 | Màu: dựng **picker đầy đủ** (vùng hue/sat + slider) trong `crates/ui` | Người dùng chọn; là phase riêng |

## Ba chỗ tôi sửa lại đề bài, và vì sao

**"Rename" → "Đổi chữ viết tắt…".** Phạm vi đã chốt là chỉ initials trên avatar. Một mục
menu tên *Rename* mà panel vẫn gọi tên cũ, tooltip vẫn path cũ, title cửa sổ vẫn cũ —
đó là cùng loại lỗi với nút chết: nhãn hứa nhiều hơn việc nó làm. Tên mục phải nói đúng
việc: đổi hai chữ viết tắt khi hai project trùng initials.

**Kéo ra ngoài phải hỏi xác nhận.** `open_project_group_in_new_window`
(`multi_workspace.rs:1269`) **gọi `remove_project_group`** trước khi mở cửa sổ mới. Quyết
định 4 nói Remove luôn hỏi. Nếu kéo lệch tay mà không hỏi thì cùng một bước phá huỷ có
hai cửa — một có gác, một không. Kéo-ra-ngoài dùng **cùng hộp xác nhận đó**, nêu tên
project.

**Picker màu không có nền móng.** Grep `crates/ui/src/components/`: không có
`ColorPicker`, không slider, không component màu nào. Cái có sẵn là
`theme::try_parse_color(&str) -> Hsla` (`theme/src/schema.rs:17`) và `gpui::rgba(u32)`.
Nên picker đầy đủ là **một phase độc lập** với vẽ, hit-test, bàn phím và test của riêng
nó — không phải một dòng trong menu. Đã chốt trả giá đó; kế hoạch phải xếp nó tách khỏi
menu và kéo thả để hai phần kia không bị chặn.

## Cái tôi không đồng ý (đã nêu, người dùng giữ quyết định)

Tôi khuyến nghị **một** menu dùng ở hai nơi. Hai menu riêng nghĩa là cùng một project có
hai bộ tùy chọn khác nhau tuỳ bạn bấm ở đâu — loại lệch mà người dùng phát hiện trước
người viết. Người dùng chọn hai menu; tôi tôn trọng, và giảm thiểu bằng cách: **mọi
handler nằm trong một module actions dùng chung**, hai menu chỉ khác *danh sách mục*, chứ
không được có hai bản logic. Hành vi không được phép rẽ nhánh, chỉ giao diện được.

## Rủi ro cần canh

| Rủi ro | Đối phó |
|---|---|
| Kéo lệch tay làm project bay sang cửa sổ khác | Hộp xác nhận (quyết định trên) + ngưỡng khoảng cách rõ ràng mới tính là "ra ngoài" |
| State mới (initials, màu) làm state cũ không đọc được | Thêm field vào `SerializedProjectGroup` bằng `#[serde(default)]` — file cũ vẫn parse |
| Màu người dùng chọn làm chữ trên avatar không đọc được | Tính độ tương phản và tự chọn chữ sáng/tối; không tin người dùng tự canh |
| Picker màu phình ra thành nửa cái plan | Phase riêng, làm sau menu + kéo thả; menu vẫn dùng được với hex/swatch nếu picker chưa xong |
| `stable_id_for_group` đã có — id phải theo project, không theo index | Dùng lại nó cho menu rail; index đi lạc khi list reorder giữa lúc menu đang mở (chính lý do nó tồn tại) |

## Định nghĩa xong

1. Chuột phải avatar → menu 6 mục; mục nào không làm được thì **disabled kèm lý do**,
   không ẩn.
2. Remove và kéo-ra-ngoài đều hỏi xác nhận nêu tên project.
3. Kéo lên/xuống đổi thứ tự, có vạch chỉ chỗ sẽ rơi; thoát app mở lại **giữ đúng thứ tự**.
4. Mở project mới sau khi đã sắp tay: nó vào đầu rail, các cái còn lại **không đổi thứ tự
   tương đối**.
5. Đổi chữ viết tắt và màu: hiện ngay trên avatar, sống qua restart, và state cũ (chưa có
   hai field này) vẫn mở được.
6. Panel rộng vẫn giữ ellipsis 2 mục, và hành vi của `Remove` ở hai nơi **giống nhau** —
   cùng một handler.

## Tiếp theo

`/tkm:create-plan` — ba mảnh độc lập (menu + actions · state mới cho initials/màu · kéo
thả), cộng một phase riêng cho color picker. Hai trong ba mảnh chạm persistence, nên thứ
tự phase phải cho state đi trước UI đọc nó.
