# Usage của agent trên thanh status

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Branch:** feat.release-v0.1.1

## Yêu cầu

Hiện quota đã dùng của Claude và Codex trên thanh dưới cùng của IDE, theo mẫu:

```
✳ ▬▬  50% used 1h 17m · 10% used 6d 12h · 0% used Fable   ⟳
```

## Hai nguồn, và chúng lệch nhau đúng chỗ không ai đoán được

| | Claude | Codex |
|---|---|---|
| Cơ chế | `GET api.anthropic.com/api/oauth/usage` | spawn `codex app-server`, JSON-RPC |
| Tài liệu công khai | **không có** | **có**, OpenAI mời tích hợp bên thứ ba |
| Credential | zode phải tự đọc token | **không** — subprocess tự giữ |
| Payload | **đã probe thật, HTTP 200** | **chưa verify byte nào** |
| Độ ổn định | đọc như đã settle | đang stabilize, 2 issue regression mở |

Cái *có* tài liệu là cái không verify được; cái *không* có tài liệu thì đã gọi thật
và thấy đúng payload. Ground truth phía Claude:
[`researcher-260821-claude-usage-api-ground-truth.md`](../reports/researcher-260821-claude-usage-api-ground-truth.md).

Người dùng đã nghe rủi ro và chọn làm cả hai ngay. Nên phase 05 mang một yêu cầu
mà các phase khác không có: **thất bại phải chẩn đoán được**, không im lặng.

## Quyết định đã chốt

| Câu hỏi | Chốt |
|---|---|
| Nguồn Claude | zode tự đọc token + tự gọi API (không phụ thuộc cache của kit) |
| Nguồn Codex | `codex app-server` → `account/rateLimits/read` + notification `updated` |
| Vị trí | **trái**, cạnh `activity_indicator` |
| Làm mới | poll ~60s **chỉ khi cửa sổ đang focus** + bấm để refresh ngay |
| Codex chưa verify | làm theo tài liệu, nhưng log payload thật khi parse lỗi |

## Đọc `limits[]`, không đọc `five_hour`/`seven_day`

Response mang hai cách biểu diễn song song. Kit takumi đọc cặp field có tên; nhưng
`limits[]` mang **nhiều hơn hẳn**, và đó là chỗ mục thứ ba trong ảnh đến từ:

```json
{"kind":"weekly_scoped","percent":0,"resets_at":null,
 "scope":{"model":{"display_name":"Fable"}},"is_active":false}
```

Chữ "Fable" là `scope.model.display_name`. Thêm nữa `percent` là số nguyên, còn
`utilization` là float mà thang đo nhập nhằng — kit phải mang riêng một hàm
`normalizeUtilization` để đoán giữa 0–1 và 0–100. Và response còn chứa
`amber_ladder`, `cinder_cove`, `nimbus_quill`, `tangelo`… — codename của những thứ
chưa công khai. Đọc `limits[]` là không phải gọi tên cái nào trong số đó.

**`is_active` KHÔNG phải cờ "hiện cái này".** Chỉ `session` là `true`, mà ảnh hiện
cả ba. Nó đánh dấu cửa sổ nào đang chặn. Filter theo nó là hiện một mục ở chỗ đáng
ra ba — và một test viết từ cùng giả định sai đó sẽ xanh.

## Phase

| # | Việc | Người dùng thấy gì |
|---|------|--------------------|
| [01](phase-01-the-crate-and-an-empty-indicator.md) | Crate `agent_usage`, kiểu chung, item rỗng | **không gì cả** — cố ý |
| [02](phase-02-the-claude-source.md) | Credential + fetch + parse `limits[]` | Số của Claude hiện ra |
| [03](phase-03-rendering-that-matches-the-reference.md) | `% used` + đếm ngược / tên model | Đúng mẫu trong ảnh |
| [04](phase-04-polling-and-click-to-refresh.md) | Poll khi focus + bấm refresh | Số luôn mới, ⟳ bấm được |
| [05](phase-05-the-codex-source.md) | `codex app-server` JSON-RPC | Số của Codex cạnh Claude |
| [06](phase-06-finish-the-feature.md) | Full suite + docs | — |

Phase 01 vô hình có chủ đích: khung tồn tại, được vẽ, chưa có dữ liệu — nên mọi rủi
ro của 02/05 đứng sau một bước đã xanh. Phase 05 đứng **cuối** vì nó là phần duy
nhất dựa trên field chưa kiểm chứng; sai thì bốn phase trước vẫn còn nguyên giá trị.

## Chỗ phải cẩn thận

- **Token là secret.** Đọc một lần cho mỗi lần fetch, **không cache token** (cache
  *kết quả*), không log, không ghi ra đâu. `expiresAt` là thật — token hết hạn, và
  refresh nó là việc của Claude Code chứ không phải của editor: hai process cùng
  refresh một credential là đua nhau. Gặp 401 thì coi như "chưa có dữ liệu".
- **Không gọi mạng khi người dùng không nhìn.** Poll chỉ khi cửa sổ focus.
- **Không hiện `spend`/`extra_usage`.** Đó là tiền, không ai yêu cầu, và là phần
  nhạy cảm nhất của payload.
- **Mọi thứ nặng phải ở background executor.** Đọc keychain là shell-out, fetch là
  mạng. Crate này (`agent_ui`/`workspace`) đã trả giá nhiều lần cho việc với vào
  entity giữa update — status item mới không được thêm một lần nữa.
- **`codex app-server` là subprocess thứ hai** bên cạnh process ACP đang chạy.
  Researcher cảnh báo có thể đua nhau khi refresh `auth.json`. Phải kiểm thực nghiệm
  trước khi giữ nó sống lâu.
- **Hai agent là nhiều số.** Ba mục của Claude cộng hai của Codex là năm — thanh
  status sẽ dài. Mỗi agent mang icon của nó đứng đầu (ảnh cũng vậy), để biết số nào
  của ai; không có icon thì năm con số dính nhau là vô nghĩa.

## Định nghĩa xong

Thanh dưới hiện `✳ 53% 1h17m · 10% 6d12h · 0% Fable` cho Claude, và số của Codex
cạnh đó với icon của nó. Bấm ⟳ thì cập nhật ngay. Không có credential / chưa login /
có `ANTHROPIC_BASE_URL` → phần đó **biến mất** thay vì hiện số sai. Cửa sổ không
focus thì không có request nào đi ra.

---

## Xong (2026-08-21)

Crate mới `agent_usage` (~700 dòng), **31 test**. Tổng suite: 424 xanh.

### Phase 06 tìm ra hai lỗi thật, cả hai do tôi gây ra

**1. Indicator làm IO thật ngay trong `new()` → 30 test của `zode` đỏ.**
Backtrace chỉ `blocking::Unblock<std::fs::File>::poll_read` — tức đọc filesystem thật.
Mỗi test dựng workspace đều kéo theo shell-out keychain, đọc home dir, gọi HTTP và
spawn subprocess, và test scheduler tất định báo đó là non-determinism.

Sửa bằng feature `test-support` (pattern repo đã dùng rộng): trong build test
`may_read_usage()` trả `false` và vòng poll không khởi động. Gate theo *feature* chứ
không phải `cfg(test)` — `cfg(test)` chỉ bật cho crate đang được test, nên nó sẽ không
với tới test của `zode`.

Cái giá của bản sửa được ghi vào phase 04: đường poll/click không còn chạy trong unit
test của crate này. Logic thật thì có test (`apply`, hai parser, luật render); phần
mất là 10 dòng timer.

**2. Log payload nguyên văn từ một process đang giữ session đã xác thực.**
Chính tôi thêm log đó cho yêu cầu chẩn đoán của phase 05 — nhưng chép nguyên payload
mà không ai đọc trước vào file log là sai. Nhu cầu chẩn đoán chỉ là *tên field*, nên
đổi sang `key_paths()` trả các key path, và có test cấm bất kỳ giá trị nào xuất hiện.

### `doc-writer` bắt được một mâu thuẫn tôi đã tạo ra

`docs/src/telemetry.md` liệt kê mọi outbound request của build này và chốt: *"These
are downloads and connections you initiate. None of them carry usage data."*
Feature này là **request tự động đầu tiên** — poll 60s, không do người dùng bấm — nên
câu đó thành sai ngay lúc code vào. Trang đó giờ có mục riêng nói rõ nó gửi gì.

Đây là loại hệ quả không nằm trong bất kỳ todo nào của plan.

### Đổi thiết kế so với blueprint, có lý do

Blueprint chốt dùng notification `account/rateLimits/updated` để không phải poll
Codex. Bỏ: nhận push đòi giữ `app-server` sống lâu, tức session thứ hai cạnh session
ACP mà agent panel đã mở — đúng cuộc đua `auth.json` researcher cảnh báo. Chọn spawn
ngắn hạn mỗi lần đọc: một process mỗi phút, nhưng không có cuộc đua nào.

### Codex verify được sau khi người dùng cài CLI — và nó cứu feature

Ban đầu phía Codex là phần duy nhất chưa kiểm chứng byte nào. Người dùng cài
`codex-cli 0.149.0` và login, và verify cho hai kết quả trái ngược:

- **Tên field đúng.** Xác nhận hai lần — probe thật, và schema chính thức do chính
  CLI sinh (`codex app-server generate-json-schema`).
- **Code sẽ không bao giờ chạy.** App-server trả `-32600 "Not initialized"` cho mọi
  request trước khi `initialize` được đáp. Không tài liệu nào nói, researcher cũng
  không biết. Không cài codex thì lỗi này ship thẳng, Codex im lặng mãi mãi.

Đã sửa: gửi `initialize` trước, khớp reply theo `id` (server chèn notification giữa
hai reply — quan sát được thật), fixture đổi sang **payload thật**, và thêm smoke
test `#[ignore]` chạy `read_windows` thật. Kết quả: `0% used, resets_at 2026-09-20`.

Bài học đắt hơn cả bug: `smol::future::or(work, timer)` không kiểm được dưới test
executor của gpui vì timer chạy trên clock ảo và luôn thắng. Tách `read_windows` ra
khỏi `fetch` để timeout là *policy* và đọc là *việc* — và việc thì test được. Thêm
nữa, `#[gpui::test]` cấm parking, nên smoke test phải là `#[test]` thường trên
executor riêng.

### Còn chưa kiểm được

- **Cuộc đua `auth.json`** giữa `app-server` và session ACP — giờ có binary nên làm
  được, nhưng cần một session ACP chạy song song. Né bằng thiết kế (spawn ngắn hạn).
- **Đường poll/click** — xem phase 04.
### Một lỗi test có sẵn, đã định danh — không phải "flake"

`multi_workspace_tests::test_hibernate_after_ms_zero_disables_hibernation`
(`multi_workspace_tests.rs:1529`, assert `project.activity() == Warm`).

Nó xuất hiện 3 lần trong ~25 lần chạy full suite nên trông như flake. Không phải —
nó **phụ thuộc thứ tự chạy**:

| cách chạy | kết quả |
|---|---|
| riêng lẻ, cây hiện tại | **đỏ 3/3** |
| riêng lẻ, baseline `f2b53d3` (trước mọi thay đổi của tôi) | **đỏ ngay lần đầu** |
| trong full suite | xanh ~22/25 |

Tức nó xanh *nhờ* test khác chạy trước — có state dùng chung ở đâu đó — và đỏ khi
không được nhận cái đó. Đã dựng worktree ở `f2b53d3` để đối chiếu, và tôi chưa từng
chạm `multi_workspace_tests.rs` (`git diff` rỗng cả trong commit lẫn working tree).

**Có sẵn từ trước, ngoài phạm vi feature này, không sửa.** Nhưng "order-dependent"
là chẩn đoán tìm được, còn "flaky" thì không — nên ghi lại bằng tên thật. Liên quan
tới bẫy đã biết: `Project::activity()` có thể nói một đằng còn tầng resource làm một
nẻo khi barrier hoãn lại.
