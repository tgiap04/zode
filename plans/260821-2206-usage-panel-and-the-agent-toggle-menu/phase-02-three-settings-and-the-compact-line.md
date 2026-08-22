# Phase 02 — Ba setting, và dòng Compact

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** Compact rút dòng lại; tắt được từng agent

## Việc

Ba setting mới trong khối `status_bar` đã có sẵn:

| Key | Kiểu | Default | Ai đọc |
|---|---|---|---|
| `claude_usage_button` | `bool` | `true` | thanh status, menu phải |
| `codex_usage_button` | `bool` | `true` | thanh status, menu phải |
| `agent_usage_display` | `"detailed" \| "compact"` | `"detailed"` | thanh status, toggle panel |

Gom cả ba vào một phase vì chúng chạm cùng 4 file. Tách ra là sửa `default.json` ba lần.

**Luật Compact:** cửa sổ *căng nhất* = `percent` cao nhất; bằng nhau thì mốc reset gần
nhất; vẫn bằng thì cái đầu. Là hàm thuần, nhận `&[UsageWindow]` + `now`, test được mà
không cần window.

## Related code files

- `crates/settings_content/src/workspace.rs` — `StatusBarSettingsContent` + enum `AgentUsageDisplay` (theo khuôn `EncodingDisplayOptions` ngay dưới nó)
- `crates/workspace/src/workspace_settings.rs` — `StatusBarSettings` 3 field + `from_settings`
- `assets/settings/default.json` — khối `status_bar`, 3 key + comment
- `crates/agent_usage/src/agent_usage.rs` — `render` đọc setting; `most_constrained()`

## Todo

- [x] `AgentUsageDisplay { Detailed, Compact }`, `#[serde(rename_all = "snake_case")]`
- [x] 3 field vào `StatusBarSettingsContent` với doc-comment `Default:` như các field cạnh
- [x] 3 field vào `StatusBarSettings` + `from_settings`
- [x] **3 key vào `assets/settings/default.json`** — cùng commit, không để sau
- [x] `most_constrained(&[UsageWindow], now) -> Option<&UsageWindow>`
- [x] `render` lọc source theo `*_usage_button`
- [x] `render` chọn Detailed (tất cả) vs Compact (một)
- [x] Test: `most_constrained` — percent cao thắng; bằng percent thì reset **xa hơn** thắng
      (blueprint viết "gần hơn" và đó là chỗ tôi viết sai: hai cửa sổ đầy bằng nhau thì cái
      giải phóng sớm hơn là cái *ít* cấp bách, nên nó phải thua)
- [x] Test: Compact trên fixture 3 cửa sổ ra đúng một cửa sổ
- [x] Test: tắt Claude → chỉ còn nhóm Codex; tắt cả hai → `has_anything_to_show()` false
- [x] `cargo check -p zode` (settings đụng nhiều crate)

## Bẫy

- **`from_settings` gọi `.unwrap()` từng field.** Thiếu key trong `default.json` là panic
  lúc mở app, không phải fallback. Đây là bẫy đã ghi lại và đã trả giá một lần.
- **`StatusBarSettings` đã có `#[derive(RegisterSetting)]`.** Đây là thêm *field*, không
  phải kiểu setting mới — đừng thêm lời gọi `register` nào ở đâu.
- Tắt cả hai agent thì `render` trả `div()` trơn, tức **không còn gì để chuột phải vào**.
  Chấp nhận, ghi vào docs ở phase 06.

## Success criteria

Đặt `"agent_usage_display": "compact"` trong settings → thanh status còn một cửa sổ, và
vẫn thế sau khi khởi động lại. `"codex_usage_button": false` → phần Codex biến mất, phần
Claude không dịch chỗ sai.
