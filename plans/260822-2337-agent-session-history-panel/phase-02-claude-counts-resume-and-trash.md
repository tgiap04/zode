# Phase 02 — Số đếm, lệnh resume, xoá vào Trash

## Context Links

- Phase 01: `phase-01-session-index-crate-and-claude-listing.md`
- `crates/fs/src/fs.rs:121` — `Fs::trash(&self, path, RemoveOptions) -> Result<TrashedEntry>`
- `crates/agent_ui/src/agent_view.rs:614` — `agent_task()`, nơi `args` hiện là `Vec::new()`

## Overview

- **Priority:** P1
- **Status:** **done** (23/08) · **Phụ thuộc:** 01
- Ba method còn lại của `SessionProvider`, cho Claude. Không UI.

## Key Insights

- Số subagent = **đếm file** `<log_dir>/<session-id>/subagents/*.meta.json`, một
  `read_dir`. Không phải parse `isSidechain` — đo thật: `isSidechain` = 0 trong một
  session có 27 subagent, nên nó không phải nguồn.
- Session có thư mục sidecar `<session-id>/` **cạnh** file `<session-id>.jsonl`, chứa
  `subagents/` và `tool-results/`. Xoá session phải xoá cả hai.
- `msgs` = số dòng có `"type":"user"` hoặc `"type":"assistant"`. Đếm bằng cách quét
  dòng và so **prefix chuỗi**, không `serde_json::from_str` cả dòng — dòng
  `assistant` có thể vài trăm KB, deserialize hết là phí toàn bộ ngân sách.
- `claude --resume <id>` tiếp session; `--fork-session` tạo session mới từ nó.
  `codex resume <SESSION_ID>` — đã xác nhận trong `codex resume --help`.
- `Fs::trash` với thư mục cần `RemoveOptions { recursive: true, ignore_if_not_exists: .. }`.

## Requirements

**Functional**

- FR8 — `counts(&SessionId) -> Result<SessionCounts { messages, subagents }>`.
- FR9 — `resume_command(&SessionSummary, Fork) -> ResumeCommand { program, args, cwd }`.
  `Fork::Continue` → `["--resume", id]`; `Fork::New` → `["--resume", id, "--fork-session"]`.
- FR10 — `delete(&SessionSummary)` đưa `<id>.jsonl` **và** thư mục `<id>/` vào Trash của
  OS. Thiếu thư mục sidecar không phải lỗi.
- FR11 — `ResumeCommand::to_shell_string()` cho "Copy Resume Command" — quote đúng
  path có khoảng trắng.

**Non-functional**

- `counts()` một file 13MB dưới **50ms**.
- `delete()` không bao giờ dùng `remove_file`/`remove_dir_all`. Chỉ `Fs::trash`.

## Architecture

`counts()` nhận `&dyn Fs` và trả `SessionCounts`. Không cache trong crate này —
cache là việc của panel (RAM, theo session id), vì crate không biết cái gì đang hiển
thị. Giữ crate không state là điều kiện để test nó không cần window.

`ResumeCommand` là dữ liệu thuần (`program: PathBuf, args: Vec<String>, cwd: PathBuf`),
crate **không** spawn gì. Phase 06 chuyển nó thành `SpawnInTerminal`.

## Related Code Files

**Sửa**
- `crates/agent_sessions/src/provider.rs` — thêm `counts`, `resume_command`, `delete`
- `crates/agent_sessions/src/claude.rs` — thân ba method
- `crates/agent_sessions/src/summary.rs` — `SessionCounts`, `ResumeCommand`, `Fork`

## Implementation Steps

1. `SessionCounts`, `ResumeCommand`, `Fork` trong `summary.rs`.
2. Mở rộng trait; `cargo check` đỏ ở đúng một chỗ (ClaudeProvider) — sửa cho xanh.
3. `counts()`: đọc file theo chunk 64KB, đếm dòng khớp prefix. Test: fixture có
   3 user + 2 assistant + 4 dòng loại khác → `messages == 5`.
4. Subagent: `read_dir` trên `<id>/subagents`, đếm `*.meta.json`. Thư mục không tồn
   tại → `subagents: 0`, không phải `Err`.
5. `resume_command()` + test bảng cho hai `Fork` và hai agent.
6. `delete()` qua `FakeFs` — kiểm tra cả hai đường đi vào trash, và trường hợp không
   có sidecar.
7. `to_shell_string()` + test với path chứa khoảng trắng.

## Todo List

- [x] `SessionCounts` / `ResumeCommand` / `Fork`
- [x] Trait mở rộng, ClaudeProvider implement đủ
- [x] `counts()` đếm theo prefix, test số chính xác
- [x] `subagents` qua `read_dir`, thiếu thư mục = 0
- [x] `resume_command()` + test bảng
- [ ] `delete()` qua `FakeFs`, cả `.jsonl` và `<id>/`
- [x] `to_shell_string()` quote đúng
- [x] Test `#[ignore]` đo `counts()` trên file 13MB thật
- [x] clippy sạch

## Success Criteria

1. `counts()` trên session `16819818-…` trả `subagents` khớp
   `ls ~/.claude/projects/-Users-tgiap-dev-devs-zode/16819818-*/subagents/*.meta.json | wc -l`.
2. `messages` khớp `grep -c '"type":"user"\|"type":"assistant"'` trên cùng file.
3. `delete()` gọi `Fs::trash` đúng hai lần (file + thư mục) và **không** gọi bất kỳ
   API xoá thẳng nào — kiểm bằng `FakeFs::trash_entries()`.
4. `resume_command(claude, Fork::New)` ra `claude --resume <id> --fork-session`.
5. Đo `counts()` trên file 13MB < 50ms.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Prefix `"type":"user"` xuất hiện trong **nội dung** message → đếm lố | So khớp ở đầu dòng sau `{`, không `contains()` cả dòng. Test có một dòng assistant chứa đúng chuỗi đó trong text. |
| Xoá nhầm khi id trùng tên thư mục khác | Ghép path từ `log_path.parent()` + id, không format chuỗi tay |
| Trash không có trên môi trường không đầu (CI, remote) | `Fs::trash` đã có fallback riêng; nếu `Err` → phase 06 hiện lỗi cho người dùng, **không** rơi về xoá thẳng |
| Session đang chạy bị xoá | Không chặn được từ crate; phase 06 chịu trách nhiệm hộp xác nhận |

## Security Considerations

- Xoá là hành vi phá huỷ duy nhất của cả plan. Nó nằm ở một hàm, gọi một API
  (`Fs::trash`), và có test chứng minh không có đường xoá thẳng nào tồn tại.
- `to_shell_string()` đi vào clipboard, không đi vào shell. Nhưng vẫn quote đúng:
  người dùng sẽ dán nó vào shell thật.

## Next Steps

Phase 03 (Codex) dùng đúng trait này. Phase 06 tiêu thụ cả ba method.

## Ghi chú khi làm xong

- `delete()` **không tồn tại trong crate** — đổi thiết kế: trait chỉ trả `paths_to_trash()`, còn `Fs::trash` gọi ở tầng UI, nơi có hộp xác nhận. Lý do: `Fs::trash` là async, còn cả crate này là đồng bộ để test được không cần runtime; và hành vi phá huỷ nên nằm cùng chỗ với thứ hỏi người dùng. Test tương ứng: `a_delete_names_the_sidecar_before_the_log`.
- `counts()` trên file 13MB thật khớp `grep -c` (1981) và `ls subagents/*.meta.json` (13).
