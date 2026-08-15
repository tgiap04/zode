# Phase 00 — Khôi phục 4 crate ACP

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 4-6d *(thực tế: một buổi)* · **Blocked by:** —

Lấy lại tầng protocol từ chính history của fork và làm nó compile trên cây code hôm nay. **Không UI, không tính năng nhìn thấy được** — phase này chỉ dựng nền và trả lời bằng compiler câu hỏi "drift thật là bao nhiêu".

Chạy song song được với phase 01 (01 chỉ chạm `crates/project` + assets).

## Key insights

- Nguồn là `c3e2ac3^` — commit ngay trước `refactor!: remove auth, collab, AI and cloud subsystems (54 crates)`. Cùng version gpui tại điểm cắt, nên đây **không** phải port từ upstream main.
- Drift đã đo (bảng trong plan.md): `gpui`, `editor`, `language`, `multi_buffer`, `buffer_diff`, `prompt_store` — **0 commit** kể từ điểm cắt. Đây là phần lớn bề mặt API mà 4 crate này chạm.
- `action_log` (3.434 dòng) có **toàn bộ dependency đã tồn tại**: `anyhow buffer_diff log clock collections fs futures gpui language project telemetry text util watch`. Không thiếu gì → khôi phục sạch nhất, làm đầu tiên.
- `acp_thread` tham chiếu `language_model` đúng **2 lần**; `agent_servers` cũng **2 lần**. Gỡ được, không kéo `language_models` (14k dòng).
- **Không khôi phục `acp_tools`** (902 dòng, viewer log protocol) — YAGNI. Phải gỡ nó khỏi `[dependencies]` của `agent_servers`.
- **Không khôi phục `google_ai`** — chỉ phục vụ Gemini, ngoài scope hai agent đã chốt. Gỡ khỏi `agent_servers`.
- Mọi external crate cần đến đã có trong workspace `Cargo.toml`: `portable-pty` (:569), `urlencoding` (:685), `indoc` (:502), `env_logger` (:477), `tempfile` (:632), `thiserror` (:633), `itertools` (:504), `base64` (:435).
- `agent_settings` (1.734 dòng) mang cả `agent_profile.rs` + 2 prompt tóm tắt thread của agent **native** — phần đó không phục vụ ACP. Đây là R3.

## Requirements

**Functional**
1. Bốn crate `action_log`, `agent_settings`, `acp_thread`, `agent_servers` build được trong workspace.
2. `agent-client-protocol = "=0.11.1"` (`Cargo.toml:399`) chuyển từ "pin không ai dùng" sang có consumer thật.
3. `agent_settings` chỉ còn key mà ACP thực sự đọc — không key model/provider của agent native.
4. Test có sẵn của `acp_thread` và `action_log` chạy và xanh.

**Non-functional:** mỗi crate một commit riêng; `./script/clippy` xanh giữa từng commit; không đụng crate nào đang chạy ngoài việc thêm member vào workspace.

## Architecture

```
action_log ──────────┐
                     ├──> acp_thread ──> agent_servers
agent_settings ──────┘         │              │
(đã trim)                      │              └── (đã gỡ: acp_tools, google_ai, language_model)
                               └── (đã gỡ: language_model)
```

Thứ tự bắt buộc là thứ tự mũi tên. `agent_servers` cuối vì nó phụ thuộc mọi thứ còn lại.

## Related code files

**Tạo (khôi phục từ `c3e2ac3^`):**
- `crates/action_log/` — nguyên vẹn, không sửa gì ngoài đường dẫn `[lib]` nếu cần
- `crates/agent_settings/` — **trim**: bỏ `agent_profile.rs` + `src/prompts/*` nếu không consumer nào của ACP đọc; bỏ dep `language_model`
- `crates/acp_thread/` — bỏ dep `language_model` (2 ref)
- `crates/agent_servers/` — bỏ dep `acp_tools`, `google_ai`, `language_model`; bỏ `e2e_tests.rs` nếu nó gọi agent thật qua mạng

**Sửa:**
- `Cargo.toml` (root) — thêm 4 crate vào `[workspace.members]` và `[workspace.dependencies]`

**Xoá:** không

## Implementation steps

1. `git checkout c3e2ac3^ -- crates/action_log` → thêm vào workspace members + dependencies → `./script/clippy -p action_log` → **commit**.
2. `git checkout c3e2ac3^ -- crates/agent_settings` → gỡ dep `language_model`, sửa các chỗ dùng nó; đọc `agent_settings.rs` và cắt mọi field không phục vụ ACP (profile, model, provider). Ghi lại **đúng những key còn giữ** vào phase note. → clippy → **commit**.
3. `git checkout c3e2ac3^ -- crates/acp_thread` → gỡ 2 ref `language_model` → clippy + `cargo test -p acp_thread` → **commit**.
4. `git checkout c3e2ac3^ -- crates/agent_servers` → gỡ dep `acp_tools`/`google_ai`/`language_model`; xoá nhánh Gemini nếu nó dính `google_ai`; xem `custom.rs` (nơi có chuỗi `@zed-industries`) và cập nhật sang org mới `@agentclientprotocol` nếu có hardcode → clippy → **commit**.
5. Chạy `./script/clippy` trên **toàn workspace** — bắt các va chạm tên/feature ngoài 4 crate.
6. Ghi vào phase note: mỗi chỗ phải sửa vì drift, kèm API cũ → API mới. Đây là dữ liệu đầu vào để ước lượng lại phase 04.

## Todo

- [x] `action_log` build + clippy xanh — **28/28 test**
- [ ] ~~`agent_settings` đã trim~~ — **hoãn sang phase 04, có chủ ý.** Xem điều #1
- [x] `acp_thread` build, **48/48 test** xanh
- [x] `agent_servers` build, **14/14 test** xanh; đã gỡ `google_ai` + `language_model` + `e2e_tests`
- [x] ~~gỡ `acp_tools`~~ → **đảo quyết định: khôi phục nó.** Xem điều #2
- [x] `cargo check --workspace --all-targets` exit 0 · `./script/clippy` xanh
- [x] Bảng drift thật (dưới)

## Kết quả (2026-08-12)

**16.544 dòng khôi phục. Tổng cộng ~13 dòng phải thích nghi.**

| Crate | Dòng | Khác bản gốc `c3e2ac3^` | Test |
|---|---|---|---|
| `action_log` | 3.434 | **0 / 0** | 28/28 |
| `acp_thread` | 8.009 | +7 / −13 | 48/48 |
| `acp_tools` | 902 | **0 / 0** | 0 (không có test) |
| `agent_servers` | 4.199 | +12 / −528 | 14/14 |

`−528` gần như trọn vẹn là `e2e_tests.rs` (500 dòng) bị xoá. Trừ nó ra, bốn crate cộng lại phải sửa **đúng 13 dòng**.

**Toàn bộ drift, liệt kê hết:**

| Chỗ | Nguyên nhân | Xử lý |
|---|---|---|
| `acp_thread/connection.rs` — `AuthRequired.provider_id` | `language_model::LanguageModelProviderId`, chỉ để định tuyến auth về LLM provider native của Zed | Xoá field + builder + import. Agent ngoài tự lo auth trong CLI |
| `acp_thread/mention.rs:307` — `IconName::Thread` | Fork đã xoá 35 icon cùng stack AI | Khôi phục `assets/icons/thread.svg` + variant. **Kiểm luôn: phase 04 cũng chỉ thiếu đúng icon này** |
| `agent_servers/custom.rs` — `api_key_for_gemini_cli` | `language_model::{ApiKey, EnvVar}` + `google_ai::API_URL`, chỉ phục vụ Gemini | Xoá hàm + call site. Gemini vẫn chạy nếu người dùng tự export `GEMINI_API_KEY` |
| `agent_servers/agent_servers.rs` — `pub mod e2e_tests` | Gọi agent thật qua mạng | Xoá file. **Bài học: xoá dòng `mod` mà bỏ lại `#[cfg]` của nó thì attribute rơi xuống `use` kế tiếp** — đúng lỗi tôi tự gây ra rồi tự sửa |

Ước lượng 4-6 ngày dựa trên "drift thấp nhưng có thật". Thực tế **drift gần bằng không** — điều kiện dừng (vượt 6d ⇒ ước lượng lại 04–06) không kích hoạt, và ước lượng 8-12d của phase 04 giờ có cơ sở vững hơn nhiều.

## Ba điều phase này dạy lại cho plan

**1. `agent_settings` không thuộc phase này — nó không có consumer ở đây.**
Đã kiểm `Cargo.toml` của cả ba crate ở `c3e2ac3^`: `acp_thread`, `agent_servers`, `action_log` đều **không** khai `agent_settings`. Consumer duy nhất trong phạm vi port là `agent_ui` (phase 04). Trim nó ở đây là trim mù — không compiler nào dẫn đường được vì chưa có ai gọi. Rủi ro R3 vì thế **chuyển sang phase 04**, nơi trim đúng nghĩa "chỉ giữ cái có consumer".

**2. Đảo quyết định "YAGNI, bỏ `acp_tools`".**
Quyết định ban đầu đưa ra khi chưa biết `acp.rs` gọi `log_tap` ở **9 chỗ**. Mổ 9 chỗ trong code vừa port để bỏ một facility đang chạy tốt là trả giá để **mất** giá trị — ngược hẳn YAGNI. `acp_tools` 902 dòng, không thiếu dep nào, compile sạch lần đầu, và phase 05 sẽ cần đúng nó khi agent cư xử lạ. Khôi phục là lựa chọn rẻ hơn về mọi mặt.

**3. Bẫy feature unification: `remote/test-support` và `workspace/test-support` là hai công tắc khác nhau.**
`RemoteConnectionIdentity::Mock` tồn tại khi **`remote/test-support`** bật (qua `project/test-support`), nhưng arm xử lý nó trong `workspace/src/persistence.rs:1647` lại gate bằng **`workspace/test-support`**. Bật cái trước mà không bật cái sau ⇒ `workspace` (lib) hỏng vì match không vét cạn. Bẫy này **có sẵn trong fork**, `acp_tools` chỉ là crate đầu tiên giẫm phải (`agent_servers` → `acp_tools` → `workspace`).

Đã theo quy ước sẵn có của repo — `sidebar`, `terminal_view`, `title_bar` đều khai cả hai — bằng cách thêm `workspace = { features = ["test-support"] }` vào dev-deps của `agent_servers`, kèm comment giải thích.

> **Đề xuất cho `.rules`:** ràng buộc "test graph chạm `remote/test-support` thì phải bật cả `workspace/test-support`" hiện không được ghi ở đâu và chỉ được giữ nhờ may mắn. Đây đúng dạng "trap to avoid" mà `.rules` gốc repo yêu cầu: non-obvious, đã gặp thật, và sửa được bằng một dòng cụ thể.

## Success criteria

`cargo build` toàn workspace xanh · `./script/clippy --deny warnings` exit 0 · test của `acp_thread`/`action_log` xanh · `agent_settings` không xuất key nào của agent native ra JSON schema.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R2 | Drift lớn hơn đo được | **Nếu phase này vượt 6d → dừng, báo lại, ước lượng lại 04–06.** Không âm thầm chạy tiếp |
| R3 | Trim `agent_settings` hụt tay, mất key ACP thật cần | Trim theo hướng "chỉ giữ cái có consumer trong `acp_thread`/`agent_servers`", kiểm bằng compiler chứ không bằng mắt |
| — | `e2e_tests.rs` của `agent_servers` gọi agent thật | Loại khỏi build mặc định; test thật thuộc phase 05 |
