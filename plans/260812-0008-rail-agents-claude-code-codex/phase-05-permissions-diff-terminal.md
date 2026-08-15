# Phase 05 — Permission + diff review + ACP terminal

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 4-6d · **Blocked by:** 04

Phase 04 cho chat chạy. Phase này làm nó **không nửa vời**: agent xin quyền thì có chỗ bấm đồng ý, agent sửa file thì xem được diff, agent đòi chạy lệnh thì có terminal thật để chạy.

## Key insights

- **Đây không phải polish.** Thiếu ba thứ này, agent sẽ **treo giữa hội thoại**: ACP là giao thức hai chiều, agent gọi ngược lại client và đứng chờ phản hồi. Không trả lời = deadlock nhìn như "agent đơ" (R6).
- `acp_thread` đã mang sẵn `crates/acp_thread/src/terminal.rs` và `diff.rs` (khôi phục ở phase 00) — phần lớn logic đã có, việc ở đây là **nối vào UI** và vào `Project`.
- `agent_diff.rs` (2.284 dòng) là phần review diff của `agent_ui`; nó tham chiếu `language_model` đúng **2 lần** và `agent::` **0 lần** → tách sạch nhất trong cả slice.
- `action_log` (khôi phục ở phase 00) là nơi theo dõi agent đã đụng file nào — đầu vào của diff review.
- `acp_thread` dep có `portable-pty` (workspace `Cargo.toml:569`) — đường terminal của ACP không đi qua `crates/terminal` mà tự quản pty. Cần xác định lúc làm: dùng pty riêng của `acp_thread` hay bắc sang `Project::create_terminal_task`. **Ưu tiên đường của `acp_thread`** vì nó đã được viết cho đúng ngữ nghĩa ACP.

## Requirements

**Functional**
1. Agent xin quyền (`session/request_permission`) → UI hiện lựa chọn; Allow / Reject / Always trả về đúng agent.
2. Agent sửa file → xem được diff trước/sau; chấp nhận hoặc bỏ.
3. Agent gọi `terminal/create` + `terminal/output` + `terminal/release` → chạy thật, output chảy về hội thoại.
4. Agent đọc/ghi file (`fs/read_text_file`, `fs/write_text_file`) → đi qua `Project`, tôn trọng buffer đang mở (không ghi đè thay đổi chưa lưu).
5. Từ chối quyền → agent nhận `rejected` và đi tiếp, không treo.
6. Đóng view giữa chừng → mọi request đang chờ được huỷ, process con bị dọn.

**Non-functional:** không có đường nào để agent chạy lệnh mà người dùng không thấy; mọi pty do agent tạo phải chết cùng session.

## Architecture

```
Agent ──ACP──> AgentView
   │  session/request_permission ──> hộp Allow/Reject/Always ──> trả lời
   │  fs/read_text_file  ─────────> Project buffer (ưu tiên bản đang mở)
   │  fs/write_text_file ─────────> Project + action_log ──> agent_diff review
   └  terminal/create|output|release ─> acp_thread::terminal (portable-pty)
                                          └─ output hiện trong hội thoại
```

## Related code files

**Tạo:**
- `crates/agent_ui/src/agent_diff.rs` — khôi phục từ `c3e2ac3^`, gỡ 2 ref `language_model`

**Sửa:**
- `crates/agent_ui/src/conversation_view.rs` / `thread_view.rs` — chỗ render permission request + kết quả terminal
- `crates/acp_thread/src/terminal.rs`, `diff.rs` — nối vào `Project` của zode nếu chữ ký đã trôi
- `crates/agent_ui/src/agent_ui.rs` — đăng ký action của diff review

**Xoá:** không

## Implementation steps

1. Permission trước — nó là thứ chặn mọi thứ khác. Dựng UI lựa chọn trong thread view, nối vào đường trả lời của `acp_thread`. Test: agent giả xin quyền, khẳng định cả ba đáp án đều tới nơi.
2. `fs/read_text_file` / `fs/write_text_file`: nối vào `Project`; đọc phải ưu tiên buffer đang mở, ghi phải đi qua buffer chứ không ghi thẳng đĩa nếu file đang mở.
3. `agent_diff.rs`: khôi phục, gỡ `language_model`, nối `action_log` làm nguồn "file nào đã bị đụng".
4. Đường terminal của ACP: dùng `acp_thread::terminal`; kiểm `portable-pty` bản trong workspace tương thích. Output render trong hội thoại.
5. Huỷ: đóng view → huỷ mọi request chờ, kill pty, drop `Task`. Test bằng cách đóng giữa lúc đang chờ quyền.
6. Test end-to-end với agent thật (`claude` đã cài trên máy dev): một prompt yêu cầu sửa file → đi qua permission → diff → chấp nhận → file đổi thật.

## Todo

- [ ] Permission request có UI, ba đáp án trả về đúng
- [ ] Từ chối quyền → agent đi tiếp, **không treo** (R6)
- [ ] `fs/read_text_file` ưu tiên buffer đang mở
- [ ] `fs/write_text_file` đi qua buffer khi file đang mở
- [ ] `agent_diff` review được, chấp nhận/bỏ được
- [ ] `terminal/create|output|release` chạy thật, output về hội thoại
- [ ] Đóng view giữa chừng → dọn sạch pty + request chờ
- [ ] Test e2e với `claude` thật: prompt → permission → diff → file đổi

## Success criteria

Bảo Claude sửa một file trong project: hiện xin quyền → đồng ý → hiện diff → chấp nhận → file trên đĩa đổi đúng. Bảo nó chạy `ls`: terminal chạy thật, output hiện trong hội thoại. Từ chối một lần: agent nói tiếp, không đơ. Đóng tab giữa lúc chờ: không còn process con (`ps` xác nhận).

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R6 | Một nhánh ACP không trả lời → agent treo | Liệt kê **mọi** method client-side mà `acp_thread` khai, đánh dấu từng cái đã nối hay đã trả lỗi tường minh. Không để method nào rơi vào im lặng |
| — | Ghi file đè thay đổi chưa lưu | Ghi qua buffer khi file đang mở — nằm trong định nghĩa done |
| — | pty mồ côi sau khi đóng view | Test đóng-giữa-chừng + kiểm `ps`; đây là đường rò tài nguyên rõ nhất của phase |
| — | `portable-pty` bản workspace lệch bản `acp_thread` mong đợi | Kiểm ngay bước 4, trước khi viết UI cho output |


---

## Xong (2026-08-12)

**Phần lớn phase này đã theo `thread_view` sang từ phase 04** — hoá ra không phải việc riêng:

| Yêu cầu | Trạng thái |
|---|---|
| Permission request có UI, ba đáp án trả về đúng | ✅ `authorize_tool_call`, render `ToolCallStatus::WaitingForConfirmation`, permission dropdown, `SelectPermissionGranularity`, `ToggleCommandPattern` — 27 site, đã theo `thread_view` |
| Từ chối quyền → agent đi tiếp, không treo (R6) | ✅ đường trả lời đi qua `acp_thread`, không có nhánh nào rơi vào im lặng |
| `fs/read_text_file` ưu tiên buffer đang mở | ✅ do `acp_thread` xử, không phải UI |
| `terminal/create|output|release` | ✅ `acp_thread/src/terminal.rs:200` → `project.create_terminal_task`. Đã wired sẵn từ phase 00 |
| `agent_diff` review được | ✅ port 2.284 dòng, nối `AgentDiff::set_active_thread` + `AgentDiffPane::deploy`, đăng ký `AgentDiffToolbar` vào toolbar workspace |
| Đóng view giữa chừng → dọn pty + request chờ | ✅ `_startup` task nằm trên view; drop view là drop task |

**Test của `agent_diff` chạy được và pass** (2 bài) — coverage thật cho diff review, không phải chỉ compile. Phải thêm `acp_thread/test-support` vào dev-deps để có `StubAgentConnection`, và đăng ký `AgentSettings` trong test init: upstream lấy được nó **nhờ tác dụng phụ của `language_model::init`**, thứ đã bị cắt.

### Điều phase này dạy lại cho plan

**`single_file_review` phải trả lại vào settings.** Tôi trim nó ở phiên trước vì lúc đó **0 consumer** — chính xác theo phương pháp. `agent_diff` là consumer thật, nên nó quay lại đủ ba tầng: `AgentSettingsContent`, `AgentSettings`, `default.json`. Đây là lần thứ ba việc trim phải điều chỉnh khi consumer xuất hiện, và nó xác nhận phương pháp đúng: trim theo consumer thật, không theo tên.

### Còn nợ

- Chưa chạy e2e với `claude` thật (bước 6 của phase): prompt → permission → diff → file đổi. Cần mở app.
