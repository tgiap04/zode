# Phase 02 — Nguồn Claude: credential, fetch, parse

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** số của Claude hiện ra

## Mục tiêu

Lấy quota Claude thật và đổ vào `AgentUsage`. Đây là nửa **đã được verify** của
feature — payload trong ground-truth report là kết quả gọi thật, không suy luận.

## Bối cảnh

Đọc [`researcher-260821-claude-usage-api-ground-truth.md`](../reports/researcher-260821-claude-usage-api-ground-truth.md)
trước khi viết dòng nào. Nó có request đầy đủ, shape response thật, và luật render.

## Việc

**Credential** (`claude.rs`)

- macOS: `security find-generic-password -s "Claude Code-credentials" -w` → JSON.
- Nền khác / macOS thất bại: `~/.claude/.credentials.json`.
- Lấy `claudeAiOauth.accessToken`.
- **Trên background executor** — shell-out và đọc file đều không được chặn UI.
- Token **không** được cache, không log, không ghi ra đâu. Đọc mới mỗi lần fetch.

**Điều kiện hiện (bỏ qua sớm, trước cả khi đọc credential)**

- Có bất kỳ biến `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` /
  `ANTHROPIC_API_KEY` → **không hiện gì**. Người dùng đang trỏ vào endpoint khác,
  quota gói đăng ký vô nghĩa.
- `claudeAiOauth.subscriptionType` là `free`/`none` và `rateLimitTier` không khớp
  `claude|max|pro|team|enterprise` → không hiện.

**Fetch + parse**

- `cx.http_client()` → `http::Request::get` với 3 header:
  `Authorization: Bearer …`, `anthropic-beta: oauth-2025-04-20`, `Accept: application/json`.
- Parse `limits[]` → `Vec<UsageWindow>`:
  - `percent` → `percent`
  - `resets_at` → `resets_at` (**nullable**, `weekly_scoped` chứng minh điều đó)
  - `scope.model.display_name` → `label`
- **KHÔNG filter theo `is_active`** — xem lý do trong plan.md và report.
- Bỏ hẳn `spend` và `extra_usage`: là tiền, không ai yêu cầu.
- Lỗi bất kỳ (401, timeout, JSON lạ) → `None`, không hiện, **không** hiện số cũ.

## Files

- tạo: `crates/agent_usage/src/claude.rs`
- sửa: `crates/agent_usage/src/agent_usage.rs`

## Todo

- [x] Đọc credential hai đường, trên background executor
- [x] Điều kiện hiện: env override + subscription
- [x] Fetch + parse `limits[]` → `Vec<UsageWindow>`
- [x] Test: parse payload thật (fixture chép từ report) ra **ba** cửa sổ
- [x] Test: fixture chỉ có `is_active:false` vẫn ra đủ cửa sổ (chống bẫy filter)
- [x] Test: `resets_at: null` + có `scope.model` → `label` là `Some("Fable")`
- [x] Test: `runtime_override_present` nhận đúng cả ba biến, và coi giá trị rỗng là
  không-override
- [ ] ~~Test: override thì **không đọc** credential~~ — chỉ kiểm predicate, chưa kiểm
  thứ tự gọi. `fetch` có kiểm override trước khi đọc credential (đọc code thấy vậy),
  nhưng chưa có test nào ghim thứ tự đó
- [x] Test: JSON rác / thiếu `limits` → `None`, không panic
- [x] `cargo check -p agent_usage` xanh

## Success criteria

Máy có Claude Code đã login: thanh status hiện đúng ba con số khớp với những gì
`claude` tự báo. Máy không có credential: **không hiện gì**, không lỗi, không log ồn.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Filter `is_active` → mất hai mục | Test riêng với fixture toàn `is_active:false` |
| Token lọt vào log | Không có đường nào in nó; review diff tìm mọi `{:?}` trên struct chứa token |
| Chặn UI khi shell-out keychain | Toàn bộ trên background executor |
| Endpoint đổi (không có tài liệu) | Lỗi → ẩn, không bao giờ hiện số cũ như số mới |
