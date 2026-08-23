# Phase 03 — Provider Codex trên sqlite

## Context Links

- Phase 02: `phase-02-claude-counts-resume-and-trash.md`
- Nguồn: `~/.codex/state_5.sqlite`, bảng `threads`
- `Cargo.toml:540` — `rusqlite = { version = "0.32", features = ["bundled", "column_decltype"] }`
  đã là workspace dependency

## Overview

- **Priority:** P2
- **Status:** **done** (23/08) — chặn đã mở, người dùng chạy `codex` ngày 23/08
- **Phụ thuộc:** 02 (trait phải đủ hình dạng) + một row dữ liệu thật

## Chặn đường (đọc trước khi bắt đầu)

`select count(*) from threads` = **0** và `~/.codex/sessions/` rỗng tại thời điểm lập
kế hoạch. Quyết định ở buổi consultation: **không viết provider mù**. Trước phase này:

```
codex        # nói một câu, thoát
sqlite3 ~/.codex/state_5.sqlite "select id, title, cwd, git_branch, model, created_at, created_at_ms, rollout_path from threads limit 3;"
```

Nếu vẫn 0 row → dừng phase, báo lại. Đừng đoán schema.

## Key Insights

- Schema (đã đọc) có sẵn gần hết những gì UI cần: `id, title, cwd, git_branch, model,
  preview, first_user_message, created_at, updated_at, created_at_ms, updated_at_ms,
  tokens_used, archived, rollout_path, cli_version`.
- **Tên file mang số version schema**: `state_5.sqlite`. Codex bump schema là `state_6`.
  Hardcode tên là chết im lặng.
- Có **hai** cặp cột thời gian: `created_at`/`updated_at` (giây) và `*_ms`. Trigger
  trong DB tự lấp `_ms` từ giây khi insert. Không giả định cột nào có giá trị.
- Không có cột nào là số message. `tokens_used` có. Nghĩa là `counts().messages` cho
  Codex phải hoặc đọc `rollout_path`, hoặc trả `None`.
- Mở read-only một DB đang WAL mà `-wal` cần recovery sẽ **lỗi** ("unable to open
  database file") vì recovery cần quyền ghi. Đây là bẫy thật, không phải lý thuyết.

## Requirements

**Functional**

- FR12 — Tìm DB: glob `~/.codex/state_*.sqlite`, chọn suffix số **lớn nhất**. Không có
  file nào → provider báo *unavailable*, không phải `Err`.
- FR13 — Mở `OpenFlags::SQLITE_OPEN_READ_ONLY`. Lỗi vì WAL → copy `state_N.sqlite`,
  `-wal`, `-shm` sang thư mục tạm rồi mở bản copy. Copy thất bại → *unavailable*.
- FR14 — Thiếu bảng `threads`, hoặc thiếu cột bắt buộc → *unavailable* kèm một dòng
  `log::warn!` nói rõ cột nào. Không panic, không `unwrap`.
- FR15 — `list()`: `select … from threads where archived = 0 and has_user_event = 1
  order by coalesce(recency_at_ms, updated_at_ms, updated_at*1000) desc`.
- FR16 — `counts()` trả `messages: None` cho Codex (UI ẩn cột đó cho hàng Codex thay vì
  hiện `0` — số 0 là một lời nói dối).
- FR17 — `resume_command()` → `codex resume <id>`, cwd = `cwd` của thread.
  `Fork::New` không có tương đương → trả `None`, phase 06 disable nút đó cho Codex.
- FR18 — `delete()` cho Codex: **không xoá row trong DB của Codex.** Đưa file ở
  `rollout_path` vào Trash nếu có, và báo cho UI rằng row DB không xoá được từ đây.

## Architecture

`SessionCounts.messages` đổi từ `usize` thành `Option<usize>` — đây là thay đổi kiểu
lan sang phase 02 và 05, làm ở đây và sửa cả hai chỗ dùng.

Provider giữ một `Availability { Ready, Unavailable(reason) }` đọc được từ UI, để panel
hiện được "Codex: không tìm thấy lịch sử" thay vì im lặng bỏ qua nửa yêu cầu.

`rusqlite` dùng trực tiếp, **không** qua `sqlez`: sqlez xây quanh Domain + migration của
chính app này, còn đây là DB của tiến trình khác mà ta không sở hữu và không được ghi.

## Related Code Files

**Tạo mới**
- `crates/agent_sessions/src/codex.rs`
- `crates/agent_sessions/src/codex_db.rs` — chọn file, mở, và **map row → summary** thuần

**Sửa**
- `crates/agent_sessions/Cargo.toml` — thêm `rusqlite`
- `crates/agent_sessions/src/summary.rs` — `messages: Option<usize>`, `Availability`
- `crates/agent_sessions/src/claude.rs` — theo kiểu mới của `messages`

## Implementation Steps

1. Xác nhận có row thật (mục "Chặn đường"). Chép schema thật vào test fixture bằng
   `sqlite3 … .dump threads`.
2. `codex_db.rs`: chọn file theo FR12 + test với thư mục tạm chứa `state_2/state_10`
   (kiểm `10 > 2` chứ không so chuỗi).
3. Mở read-only + nhánh copy-tạm cho WAL. Test: tạo DB, bật WAL, ghi rồi mở read-only.
4. `list()` với query FR15, test trên fixture dump.
5. `Availability`: test ba trường hợp — không file, file không có bảng, file thiếu cột.
6. `resume_command` + `delete` theo FR17/FR18.
7. Đổi `messages` sang `Option`, sửa Claude + mọi call site cho xanh.

## Todo List

- [x] Có ≥ 1 row Codex thật, dump ra fixture
- [x] Chọn `state_*.sqlite` theo suffix số, test 2 vs 10
- [x] Mở read-only, nhánh copy-tạm cho WAL, có test
- [x] `list()` + test trên fixture
- [x] `Availability` + 3 test xuống thang
- [x] `resume_command` (Fork::New → None), `delete` theo rollout_path
- [x] `messages: Option<usize>` lan sang Claude + call site
- [x] clippy sạch, `cargo test -p agent_sessions` xanh

## Success Criteria

1. Session Codex thật hiện trong `list()` với title, cwd, branch, model đúng như
   `sqlite3` in ra.
2. Đổi tên `state_5.sqlite` thành `state_6.sqlite` → vẫn tìm thấy, vẫn hoạt động.
3. Xoá hết `~/.codex/state_*.sqlite` → `Availability::Unavailable`, `list()` rỗng,
   **không** `Err`, panel vẫn chạy với Claude.
4. `drop table threads` trên bản copy → `Unavailable` kèm warn nêu tên bảng.
5. Codex đang chạy (DB đang WAL) mà panel vẫn đọc được danh sách.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Không có dữ liệu để đối chiếu | Phase bị **chặn** ngay từ đầu file này. Không viết mù. |
| Schema đổi ở bản Codex sau | `Availability::Unavailable` + warn. Panel mất một agent, không mất chính nó. |
| Read-only trên WAL cần recovery | Nhánh copy sang thư mục tạm (FR13), có test |
| `created_at` giây vs `_ms` lệch nhau | `coalesce` theo thứ tự ms → giây×1000, không giả định |
| Ta ghi vào DB của Codex | Read-only ở tầng `OpenFlags`, không phải ở tầng quy ước |

## Security Considerations

- Mở read-only là ràng buộc kỹ thuật, không phải lời hứa: cờ `SQLITE_OPEN_READ_ONLY`
  làm mọi `INSERT`/`UPDATE` fail tại tầng sqlite.
- Bản copy tạm chứa transcript của người dùng → tạo trong thư mục tạm của app, xoá
  ngay sau khi đọc, không để lại.
- `rollout_path` từ DB là dữ liệu ngoài: kiểm nó nằm dưới `~/.codex/` trước khi đưa
  vào Trash. Path bất kỳ từ một DB bị sửa không được biến thành lệnh xoá.

## Next Steps

Phase 06 mở rộng menu sang Codex: `Fork::New` disable, `messages` ẩn.

## Ghi chú khi làm xong

- Fixture là schema **viết lại bằng tay** theo `.schema threads` thật, không phải `.dump` — dump chứa hội thoại thật của người dùng.
- Phát hiện quyết định: `has_user_event = 0` trên thread người dùng đã gõ vào, nên FR15 (`where has_user_event = 1`) bị **bỏ**. Nếu viết mù, Codex sẽ ẩn sạch session thật.
- `messages` cho Codex **không** phải `None` như plan dự: `rollout_path` có thật, đếm được từ đó (3 message trên thread thật).
- Nhánh copy-tạm cho WAL chưa có test riêng — chưa dựng được một DB mà read-only open thật sự fail; nhánh đó chỉ có `log::warn` + đường code, chưa có bằng chứng chạy.
