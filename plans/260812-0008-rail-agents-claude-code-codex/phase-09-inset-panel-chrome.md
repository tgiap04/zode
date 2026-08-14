# Phase 09 — Vỏ cửa sổ: bo góc và khoảng cách giữa các section

**Context:** [plan.md](plan.md) · [phase-08](phase-08-agents-share-a-pane-group.md)
**Priority:** P2 · **Status:** planned · **Blocked by:** 08

Yêu cầu: agent section cần **border radius**, cần **px hai bên** — và không riêng nó: **cả code editor, sidebar và rail**. Giữa hai agent section cần **px, py**.

Nghĩa là không phải vá một cái seam nữa mà đổi cả **vỏ cửa sổ**: mỗi section là một tấm bo góc nổi trên nền cửa sổ, cách nhau bằng khoảng trống thật.

## Vì sao phải là thay đổi ở tầng vỏ

Hai lần trước tôi cố tạo khoảng cách từ *bên trong* agent view và cả hai đều sai — một item không tạo được khoảng trống ngoài chính nó. Khoảng trống giữa các section thuộc về nơi các section được đặt cạnh nhau:

| Seam | Ai sở hữu |
|---|---|
| rail ↔ sidebar ↔ editor | `multi_workspace.rs` (chỗ compose 3 khối) |
| editor ↔ dock agent | `Dock::render` + `Workspace::render` |
| agent ↔ agent | `PaneGroup` **của `AgentPanel`** (phase 08) |

Nên đây là một lượt thống nhất, không phải bốn miếng vá rời.

## Hình dạng

```
┌ nền cửa sổ ────────────────────────────────────┐
│ ╭───╮ ╭──────────╮ ╭─────────────╮ ╭─────────╮ │
│ │ ▣ │ │ sidebar  │ │ code editor │ │ Claude  │ │
│ │ ▢ │ │          │ │             │ ├─────────┤ │
│ │ ◉ │ │          │ │             │ │ Codex   │ │
│ ╰───╯ ╰──────────╯ ╰─────────────╯ ╰─────────╯ │
└────────────────────────────────────────────────┘
   gap    gap          gap            gap trong
                                      pane group
```

## Việc

- [ ] Token dùng chung cho gap + radius, một chỗ khai (không rải số ma thuật)
- [ ] Nền cửa sổ lộ ra giữa các khối; mỗi khối tự bo góc
- [ ] Gap ở `multi_workspace.rs` giữa rail / sidebar / center
- [ ] Gap giữa center và dock
- [ ] px+py giữa các pane trong `AgentPanel`
- [ ] Kiểm 13" (R4): gap ăn chiều ngang, phải đo lại chỗ này

## Rủi ro

Đây là thay đổi chạm **toàn bộ chrome của app**, không riêng agent. Border/divider hiện có ở rail (`rail.rs:106`) và dock (`dock.rs:1153`) sẽ thành thừa hoặc phải đổi vai khi đã có gap thật — bỏ sót thì thành "vừa có viền vừa có khoảng cách", trông bẩn hơn cả hai.

R4 quay lại: mỗi gap ăn chiều ngang thật, và phase 06 đã ghi nợ kiểm 13".

---

## Một phần đã làm (2026-08-13)

**Hệ thống này đã có sẵn trong fork, tôi chỉ chưa cho agent tham gia.** `pane_group.rs` giữ `SURFACE_MARGIN = 3px` và `SURFACE_ROUNDING = 5px`, nhưng `PaneGroup::render` chỉ áp cho **centre group**:

```rust
if !self.is_center { return element; }
```

Group của `AgentPanel` không phải centre nên nhận element trần — đúng lý do agent section không bo góc, không khoảng cách.

Đã cho agent dock vẽ **cùng một surface**, dùng chung hằng số (đổi từ private sang `pub`) chứ không chép số sang file khác: hai surface bo khác nhau thì đọc ra là hai app khác nhau, mà số chép đi thì lệch ngay lần sửa đầu tiên.

### Điều code cũ đã ghi lại, và nó là cảnh báo cho phần còn lại

Comment ngay tại chỗ áp margin nói rõ vì sao chỉ centre mới có:

> *The centre alone carries the seam. Everything it touches — the project rail, the docks — is drawn flush, so this margin is the entire gap between them rather than one half of a doubled channel, which is what read as a grey bar when the sidebar had a surface of its own.*

Tức fork **đã thử** cho sidebar surface riêng và rút lại vì hai margin cạnh nhau tạo ra một rãnh xám đôi. Nên "px hai bên cho cả editor, sidebar, rail" không phải thêm margin ở bốn chỗ — phải quyết ai chịu seam, nếu không lặp lại đúng lỗi cũ.

Riêng seam agent–editor giờ là 3px (centre) + 3px (agent) = 6px. Cần nhìn mới biết dày quá không.

### Surface đó vẽ ra chiều cao 0 (2026-08-14)

Người dùng báo: bấm Claude/Codex, dock mở ra nhưng **trống trơn** — không tab bar, không terminal, không cả nút Chat|Terminal. Icon trên rail vẫn sáng, tức `has_agent` = true, item có thật.

Mọi assertion trạng thái đều xanh (dock mở, `items_len == 1`, `has_agent`) trong khi panel vẽ ra một cái hộp rỗng. Chỉ **đo bounds thật** mới bắt được:

```
root:    479px × 1073px
surface: 473px ×    0px   ← đây
```

Full width, **zero height**. `flex_1` cho chiều ngang (trục chính), còn chiều dọc thì không ai cấp — `self_stretch` không sinh ra gì cả.

Vì sao centre group dùng đúng pattern đó lại chạy: cha của nó là `h_flex()` = `.flex().flex_row().items_center()` (`ui/src/traits/styled_ext.rs:30`) — `self_stretch()` ở đó tồn tại để **ghi đè** `align_items: center`. Cha của panel này là `div()` trần (`align_items: None`), và ở đó stretch không ra gì. Đã tách riêng để loại trừ margin: bỏ margin đi vẫn `479 × 0`, nên **margin vô can** — thủ phạm là `self_stretch`.

Sửa: đưa gap lên cha thành **padding**, con dùng `size_full`. Percentage resolve theo content box nên padding *chính là* khoảng cách — đồng thời né luôn cái bẫy `size_full` + margin tràn/bị clip mà comment của centre đã ghi. `TerminalPanel::render` là tiền lệ đã chạy: `size_full` trơn, không stretch.

Test `a_shown_agent_actually_occupies_the_panel` đo bounds thật (`debug_selector` + `cx.debug_bounds` sau khi dock + `run_until_parked` vẽ window). Kiểm chứng ngược: trả lại `flex_1().self_stretch()` → đỏ đúng `473px × 0px`. Assertion không chỉ `> 0` mà còn: agent phủ hết chiều ngang surface, phần chiều cao duy nhất nó nhường là tab bar (< 1/4), và surface trải hết panel trừ inset — vì một assertion `> 0` sẽ pass trên một sợi chỉ 1px.

Bài học đã ghi memory: smoke test "có vẽ không panic" **không chứng minh gì** về layout; phải đo.

### Chưa làm

- **px/py giữa hai agent pane.** Surface đang áp cho cả group; muốn từng pane một tấm riêng thì phải luồn cờ qua `Member::render` (`pane_group.rs:561`), và đó là code dùng chung với centre — đáng một thay đổi riêng, review riêng.
- Inset cho editor / sidebar / rail — xem cảnh báo trên.
