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
