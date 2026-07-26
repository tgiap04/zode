# Consultation — Hard fork Zed: bỏ auth + cloud subsystems

**Date**: 2026-07-26
**Lens**: CTO (default)
**Level**: medium
**Status**: Design sealed — awaiting blueprint

---

## The Commission

Biến Zed thành một IDE fork riêng, rebrand, phát hành công khai, không gửi dữ liệu người dùng ra ngoài, và loại bỏ toàn bộ tính năng phụ thuộc tài khoản Zed.

**Ràng buộc do người ủy thác chốt:**

| Quyết định | Giá trị |
|---|---|
| Động cơ | Fork riêng / rebrand + privacy |
| Upstream sync | **Không** — hard fork, tự maintain |
| Phạm vi tính năng | Bỏ hết: AI, collab, edit prediction, voice call |
| Privacy | Không gửi dữ liệu người dùng đi; **vẫn cho phép tải về** (extension, LSP binary, update) |
| Phân phối | **Có** — phát hành công khai |
| Phương pháp thi công | **B — đại phẫu một lần** (chọn sau khi đã được cảnh báo 2 lần) |
| Kênh update | **Package manager** (Homebrew/apt/winget) — xóa hẳn `auto_update` |
| Crash reporting | **Bỏ hoàn toàn** — xóa `crates/crashes` + Sentry |
| Rebrand | **Đầy đủ**, nhưng là **phase riêng sau khi build xanh** |
| `crates/collab` | **Xóa hẳn** — nghĩa vụ AGPL biến mất |

---

## Phát hiện quyết định (từ khảo sát codebase)

### 1. `client` không xóa được — phải rút ruột

`crates/client` là dependency của **39 crates**, gồm cả `editor`, `project`, `workspace`.

Nhưng phần lớn không dùng auth — chúng dùng `client::proto`:

| Crate | Dùng gì từ `client` | Là auth? |
|---|---|---|
| `editor` | `Collaborator, ParticipantIndex, parse_zed_link` (1 dòng) | Không |
| `workspace` | `proto`, `init`, `Subscription`, `CONNECTION_TIMEOUT` | Không |
| `project` | `proto` trong `lsp_command.rs`, `lsp_store.rs`, `search.rs` | **Không** |

**`client::proto` là wire format nội bộ để serialize lệnh LSP.** Xóa `client` ⇒ phá LSP.
→ **Rút ruột phần auth, giữ `proto`/`rpc`.**

### 2. SSH remote development suýt bị xóa nhầm

`crates/remote_server` phụ thuộc `client` nhưng **không cần tài khoản Zed** — nó dùng chung tầng `proto`/`rpc`, chạy giữa máy người dùng và server của chính họ. Đây là tính năng thật, phải giữ.

### 3. `crates/telemetry` cũng không nên xóa

66 dòng macro shim, được 25+ crates gọi. Không có auth dependency. Xóa = 25 crates vỡ vô ích.
→ **Rút ruột `send_event()` thành no-op.** Auth coupling thật nằm ở `crates/client/src/telemetry.rs::set_authenticated_user_info` (client.rs:707, user.rs:842).

### 4. License là ràng buộc cứng (do đã chốt phân phối công khai)

| Crate | License | Hệ quả |
|---|---|---|
| `zed`, `editor`, `project`, `workspace`, `client` | GPL-3.0-or-later | **Bắt buộc công khai source**, giữ copyright notice |
| `collab` | AGPL-3.0-or-later | Xóa `collab` ⇒ **nghĩa vụ AGPL biến mất** (lợi ích phụ) |
| `gpui` | Apache-2.0 | Thoáng |

"Zed" là trademark → rebrand đầy đủ là **nghĩa vụ pháp lý**, không phải tùy chọn: tên binary, bundle id (`dev.zed.Zed`), icon, `crates/client/src/zed_urls.rs`.

### 5. Quy mô thật của phần bị xóa

```
agent               99,915 dòng   ← crate lớn nhất codebase
agent_ui            62,479
collab              14,998
language_models     14,089
edit_prediction     12,425
collab_ui            6,310
livekit_client       3,526
edit_prediction_ui   3,414
call                 2,964
channel              1,929
auto_update          1,544
ai_onboarding          898
notifications          689
crashes                510
cloud_api_client       470
─────────────────────────────
                  ~226,000 dòng
```

---

## Các hướng đã cân nhắc

### Hướng A — Kill-switch trước, xóa dần
Giai đoạn 1: chặn auth ở `ClientCredentialsProvider::read_credentials()` → 14 nhánh signed-out có sẵn tự kích hoạt → build xanh, dùng được ngay. Giai đoạn 2+: xóa từng crate, mỗi commit build xanh.

- Được: luôn revert được; phát hiện sớm thứ xóa nhầm; ship được bất cứ lúc nào.
- Mất: cảm giác chậm; code chết tồn tại tạm thời.
- **Đích đến giống hệt B.**

### Hướng B — Đại phẫu một lần ✅ **ĐÃ CHỌN**
Xóa 16 crates + sửa toàn bộ compile error trong một đợt.

- Được: dứt điểm, không giai đoạn nửa vời.
- Mất: build đỏ kéo dài; 4 hệ thống *dữ liệu* vỡ đồng thời (xem "Điều cần canh chừng").

### Hướng C — Rút ruột + giữ AI local
Như A nhưng giữ `language_models` + `agent` chạy Ollama/API key riêng (zero egress vẫn đạt).

- Bị bác: người ủy thác muốn bỏ hết AI.

---

## Hướng đã chốt

**Hướng B**, với các điều chỉnh bắt buộc phát sinh từ khảo sát:

1. **Rút ruột, không xóa**: `client` (giữ `proto`/`rpc`), `telemetry` (no-op `send_event`).
2. **Giữ lại**: `remote_server` (SSH remote dev), `extension_host` (được phép tải về), cơ chế tự tải LSP binary.
3. **Xóa**: collab stack, AI stack, edit prediction stack, cloud API clients, `auto_update*`, `crashes`.
4. **Rebrand**: phase riêng, chạy sau khi build đã xanh.

### Đường vá bảo mật: package manager

Cảnh báo ban đầu của tôi ("xóa `auto_update` ⇒ không còn đường phát hành security patch") **đã được giải quyết** bằng lựa chọn package manager. Homebrew cask / apt repo / winget **chính là** cơ chế phân phối patch. Hệ quả: xóa được cả 3 crate thay vì 1 —

```
auto_update           1,544 dòng
auto_update_ui           ~?
auto_update_helper       ~?
```

### Crash reporting: bỏ hoàn toàn (rủi ro đã chấp nhận)

Xóa `crates/crashes` (510 dòng) + gỡ Sentry trong `crates/zed/src/reliability.rs:295-311` và `main.rs:574-582`.

**Rủi ro đã được nêu và người ủy thác chấp nhận**: phát hành công khai + không upstream + không crash data ⇒ khi người dùng báo "nó crash", không có dữ liệu để điều tra. Chỉ còn cách tái hiện thủ công.

Dọn kèm: `script/sentry-fetch`, `.factory/prompts/crash/*`, và mục "Crash Investigation" trong `.rules` (`CLAUDE.md` là symlink tới nó) sẽ thành tài liệu chết.

### Lý do chấp nhận B dù đã khuyến cáo

Người ủy thác được trình bày rõ rằng A và B cùng đích đến, khác biệt duy nhất là checkpoint, và B không nhanh hơn. Vẫn chọn B, chấp nhận rủi ro lịch trình. Quyết định có đủ thông tin.

---

## Kết quả verify bằng `cargo metadata` (2026-07-26)

Phân tích reverse-dependency trên toàn bộ 240 crates. **Bốn giả định ban đầu đã sai.**

### Sai lầm đã chặn được

| # | Giả định ban đầu | Thực tế | Sửa thành |
|---|---|---|---|
| ① | Xóa `notifications` | `status_toast` là toast UI dùng chung — 8 crate sống sót gọi (`git_ui` 8 chỗ, `zed` 6, `project_panel` 3, `keymap_editor`, `debugger_ui`, `component_preview`, `onboarding`, `title_bar`) | **Rút ruột** `notification_store`, giữ `status_toast` |
| ② | Xóa `cloud_api_types` | `extension`/`extension_host`/`extensions_ui`/`extension_cli` cần `ExtensionMetadata`, `ExtensionProvides` — DTO của extension registry | **Giữ**, chỉ gỡ `Plan` (billing) |
| ③ | `auto_update` = chỉ tự cập nhật app | `remote_connection` gọi `AutoUpdater::download_remote_server_release()` để đẩy binary sang máy SSH | Xung đột → giải bằng bỏ SSH remote dev |
| ④ | `crashes` độc lập | `remote_server` gọi `crashes::init()` ×2 + `crash_server()`; `proto/app.proto` có `GetCrashFiles`/`CrashReport`, envelope 361/362 | Xung đột → giải bằng bỏ SSH remote dev |

### Quyết định phát sinh: bỏ SSH remote development

Xung đột ③④ buộc phải chọn. Người ủy thác chọn **bỏ SSH remote dev**.

Nhưng đo đạc cho thấy "xóa sạch mọi thứ remote" **đắt hơn** chứ không rẻ hơn:

| Phương án | Crates xóa | Sống sót phải sửa | Đụng core? |
|---|---|---|---|
| Giữ SSH remote dev | 54 | 18 | không |
| **Xóa `remote_server` + `remote_connection`, giữ `remote`** ✅ | **56** | **17** (+2 test) | **không** |
| Xóa cả `remote` | 57 | 20 (+3 test) | **có** — `project`, `workspace`, `extension_host`, `repl`, `terminal_view` |

`remote` được 16 crate dùng; `project` dùng `RemoteClient`, `RemoteClientEvent`, `ConnectionState`, `RemoteConnectionOptions`.
→ **Chốt phương án giữa.** Sau khi mất `remote_connection`, code path trong `remote` không ai construct được nữa — đã chết sẵn. Dọn nó là refactor riêng, làm khi build đã xanh.

### Con số cuối

- **Xóa: 56 crates.** Gồm 6 crate tự thành mồ côi mà seed không liệt kê: `x_ai`, `vercel`, `opencode`, `aws_http_client`, `rules_library`, `streaming_diff`, `zeta_prompt`.
- **Sống sót cần sửa code: 17 crates.** Nặng nhất:

```
zed                22 dependency vào delete set   ← nơi đau nhất
settings_ui         7
sidebar             5
title_bar           5
git_ui              4
client              2   (rút ruột)
project             1   (context_server)
workspace           1   (agent_settings)
editor              1   (edit_prediction_types)
settings_content    1   (language_model_core)   ← xác nhận cảnh báo settings schema
+ 7 crate khác, 1 dep mỗi crate
```

- **Chỉ hỏng test: 2 crates** (`git_graph`, `project_panel` → `remote_connection`)
- **Rút ruột, không xóa: 4 crates** — `client`, `telemetry`, `notifications`, `cloud_api_types`

### Đã xác nhận an toàn (không dính delete set)

`proto`, `rpc`, `telemetry`, `telemetry_events`, `audio`, `feature_flags`, `command_palette`, `denoise`, `extension_host`

---

## Điều cần canh chừng

### Nguy cơ cao nhất: 4 hệ thống *dữ liệu* tham chiếu chéo tới crate bị xóa

Đây không phải code — đây là dữ liệu, compiler **không bắt được hết**:

| Hệ thống | Vỡ thế nào |
|---|---|
| `crates/settings_content` | JSON schema chứa field `agent`, `edit_prediction`, `collaboration_panel`. Xóa crate ⇒ **`settings.json` của người dùng thành invalid** |
| `assets/keymaps/*.json` | Keybinding trỏ tới action của agent/collab đã biến mất ⇒ keymap load fail |
| `crates/zed/src/main.rs` | Toàn bộ `init()` registration |
| `command_palette` | Action registry |

**Đã verify:** `settings_content -> language_model_core` là dependency thật. Cảnh báo này được xác nhận, không phải suy đoán.

### Cạm bẫy khác

- ~~`audio` crate~~ — **đã verify an toàn**, không dính delete set.
- ~~`language_model` / `language_model_core`~~ — **đã verify**: cả hai xóa được; `settings_content` là chỗ duy nhất cần sửa.
- **`copilot` / `copilot_chat` / `copilot_ui`**: auth GitHub riêng, không phải tài khoản Zed. Nằm trong phạm vi "bỏ hết AI" ⇒ xóa. `copilot_ui` phải thêm tay vào danh sách (nó không tự thành mồ côi vì `zed` còn tham chiếu).
- **`context_server` (MCP)**: đan vào `project` qua `crates/project/src/context_server_store.rs` (~370 dòng, có cả OAuth riêng của MCP). Xóa file này + unwire khỏi `project.rs`.
- **`remote` để lại**: sau khi xóa `remote_connection`, các code path `RemoteClient` trong `project`/`workspace` thành unreachable. Không hỏng build, nhưng là nợ dọn sau.
- **`feature_flags`**: flags đến từ server qua response authenticated user (`user.rs:837-838`). Không còn auth ⇒ không còn server flags ⇒ mọi feature gate phải có default tĩnh. Có sẵn cơ chế override local (`store.rs:118-142`).
- **`client.rs:981-1002`**: `sign_in_with_optional_connect` chờ `cx.on_flags_ready(...)` — signed-out thì oneshot không bao giờ fire, task treo (vô hại nhưng nên dọn).
- **Crash visibility = 0** sau khi xóa Sentry upload. Cân nhắc giữ crash dump ghi ra đĩa để người dùng gửi thủ công.

### Đòn bẩy giảm đau cho B

1. **`cargo check --workspace` thay `cargo build`** trong vòng lặp sửa lỗi — nhanh hơn 3–5×.
2. **Xóa hết một lượt rồi mới sửa.** Vừa xóa vừa sửa ⇒ lỗi mới đè lên lỗi đang sửa.
3. Chỉ chạy `./script/clippy` ở bước cuối.

---

## Đo lường thành công

- [ ] `cargo check --workspace` sạch, `cargo build --release` thành công
- [ ] `cargo test --workspace` xanh (test nào phụ thuộc collab/AI thì xóa cùng crate)
- [ ] `./script/clippy` sạch
- [ ] **Không outbound tới `*.zed.dev`** — verify bằng network monitor khi chạy app (mở project, gõ code, cài extension)
- [ ] `settings.json` cũ vẫn load được, hoặc có migration rõ ràng
- [ ] Keymap mặc định load không lỗi
- [ ] SSH remote development vẫn hoạt động
- [ ] Extension vẫn cài được, LSP vẫn tự tải
- [ ] Source công khai kèm GPL-3.0 notice nguyên vẹn
- [ ] Không còn tham chiếu Sentry / crash upload nào

**Phase rebrand (sau khi build xanh):**

- [ ] Không còn chuỗi "Zed" trong tên binary / bundle id (`dev.zed.Zed`) / icon / URL
- [ ] `crates/client/src/zed_urls.rs` không còn trỏ zed.dev
- [ ] Homebrew cask (hoặc package manager đã chọn) cài đặt và update được

---

## Bước tiếp theo

1. Blueprint chi tiết qua `/tkm:create-plan` — cần liệt kê chính xác danh sách crate xóa (đã verify dependency edge), thứ tự xóa tối ưu, và kế hoạch cho 4 hệ thống dữ liệu.
2. Trước khi xóa: chạy `cargo tree` để verify từng crate ứng viên thật sự không còn ai dùng.
3. Tách riêng việc rebrand + tuân thủ GPL thành phase cuối (độc lập, không chặn).

---

## Câu hỏi đã giải quyết

| # | Câu hỏi | Quyết định |
|---|---|---|
| 1 | Endpoint auto-update? | Package manager (Homebrew/apt/winget) — **xóa `auto_update*`** |
| 2 | Crash dump? | **Bỏ hoàn toàn** — rủi ro mù crash đã chấp nhận |
| 3 | Rebrand? | **Đầy đủ**, phase riêng sau khi build xanh |
| 4 | `crates/collab`? | **Xóa hẳn** — git history vẫn giữ; AGPL biến mất |

## Câu hỏi còn treo

1. **Tên/brand mới là gì?** Chưa quyết — không chặn đợt đại phẫu, nhưng chặn phase rebrand.
2. **Hỗ trợ package manager nào trước?** Homebrew cask (macOS) là rẻ nhất để khởi động; apt/winget tốn hơn. Cần quyết trước lần release đầu.
