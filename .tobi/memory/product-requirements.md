# Yêu cầu sản phẩm — Zode + web

Sổ ghi các yêu cầu của chủ sản phẩm, xếp theo thứ tự ghi nhận. Mỗi mục ghi trạng thái, việc cần làm
và câu hỏi còn mở. Mục nào xong thì đánh dấu và ghi commit/PR, không xoá.

Trạng thái: `chưa làm` · `đang làm` · `xong` · `huỷ`

---

## R1 — Quota relay remote control theo gói

- **Ghi nhận:** 2026-11-22
- **Trạng thái:** xong (2026-11-22, chưa commit) — plan `.tobi/outputs/plans/261122-1900-relay-quota-and-plan-mismatch-fixes/`
- **Yêu cầu:**
  - **Free:** 5 GiB mỗi ngày (UTC).
  - **Pro và Team:** không giới hạn.
- **Hiện tại:**
  - Free 2 GiB, Pro/Team 20 GiB.
  - Khi `PLAN_ENFORCEMENT=off` (mặc định), ai cũng chỉ được 2 GiB.

**Việc cần làm**

| Repo | Chỗ sửa | Thay đổi |
|---|---|---|
| web | `backend/src/config/env.schema.ts:116` | `RELAY_DAILY_BYTES` mặc định 2 GiB → 5 GiB (5_368_709_120) |
| web | `backend/src/plans/plan-limits.ts` | `relayDailyBytes` của Pro/Team thành `null` (không giới hạn), giống `syncDevices`. Bỏ hoặc ngưng dùng `RELAY_DAILY_BYTES_PRO` cùng bước kiểm `>= RELAY_DAILY_BYTES` ở `env.schema.ts:274` |
| web | `backend/src/remote/relay-server.provider.ts:137`, `remote/relay-quota.ts` | Hạn mức `null` thì không đếm và không chặn. Vẫn giữ giới hạn tốc độ mỗi socket và số host/thiết bị/phiên, vì đó là lớp bảo vệ server, không phải quota |
| web | `GET /account/plan` | `limits.relayDailyBytes` trả `null` cho Pro/Team |
| web | `frontend` UsageCard (`account.usage`) | Hiện "Không giới hạn" khi `null`. Sửa câu chữ bảng giá ("A much larger daily remote relay allowance" → "Unlimited remote relay") ở cả en và vi |
| zode | `crates/zode_account/src/plan.rs` | `relay_daily_bytes: u64` → `Option<u64>` (`null` = không giới hạn) và giữ tương thích khi server cũ trả số |
| zode | `crates/remote_control/src/remote_host.rs:85-88` | Câu "Pro raises it" → "Pro removes the limit" |
| zode | `docs/src/remote-control.md:206`, `docs/i18n/vi/remote-control.md` | Bảng giới hạn: Free 5 GiB/ngày, Pro/Team không giới hạn |
| web | `docs/content/remote-control.md:206`, terms s8 | Cập nhật con số |

**Đã chốt (2026-11-22)**
- `PLAN_ENFORCEMENT=off` → mọi người **không giới hạn**. Chỉ khi bật enforcement thì Free mới bị chặn ở 5 GiB. Sửa `PlanLimits.relayDailyBytes` (`plan-limits.ts:39`), hiện đang trả mức Free khi off.
- Pro/Team **không cần trần** chống lạm dụng. Giới hạn tốc độ mỗi socket và số host/thiết bị/phiên vẫn giữ.
- 5 GiB = 5 × 1024³ byte = 5_368_709_120.

---

## R2 — Thông báo đẩy về điện thoại/máy tính bảng khi dùng remote control

- **Ghi nhận:** 2026-11-22
- **Trạng thái:** đang làm (code + docs xong 2026-11-22, chưa commit; còn kiểm trên thiết bị thật) — brainstorm `.tobi/outputs/reports/261122-2000-brainstorm-remote-push-notifications.md`, plan `.tobi/outputs/plans/261122-2030-remote-push-notifications/`
- **Yêu cầu:**
  - Agent trên máy A **chờ duyệt quyền**, **trả lời xong** hoặc **kết thúc phiên** → thiết bị web đã ghép cặp nhận thông báo đẩy, kể cả khi trang `/remote` đã đóng.
  - Chỉ **Pro và Team**. Free giữ remote control đầy đủ và thông báo desktop.
  - Nội dung có tên tab + sự kiện, **server không đọc được** (Zode A mã hoá payload bằng khoá của trình duyệt, backend chỉ chuyển tiếp).
- **Hướng đã chọn:** Web Push + PWA trên zodekit.com. iOS cần thêm vào màn hình chính (16.4+).

**Đã chốt (2026-11-22)**
- `PLAN_ENFORCEMENT=off` → cho mọi người, giống R1.
- Bấm thông báo mở `/remote` đúng phiên; chưa có nút Duyệt trên thông báo.

---

## R3 — Panel Devices: máy ảo, điện thoại thật, chạy app

- **Ghi nhận:** 2026-11-22
- **Trạng thái:** code xong 2026-11-23, chưa commit; chờ người dùng thử trên bản dev và CI Windows — brainstorm `.tobi/outputs/reports/261122-2330-brainstorm-devices-panel-simulators-emulators.md`, plan `.tobi/outputs/plans/261122-2340-devices-panel-simulators-and-run-app/`
- **Yêu cầu:**
  - Icon điện thoại trong dải icon của right dock, cạnh project panel và agent history.
  - macOS: chọn simulator iOS Xcode đã cài. Mọi hệ điều hành: chọn AVD Android nếu có Android SDK; chọn máy thật cắm USB hoặc kết nối không dây.
  - Chạy nhiều máy cùng lúc.
  - "Run app": chạy app của project đang mở lên máy đã chọn.

**Đã chốt (2026-11-22)**
- Máy mở trong cửa sổ riêng của hệ thống (Simulator.app, Android Emulator), không nhúng vào Zode.
- Run app chạy CLI của chính framework (Flutter, Expo, React Native, Gradle, Xcode) trong terminal của Zode; Zode không tự build.
- iOS native trên iPhone thật (signing) để sau. Remote project (SSH/WSL) chưa hỗ trợ ở v1.
