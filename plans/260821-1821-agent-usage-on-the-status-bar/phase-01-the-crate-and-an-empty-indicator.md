# Phase 01 — Crate mới và một chỉ báo còn rỗng

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** không gì cả — cố ý

## Mục tiêu

Dựng khung: crate mới, kiểu dữ liệu chung cho cả hai nguồn, một `StatusItemView`
được vẽ thật nhưng chưa có dữ liệu nên không chiếm chỗ. Mọi rủi ro của phase 02 và
05 đứng sau một bước đã xanh.

## Vì sao là crate riêng

Repo này **mỗi status item một crate**: `activity_indicator`, `encoding_selector`,
`language_selector`, `line_ending_selector`, `toolchain_selector`, `go_to_line`,
`image_viewer`. Nhất quán 100%, nên `crates/agent_usage/` là đúng lối — và nó
**không** cần depend `agent_ui` (chỉ cần `ui`, `gpui`, `workspace`, `http_client`,
`fs`, `project` để định vị binary codex ở phase 05).

## Kiểu dữ liệu chung

Hai nguồn có hình dạng khác nhau hoàn toàn (HTTP JSON vs. JSON-RPC qua stdio), nên
chúng phải gặp nhau ở một kiểu do **chúng ta** định nghĩa, không phải ở kiểu của
bên nào:

```rust
/// Một cửa sổ quota, đã chuẩn hoá từ bất kỳ nguồn nào.
pub struct UsageWindow {
    /// Phần trăm đã dùng, 0..=100. Số nguyên: Anthropic đã trả số nguyên ở
    /// `limits[].percent`, và một float với thang đo nhập nhằng là thứ phải đoán.
    pub percent: u8,
    /// Khi nào reset, nếu cửa sổ này có mốc reset. `weekly_scoped` không có.
    pub resets_at: Option<DateTime<Utc>>,
    /// Nhãn thay cho đếm ngược khi không có `resets_at` — tên model, ví dụ "Fable".
    pub label: Option<SharedString>,
}

pub struct AgentUsage {
    pub agent: AgentId,
    pub windows: Vec<UsageWindow>,
}
```

`Vec` chứ không phải ba field có tên: số cửa sổ phụ thuộc tài khoản, và phase 05 sẽ
đổ hai cửa sổ của Codex vào cùng kiểu này.

## Việc

1. `crates/agent_usage/` + `Cargo.toml`, thêm vào workspace members.
2. `UsageWindow` / `AgentUsage` như trên.
3. `AgentUsageIndicator`: `Render` + `impl StatusItemView`.
   - `set_active_pane_item` là **no-op** có chú thích: usage không phụ thuộc item
     đang mở, khác mọi status item khác trong repo.
   - `render` trả `div()` rỗng khi `usage` trống — không chiếm chỗ, không viền.
4. Đăng ký ở `zed.rs` bằng `add_left_item`, **ngay sau** `activity_indicator`.

## Files

- tạo: `crates/agent_usage/{Cargo.toml, src/agent_usage.rs}`
- sửa: `Cargo.toml` (workspace members + dependency), `crates/zed/Cargo.toml`
- sửa: `crates/zed/src/zed.rs`

## Todo

- [x] Crate mới vào workspace, `cargo check -p agent_usage` xanh
- [x] `UsageWindow` — và `AgentUsage` đã bị **xoá** ở phase 05: `SourceState` thay chỗ nó,
  còn lại một `pub struct` không ai dùng (clippy im vì `pub`) nên bỏ hẳn
- [x] `AgentUsageIndicator` + `StatusItemView`, render rỗng khi không có dữ liệu
- [x] Đăng ký `add_left_item` cạnh activity_indicator
- [x] Test: item rỗng không có gì để hiện (`an_indicator_with_no_data_shows_nothing`,
  `an_agent_with_no_windows_is_still_nothing_to_show`)
- [ ] ~~Test: vẽ thật không panic~~ — chỉ kiểm `has_anything_to_show`, **không** gọi
  `cx.draw`. Nhớ bài học cũ: `cx.draw` không publish frame nên một smoke test kiểu đó
  chứng minh rất ít; muốn đo layout thật thì phải để cửa sổ tự vẽ
- [x] `cargo check --workspace` xanh

## Success criteria

Thanh status **trông y như trước**. Không thêm khoảng trống, không thêm viền. Item
tồn tại trong `status_bar` và vẽ được.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Item rỗng vẫn chiếm chỗ / vẽ viền | Test đo: không có dữ liệu thì không có gì được vẽ |
| Crate mới kéo theo dependency vòng | `agent_usage` không depend `agent_ui`; chỉ `zed` biết cả hai |
