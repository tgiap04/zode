# Phase 08 — Hai agent cùng lúc: pane group trong dock

**Context:** [plan.md](plan.md) · [phase-07](phase-07-agent-as-dock-panel.md)
**Priority:** P1 · **Status:** completed *(trừ serialize layout)* · **Blocked by:** 07

Phase 07 đưa agent vào dock và trả giá bằng "một dock một agent". Người dùng muốn mở **Claude và Codex cùng lúc**, mặc định là hai section riêng, và kéo thả được thành tab / split ngang / split dọc.

## Khuôn đã có sẵn

`TerminalPanel` làm đúng việc này: nó sống trong dock nhưng giữ `active_pane: Entity<Pane>` + `center: PaneGroup` **của riêng nó** (`terminal_panel.rs:77-88`), split bằng `self.center.split(&pane, &new_pane, direction, cx)` (`:401`), và quyết cái gì được thả vào bằng `pane.set_can_split(...)` (`:1227`).

Nghĩa là tab, split ngang, split dọc và kéo-thả **không phải viết mới** — chúng là `PaneGroup`, thứ đã chạy trong center group từ đầu. Cái phải viết là vỏ panel và luật thả.

Và tính chất quan trọng nhất của phase 07 vẫn giữ nguyên: pane group này **không phải** `workspace.panes`, nên `open_path` vẫn không có đường tới. Editor vẫn không đặt tab vào được.

## Hình dạng

```
Dock  ┌────────────────────────────────┐
      │  AgentPanel                    │
      │  ┌──────────┬──────────┐       │  center: PaneGroup
      │  │ Claude   │ Codex    │       │  ← mặc định split ngang
      │  │ [Chat|T] │ [Chat|T] │       │
      │  └──────────┴──────────┘       │  kéo thả → tab / split dọc
      └────────────────────────────────┘
```

- `AgentPanel` giữ `active_pane` + `center: PaneGroup`; `impl Panel` chuyển từ `AgentView` sang nó.
- `AgentView` quay lại `impl Item` — nhưng là item của pane group **của panel**, không bao giờ của center.
- Rail click: agent đó đã có item → activate; chưa có → thêm item mới, **split** để thành section riêng.
- `set_can_split`: chỉ `AgentView` được thả vào. Tab editor bị từ chối ngay ở predicate.

## Ràng buộc giữ từ các phase trước

1. **Lazy start** — panel dựng cùng workspace, không agent nào chạy tới khi có người bấm.
2. **Đóng item = kết thúc agent đó** — `end_previous_mode` + tripwire debug vẫn là đường tháo.
3. Mode nhớ theo từng agent (`agent_preferences.mode`).
4. Không sinh nút chung trên rail: `Panel::icon() -> None`.

## Việc

- [ ] `agent_panel.rs`: `AgentPanel` với `active_pane` + `center`, theo `terminal_panel`
- [ ] `impl Panel` chuyển sang `AgentPanel`; `AgentView` trả lại `impl Item`
- [ ] Luật thả: chỉ agent item, editor tab bị từ chối
- [ ] `AgentView::open` → panel thêm/activate item của agent đó
- [ ] Serialize layout của panel (tab/split sống qua khởi động lại)
- [ ] `zed.rs` đăng ký `AgentPanel`
- [ ] Test theo hình mới

## Định nghĩa xong

Bấm Claude rồi bấm Codex → hai section cạnh nhau trong dock. Kéo tab Codex chồng lên Claude → thành hai tab. Kéo xuống dưới → split dọc. Kéo một tab editor vào → không thả được. Đóng app mở lại → đúng layout đó.

---

## Xong (2026-08-13)

`AgentPanel` giữ `active_pane` + `center: PaneGroup` của riêng nó, `impl Panel` chuyển sang nó, `AgentView` quay lại làm `Item` — nhưng là item của pane group **này**, không bao giờ của center.

| Yêu cầu | Trạng thái |
|---|---|
| Hai agent cùng lúc | ✅ agent thứ hai vào pane mới, split ngang |
| Mặc định là hai section riêng | ✅ `show()` split khi pane hiện tại đã có agent |
| Kéo thành tab | ✅ `PaneGroup` lo — thả tab lên pane kia là thành tab |
| Kéo thành split ngang/dọc | ✅ `pane::Event::Split { MovePane }` |
| Editor không thả tab vào được | ✅ `set_can_split` từ chối mọi item không phải `AgentView` |
| Lazy start | ✅ panel dựng cùng workspace, không agent nào chạy tới khi bấm |

### Chỗ suýt quên, và nó là trọng tâm yêu cầu

Bản đầu tôi chỉ xử `Focus` và `Remove`, **quên `pane::Event::Split`** — tức là kéo thả để tách ngang/dọc sẽ rơi vào hư không, đúng cái người dùng yêu cầu. Tự soi lại code trước khi commit mới thấy. `ClonePane`/`EmptyPane` thì cố tình bỏ: nhân bản một agent là dựng process thứ hai giả vờ là chính nó, còn pane agent rỗng thì không có gì để hiện.

### Test

- `the_agent_panes_are_not_workspace_panes` — khẳng định pane của dock **không** nằm trong `workspace.panes`. Đó chính là tính chất khiến `open_path` không thể đổ tab editor vào agent; mất nó là quay lại đúng bug ban đầu.
- `the_panel_docks_on_the_side_the_setting_names` chuyển từ `AgentView` sang `AgentPanel`.

34 test `agent_ui` + 16 `sidebar` xanh · clippy sạch · `cargo check --workspace --all-targets` sạch.

### Còn nợ

- **Layout không được serialize.** Dock nhớ mở/đóng và width, nhưng tab/split bên trong thì chưa — mở lại app sẽ về một pane. `TerminalPanel` làm việc này bằng DB riêng của nó; đây là phần chưa làm.
- Phase 09 (bo góc + gutter cả vỏ cửa sổ) chưa bắt đầu.

### Panic khi kéo tab (2026-08-13)

`cannot read workspace::pane::Pane while it is already being updated` — abort cả process, trong `handle_view_event` (tức lúc xử lý sự kiện chuột).

Chỗ sai nằm trong predicate `set_can_split` tôi viết: nó gọi `tab.pane.read(cx)` **vô điều kiện**. Kéo một tab **trong cùng một pane** — trường hợp thường gặp nhất, không phải hiếm — thì `tab.pane` chính là pane đang được update, nên đọc lại nó là re-entrant.

`TerminalPanel` có đúng guard đó và tôi đã sao thiếu (`terminal_panel.rs:1241`):

```rust
let item = if is_current_pane { pane.item_for_index(tab.ix) }
           else { tab.pane.read(cx).item_for_index(tab.ix) };
```

Dùng tham số `pane` đã mượn sẵn khi tab đến từ chính pane đó. Đã sửa y vậy. Cùng lớp rủi ro: `focus_handle` của tôi đọc pane rồi lần vào active item — rút về `self.active_pane.focus_handle(cx)` như `TerminalPanel`, ít bề mặt hơn trên một đường mà workspace gọi liên tục.

### Active-icon trên rail, và một vụ đi lạc theo default dock (2026-08-14)

Yêu cầu: bấm agent nào thì icon đó phải sáng lên trên rail — cùng ngôn ngữ hiệu ứng với `render_rail_panels` (`toggle_state`), chứ rail agent hiện tại chỉ là nút trơn.

Thêm `AgentPanel::has_agent(&AgentId, cx)` (đọc `view_for`), và `Sidebar::agent_panel(cx)` để rail gọi vào — kéo theo `sidebar` phải phụ thuộc `agent_ui` (chiều mới, không vòng, cùng kiểu `sidebar` đã phụ thuộc `recent_projects`). `render_rail_agents` giờ tính `is_active` mỗi agent độc lập — **không** phải kiểu "chỉ một active_index" như `render_rail_panels`, vì hai agent có thể đứng cạnh nhau cùng lúc, nên cả hai icon có thể sáng cùng lúc.

**Đi lạc giữa đường:** cùng lúc đó, người dùng báo "bấm Claude/Codex ra section trống" và "muốn mở git panel + agent cùng lúc" — tôi đọc `SidebarDockPosition::default() = Left` trong `settings_content/src/agent.rs`, thấy git panel/project panel cũng "Default: left" trong doc comment, và **suýt đổi default sang Right** dựa trên đó. Sai: doc comment không phải giá trị thật — `assets/settings/default.json` mới là default thật sự app dùng, và nó đã ghi đè: `git_panel.dock = "right"`, `outline_panel.dock = "right"`, chỉ `project_panel.dock = "left"`. Đổi sang Right sẽ đâm thẳng vào git+outline, tệ hơn nguyên trạng. Đã revert cả hai file trước khi commit.

Đọc tiếp `~/.config/zed/settings.json` của người dùng mới ra đầu mối thật: họ có `"git_panel": {"dock": "left"}` (override cá nhân) và `"agent": {"dock": "left", ...}` — nhưng field đúng của agent trong fork này là `sidebar_side`, không phải `dock`. Key `dock` bị bỏ qua lặng lẽ (schema không nhận field lạ), agent rơi về default thật (`left`) — đúng ngay chỗ họ đã dời git panel tới. Đây là **setting cá nhân lệch schema**, không phải bug trong repo — không sửa trong lần này, đã báo lại cho người dùng để họ tự đổi `sidebar_side` hoặc dời git panel.

Bài học: khi một field có tài liệu "Default: X" ở nhiều struct, luôn tra `assets/settings/default.json` trước khi suy ra có va nhau hay không — doc comment có thể lệch với giá trị ship thật.

### Sửa nhầm file settings — zode không đọc config của zed gốc (2026-08-14)

Chẩn đoán trên đúng hướng (git panel + agent chung dock) nhưng tôi tra **sai file**: `~/.config/zed/settings.json` — đó là config của Zed gốc. `crates/paths/src/paths.rs:87-105` (`config_dir()`) cho biết zode dùng thư mục riêng: `~/.config/zode/`. Người dùng nghe theo hướng dẫn sai, sửa file zode không hề đọc, nên bug vẫn còn nguyên — và quay lại báo "bấm Claude không hiện gì" ở lượt sau, tưởng là bug mới.

Đọc đúng file (`~/.config/zode/settings.json`) mới lộ ra: không có key `"agent"` nào — không phải "field sai tên" như tôi đoán lần trước, đơn giản là **chưa override**, nên rơi thẳng về default gốc (`left`), trùng với `git_panel.dock: "left"` họ đã tự đặt. Đã thêm `"agent": {"sidebar_side": "right"}` vào đúng file.

Đánh đổi cần nói rõ với người dùng: `project_panel` của họ đang ở phải và `starts_open: true` — đưa agent sang phải giải quyết đúng xung đột họ báo (git + agent), nhưng đổi sang xung đột khác ít nghiêm trọng hơn (agent + project panel, project panel chỉ mở sẵn lúc khởi động, không phải lúc đang làm việc).

Bài học ghi vào memory: khi debug một fork đổi tên (zed → zode), đừng suy luận đường dẫn config từ tên thư mục quen thuộc — tra `paths.rs`/`config_dir()` trước khi đọc bất kỳ file settings nào.

### Default mode: CLI trước, Chat sau (2026-08-14)

Người dùng muốn bấm agent lần đầu (chưa có preference lưu) phải vào **Terminal/CLI**, không phải Chat. `AgentViewMode`'s `#[default]` (phase 04 để là `Chat`) đổi sang `Terminal` tại `crates/zed_actions/src/lib.rs`. Test `a_first_click_opens_the_cli_rather_than_chat` chốt giá trị này trực tiếp trên enum, không qua mô phỏng UI.

**Chưa tái hiện được bằng test.** Ba lần thử (show → docked+focused → có vẽ) đều pass; đường drag cần `DraggedTab` thật và máy kéo-thả đứng sau nó. Test `showing_an_agent_in_a_docked_panel_draws` chỉ phủ đường thêm-và-vẽ, và tên nó nói đúng chừng đó — không nhận vơ phủ luôn drag.

### Section rỗng lúc khởi động (2026-08-14)

Mở app lên, chưa bấm Claude hay Codex, vẫn có một dải trống chiếm chỗ bên phải.

Đây chính là **hệ quả trực tiếp của món nợ ghi ngay trên**: dock nhớ trạng thái mở qua các phiên (`Dock::restore_state` đọc `visible` từ DB), còn thứ đứng bên trong nó thì không được serialize. Phiên trước có Claude → lần sau dock mở lại, rỗng.

Luật đã thêm: **panel rỗng thì tự đóng** (`close_if_empty` → `PanelEvent::Close`). Không vá riêng đường restore, mà đặt ở `Panel::set_active(true)` — chỗ duy nhất mọi đường dẫn tới một panel hiện hình đều đi qua: restore, `focus_panel`, và cả `ToggleRightDock` dạng chung. Cộng thêm ở nhánh `pane::Event::Remove`, cho trường hợp đóng agent cuối cùng.

**Cái giá của luật đó, và nó không hiển nhiên:** `AgentView::open` trước đây gọi `focus_panel` **trước** rồi mới đọc DB và `show()` trong task. Với luật mới, dock mở ra lúc còn rỗng → tự đóng ngay → agent đi vào một section không ai thấy. Tức là bấm rail sẽ **hỏng hoàn toàn**. Đã đảo thứ tự: `show()` xong mới `focus_panel`.

Ba test, mỗi test đều đã kiểm chứng ngược (tắt fix → đỏ đúng dòng assert đó):

- `a_dock_shown_holding_nothing_closes_itself` — triệu chứng người dùng báo
- `closing_the_last_agent_puts_the_dock_away` — cùng luật, đọc từ đầu kia
- `a_rail_click_opens_the_dock_onto_its_agent` — canh đúng cái ordering vừa nói; đảo lại thứ tự cũ là nó đỏ

Còn hở: nếu ai đó bấm `ToggleRightDock` khi panel rỗng, `set_active(true)` bắt được nên dock cũng đóng lại ngay — nhưng đó là "mở rồi đóng trong cùng một chu kỳ", chưa phải "không mở được". Chấp nhận, vì panel này không có icon nên không có nút nào dẫn tới đường đó.
