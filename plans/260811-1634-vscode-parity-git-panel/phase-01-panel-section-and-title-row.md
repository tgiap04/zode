# Phase 01 — `PanelSection` + title row + zoom

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** pending · **Effort:** 2-3d · **Blocked by:** 00

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

- [ ] `PanelSection` component
- [ ] `SectionCollapseState` + persist qua `SerializedGitPanel`
- [ ] Đọc lại serialize cũ (field `None`) không panic, default đúng
- [ ] `render_title_row` với `⋯` + `⛶` + `✕`
- [ ] `is_zoomed` / `set_zoomed` trên `impl Panel for GitPanel`
- [ ] `Changes` section bọc list, badge + hover actions
- [ ] Group header `Tracked`/`Untracked` **còn checkbox staging** (R5)
- [ ] Test persist collapse state
- [ ] `graph` default = collapsed

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
