---
title: Tab Switcher
description: "The Tab Switcher provides a quick way to navigate between open tabs in Zode. It displays a list of your open tabs sorted by recent usage, making it easy to ju..."
---

# Tab Switcher

Tab Switcher giúp chuyển nhanh giữa các tab đang mở trong Zode. Nó
hiện danh sách tab đang mở, xếp theo mức dùng gần đây, để bạn dễ quay
lại thứ vừa làm.

![Tab Switcher với nhiều khung](https://zed.dev/img/features/tab-switcher.png)

## Chuyển nhanh

Khi Tab Switcher được mở bằng {#kb tab_switcher::Toggle}, thay vì
chạy {#action tab_switcher::Toggle} từ bảng lệnh (command palette), nó sẽ giữ
hoạt động chừng nào bạn còn giữ phím <kbd class="keybinding">ctrl</kbd>.

Khi giữ <kbd class="keybinding">ctrl</kbd>, mỗi lần nhấn <kbd
class="keybinding">tab</kbd> tiếp theo sẽ chuyển sang mục kế (<kbd
class="keybinding">shift</kbd> để chuyển ngược lại) và, khi thả <kbd
class="keybinding">ctrl</kbd>, mục đang chọn được xác nhận và
bộ chuyển tab đóng lại.

## Mở Tab Switcher

Tab Switcher cũng có thể mở bằng {#action tab_switcher::Toggle} ({#kb tab_switcher::Toggle})
hoặc {#action tab_switcher::ToggleAll}.

Khi Tab Switcher đang mở, bạn có thể:

- Nhấn {#kb menu::SelectNext} để sang tab kế tiếp trong danh sách
- Nhấn {#kb menu::SelectPrevious} để về tab trước
- Nhấn <kbd class="keybinding">enter</kbd> để xác nhận tab đang chọn và đóng bộ chuyển
- Nhấn <kbd class="keybinding">escape</kbd> để đóng bộ chuyển và quay về tab ban đầu nơi
  bộ chuyển được mở
- Nhấn {#kb tab_switcher::CloseSelectedItem} để đóng tab đang chọn

Khi bạn duyệt trong danh sách, Zode cập nhật mục đang hoạt động của khung
cho khớp với tab được chọn.

## Tham khảo action

| Action                                    | Mô tả                                  |
| ----------------------------------------- | -------------------------------------- |
| {#action tab_switcher::Toggle}            | Mở Tab Switcher cho khung hiện tại     |
| {#action tab_switcher::ToggleAll}         | Mở Tab Switcher hiện tab của mọi khung |
| {#action tab_switcher::CloseSelectedItem} | Đóng tab đang chọn trong Tab Switcher  |
