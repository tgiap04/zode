# Baseline action inventory

Captured from `cargo run -p zed -- --dump-all-actions` on the green baseline.

- **1292 actions** across **88 namespaces**
- **142 actions** in namespaces that vanish entirely with their crate
- plus partial removals in 2 surviving namespaces

## Namespaces removed entirely (crate deleted)

| Namespace | Actions |
|---|---|
| `agent` | 75 |
| `agents_sidebar` | 4 |
| `assistant` | 1 |
| `auto_update` | 4 |
| `bedrock` | 2 |
| `channel_modal` | 4 |
| `collab` | 14 |
| `collab_panel` | 13 |
| `copilot` | 6 |
| `edit_prediction` | 5 |
| `inline_assistant` | 2 |
| `rules_library` | 5 |
| `zed_predict_onboarding` | 1 |
| `zeta` | 6 |
| **total** | **142** |

## Surviving namespaces with PARTIAL action removal

These are the ones a namespace-prefix sweep would miss — the namespace stays, only some actions go.

| Namespace | Actions today | What goes |
|---|---|---|
| `client` | 3 | 3 auth actions (SignIn/SignOut/Reconnect) -> Phase 5 |
| `onboarding` | 4 | SignIn + OpenAccount -> Phase 8d; Finish/ResetHints stay |

## Findings this dump corrected

1. **`bedrock` (2 actions)** — the vendor crate is in the delete set but was absent from the
   plan's namespace list. `bedrock::Tab`, `bedrock::TabPrev`.
2. **`zed_predict_onboarding` (1)** — edit-prediction onboarding action, also unlisted.
3. **`client` (3)** — the crate SURVIVES but `client::{SignIn,SignOut,Reconnect}` are declared
   at `crates/client/src/client.rs:90-98` and must be deleted in Phase 5. A crate-level view
   misses this entirely.
4. **`onboarding` (4)** — `SignIn` and `OpenAccount` go with Phase 8d; `Finish` and
   `ResetHints` stay. Another partial.
5. **`context_server::Restart` (1)** — survives, consistent with keeping the crate. But if
   Phase 8f removes `context_server_store` from `project`, re-check whether this action still
   has a handler.

## Why this differs from "403 keymap action strings"

Different units, both correct. **403** counts *keymap bindings* referencing deleted actions
across 8 files (one action can be bound many times, per-platform). **139** counts *distinct
actions* that cease to exist. Phase 9 must purge all 403 bindings; this file lists what makes
them invalid.
