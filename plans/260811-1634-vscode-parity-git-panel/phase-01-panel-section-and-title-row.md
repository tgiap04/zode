# Phase 01 — `PanelSection` + title row + zoom

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** completed · **Effort:** 2-3d · **Blocked by:** 00

Miếng **B**. Dựng hạ tầng section collapsible + title row `Source Control`. Kết thúc phase này panel **vẫn dùng như cũ**, chỉ khác: có title row, và list changes đã nằm trong một section `Changes` gập được. Đây là nền cho 02/03/04/05.

## Key insights

- `ui::Disclosure` đã có (`crates/ui/src/components/disclosure.rs`) — không xây từ đầu.
- `SerializedGitPanel` (`git_panel.rs:254`) + `serialize()` (`:901`) + `serialization_key()` (`:893`) + đường đọc lại ở `:5132-5143` **đã có sẵn**. Persist collapse state = thêm field vào struct đó, không dựng hạ tầng mới. Đây là lý do R3 rẻ.
- `impl Panel for GitPanel` (`:5345-5400`) **không** implement `is_zoomed` / `set_zoomed` — cả hai có default no-op ở `crates/workspace/src/dock.rs:70,76`. Nút maximize phải implement cả hai + nối `workspace::ToggleZoom`. Nhỏ, nhưng không miễn phí.
- **Cẩn thận R5:** `render_list_header` (`:4447`) **không phải** section header — nó là một list *entry* mang nhãn `Tracked`/`Untracked` **kèm `Checkbox` staging cả nhóm**. Sau phase này sẽ có **hai tầng header**. Không được xoá tầng trong, nó mang chức năng staging.
- `crates/panel/src/panel.rs` giữ `panel_header_container` (`:20`), `panel_filled_button` (`:60`), `panel_icon_button`. Theo YAGNI, `PanelSection` để trong `git_ui` trước; chỉ promote sang `crates/panel` khi có panel thứ hai cần.

## Requirements

**Functional**
1. Title row trên cùng: nhãn `Source Control` + `⋯` (overflow menu hiện có) + `⛶` (maximize/zoom) + `✕` (đóng dock).
2. `PanelSection`: disclosure triangle + nhãn + count badge (optional) + hàng action hiện khi hover + nội dung gập được.
3. List changes hiện tại được bọc trong section `Changes`, badge = `changes_count`, hover actions = `+` (stage all) / `↺` (discard) / `⋯`.
4. Trạng thái gập/mở của **từng** section sống qua restart.
5. `⛶` phóng panel; bấm lại thu về. `✕` đóng dock.

**Non-functional:** `⋯` menu và mọi action hiện có phải còn nguyên đường bấm. Không regression focus.

## Architecture

```
┌──────────────────────────────────┐
│ Source Control        ⋯  ⛶  ✕    │  ← title row (mới)
├──────────────────────────────────┤
│ ▼ Changes  ⑤           + ↺ ⋯     │  ← PanelSection (mới), badge + hover actions
│   ▸ Tracked            ☑         │  ← render_list_header, GIỮ NGUYÊN (R5)
│     <file entries…>              │
├──────────────────────────────────┤
│ zode / main            ⟳ Fetch   │  ← footer: GIỮ NGUYÊN ở phase này
│ <commit editor>                  │     (chuyển ở phase 02)
│ Commit Tracked              ⌄    │
│ Merge PR #7…            ↺  ⑂     │
└──────────────────────────────────┘
```

`PanelSection` là `RenderOnce` (`#[derive(IntoElement)]`), nhận:

```rust
pub(crate) struct PanelSection {
    id: ElementId,
    label: SharedString,
    expanded: bool,
    badge: Option<usize>,
    actions: Vec<AnyElement>,      // hiện khi hover header
    on_toggle: Box<dyn Fn(&mut Window, &mut App) + 'static>,
    children: Vec<AnyElement>,
}
```

Trạng thái gập ở trong `GitPanel`, không ở trong component:

```rust
// git_panel.rs — cạnh SerializedGitPanel
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct SectionCollapseState {
    repositories: bool,   // false = expanded
    changes: bool,
    graph: bool,
    commits: bool,
}
```

Thêm vào `SerializedGitPanel` dưới dạng `Option<SectionCollapseState>` để bản serialize cũ đọc lại được (default = tất cả expanded, trừ `graph` = collapsed để tránh R4 ngay từ đầu).

## Related code files

**Sửa:**
- `crates/git_ui/src/git_panel.rs` — `SerializedGitPanel` (+field), `serialize()`, đường đọc lại `:5132-5143`, `impl Panel` (+`is_zoomed`/`set_zoomed`), `Render for GitPanel` (thêm title row + bọc section)
- `crates/git_ui/src/git_panel/render_header.rs` — thêm `render_title_row`, `render_changes_section_actions`
- `crates/git_ui/src/git_panel/render_entries.rs` — `render_entries` nhận thêm việc nằm trong section

**Tạo:** `crates/git_ui/src/git_panel/panel_section.rs`
**Xoá:** không

## Implementation steps

1. `panel_section.rs`: `PanelSection` component. Dùng `Disclosure` + `panel_header_container` cho hàng header, `CountBadge` cho badge, `visible_on_hover` cho hàng action (cùng cơ chế `render_list_header` đang dùng ở `:4507`).
2. `SectionCollapseState` + field `Option<SectionCollapseState>` trên `SerializedGitPanel`; field runtime trên `GitPanel`; đọc lại ở `:5137-5143` với default khi `None`.
3. `toggle_section(&mut self, section, cx)` → đổi state, `cx.notify()`, gọi `self.serialize(cx)`.
4. `render_title_row` trong `render_header.rs`: nhãn + `⋯` (dùng lại `render_overflow_menu`) + `⛶` + `✕`.
5. `impl Panel for GitPanel`: thêm `is_zoomed` / `set_zoomed` (field `zoomed: bool` trên struct). Nối `⛶` vào `workspace::ToggleZoom`; `✕` dispatch `toggle_action()` của dock.
6. Bọc `render_entries` trong `PanelSection` nhãn `Changes`, badge `changes_count`, hover actions `+`/`↺`/`⋯`.
7. `render_panel_header` cũ (`No Changes` + `Stage All`) **giữ lại tạm** ở phase này để không mất affordance — nó sẽ được giải tán ở phase 02 khi checklist tái định cư chạy. Ghi `// TODO(phase-02)` ngay tại đó.
8. Test: một test dựng panel, gập `Changes`, `serialize`, đọc lại, khẳng định vẫn gập.

## Todo

- [x] `PanelSection` component — `git_panel/panel_section.rs`, 138 dòng
- [x] `SectionCollapseState` + persist qua `SerializedGitPanel`
- [x] Đọc lại serialize cũ (field `None`) không panic, default đúng
- [x] `render_title_row` với `⋯` + `⛶` + `✕`
- [x] `is_zoomed` / `set_zoomed` trên `impl Panel for GitPanel`
- [x] `Changes` section bọc list, badge + hover actions
- [x] Group header `Tracked`/`Untracked` **còn checkbox staging** (R5) — chỉ hạ `LabelSize` xuống `XSmall`
- [x] Test persist collapse state
- [x] `graph` default = collapsed

## Kết quả (2026-08-11)

Clippy `--deny warnings` exit 0, **66/66 test xanh** (60 trước phase này + 6 mới). `cargo check -p git_graph` xanh — không có API công khai nào đổi.

| File | Δ |
|---|---|
| `git_panel/panel_section.rs` | +138 (mới) |
| `git_panel.rs` | +146 / −28 |
| `git_panel/render_header.rs` | +102 |
| `git_panel/tests.rs` | +218 |
| `git_panel/render_entries.rs` | +3 / −1 |

Sáu test mới: 3 test serde thuần (default, blob cũ không có field, blob thiếu field lẻ), 1 test persist đi qua **kvp thật** (`KeyValueStore::global` rơi về DB in-memory dưới `cfg(test)`), 1 test `cx.draw()` vẽ cả panel ở **hai** trạng thái gập, 1 test zoom/close chạy qua **dock thật** (`add_panel` → `open_panel` → `toggle_zoom` → khẳng định `is_zoomed` lật rồi lật lại → `close_panel` đóng dock).

### Bốn điều phase này dạy lại cho plan

**1. `⛶` không cần sửa `workspace`.** Rủi ro "`set_zoomed` không đủ" trong bảng risk **không thành hiện thực**. Cơ chế đã có sẵn: panel `cx.emit(PanelEvent::ZoomIn/ZoomOut)`, `Dock` bắt event ở `dock.rs:637-668` rồi gọi lại `set_zoomed` và cập nhật `workspace.zoomed_position`. `GitPanel` chỉ cần giữ một field `bool` + hai method trait. Sao y nguyên pattern `debugger_panel.rs:1305-1319,1611-1618`. Không tách commit riêng, không chạm crate `workspace`.

**2. `CountBadge` của `ui` không dùng được inline.** `count_badge.rs:34` là `absolute().top_0().right_0()` — nó thiết kế để phủ lên một icon (badge trên rail), không phải nằm cạnh nhãn. Badge trong `PanelSection` là pill tự dựng (~10 dòng). Phase 03/04 gặp lại điều này.

**3. Trùng `ElementId` không phải rủi ro như tưởng.** `render_overflow_menu` giờ dựng **3 lần mỗi frame** mà trigger bên trong mang id hằng `"overflow-menu-trigger"`. Không sao: `PopoverMenu` implement `Element::id()`, nên nó đẩy id của mình vào `element_id_stack` trước khi xuống con → ba đường dẫn `GlobalElementId` khác nhau. Đủ điều kiện: **id ngoài phải khác nhau**. Test `cx.draw()` là thứ chứng minh điều này, không phải suy luận.

**4. Nội dung section phải gate ở call site, không chỉ trong component.** `PanelSection::render` chỉ gắn `children` khi `expanded`, nhưng nếu call site vẫn dựng `render_entries` thì cái element đó bị dựng rồi bỏ mỗi lần re-render lúc đang gập (kể cả re-render do git-status poll). Gate bằng `.when(expanded, …)` **ở cả hai** chỗ.

### Lệch so với plan

- Plan viết `on_toggle: Box<dyn Fn(&mut Window, &mut App)>`. Thực tế dùng `Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>` để khớp thẳng `Disclosure::on_toggle_expanded`. Header row **cũng** clickable (VSCode gập khi bấm cả hàng); không double-toggle vì `ButtonLike::render` gọi `cx.stop_propagation()` trước handler (`button_like.rs:767-772`).
- `PanelSection::new` nhận `id: impl Into<SharedString>` thay vì `ElementId` — id được dùng cho cả element id, id disclosure và tên hover group, nên một `SharedString` là nguồn duy nhất; nhãn hiển thị không còn dính vào id.
- `PanelSectionKind` mang `#[allow(dead_code)]`: ba variant `Repositories`/`Graph`/`Commits` chưa có section nào dựng cho tới phase 03/05/04, mà `./script/clippy` chạy `--deny warnings`.
- Thêm một `div().flex_1()` khi `Changes` gập, để commit box còn nằm đáy panel. Xoá ở phase 02.

### Còn nợ mắt người

Test chứng minh **không panic, không trùng id, state đúng, dock lật đúng** — nhưng `cx.draw()` không khẳng định pixel. Ba tiêu chí sau cần mở app xem thật: (a) hai tầng header có thực sự đọc ra thứ bậc, (b) hover actions không đè badge ở panel hẹp, (c) `⛶` phóng ra đúng kích thước mong đợi.

## Success criteria

- Gập/mở `Changes`; restart app; vẫn đúng trạng thái.
- `⛶` phóng panel, bấm lại thu; `✕` đóng dock.
- Bản serialize từ trước phase này đọc lại không panic.
- Checkbox stage-cả-nhóm trên `Tracked`/`Untracked` vẫn hoạt động.
- `./script/clippy` sạch, test `git_ui` xanh.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| R3 collapse state không persist | Test bắt buộc ở bước 8, không chỉ thử tay |
| R5 hai tầng header trông rối | Group header nhỏ hơn section header, không cấp disclosure riêng cho nó |
| Bản serialize cũ vỡ | Field là `Option<…>`, có test đọc lại bản không có field |
| `set_zoomed` không đủ — panel zoom cần workspace hợp tác | Đọc `dock.rs:166-170` (`PanelHandle::set_zoomed`) trước khi implement; nếu cần thêm ở `workspace` thì tách commit riêng |
| Title row `✕` trùng đường đóng của rail | Đã chốt ở quyết định 6, chấp nhận |

## Security

Không có bề mặt mới.

## Next steps

02 (commit box), 03 (Repositories), 04 (Commits) mở song song sau phase này. 05 chờ 04.
