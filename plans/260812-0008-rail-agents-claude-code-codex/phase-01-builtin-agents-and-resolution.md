# Phase 01 — Built-in agent + resolve binary

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 2-3d · **Blocked by:** —

Làm cho `AgentServerStore` — thứ **đã đang chạy** trong `Project` — biết đến Claude Code và Codex, và resolve được cả hai đường: binary local cho terminal mode, npx adapter cho chat UI. Chạy song song được với phase 00.

## Key insights

- **Tầng resolve đã có sẵn, chỉ thiếu một mắt xích.** `AgentRegistryStore::init_global` chỉ được gọi ở `crates/remote_server/src/headless_project.rs:233`. App desktop không gọi ⇒ `AgentRegistryStore::try_global(cx)` tại `agent_server_store.rs:433`/`:647`/`:864` trả `None` ⇒ mọi registry agent im lặng không resolve. Đây là **phần lớn** công việc của phase này.
- Nhánh `CustomAgentServerSettings::Registry` đã dựng server qua `node_runtime` + `registry_id` (`agent_server_store.rs:551`, `:572`), có `refresh_if_stale` (`:437`) và `cx.observe` registry để rebuild (`:648`). **Không viết resolve mới.**
- `assets/settings/default.json:2337` hiện là `"agent_servers": {}`.
- Registry thật (đã fetch, version `1.0.0`): `claude-acp` → npx `@agentclientprotocol/claude-agent-acp@0.66.0`; `codex-acp` → npx `@agentclientprotocol/codex-acp@1.1.14`. **Đổi org rồi** — không còn `@zed-industries/*`.
- **Binary local không nói ACP.** `claude` v2.1.227 chỉ có `--output-format=stream-json`, `mcp`, `agents`. Nên hai đường resolve là **hai thứ khác nhau**, không phải fallback của nhau:
  - terminal mode → binary local (`claude`, `codex`), mang auth/config/MCP của người dùng
  - chat UI → npx adapter qua `node_runtime`
- `AgentServerStoreState::Local` đã giữ `project_environment` (`agent_server_store.rs:150-156`) — đúng thứ cần cho R1. Máy dev có `claude` ở `~/.local/bin/claude`, ngoài PATH của app GUI.
- `AgentServerCommand { path, args, env }` (`:33-38`) là kiểu trả về đã có, không cần kiểu mới cho lệnh.

## Requirements

**Functional**
1. `AgentRegistryStore::init_global` được gọi trong app desktop, cùng `fs` + `http_client` như headless đang làm.
2. Hai entry mặc định `claude-acp` và `codex-acp` xuất hiện trong `AgentServerStore::external_agents` mà người dùng không phải cấu hình gì.
3. Version npx **ghim** trong default settings — mất mạng vẫn resolve được; registry chỉ dùng để nâng version (R8).
4. Có API resolve binary local cho terminal mode, đi qua `ProjectEnvironment`, **không** dùng env của process.
5. Binary thiếu → lỗi **có kiểu** mang đủ dữ liệu để UI dựng empty state: agent nào, lệnh cài theo OS, link docs. Không phải `anyhow` chuỗi.
6. Người dùng override được cả hai entry qua settings `agent_servers` (giữ nguyên cơ chế Custom đã có).

**Non-functional:** không request mạng nào ở đường resolve binary local; registry refresh không chặn UI.

## Architecture

```
AgentServerStore (đã sống trong Project)
├── external_agents: { "claude-acp", "codex-acp", …user override }
│
├── đường ACP (chat UI)      → Registry entry → node_runtime → npx @agentclientprotocol/…@ghim
└── đường binary (terminal)  → ProjectEnvironment.get_directory_environment(worktree)
                              → which_in("claude"|"codex", env.PATH, cwd)
                              → Ok(PathBuf) | Err(AgentBinaryMissing{ agent, install, docs })
```

Lỗi có kiểu:

```rust
pub struct AgentBinaryMissing {
    pub agent: AgentId,
    pub binary: &'static str,           // "claude" | "codex"
    pub install_command: String,        // theo OS, xem phase 03
    pub docs_url: &'static str,
}
```

## Related code files

**Sửa:**
- `crates/project/src/agent_server_store.rs` — thêm hằng hai built-in agent; thêm hàm resolve binary local + `AgentBinaryMissing`; export qua `project.rs`
- `crates/project/src/project.rs` — re-export kiểu lỗi mới (cạnh `:46-47`)
- `crates/zed/src/main.rs` (hoặc nơi khởi tạo global tương ứng) — gọi `AgentRegistryStore::init_global(cx, fs, http_client)`, đối chiếu `headless_project.rs:233`
- `assets/settings/default.json:2337` — `"agent_servers"` mang hai entry `Registry` với version ghim

**Tạo:** không · **Xoá:** không

## Implementation steps

1. Đọc `headless_project.rs:233` để lấy đúng chữ ký `init_global`; gọi tương ứng trong app desktop. Kiểm bằng log rằng `try_global` không còn `None`.
2. Điền `assets/settings/default.json` hai entry `claude-acp` + `codex-acp` dạng `Registry`, version ghim theo registry đã fetch.
3. Kiểm `AgentServerStore::external_agents()` liệt kê đúng hai id sau khi settings nạp — **test**, không phải nhìn log.
4. Viết `resolve_local_binary(agent, worktree, cx) -> Task<Result<PathBuf, AgentBinaryMissing>>`: lấy env qua `project_environment`, tra bằng `which_in` với `PATH` **của env đó**.
5. Bảng lệnh cài theo OS (nội dung ở phase 03) đặt cạnh định nghĩa built-in agent để một chỗ duy nhất giữ sự thật.
6. Test R1: `FakeFs` + env giả có `~/.local/bin` **không** nằm trong `PATH` của process → khẳng định vẫn resolve ra binary. Đây là test quan trọng nhất của phase.
7. Test: binary vắng → trả `AgentBinaryMissing` đúng agent, đúng lệnh theo OS hiện tại.
8. Test: settings người dùng khai `claude-acp` dạng `Custom` → override thắng built-in.

## Todo

- [ ] ~~`AgentRegistryStore::init_global` được gọi trong app desktop~~ — **KHÔNG làm, có chủ ý.** Xem "Điều phase này dạy lại cho plan" #1
- [x] Hai agent mặc định, version **ghim** — nhưng trong **code**, không phải `default.json`. Lý do như trên
- [x] `external_agents()` liệt kê đúng 2 id — `builtin_agents_register_without_the_registry`
- [x] `resolve_agent_binary` đi qua `ProjectEnvironment`
- [x] `AgentBinaryMissing` có kiểu, export ra ngoài `project`
- [x] **Test R1**: binary ngoài PATH process vẫn tìm thấy — `locates_a_binary_that_the_process_path_cannot_see`
- [x] Test: shell env không có PATH → fallback, không kết luận "chưa cài"
- [x] Test: settings override thắng built-in — `user_settings_override_a_builtin_agent`

## Kết quả (2026-08-12)

`./script/clippy -p project` exit 0 · **13/13** test `agent_server_store` xanh · diff **324 thêm / 0 xoá** trong `agent_server_store.rs` (thuần bổ sung, không reformat code cũ) + 5/−1 trong `project.rs`.

| File | Δ |
|---|---|
| `crates/project/src/agent_server_store.rs` | +324 |
| `crates/project/src/project.rs` | +5 / −1 |

`default.json` **không đổi** — `"agent_servers": {}` giữ nguyên, người dùng vẫn override được.

## Ba điều phase này dạy lại cho plan

**1. Ship agent dạng `Registry` trong `default.json` là đặt một HTTPS request lên đường cold-start của mọi user.**
`AgentRegistryStore::init_global` (`agent_registry_store.rs:137-141`) gọi `refresh()` bất cứ khi nào cache rỗng, và `refresh_if_stale` (`:248-252`) cũng fetch ở lần đầu vì `last_refresh: None`. Còn `agent_server_store.rs:434-438` kích `refresh_if_stale` mỗi lần settings đổi **nếu settings có registry agent**. Ship 2 entry `Registry` mặc định ⇒ `has_registry_agents()` luôn true ⇒ mọi user mở app đều gọi `cdn.agentclientprotocol.com`, kể cả người không bao giờ đụng agent. Trên một fork tự nhận "privacy-first IDE" thì đó là hai lỗi cùng lúc: chậm và phone-home.

**Đã làm thay:** `BUILTIN_AGENTS` là bảng hằng trong code, mỗi agent mang `package_name` + `version` ghim, dựng thẳng `LocalRegistryNpxAgent` (cùng struct, cùng đường npx, cùng `node_runtime`) mà **không** tra registry. Zero network lúc khởi động. Registry vẫn dùng được cho ai khai tay trong settings — nó chỉ không còn nằm trên đường mặc định. Thêm variant `ExternalAgentSource::Builtin` để nguồn gốc không bị nói dối là `Registry`.

**Hệ quả cho phase 06:** muốn có "có version mới" thì phải wire `init_global` **lazily** — lúc người dùng mở agent view, không phải lúc app khởi động.

**2. Test build luôn trả environment rỗng, nên không inject PATH qua `ProjectEnvironment` được.**
`get_cli_environment()` (`environment.rs:71-73`) trả `Some(HashMap::default())` dưới `cfg(any(test, feature = "test-support"))`, và `local_directory_environment` short-circuit ngay ở đó. Vì vậy phần tra cứu được tách thành hàm thuần `locate_binary(binary, search_path)`, và bài R1 dùng **chính test binary** làm "CLI nằm ngoài PATH": tìm thấy khi truyền thư mục của nó, **không** tìm thấy khi để `None`. Hai nửa mới là bài test — nửa đầu một mình chứng minh được rất ít.

**3. Shell env không có `PATH` thì phải fallback, không được kết luận "chưa cài".**
Một login shell lỗi mà bị dịch thành "bạn chưa cài Claude Code" là câu trả lời tệ hơn nhiều so với việc tìm trong `PATH` hẹp hơn. `locate_binary` fallback sang `std::env::var("PATH")` khi env nạp về rỗng. Lỗi cần vá là "**chỉ** đọc PATH của process", không phải "không bao giờ đọc".

## Success criteria

Không cần UI: một test khẳng định store liệt kê 2 agent, một test khẳng định resolve thấy binary nằm ngoài PATH của process, một test khẳng định lỗi thiếu binary mang đủ 4 trường. `./script/clippy` xanh.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| R1 | Resolve nhầm sang PATH của process | Test số 6 là bài kiểm chính của phase. Không merge nếu thiếu nó |
| R5/R8 | Registry là dependency mạng + npx tải lúc runtime | Version ghim trong settings; registry chỉ nâng version. Việc **chạy** npx thuộc phase 03 và phải qua click tường minh |
| — | `init_global` cần `http_client` chưa sẵn ở điểm khởi tạo | Đối chiếu `headless_project.rs:233` để lấy đúng thứ tự khởi tạo |
