# Phase 04 — Icon asteroid + vỏ Panel

## Context Links

- `crates/workspace/src/dock.rs` — `render_pending_agent_history_button()` (placeholder cần xoá),
  `dock_header_draws_panel()`, `DockButton::for_entry()`
- `crates/zed/src/zed.rs:645` — `initialize_panels()` + `add_panel_when_ready()`
- `crates/project_panel/src/project_panel.rs:7348` — `Panel` impl mẫu gần nhất (pinned Right)
- `crates/project_panel/src/project_panel_tests.rs` — `the_panels_button_moves_into_the_docks_own_header`
  (test này **sẽ phải sửa**, xem Key Insights)

## Overview

- **Priority:** P1
- **Status:** pending · **Phụ thuộc:** không — chạy song song với phase 01
- Kết quả: bấm icon asteroid ở header dock phải → panel mở, hiện "Chưa có gì" (danh
  sách thật là phase 05). Placeholder disabled bị xoá.

## Key Insights

- Header của dock **tự liệt kê** panel từ `panel_entries` (`Dock::render_header`). Panel
  thật xuất hiện ở đó không cần code header nào — đó chính là lý do placeholder được
  viết như một hàm dùng một lần.
- **Test của chính session này sẽ đỏ:** `the_panels_button_moves_into_the_docks_own_header`
  assert `debug_bounds("dock-header-agent-history").is_some()`. Khi placeholder biến
  thành panel thật, selector đổi thành `dock-header-button:Agent History`. Sửa
  assertion, **không** xoá test — nó vẫn là bằng chứng nút nằm cạnh nút project panel.
- `zed.rs::test_action_namespaces` assert **danh sách namespace đầy đủ**. Thêm namespace
  action mới → test đỏ cho tới khi cập nhật danh sách. Đây là hàng rào, không phải lỗi.
- `register_action` **lease Workspace**: handler với reach ngược qua workspace handle sẽ
  abort tiến trình. Cùng một method gọi từ `cx.listener` của panel thì chạy. Bắt buộc
  có test dispatch action, không chỉ test gọi method.
- `Panel::persistent_name()` là method **static** — nó đặt tên cho một *kiểu*, không
  cho một instance.
- Kích thước panel được dock lưu qua `KeyValueStore` (`persist_panel_size_state`), không
  cần thêm bảng.
- Panel mặc định **đóng**. Mở lần đầu là do người dùng bấm, không phải khởi động.

## Requirements

**Functional**

- FR19 — `assets/icons/asteroid.svg`: 16×16 viewBox, `stroke="black"`, `stroke-width="1.2"`,
  `stroke-linecap/linejoin="round"`, `fill="none"` — đúng khuôn `star.svg`/`history_rerun.svg`.
- FR20 — `IconName::Asteroid` trong `crates/icons/src/icons.rs` (giữ thứ tự alphabet).
- FR21 — `AgentHistoryPanel` impl `Panel`: `persistent_name() = "Agent History"`,
  `position() = Right` cố định, `position_is_valid(Right) = true` còn lại `false`,
  `set_position` không ghi gì, `icon() = Asteroid`, `icon_tooltip() = "Agent History"`,
  `activation_priority` sau project panel.
- FR22 — Action `agent_history::ToggleFocus` (hoặc tên tương đương theo macro panel của
  repo), có trong command palette.
- FR23 — Xoá `render_pending_agent_history_button()` và call site trong `Dock::render_header`.
- FR24 — Đăng ký panel trong `initialize_panels` qua `add_panel_when_ready`.
- FR25 — Mở panel → **thay chỗ** project panel trong cột (`activate_panel`), không stack.
- FR26 — Xoá action mồ côi `zed_actions::agents_sidebar::ToggleThreadSwitcher` (không
  handler nào, và giờ có thứ thật thay nó). Kèm sửa `test_action_namespaces` nếu
  `agents_sidebar` không còn action nào khác.

**Non-functional**

- Panel chưa mở thì không đọc file nào. Không có IO ở đường khởi động.

## Architecture

```
crates/agent_ui/src/session_history/
  mod.rs      pub use AgentHistoryPanel
  panel.rs    struct + Panel impl + load() + render "chưa có gì"
```

`agent_ui/Cargo.toml` thêm `agent_sessions`. Panel giữ `Vec<Arc<dyn SessionProvider>>`,
dựng ở `load()`, nhưng **chưa gọi** `list()` ở phase này.

## Related Code Files

**Tạo mới**
- `assets/icons/asteroid.svg`
- `crates/agent_ui/src/session_history/{mod,panel}.rs`

**Sửa**
- `crates/icons/src/icons.rs` — `Asteroid`
- `crates/agent_ui/src/agent_ui.rs` — `pub mod session_history`, init action
- `crates/agent_ui/Cargo.toml` — dep `agent_sessions`
- `crates/workspace/src/dock.rs` — xoá placeholder + call site
- `crates/zed/src/zed.rs` — đăng ký panel, cập nhật `test_action_namespaces`
- `crates/zed_actions/src/lib.rs` — xoá `agents_sidebar::ToggleThreadSwitcher`
- `crates/project_panel/src/project_panel_tests.rs` — sửa assertion selector

## Implementation Steps

1. Vẽ `asteroid.svg` (một khối đá bất đối xứng + 2-3 hố tròn nhỏ), so cạnh nhau với
   `star.svg` ở cùng kích thước để cân nét.
2. `IconName::Asteroid`; `cargo test -p ui` (nếu có test liệt kê icon/asset) để chắc
   asset được nhúng.
3. `panel.rs`: struct + `Panel` impl + `load()` theo khuôn `ProjectPanel::load`.
4. Action + `init(cx)` + đăng ký trong `zed.rs`; chạy `cargo test -p zode` và sửa
   `test_action_namespaces`.
5. Xoá placeholder trong `dock.rs`; sửa assertion trong project_panel test sang
   `dock-header-button:Agent History`.
6. Xoá `ToggleThreadSwitcher`.
7. Test: dock panel, `run_until_parked`, đo `debug_bounds` — panel vẽ ở nửa phải, nút
   của nó nằm trong header cạnh nút project panel.
8. Test dispatch action `ToggleFocus` từ workspace (bẫy lease).

## Todo List

- [ ] `asteroid.svg` đúng khuôn, nhìn cân bên cạnh icon khác
- [ ] `IconName::Asteroid`
- [ ] `AgentHistoryPanel` + `Panel` impl + `load()`
- [ ] Action + init + đăng ký trong `initialize_panels`
- [ ] `test_action_namespaces` cập nhật
- [ ] Xoá placeholder + call site trong `dock.rs`
- [ ] Sửa `the_panels_button_moves_into_the_docks_own_header` sang selector mới
- [ ] Xoá `agents_sidebar::ToggleThreadSwitcher`
- [ ] Test vẽ thật: panel ở nửa phải, nút trong header
- [ ] Test dispatch action (bẫy lease workspace)
- [ ] `cargo test -p zode -p workspace -p project_panel -p agent_ui`, clippy, build

## Success Criteria

1. Bấm icon asteroid → panel mở, chiếm cột phải, project panel nhường chỗ; bấm icon
   folder → quay lại.
2. `debug_bounds("dock-header-button:Agent History")` tồn tại và nằm cạnh
   `dock-header-button:Project Panel`.
3. `grep -rn "render_pending_agent_history_button" crates/` không còn kết quả.
4. `grep -rn "ToggleThreadSwitcher" crates/` không còn kết quả.
5. Dispatch `ToggleFocus` từ command palette không abort tiến trình.
6. Panel đóng → không có syscall đọc `~/.claude/` nào (kiểm bằng cách provider chưa được
   gọi: test rằng `list()` count = 0 lần).

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| `test_action_namespaces` đỏ và bị "sửa" bằng cách nới lỏng assert | Cập nhật danh sách, giữ tính đầy đủ. Nó tồn tại để chặn action lạc. |
| Xoá `ToggleThreadSwitcher` làm keymap đỏ | `grep` cả `assets/keymaps/` trước khi xoá |
| Icon tự vẽ nhìn lệch với bộ icon | So cạnh nhau ở 16px **và** ở IconSize::Small trên máy thật, không chỉ trong SVG viewer |
| Panel mở lần đầu chậm vì `load()` làm IO | `load()` chỉ dựng provider, không gọi `list()` |

## Security Considerations

- Không có. Phase này không đọc dữ liệu người dùng, không xoá gì.
- Icon là asset tự vẽ → không kéo license lạ vào repo.

## Next Steps

Phase 05 lấp nội dung vào cái vỏ này.
