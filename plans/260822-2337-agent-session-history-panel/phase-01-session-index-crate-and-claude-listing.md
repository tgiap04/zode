# Phase 01 — Crate `agent_sessions` + đọc danh sách Claude

## Context Links

- Thiết kế: `plans/reports/brainstorm-260822-agent-session-history.md`
- Nguồn dữ liệu thật: `~/.claude/projects/<cwd-encoded>/<uuid>.jsonl` — 292 file, 408MB
- Tham chiếu API: `crates/fs/src/fs.rs` (`Fs` trait), `crates/fuzzy` (search sau này ở phase 05)

## Overview

- **Priority:** P1 (mọi phase khác đứng trên nó)
- **Status:** pending
- Crate mới, không UI, test được không cần window. Nhiệm vụ duy nhất: biến 292 file
  jsonl thành `Vec<SessionSummary>` đủ để vẽ một hàng, **không** đọc hết file.

## Key Insights

- `ai-title` xuất hiện **nhiều lần** trong một file (đo được: 74 lần trong một session)
  — dòng cuối cùng thắng. Nghĩa là title nằm ở **đuôi** file, không phải đầu.
- Mọi dòng `user`/`assistant` đều mang `cwd`, `gitBranch`, `timestamp`, `sessionId`,
  `version`. `message.model` chỉ có ở dòng `assistant`.
- Có nhiều `type` khác không phải hội thoại: `mode`, `permission-mode`, `attachment`,
  `last-prompt`, `file-history-snapshot`, `file-history-delta`, `queue-operation`,
  `system`, `ai-title`, `pr-link`. Chỉ `user`/`assistant` là message.
- Đo thật: `wc -l` trên file 13MB = **17ms**. Đọc đuôi 256KB thì không đáng kể.
- Dòng đầu file **không** phải message (`{"type":"mode",...}`), nên head-read phải
  quét vài KB chứ không đọc một dòng.

## Requirements

**Functional**

- FR1 — `SessionSummary { id, agent, title, preview, cwd, branch, model, updated_at, created_at, log_path }`
- FR2 — `SessionProvider` trait: `agent()`, `list()`, `counts()`, `resume_command()`, `delete()`.
  Phase này chỉ implement `list()`; ba cái sau `todo!()` là **không** được — trả
  `Err(anyhow!("not implemented"))` cũng không. Khai báo trait ở phase này chỉ với
  `agent()` + `list()`, ba method còn lại thêm ở phase 02 khi có thân thật.
- FR3 — `ClaudeProvider::list()` đọc **đuôi 256KB** (nới tới 1MB nếu chưa gặp `ai-title`)
  + **đầu 16KB** mỗi file.
- FR4 — Title: `ai-title` cuối cùng; không có → text của message `user` đầu tiên, cắt
  một dòng.
- FR5 — Preview: message `user`/`assistant` cuối cùng, kèm nhãn role (`Agent:` / `You:`).
- FR6 — Bỏ session không có message `user` nào.
- FR7 — Sort mới nhất trước theo mtime của file (không parse timestamp để sort).

**Non-functional**

- `list()` trên 292 file phải xong dưới **200ms** khi page cache ấm. Đo, đừng đoán.
- Không panic trên bất kỳ input nào: file rỗng, JSON hỏng giữa dòng, thiếu field,
  UTF-8 sai.

## Architecture

```
crates/agent_sessions/
  Cargo.toml        anyhow, serde, serde_json, smol/futures, fs, util, paths
  src/lib.rs        pub use; SessionId; AgentKind { Claude, Codex }
  src/summary.rs    SessionSummary, SessionCounts, ResumeCommand
  src/provider.rs   trait SessionProvider
  src/claude.rs     ClaudeProvider: list()
  src/claude_log.rs đọc đuôi/đầu + parse dòng (thuần, không IO → test dễ nhất)
```

`claude_log.rs` là nơi mọi quyết định parse sống, và nó nhận **`&str`** chứ không
nhận path — thế thì test không cần fs, và nó là chỗ duy nhất phải chịu format đổi.

Root `Cargo.toml`: thêm `"crates/agent_sessions"` vào `members` và
`agent_sessions = { path = "crates/agent_sessions" }` vào `[workspace.dependencies]`.

## Related Code Files

**Tạo mới**
- `crates/agent_sessions/Cargo.toml`
- `crates/agent_sessions/src/{lib,summary,provider,claude,claude_log}.rs`

**Sửa**
- `Cargo.toml` (workspace members + dependencies)

**Đọc để hiểu**
- `crates/fs/src/fs.rs` — `Fs::load`, `Fs::read_dir`, và cách các crate khác nhận
  `Arc<dyn Fs>` để test bằng `FakeFs`

## Implementation Steps

1. Dựng crate, thêm vào workspace, `cargo check -p agent_sessions` xanh với `lib.rs` rỗng.
2. `summary.rs`: các struct dữ liệu, không logic.
3. `claude_log.rs`: `parse_tail(&str) -> TailFacts` và `parse_head(&str) -> HeadFacts`.
   Viết test trước bằng chuỗi jsonl inline (5-6 dòng, gồm một dòng hỏng cố ý).
4. `claude.rs`: enumerate `~/.claude/projects/*/*.jsonl` qua `Fs`, stat lấy mtime,
   đọc đuôi + đầu, gọi parser, lọc theo FR6, sort theo FR7.
5. Test trên **fixture thật**: copy 2 file nhỏ từ `~/.claude/projects/` vào
   `crates/agent_sessions/test_data/` (chọn file < 200KB, **xoá nội dung nhạy cảm nếu có**).
6. Đo `list()` trên thư mục thật bằng một test `#[ignore]` in ra thời gian.

## Todo List

- [ ] Crate + workspace wiring, `cargo check` xanh
- [ ] `SessionSummary`/`SessionCounts`/`ResumeCommand`
- [ ] `trait SessionProvider { agent, list }`
- [ ] `claude_log.rs` + test parse thuần (gồm dòng hỏng, file rỗng, không `ai-title`)
- [ ] `ClaudeProvider::list()`
- [ ] Fixture thật + test end-to-end qua `FakeFs`
- [ ] Test `#[ignore]` đo thời gian trên `~/.claude/projects` thật
- [ ] `cargo clippy -p agent_sessions --all-targets` sạch

## Success Criteria

1. `list()` trả đúng 292 session trên máy này (khớp `find ~/.claude/projects -name '*.jsonl' | wc -l`
   trừ đi số session không có message user).
2. Session `16819818-0732-422f-ba8d-4202c6005f37` có title `Push and continue implementation`
   (lấy từ `ai-title`), model `claude-opus-5`, cwd `/Users/tgiap.dev/devs/zode`,
   branch `feat/vscode-parity-git-panel`.
3. Một file jsonl có dòng JSON hỏng vẫn ra summary đúng, không panic.
4. File không có `ai-title` nào → title là message user đầu tiên.
5. Test đo thời gian in ra < 200ms (page cache ấm).

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| `ai-title` cuối nằm xa hơn 256KB so với cuối file | Nới đuôi theo bậc 256KB → 1MB, hết bậc thì fallback FR4 |
| Đọc đuôi cắt giữa một dòng UTF-8 nhiều byte | Bỏ đoạn trước `\n` đầu tiên trong buffer đuôi, luôn |
| File đang được ghi (session đang chạy — chính hội thoại này) | Đọc là snapshot; dòng cuối có thể chưa xong → bỏ dòng không parse được, đó là FR mặc định |
| 408MB làm test CI chậm | Test trên fixture nhỏ; test trên thư mục thật là `#[ignore]` |

## Security Considerations

- Chỉ **đọc**. Không ghi gì vào `~/.claude/` ở phase này.
- Fixture commit vào repo là transcript thật → **đọc lại trước khi commit**, cắt
  hết token/path riêng tư. Nếu không chắc, tự viết fixture tay.
- Không log nội dung message ra `log::`. Preview đi lên UI, không đi vào log file.

## Next Steps

Phase 02 (số đếm + resume + trash) và phase 05 (danh sách) đều đứng trên phase này.
Phase 04 chạy song song, không chờ.
