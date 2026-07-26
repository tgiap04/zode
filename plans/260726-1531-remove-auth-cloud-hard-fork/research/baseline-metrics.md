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
