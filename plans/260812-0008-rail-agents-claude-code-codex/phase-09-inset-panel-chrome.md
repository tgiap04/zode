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
