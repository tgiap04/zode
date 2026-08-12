# Phase 03 — Đường chưa-cài-CLI

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 2-3d · **Blocked by:** 02

Yêu cầu gốc số 3, làm cho trọn: chưa cài CLI thì được báo bằng **toast và** empty state, kèm lệnh đúng OS và một nút mở terminal sẵn lệnh (chưa Enter).

## Key insights

- **Điền lệnh mà không chạy là làm được.** `Terminal::input(impl Into<Cow<'static, [u8]>>)` (`terminal.rs:1517`) ghi thẳng vào pty; không kèm `\r` thì lệnh nằm đó chờ Enter. `paste()` (`:1665`) là đường thứ hai nếu cần.
- **Lệnh cài đã lấy từ docs chính hãng** (không viết theo trí nhớ). Đáng chú ý: `npm i -g @anthropic-ai/claude-code` **không** deprecated — nó nằm ở mục "Advanced installation options"; native installer mới là "Recommended".
- Binary native của Claude Code nằm ở `~/.local/bin/claude`, symlink sang `~/.local/share/claude/versions/` — đúng cấu hình máy dev, và đúng lý do R1 tồn tại.
- `claude doctor` là lệnh chẩn đoán chính hãng — đáng đưa vào empty state khi binary có mà chạy lỗi.
- **Gate mềm cho chat UI** (chốt 2026-08-12): chat UI chạy npx adapter nên *về kỹ thuật* không cần binary local. Vẫn kiểm, và khi thiếu thì cho **hai** lối đi thay vì chặn.
- `workspace::show_notification` (`notifications.rs:81`) cho toast; `MessageNotification` (`:714`) đã có sẵn dạng có nút.

## Lệnh cài — bảng sự thật

Một chỗ duy nhất giữ bảng này (đặt cạnh định nghĩa built-in agent ở phase 01).

**Claude Code** — nguồn: docs chính hãng `code.claude.com/docs/en/setup`

| Nền | Lệnh |
|---|---|
| macOS / Linux / WSL *(khuyến nghị)* | `curl -fsSL https://claude.ai/install.sh \| bash` |
| Windows PowerShell | `irm https://claude.ai/install.ps1 \| iex` |
| Windows CMD | `curl -fsSL https://claude.ai/install.cmd -o install.cmd && install.cmd && del install.cmd` |
| Homebrew | `brew install --cask claude-code` |
| WinGet | `winget install Anthropic.ClaudeCode` |
| npm *(nâng cao, cần Node 22+)* | `npm install -g @anthropic-ai/claude-code` |

**Codex** — nguồn: `learn.chatgpt.com/docs/codex/cli` + README của `openai/codex`

| Nền | Lệnh |
|---|---|
| macOS / Linux | `curl -fsSL https://chatgpt.com/codex/install.sh \| sh` |
| Windows | `powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 \| iex"` |
| npm | `npm install -g @openai/codex` |
| Homebrew | `brew install --cask codex` |

> **Cần kiểm lại lúc làm:** trang docs của Codex render lệnh trong tab nên bản fetch không lấy được chính xác dòng npm/brew; hai dòng đó đến từ kết quả tìm kiếm + README. Đối chiếu lại trước khi hardcode vào UI. Bốn dòng của Claude Code thì đã đọc trực tiếp từ trang docs.

## Requirements

**Functional**
1. Ấn nút agent mà binary vắng → **toast** hiện ngay, mang tên agent + lệnh cài + nút `Copy`.
2. Đồng thời **mở màn hình agent** với empty state: nói rõ thiếu gì, lệnh theo OS đang chạy, nút `Copy`, link docs.
3. Nút `Cài ngay` → mở center terminal với lệnh **đã điền, chưa Enter**.
4. Chat UI dùng **gate mềm**: empty state có hai nút — `[Cài CLI]` (khuyến nghị) và `[Chạy bằng npx]`. Terminal mode gate cứng (không có binary thì không có gì để chạy).
5. Lệnh chọn theo OS đang chạy, không hiện cả bảng.
6. Cài xong rồi bấm lại → chạy được, **không cần khởi động lại app** (resolve lại, không cache vĩnh viễn kết quả "thiếu").

**Non-functional:** không tự chạy lệnh cài nào (#9); toast không được nuốt empty state và ngược lại.

## Architecture

```
OpenAgent(agent, mode)
   └─ resolve_local_binary (phase 01)
        ├─ Ok(path)              → mode Terminal: spawn · mode Chat: npx adapter
        └─ Err(AgentBinaryMissing)
             ├─ show_notification(toast: tên + lệnh + [Copy])
             └─ mở AgentView ở trạng thái MissingBinary
                   ├─ mode Terminal → [Copy] [Cài ngay]
                   └─ mode Chat     → [Cài CLI] [Chạy bằng npx]   ← gate mềm
```

`Cài ngay` = `create_terminal_shell(project_root)` → `terminal.input(command.as_bytes())` **không** `\r`.

## Related code files

**Sửa:**
- `crates/project/src/agent_server_store.rs` — bảng lệnh theo OS gắn vào `AgentBinaryMissing`
- `crates/agent_ui/` — trạng thái `MissingBinary` của view + empty state + toast; nút `Cài ngay`

**Tạo:** không (view đã dựng ở phase 02) · **Xoá:** không

## Implementation steps

1. Điền bảng lệnh theo OS vào chỗ định nghĩa built-in agent; chọn theo `cfg!(target_os)` + biết cả nhánh WSL nếu rẻ.
2. Trạng thái `MissingBinary` trong view: tiêu đề, một dòng giải thích, khối lệnh chọn được, nút `Copy`, link docs.
3. Toast qua `workspace::show_notification` với `NotificationId` **theo agent** — hai agent thiếu cùng lúc không đè nhau, và bấm lại cùng agent không xếp chồng toast.
4. `Cài ngay`: tạo center terminal ở project root rồi `input(bytes)` **không** `\r`. Kiểm tay: con trỏ ở cuối dòng, chưa chạy.
5. Gate mềm cho mode Chat: nút thứ hai `Chạy bằng npx` → đi tiếp bằng registry entry của phase 01. Đây là **click tường minh** mà R5 yêu cầu.
6. Bỏ cache kết quả resolve khi view được focus lại, để yêu cầu 6 đúng.
7. Test: `AgentBinaryMissing` → view vào đúng trạng thái, lệnh khớp OS đang chạy.
8. Test: hai agent cùng thiếu → hai toast, id khác nhau.

## Todo

- [x] Bảng lệnh theo OS, một nguồn sự thật — `BuiltinAgent::install_command()` chọn theo `cfg!(target_os)`
- [x] Dùng **native installer** cho cả hai (đọc thẳng từ docs), nên không phải chốt dòng npm/brew của Codex — mục còn ngờ trong plan không còn nằm trên đường chạy
- [x] Empty state: lệnh + `Copy` + link docs
- [x] Toast có `NotificationId` theo agent
- [x] `Cài ngay` điền lệnh, **chưa Enter**
- [ ] ~~Gate mềm mode Chat: `[Cài CLI]` + `[Chạy bằng npx]`~~ — **dời sang phase 04, có chủ ý.** Xem điều #1
- [x] Cài xong bấm lại chạy được, không cần restart — **hai** đường: nút `Check Again`, và bấm lại nút rail tự retry
- [x] Test id toast tách theo agent — `each_agent_gets_its_own_toast`
- [x] Test lệnh cài không chứa newline — `install_commands_never_carry_a_newline`
- [ ] **Còn nợ mắt người:** chưa mở app bấm thử. Đặc biệt là thứ tự ghi vào pty (điều #3).

## Kết quả (2026-08-12)

`./script/clippy` 5 crate exit 0 · **33 test xanh** (3 `agent_ui`, 16 `sidebar`, 14 `agent_server_store`) · `cargo build -p zode` exit 0.

| File | Δ |
|---|---|
| `crates/agent_ui/src/missing_binary.rs` (mới) | ~195 |
| `crates/agent_ui/src/agent_view.rs` | +~45 |
| `crates/project/src/agent_server_store.rs` | +18 (test) |

## Ba điều phase này dạy lại cho plan

**1. Gate mềm thuộc về phase 04, không phải phase này.**
Quyết định gate mềm là *về mode Chat*: chưa thấy CLI thì cho hai lối — `[Cài CLI]` và `[Chạy bằng npx]`. Nhưng mode Chat chưa tồn tại ở đây, nên nút `Chạy bằng npx` sẽ dẫn vào một nhánh chưa có. Terminal mode thì gate cứng theo đúng thiết kế: không có binary là không có gì để chạy. Đã dựng empty state **cho gate cứng** (`Copy` · `Install Now` · `Check Again` · `Docs`); phase 04 chỉ thêm nút thứ hai vào cùng hàng đó khi Chat có thật.

**2. "Cài xong bấm lại được" cần hai đường, không phải một.**
Yêu cầu 6 dễ bị hiểu là chỉ cần bỏ cache. Thực tế người dùng có hai thói quen khác nhau: người thì ở lại màn hình đó rồi cài ở terminal bên cạnh (⇒ cần nút `Check Again`), người thì bỏ đi rồi bấm lại icon trên rail (⇒ `AgentView::open` phải **retry** thay vì chỉ focus vào bản án cũ). Đã làm cả hai; thiếu đường nào thì một nửa người dùng vẫn kẹt.

**3. Lệnh cài không được chứa newline — và đó là chuyện đúng-sai, không phải thẩm mỹ.**
`Install Now` ghi thẳng chuỗi lệnh vào pty rồi **dừng**, không gửi `\r`. Nếu chuỗi lệnh tự mang `\n`, "điền sẵn chờ bạn bấm Enter" lặng lẽ thành "vừa chạy `curl … | bash` hộ bạn" — đúng thứ quyết định #9 loại bỏ. Có test khoá lại (`install_commands_never_carry_a_newline`), vì đây là loại lỗi mà mắt đọc code sẽ trượt.

**Còn hở, phải kiểm bằng mắt:** chữ được ghi vào pty ngay sau khi terminal được tạo, trước khi shell chắc chắn đã sẵn sàng đọc. Tty buffer thường giữ giúp, nhưng zsh với ZLE có thể nuốt hoặc echo lạ. Nếu thấy lệnh hiện thiếu ký tự, chỗ cần sửa là đợi tín hiệu shell đã spawn rồi mới `input()`.

## Success criteria

Trên máy dev (`codex` chưa cài): bấm Codex → toast + empty state, lệnh đúng macOS; `Cài ngay` mở terminal có lệnh chờ Enter. Bấm Claude → chạy thẳng, **không** báo thiếu (R1 vẫn đứng). Cài `codex` xong bấm lại → chạy, không restart app.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R1 | Báo "chưa cài" trong khi đã cài | Đã bịt ở phase 01; phase này chỉ là bề mặt của lỗi đó — đường chạy thật trên máy dev là bài kiểm cuối |
| R5 | npx chạy mà người dùng không biết | Chỉ chạy sau click `Chạy bằng npx`; câu chữ nói rõ nó tải package |
| — | Lệnh cài lỗi thời sau vài tháng | Kèm link docs cạnh mỗi lệnh, để bản lỗi thời vẫn có đường đúng |
| — | `input()` mà shell chưa sẵn sàng → mất chữ | Điền sau khi terminal báo đã spawn, không điền ngay lúc tạo |
