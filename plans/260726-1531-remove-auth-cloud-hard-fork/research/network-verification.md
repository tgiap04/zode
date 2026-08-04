# Network verification — Phase 11, Gate B

## 11b step 9 — static sweep for surviving endpoints

```sh
rg -n 'zed\.dev|api\.zed\.dev|collab\.zed|livekit|sentry|MINIDUMP' crates/ --type rust
```

The naive grep returns ~90 hits. Nearly all are **not** egress paths — they're `cx.open_url(...)`
browser links (user-click-through to documentation, e.g. "See Docs" buttons, feature-upsell
banners, the account/upgrade links in `zed_urls.rs`), test fixtures (`hover_links.rs`,
`editor_tests.rs`, `file_finder_tests.rs`), and doc comments. A hyperlink the user must
deliberately click to open in their own browser is not the app "phoning home" — it's exactly the
same category as a README link.

After filtering those out, the real findings:

### Confirmed, expected survivors (per the plan's own prediction)

1. **`crates/client/src/zed_urls.rs`** — builds URLs (account/upgrade/terms/docs) from the
   configured `server_url` setting. Only one live caller remains anywhere in the workspace:
   `zed_urls::acp_registry_blog(cx)` in `extensions_ui.rs:1464`, used to build a "View Registry"
   **button link** (opens in browser on click). No background fetch.
2. **`crates/http_client/src/http_client.rs` `build_zed_api_url`** — reached from
   `assets/settings/default.json`'s `server_url` and consumed by `extension_host.rs` (extension
   registry queries and downloads). This is the one **disclosed, intentional** background-network
   path this fork keeps — extension install/update and LSP-server auto-download need it. It is
   *not* telemetry, crash reporting, or an LLM/collab endpoint.

### A third finding not anticipated by the plan — flagged, not fixed here

3. **`crates/context_server/src/oauth.rs` `CIMD_URL`** = `"https://zed.dev/oauth/client-metadata.json"`.
   This is **not** a URL this app fetches. Per the OAuth "Client ID Metadata Document" (CIMD)
   extension, it's the **client identifier** this fork presents to *third-party* MCP servers when
   authenticating via OAuth — those servers may themselves dereference the URL to read this
   client's registered metadata. No outbound request originates from this codebase to that URL;
   `rg` confirms it's used only as a string constant and equality-compared in tests.
   Functionally harmless (MCP/context-server OAuth still works correctly against any MCP server
   that supports CIMD), but it means this fork currently **identifies itself as "zed.dev"** to
   external OAuth servers, which is a branding/disclosure question for Phase 12's rebrand, not a
   privacy leak. Recommend Phase 12 either points this at a fork-owned CIMD document or drops CIMD
   support in favor of dynamic client registration (`registration_endpoint`, already supported as
   a fallback per `determine_registration_strategy`).

### Dead code that looked like a survivor but isn't reachable

- `crates/ui/src/components/collab/collab_notification.rs`'s storybook string
  `"zed-cloud, zed, edit-prediction-bench, zed.dev"` — `CollabNotification` has zero callers
  outside its own file; it only appears in that component's own preview/storybook examples.
- `crates/editor/src/editor.rs:30001`'s `cx.open_url(".../edit-predictions-missing-keybinding")` —
  a click-through docs link on a keybinding-conflict banner, part of `editor`'s pre-existing (zero
  changes across this fork, confirmed in Phase 10) inline-completion-acceptance UI — unrelated to
  the deleted AI/Zeta edit-prediction product.

## 11b steps 10–14 — runtime verification: NOT executed in this session

Steps 10 (`lsof` snapshot), 11 (`nettop` continuous log), 12 (the two-tier `/etc/hosts` blackhole
test), and 13 (a one-time Little Snitch/LuLu observed session) all require:

- launching the actual release GUI binary and driving it through a manual QA pass (open a
  project, edit, save, use LSP completions, open a terminal, run git operations, install an
  extension, open settings) — genuine human interaction with a window on the user's own desktop,
  not something this session can respond to;
- editing `/etc/hosts` twice under `sudo` (a shared system file, and the tiered test explicitly
  requires two *separate* runs so Tier 2's expected extension-registry failure doesn't mask a
  Tier 1 pass) — a shared-system-state change that warrants the user's explicit go-ahead before
  each edit, per this fork's "confirm before shared-state changes" discipline;
- `sudo nettop` — requires a password prompt this session cannot answer;
- Little Snitch/LuLu — third-party applications; not confirmed installed in this environment, and
  running one is inherently an interactive, GUI-driven step.

**What is verified, and stands independently of the runtime pass:** the static sweep above proves
there is exactly one intentional background-egress code path compiled into the binary
(`build_zed_api_url`, gated to the extension registry and LSP downloads), and zero telemetry/
crash/LLM/collab egress code paths exist at all (confirmed in Phase 6 — no Sentry SDK ever
existed, `MINIDUMP_ENDPOINT` and the event-upload path are deleted, `telemetry::send_event` is a
no-op). The runtime pass would *observe* this, not establish it independently — static analysis
of a codebase with zero collab/telemetry/crash networking code is sufficient to know no such
traffic can occur, short of a bug in a dependency making an undocumented call of its own (which
the runtime pass, and specifically Little Snitch, is the layer designed to catch).

**Recommended recipe for the user to run** (reproducible for every future release):

```sh
# Tier 1 — must be silent
echo "127.0.0.1 collab.zed.dev telemetry.zed.dev api.anthropic.com api.openai.com" | sudo tee -a /etc/hosts
# launch the release binary, do a full manual QA pass, confirm zero connection attempts
# then revert /etc/hosts

# Tier 2 — separate run, must degrade gracefully (extensions fail to list, nothing else breaks)
echo "127.0.0.1 api.zed.dev" | sudo tee -a /etc/hosts
# launch, try to install/update an extension, confirm no crash/hang/retry storm
# then revert /etc/hosts

# Continuous log during either pass:
sudo nettop -p $(pgrep -x zed | head -1) -J bytes_in,bytes_out -x -L 0
```

## 11c — functional regression pass: NOT executed in this session

Same constraint as above — extension install, LSP auto-download, SSH remote development, hang-trace
writing, legacy-config loading, and cycling all 7 base keymaps against the **release** build all
require driving the actual GUI application interactively. This is handed to the user (or a future
session with GUI-driving tooling) rather than claimed as done.
