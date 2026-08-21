# Phase 05 — Nguồn Codex: `codex app-server`

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** số của Codex cạnh Claude

## Đứng cuối có chủ đích — và điều đó đã cứu phase này

Ban đầu đây là phần **duy nhất** dựa trên field chưa kiểm chứng byte nào: máy không
có `codex`, không có `~/.codex/auth.json`. Tên field lấy từ tài liệu OpenAI + issue
tracker + hai implementation độc lập.

**Sau đó người dùng cài `codex` (0.149.0) và login.** Verify được ngay, và kết quả
chia làm hai nửa trái ngược:

- **Tên field tôi dùng ĐÚNG.** Xác nhận hai lần: một lần bằng probe thật, một lần
  bằng schema chính thức do chính CLI sinh ra (`codex app-server
  generate-json-schema` → `GetAccountRateLimitsResponse` require `rateLimits`,
  `RateLimitWindow` require `usedPercent`).
- **Code tôi sẽ KHÔNG BAO GIỜ chạy.** App-server trả `-32600 "Not initialized"` cho
  mọi request trước khi `initialize` được đáp. Không tài liệu nào nói điều đó, và
  researcher cũng không biết. Nếu bạn không cài codex thì lỗi này ship thẳng ra
  production và Codex im lặng mãi mãi — đúng thứ mà cả blueprint lẫn tôi đều lo.

Yêu cầu chẩn đoán của phase này vẫn giữ nguyên, vì schema còn được đánh dấu
`[experimental]` và vẫn đang dịch chuyển.

## Cơ chế

- Spawn `codex app-server` — cùng binary mà `project::agent_server_store::locate_binary`
  đã định vị cho ACP. Không cài gì mới.
- stdio, JSON-RPC 2.0 phân cách bằng newline.
- `{"jsonrpc":"2.0","id":N,"method":"account/rateLimits/read"}`
- Response *theo tài liệu*: `rateLimits.primary` / `rateLimits.secondary`, mỗi cái có
  `usedPercent`, `windowDurationMins`, `resetsAt` (unix giây).
- Notification `account/rateLimits/updated` khi số đổi → **không cần poll Codex**.
- **Không xử lý credential.** Subprocess tự giữ và tự refresh session của nó. Editor
  không bao giờ chạm token — khác hẳn phía Claude, và là lý do chọn đường này thay vì
  `chatgpt.com/backend-api/wham/usage`.

Một lưu ý thực nghiệm phải kiểm **trước** khi giữ process sống lâu: `codex app-server`
là subprocess **thứ hai** bên cạnh process ACP đang chạy. Researcher cảnh báo hai
process có thể đua nhau khi refresh `auth.json`. Nếu có dấu hiệu đua thì đổi sang
spawn ngắn hạn theo yêu cầu, đừng giữ sống.

## Yêu cầu chẩn đoán (điểm khác biệt của phase này)

Khi handshake xong nhưng parse thất bại:

- Log ở mức `warn` **tên các JSON key** đã nhận — không phải payload. Phase 06 đổi
  điểm này: log payload nguyên văn từ một process đang giữ session đã xác thực là
  chép một thứ không ai đọc trước vào file log. Nhu cầu chẩn đoán chỉ là *tên field*,
  nên `key_paths()` trả đúng thế và có test cấm giá trị xuất hiện.
- Tooltip nói rõ một trong ba: *chưa cài codex* / *chưa login* / *đã trả lời nhưng
  không đọc được* — ba nguyên nhân, ba câu khác nhau.

Đây không phải log rác: nó là cách duy nhất biến một "không hiện gì" thành một câu
trả lời được, và là điều kiện để người dùng chấp nhận rủi ro này một cách có căn cứ.

## Việc

1. `codex.rs`: định vị binary, spawn `codex app-server`, handshake (tài liệu nói cần
   chờ ~500ms trước request đầu — kiểm lại, đừng tin ngay).
2. Gửi `account/rateLimits/read`, parse `rateLimits.{primary,secondary}` →
   `Vec<UsageWindow>` (`usedPercent` → `percent`, `resetsAt` unix giây → `resets_at`).
3. Đăng ký `account/rateLimits/updated` → cập nhật tại chỗ.
4. Ba đường thất bại, ba câu khác nhau, cộng log payload khi parse lỗi.
5. Không có binary → **không hiện gì cho Codex**, và Claude vẫn hiện bình thường.

## Files

- tạo: `crates/agent_usage/src/codex.rs`
- sửa: `crates/agent_usage/src/agent_usage.rs`, `crates/agent_usage/Cargo.toml`

## Todo

- [x] Định vị binary + spawn `app-server`, **và handshake `initialize`** — thiếu nó thì
  mọi request bị từ chối; chỉ phát hiện được nhờ probe thật
- [x] `account/rateLimits/read` + parse
- [ ] ~~Notification `updated` → cập nhật, không poll~~ — **đổi thiết kế có chủ đích.**
  Nhận push đòi giữ `app-server` sống lâu, tức một session thứ hai chạy cạnh session
  ACP mà agent panel đã mở — đúng cái researcher cảnh báo là có thể đua nhau khi
  refresh `auth.json`. Chọn spawn ngắn hạn mỗi lần đọc: tốn một process mỗi phút,
  nhưng không tạo ra cuộc đua nào.
- [x] Ba câu lỗi phân biệt được + log payload khi parse lỗi
- [x] Test: fixture là **payload thật đã ghi lại** (không phải fixture theo tài liệu),
  gồm cả `secondary: null`, `credits`, `planType`, `rateLimitsByLimitId` — những thứ
  parser phải bước qua mà không vấp
- [x] Test: reply phải khớp theo `id`, không theo thứ tự đến — server chèn
  notification (`remoteControl/status/changed`) giữa hai reply, quan sát được thật
- [x] Smoke test `#[ignore]` chạy `read_windows` thật: `0% used, resets_at
  2026-09-20` — **code Rust đã chạy end-to-end với codex thật**
- [x] Test: payload đúng JSON nhưng sai tên field → có log payload, tooltip nói "đọc không được"
- [x] Test: một agent vắng → agent kia vẫn hiện, tooltip nói lý do
  (`one_agent_being_absent_does_not_hide_the_other`) — ở tầng `Outcome`, không phải
  ở tầng `which::which` thật
- [ ] ~~Kiểm thực nghiệm: hai process codex đua `auth.json`~~ — vẫn chưa làm. Giờ có
  binary rồi nên **làm được**, nhưng cần một session ACP đang chạy song song mới dựng
  được tình huống. Đã né bằng thiết kế (spawn ngắn hạn) chứ không bằng kiểm chứng.
- [x] `cargo check --workspace` xanh

## Success criteria

Máy đã cài + login codex: số của Codex hiện cạnh Claude. Máy chưa cài: Claude vẫn
chạy nguyên vẹn, Codex im lặng nhưng tooltip nói được vì sao. Và nếu tên field sai,
log nói ra ngay thay vì để người dùng đoán.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| **Tên field sai** (rủi ro chính) | Fixture theo tài liệu + log payload thật khi lệch, để sửa được trong một vòng |
| Hai process codex đua `auth.json` | Kiểm thực nghiệm trước; có dấu hiệu thì đổi sang spawn ngắn hạn |
| Subprocess sống mãi cho một status bar | Gắn vòng đời với indicator; đo lại nếu nặng |
| Schema đổi (2 issue regression đang mở) | Lỗi → ẩn + nói lý do; không bao giờ hiện số bịa |
