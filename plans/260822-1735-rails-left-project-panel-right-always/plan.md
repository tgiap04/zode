---
title: "Rail luôn bên trái, project panel luôn bên phải"
description: "Bỏ hẳn khả năng đổi bên: xoá multi_project.sidebar_side, kiểu SidebarSide và project_panel.dock; bố cục rail-trái/panel-phải thành mặc định đóng cứng."
status: completed
priority: P2
effort: 7h
branch: feat.release-v0.1.1
tags: [workspace, settings, project-panel, sidebar, breaking-change]
created: 2026-08-22
---

# Rail luôn bên trái, project panel luôn bên phải

## Yêu cầu

Bỏ tính năng đổi bên. Rail **luôn** bên trái, project panel **luôn** bên phải, không
move được. Đó là bố cục mặc định của người dùng mới.

## Chốt: xoá setting, không ghim setting

Người dùng chọn phương án tối đa — xoá `multi_project.sidebar_side`, xoá kiểu
`SidebarSide`, xoá luôn `project_panel.dock`. Lý do đứng vững: **một key setting bị bỏ
qua lặng lẽ chính là loại lỗi reviewer đã xếp High** trong session này. Ghim giá trị mà
giữ key lại là để lại đúng cái bẫy đó.

Mặc định hôm nay là **ảnh gương** của đích: `sidebar_side="right"`, `project_panel.dock=
"left"`, `outline_panel.dock="right"`, `git_panel.dock="right"`. Cả bốn đều đảo.

## Tính năng bị xoá là một hàm, không phải một nút

`write_dock_and_opposite_rail` (`project_panel.rs:7330`): một lần move panel ghi **bốn**
key — panel sang bên chọn, rail sang bên **đối diện**, outline và git theo rail. Hai đường
người dùng tới được nó: right-click nút panel → "Dock Left/Right" (`dock.rs:1911`), và
action `MoveFocusedPanelToNextPosition` → `move_to_next_position` (`dock.rs:148`).

Nửa invariant đã cứng từ trước: `PANEL_ALWAYS_IN_STATUS_BAR = "Project Panel"`
(`dock.rs:1822`) — rail không bao giờ nhận nút của project panel.

## Bố cục phase

Compiler là thứ liệt kê site, nên không cần dò tay. Nhưng **collapse một nửa consumer
rồi dừng lại sẽ cho app tự mâu thuẫn** (rail vẽ trái, cột own vẫn bám phải) — không test
nào bắt, người review phải giữ trong đầu. Nên seam đặt ở chỗ khác: phase 03 giết
**setting** (hành vi thành đích ngay ở đây), phase 04 giết **kiểu** (0% thay đổi hành vi,
chỉ dọn nhánh chết). Rollback 04 riêng là an toàn tuyệt đối.

| # | Phase | Sở hữu file | Chặn bởi | Effort |
|---|---|---|---|---|
| [01](phase-01-project-panel-dong-cung-ben-phai.md) | Xoá `project_panel.dock`, panel đóng cứng bên phải | `project_panel/*`, `settings_content/workspace.rs`, `settings_ui/page_data.rs`, `default.json` (khối `project_panel`) | — | 1h |
| [02](phase-02-menu-dock-khong-moi-doi-ben.md) | Menu dock chỉ mời khi có ≥2 bên (fix **chung**) | `workspace/src/dock.rs` | — | 1.5h |
| [03](phase-03-xoa-setting-sidebar-side.md) | Xoá `multi_project.sidebar_side`, rail chốt trái | `settings_content/workspace.rs`, `workspace_settings.rs`, `workspace.rs`, `dock.rs`, `sidebar/rail.rs`, `database_panel*`, `settings/settings.rs`, `default.json` | 01, 02 | 1.5h |
| [04](phase-04-xoa-kieu-sidebar-side.md) | Xoá kiểu `SidebarSide` + mọi nhánh chết | `multi_workspace.rs`, `status_bar.rs`, `workspace.rs`, `dock.rs`, `sidebar/*`, `platform_title_bar`, `database_ui` | 03 | 2h |
| [05](phase-05-kiem-chung-ve-that.md) | Kiểm chứng bằng vẽ thật + rà soát cuối | không sửa source (chỉ thêm test đã được 04 nhả ra) | 04 | 1h |

01 và 02 chạy **song song** được — hai crate rời nhau, không chung file nào. Từ 03 trở đi
là chuỗi cứng: `crates/workspace` là thứ 7/9 crate còn lại phụ thuộc, và Rust đặt test
cùng file với code nên không tách được "phase test" riêng. Mỗi phase mang test của chính
nó.

## Tương thích

`SettingsContent` **không** `deny_unknown_fields` (đã kiểm) — `sidebar_side` và
`project_panel.dock` còn sót trong settings người dùng sẽ bị bỏ qua, không panic. Người
dùng hiện tại đã tự right-click tới đúng bố cục đích, nên bốn override trong
`~/.config/zode/settings.json` của họ thành thừa — **ghi nhận, không hành động**.

Người dùng cũ đang ở mặc định: left dock của họ được restore là "open, active_panel =
Project Panel", mà panel giờ đăng ký ở right dock. `Dock::render` chốt trên
`visible_entry().is_some()` (`dock.rs:1667`) nên dock rỗng **không vẽ gì** — không có dải
trống. Không cần migration.

## Đã xảy ra thật — bốn chỗ blueprint đoán chưa đúng

**1. `.unwrap_or(DockPosition::Left)` không phải bug thuần.** Phase 02 gọi nó là bug và bảo
đổi sang `unwrap_or(current_position)`. Làm theo thì `test_move_focused_panel_to_next_position`
đỏ ngay: với thứ tự duyệt `[Left, Bottom, Right]`, một panel đang ở `Right` là phần tử cuối
nên `nth(1)` = `None`, và cái `Left` đó **chính là bước wrap-around**. Nó chỉ sai với panel
một-cạnh. Đáp án đúng là wrap **trong danh sách hợp lệ**: `valid[(i + 1) % valid.len()]`.

**2. Test đặc tả đặt sai crate thì không đặc tả gì.** Phase 04 nói viết chúng trong
`workspace`. Viết xong cả hai **đỏ trên code chưa sửa** — không phải code sai mà vì một
`FocusHandle` tạo bằng `cx.focus_handle()` và chưa từng được render **không giữ được focus**.
Một test như vậy sẽ luôn đỏ, và "sửa" bằng cách nới assert sẽ cho một test luôn xanh mà canh
số không. Chuyển sang `sidebar`, nơi có `MultiWorkspace` + `Sidebar` thật và vẽ thật.

**3. `debug_bounds("project-panel")` không tồn tại lúc vẽ.** Phase 05 giả định id đó đo được.
Thực tế `dock-panel` mới được vẽ. Test đo `dock-panel` cộng khẳng định **chỉ** right dock mở
— chặt hơn, vì nó loại được cả trường hợp hai dock cùng mở.

**4. `cargo check -p workspace --all-targets` đỏ sẵn ở HEAD** (`RemoteConnectionIdentity::Mock`
không cover) — artifact feature-unification khi check crate lẻ. Lệnh đúng cho crate này là
`cargo test -p workspace`. Mất thời gian ở đây vì tưởng mình gây ra.

**Một regression thật, bắt bởi đúng cổng blueprint chỉ định.** `cargo test -p zode` (không
`--lib`) → `test_open_paths_action` đỏ: nó khẳng định **left dock** mở trên workspace mới,
điều chỉ đúng khi project panel còn mặc định bên trái. Sửa thành khẳng định right dock mở
**và** left dock đóng — hai nửa, vì chỉ một nửa dương thì không bắt được việc chúng đổi chỗ.

## Định nghĩa xong

Cài mới: rail bên trái, project panel bên phải, git + outline trong rail bên trái.
Right-click nút project panel: **không có** entry "Dock Left/Right" nào.
`MoveFocusedPanelToNextPosition` khi focus ở project panel: không gì xảy ra.
`grep -rn "SidebarSide\|sidebar_side" crates/` → rỗng. `./script/clippy` 0 warning.
`cargo build --bin zode` exit 0.
