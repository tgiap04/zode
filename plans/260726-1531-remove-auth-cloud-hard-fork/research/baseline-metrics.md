# Baseline metrics — 2026-07-26 16:44

Branch: fork/remove-auth-and-cloud
Base commit: 7ac6829 Add dependency analysis scripts and output for hard fork removal

## Crate counts (three methods — they differ, red team finding 17)

| Method | Count |
|---|---|
| `crates/` directories | 232 |
| workspace members in Cargo.toml | 239 |
| cargo metadata packages | 240 |

Non-`crates/` members: extensions/glsl extensions/html extensions/proto extensions/test-extension tooling/compliance tooling/perf tooling/xtask 

Expected after Phase 4: 179 crate dirs / 186 members / 187 packages

## Test baseline

`cargo nextest list --workspace` → **5,674 tests** (names in `baseline-test-names.txt`).

`cargo nextest run --workspace --no-fail-fast` → **pre-existing failures, environmental**.

Every failure is in `collab::collab_tests db_tests::*` with a `_postgres` suffix — they require a
live Postgres instance that is not running locally (`compose.yml` provides one). They are **not**
caused by any change in this branch.

**Consequence for Phase 10:** `collab` is in the delete set, so these tests vanish at Phase 4. When
Phase 10 diffs test names, every one of them lands in bucket **(a) — disappeared with its crate**.
Do not read their absence as a regression, and do not read their failure now as a broken baseline.

To reproduce the failures deliberately (or make them pass): `docker compose up -d` per `compose.yml`.
