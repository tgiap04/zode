# Research: Removing 56 crates from a 240-crate workspace, big-bang, red-period-minimization

Date: 2026-07-26
Scope: zode (Zed hard-fork), Cargo.toml at repo root, 240 workspace members.

## Summary

Big-bang removal of 56 crates is dominated by one cost: **compiler-error round-trips**, not
build wall-clock. The single highest-leverage move is computing a **reverse-topological
deletion order** up front (via `cargo depgraph`, already vendored as `script/crate-dep-graph`)
and fixing survivors leaf-first — this converts an unbounded trial-and-error loop into a
bounded, predictable one. Second highest-leverage: **`cargo check -p <crate>` in that same
order**, not `cargo check --workspace`, because workspace-wide checks re-surface downstream
errors you haven't reached yet and waste wall-clock recompiling crates you already fixed.
`sccache` is already wired into this repo's CI but NOT local dev — worth enabling locally only
if rebuild churn is high; `mold` is not a serious option on macOS (Linux-only maturity), the
existing `.cargo/config.toml` already uses `lld` only for aarch64-linux and leaves macOS on
Apple's own (fast, native) linker, which is correct and should not be changed. For verifying
no dead references, `cargo machete` (already a repo convention via `script/clippy:15` and
`Cargo.toml:1007` metadata) plus `cargo tree -i` plus ripgrep is sufficient — no need for
`cargo udeps` (nightly-only, slower, and the false-negative profile isn't the failure mode
here — you want confirmation of *absence*, not more sensitivity). For the network-silence
goal, no single tool proves a negative; the practical recipe is layered: static grep for
telemetry/API endpoints + `cargo tree -i` for network crates, then runtime `lsof -i` snapshots
+ a `/etc/hosts` blackhole redirecting known Zed cloud domains to `127.0.0.1` + a long-running
`nettop -L 0` log, cross-checked once with Little Snitch in silent-notify mode as the
authoritative outside-observer. For gutting `crates/client` and `crates/telemetry`, the
minimum-churn pattern given this is a **hard fork with no upstream-compat requirement** is:
**delete the network/auth code and narrow the public API to match actual call sites**, not
"keep the signature, stub the body" — stubbing preserves dead API surface forever and violates
YAGNI; but for the ~2-3 call sites that are structurally awkward to remove (e.g. deeply
threaded `Arc<Client>` handles used for non-auth purposes like RPC message routing), keep a
thin no-op struct only where deletion would cascade into 10+ files, and let the compiler (not
manual grep) enumerate exactly which callers break.

## 1. Fast error iteration on a huge workspace

**`cargo check -p <crate>` beats `cargo check --workspace` for this task**, provably: when you
delete 56 crates and 39+ crates reference the deleted ones, `cargo check --workspace` compiles
the *entire dependency closure* of every member every time you fix one error, including crates
you haven't started fixing yet and whose errors are just noise until their turn comes.
`cargo check -p <crate>` only requires that crate's dependency subtree be checked, and because
`profile.dev` in this repo already sets `incremental = true` (`Cargo.toml:884`), each
successive `-p` invocation is fast once its deps are green. `cargo build` is strictly worse
than `cargo check` for this phase — it invokes codegen, which is the expensive part removal-era
errors don't need; only run `cargo build`/`cargo test` once `cargo check --workspace` is green,
as a final gate.

- `cargo check --message-format=short` truncates diagnostic bodies but keeps file:line — good
  for a first triage pass to count/bucket errors by crate. Prefer plain human output
  (`cargo check -p X`) once you're actively fixing a single crate; `short` loses the "help:"
  suggestions rustc gives, which matter more than screen real estate during a 56-crate purge.
- `--message-format=json` is the right choice for **automated bucketing**: pipe
  `cargo check --workspace --message-format=json 2>/dev/null | jq -r 'select(.reason=="compiler-message") | .target.name'` to get a
  frequency table of which crates have how many errors, before you touch any code. This gives
  you the actual blast-radius map instead of trusting the theoretical dependency graph alone
  (some "downstream" crates may turn out to have zero real errors because they only used a type
  alias that survives). [Cargo Book: cargo check](https://doc.rust-lang.org/cargo/commands/cargo-check.html);
  the JSON message format is stable for the fields that matter here (target name, message,
  spans) per cargo issue discussion — new fields get added but existing ones aren't removed
  ([rust-lang/cargo#8283](https://github.com/rust-lang/cargo/issues/8283)).
- **Grouping errors by crate in dependency order** is exactly what `cargo check -p <crate1> ; cargo check -p <crate2> ; ...`
  run in reverse-topological order gives you for free — no extra tooling needed. Don't reach
  for a fancier LSP-based grouping tool; the shell loop is simpler (KISS) and cargo already
  stops emitting noise for crates whose deps aren't green yet if you check them in the right
  order.
- **What's already configured** (`/Users/tgiap.dev/devs/zode/.cargo/config.toml:1-21`):
  - `rustflags = ["-C", "symbol-mangling-version=v0", "--cfg", "tokio_unstable"]` — better
    backtraces, irrelevant to error-loop speed.
  - `lld` is only forced for `aarch64-unknown-linux-gnu` (`.cargo/config.toml:19-20`); macOS
    gets no linker override, i.e. it uses Apple's `ld64`/`ld-prime`. **Do not introduce mold on
    macOS** — mold's macOS support is still explicitly "alpha"/experimental as of the project's
    own tracking issue, and Apple's stock linker has closed much of the historical speed gap,
    which is reportedly part of why the commercial mold fork for macOS (`sold`) has struggled to
    justify itself ([rui314/mold#1171](https://github.com/rui314/mold/issues/1171),
    [Phoronix](https://www.phoronix.com/news/Mold-1.4.1-Linker)). Given this is a check-only
    (not link-heavy) workflow anyway, linker choice barely matters for `cargo check`.
  - `sccache` exists in this repo but **only in CI** (`.github/workflows/release.yml:37-47`,
    `run_unit_evals.yml:44-63`, `run_cron_unit_evals.yml`), via `script/setup-sccache`, with
    `SCCACHE_BUCKET=sccache-zed`. It is **not** wired into local dev by default. For a
    single-machine big-bang removal session, enabling sccache locally has low payoff — sccache
    helps most on cache hits across clean builds/CI runners, not on the incremental
    edit-check-edit loop you'll be running, where `CARGO_INCREMENTAL=1` (the local default,
    since `.cargo/config.toml` doesn't override it and CI-only workflows set
    `CARGO_INCREMENTAL: '0'` — `.github/workflows/randomized_tests.yml:14` etc.) already gives
    you fast recompiles of the crate you're actively editing. Leave sccache alone for this task.
  - `cargo-nextest` isn't mentioned anywhere in `.cargo/config.toml` or `script/`; it's a test
    runner, irrelevant until the final `cargo test` gate after checks are green.
  - **`script/cargo`** (`/Users/tgiap.dev/devs/zode/script/cargo:1-323`) is a Node.js wrapper
    that auto-appends `--timings` to `build`/`check`/`run`/`test` and uploads timing data for
    staff — cosmetic to this task, safe to keep using or bypass with plain `cargo`.
  - **`script/clippy`** (`/Users/tgiap.dev/devs/zode/script/clippy:1-23`) already runs
    `cargo machete` and `typos` locally when available (lines 14-16) — confirms `cargo machete`
    is this repo's established convention for dead-dependency detection (see §4). It defaults
    to `--workspace` unless you pass `-p`/`--package` (`script/clippy:5-7`), and runs
    `--release --all-targets --all-features -- --deny warnings` (line 10) — **do not run this
    mid-purge**; it's a release-profile full-feature build meant for the final gate, not the
    error-iteration loop. Save it for after `cargo check --workspace` is green.
  - **`script/crate-dep-graph`** (`/Users/tgiap.dev/devs/zode/script/crate-dep-graph:1-19`)
    already wraps `cargo depgraph --workspace-only --offline --root=zed,cli,collab
    --dedup-transitive-deps | dot -Tsvg`. This is the tool to use for §2 below — it's already
    a first-class citizen of this repo, no new tool to introduce.

## 2. Mechanical removal order theory

**Reverse-topological (leaves-first) beats "delete all then fix survivors" for this task.**
Argument:

- The DAG has two disjoint-ish regions after removal: (a) the 56 doomed crates and their
  private dependents that are *entirely* inside the removal set, and (b) ~37-39 survivor
  crates that have an edge INTO the removal set (they `use` a symbol from `client`,
  `telemetry`, `cloud_llm_client`, etc.) but are otherwise healthy.
- "Delete all then fix survivors": you get one giant unstructured error dump across ~39
  crates simultaneously, with errors compounding (a survivor crate `A` failing because its own
  dependency `B` — also a survivor — hasn't been fixed yet produces a second, spurious round of
  errors in `A` after you fix `B`). This is worse because **you cannot tell which errors in `A`
  are "real" (A itself needs a code change) vs "transitive" (A will go green automatically once
  B compiles)** without doing the topological walk anyway — so skipping it just means
  rediscovering it ad hoc, with more total `cargo check --workspace` runs.
- Reverse-topological, leaves-first: compute the survivor subgraph's topological order (via
  `cargo depgraph` or `cargo metadata | jq` if you want the raw JSON), then walk it root-to-leaf
  in dependency order — i.e., fix the crates with **zero unfixed survivor dependencies first**
  (typically low-level crates like `proto`, `rpc`, `settings_content` if they touch the removed
  set), then their direct dependents, and so on up to `crates/zed` last. Each crate is
  `cargo check -p`'d exactly once green, ever (barring your own mistakes). This is provably the
  minimum number of "real" fix rounds: it's the same argument as topological sort being optimal
  for any DAG task-scheduling problem with only "predecessor must finish" constraints — you
  cannot do better than one visit per node when errors only flow downstream (dependency →
  dependent).
- Practical caveat: cargo won't let you `cargo check -p A` if `A`'s Cargo.toml still references
  a deleted path that no longer exists in `[workspace] members` — so directory deletion and
  Cargo.toml membership edits (§3) must happen for the 56 crates *before* you start the
  survivor fix loop, not interleaved with it. That part genuinely is "delete all, then fix" —
  the reverse-topological principle applies to **fixing survivors**, not to removing the
  doomed crates themselves (which have no useful internal order once you're committed to
  removing all 56 in one pass).
- Fewer total rounds: leaves-first survivor fixing gives you `N` check invocations for `N`
  survivor crates (one clean pass, working bottom-up) vs. an unbounded number of
  `cargo check --workspace` re-runs if you fix in arbitrary order and keep tripping over
  not-yet-fixed transitive errors reappearing as "new" errors in already-touched crates.

## 3. Cargo workspace mechanics (repo-specific)

Read `/Users/tgiap.dev/devs/zode/Cargo.toml`:

- `[workspace] members` is a flat array of `"crates/<name>"` / `"extensions/<name>"` /
  `"tooling/<name>"` strings, lines 3-253. Removing a crate means deleting its line here.
- `default-members = ["crates/zed"]` (line 254) — only `crates/zed` builds by default with a
  bare `cargo build`; unaffected by member removal unless you're removing `zed` itself (you're
  not).
- **`[workspace.dependencies]` (lines 260-493 for internal crates) DOES need touching for every
  removed crate.** Each workspace member that's referenced anywhere via `name.workspace = true`
  has a line here, e.g. `client = { path = "crates/client" }` (line 288),
  `telemetry = { path = "crates/telemetry" }` (line 453). **You must delete these lines for any
  of the 56 crates you fully remove** (not `client`/`telemetry` themselves, which you're
  gutting, not removing) — leaving a stale `[workspace.dependencies]` entry pointing at a
  deleted path directory will hard-fail `cargo metadata` for the *entire workspace* immediately,
  before you get to see any per-crate errors. This is the first thing that will break, and it
  breaks all `cargo check` invocations workspace-wide, so it must be done in the same commit as
  the member-list edit and directory deletion — all three are atomic, not sequenceable.
- Per-crate `[dependencies]` sections in the ~39 survivor crates' own `Cargo.toml` files will
  independently need their `client.workspace = true` / `telemetry.workspace = true` lines
  removed if you delete the corresponding symbols they depended on — but since you're gutting
  (not deleting) `client`/`telemetry`, most of those survivor `Cargo.toml` dependency lines stay
  intact; only the **body of client/telemetry's public API** changes, which shows up as
  *compile errors in the .rs files*, not manifest errors. Contrast with the 56 crates you
  actually delete: every survivor `Cargo.toml` that has `some_deleted_crate.workspace = true`
  needs that line removed — grep for it (§4) rather than trying to track manually.
- `Cargo.lock`: **do not hand-edit it.** After the member-list + `[workspace.dependencies]`
  edits, run `cargo check --workspace` (or `cargo metadata`) once — cargo will automatically
  drop the removed crates from the lockfile and rewrite it, since it's now unreachable from any
  member's dependency graph. `cargo update --workspace` is not necessary and is actually
  riskier here: `cargo update` may also bump *external* crate versions to latest-compatible,
  which is an orthogonal, unrelated change you don't want bundled into a structural-removal
  commit (mixes "removed 56 crates" with "bumped N external deps", making the diff
  unreviewable and the blame trail useless if something breaks later). Practice from the
  broader Rust community: keep manifest+lockfile changes in the same PR, but let plain
  `cargo check`/`cargo build` (which auto-repairs the lockfile for structural changes without
  touching versions) do the regeneration, and treat a separate `cargo update` as a distinct,
  independently-reviewed workflow
  ([discussion consistent with cargo's own lockfile-update guidance](https://doc.rust-lang.org/cargo/commands/cargo-update.html)).
  Diff `Cargo.lock` after and confirm only `[[package]]` blocks for the 56 removed crates (and
  their now-orphaned exclusive transitive deps, e.g. an auth SDK only `client` used) disappear
  — no version bumps.
- **`[patch.crates-io]`** (`Cargo.toml:872-880`): patches `async-task`, `notify`,
  `notify-types`, `windows-capture`, `calloop`, `livekit`, `libwebrtc`, `webrtc-sys`. None of
  these are among typical auth/cloud crate names, but if any of the 56 crates you're deleting
  is the *only* consumer of a patched external crate (e.g. if `livekit_client` or
  `livekit_api` is in your deletion list and nothing else pulls in `livekit`), the `[patch]`
  entry becomes dead weight — harmless to leave (cargo silently ignores unused patches) but
  worth pruning for cleanliness once the dust settles; not a blocker during the red period.
- **Feature unification**: this workspace uses `resolver = "2"` (`Cargo.toml:2`), which already
  scopes feature unification per-target (not blending host and target features) — the resolver
  version doesn't need to change for a crate removal. The one thing to watch: if any removed
  crate was the sole activator of an optional feature on a shared external dependency (e.g.
  `open_router = { path = "crates/open_router", features = ["schemars"] }` at line 404 — if
  `open_router` were deleted, nothing else may need `schemars` on `open_router`, but that's
  scoped to that one dependency edge, not global) — resolver 2's per-crate feature scoping means
  this self-corrects on next lockfile regen; no manual feature audit needed.
- **`default-members`** gotcha: none here, since only `crates/zed` is default and it's not
  being removed — but note if you delete a crate that `crates/zed` transitively used to build
  the default binary, that failure surfaces on the very first `cargo build` with no
  `-p`/`--workspace` flag, which is actually a useful cheap smoke-test to run right after the
  structural edits, before diving into the full `--workspace` check.

## 4. Verifying "no dead references left"

- **`cargo machete`**: already this repo's established tool (`script/clippy:14-16`,
  `Cargo.toml:1007-1016` `[workspace.metadata.cargo-machete] ignored = [...]`). Works at the
  text/AST level scanning `.rs` files for crate-name usage against declared deps — fast, stable
  Rust, no nightly needed. Its known blind spot is macro-generated and build-script-only usage
  (won't catch a dep only referenced via a proc-macro's generated code), which is an acceptable
  false-negative risk for THIS task because it inflates confidence conservatively (it flags
  candidates for you to double check, it won't silently hide a truly-dead removed-crate
  reference in plain Rust source) — install/run it as: `cargo machete` after your structural
  edits, on top of the ignored-list already configured. Faster and simpler than the
  nightly-only, compile-based `cargo udeps` given the existing convention already uses machete
  ([bouvier.cc writeup](https://bouvier.cc/tech/cargo-machete/); comparison confirmed by
  multiple independent sources: cargo-udeps requires nightly and is slower because it actually
  compiles, while machete is a fast static-analysis pass —
  [Rust Project Primer](https://rustprojectprimer.com/checks/unused.html),
  [users.rust-lang.org thread](https://users.rust-lang.org/t/how-to-find-unused-dependencies-within-a-workspace/103765)).
  Do NOT introduce `cargo udeps` here — it would be a new tool alien to this repo's existing
  workflow (violates the "stay within established conventions" mandate) and its accuracy
  advantage (catching macro-only usage) doesn't matter for verifying *absence* of references to
  crates you just deleted wholesale — machete's text-search approach is actually *more* directly
  suited to "does anything still say `client::` or `use zed_cloud_client`" than udeps'
  compile-based approach.
- **`cargo tree -i <crate>`** ("invert" mode): run once per deleted crate name *before* you
  delete it, to get the full reverse-dependency list up front (this is your source of truth for
  which of the ~39 survivor crates need editing) — e.g.
  `cargo tree -i cloud_llm_client -e normal,build,dev`. After the purge, `cargo tree -i <name>`
  on a deleted crate should simply error "not found" — confirming it's gone from the graph
  entirely, not just from the workspace member list.
- **ripgrep patterns** for what neither tool catches (string literals, doc comments, non-Rust
  references like build.rs, shell scripts, CI YAML, docs):
  - `rg -w '<crate_name>' --type-add 'toml:*.toml' -t toml -t rust -t yaml` per removed crate,
    across the whole repo (not just `crates/`) — catches stray `Cargo.toml` path deps outside
    `[workspace.dependencies]` (e.g. a dev-dependency declared inline in some crate's own
    manifest), leftover CI job steps, and doc references in `docs/`.
  - `rg -l 'path = "crates/(name1|name2|...)"'` as a final manifest-only sweep across every
    `Cargo.toml` in the repo, to catch any survivor crate that references a deleted crate via a
    direct `path =` dependency instead of `.workspace = true` (some crates in this repo mix both
    styles, e.g. `collections = { path = "crates/collections", version = "0.1.0" }` at line 295
    shows the workspace-level declaration also carries a path — survivor crates could in theory
    do the same instead of using `.workspace = true`).
- Recommended order: `cargo tree -i` (before deletion, build the impact map) → structural
  edits → `cargo check --workspace` until green → `cargo machete` → ripgrep sweep for
  stringly-typed/non-Rust references → final `cargo build --workspace` + `script/clippy`.

## 5. Verifying the privacy goal (no outbound connections to Zed cloud domains)

No single tool proves a universal negative ("never connects, under any code path"); the
practical, layered recipe a release checklist should use:

1. **Static pass first (cheap, catches the obvious)**: grep the final binary's source tree for
   known Zed cloud hostnames/URLs (`zed.dev`, `api.zed.dev`, collab server URLs, LiveKit
   endpoints, telemetry endpoints) — `rg -i 'zed\.dev|collab\.|telemetry\.zed|livekit'`. Also
   `cargo tree -i reqwest` / `cargo tree -i livekit` / `cargo tree -i rodio` (if audio/livekit
   crates were part of your 56) to confirm no remaining crate pulls in a networking client at
   all in the auth/telemetry path.
2. **Runtime, single-snapshot check**: `lsof -i -P | grep <binary-name>` while exercising every
   UI surface that used to trigger auth/telemetry (app launch, settings, any "Sign in" menu
   item if it still exists as UI but is now a no-op) — confirms no open sockets to the
   suspicious domains at the moment of inspection. Community guidance on macOS: use `lsof` for a
   point-in-time check and `nettop` when you want continuous/logged detail
   ([CoreLock — Find Out Which Apps Are Using the Internet on Your Mac](https://corelock.net/blog/how-to-find-what-app-is-using-internet-mac)).
3. **Continuous logged check**: `nettop -L 0 -x -P > /tmp/zode-nettop.log &` (or
   `sudo nettop -p <pid> -J bytes_in,bytes_out -x -L 0`) left running across a full manual QA
   pass exercising every menu, panel, and command in the app for 10-20 minutes — greps clean of
   the app's PID initiating connections outside localhost/expected LSP/extension-download
   endpoints.
4. **Blackhole + log as a forcing function**: add entries to `/etc/hosts` redirecting every
   known Zed cloud domain to `127.0.0.1`, then run the app normally. If the app silently
   degrades (no crash, no hung UI, no retry storm) that's a strong signal no code path
   *requires* those domains — if it does try to hit them, you'll see failed-connection log
   noise (or hung UI) that pinpoints exactly which surviving code path still assumes network
   auth/telemetry availability. This turns "prove absence" into "prove graceful absence",
   which is the actually-achievable, falsifiable version of the goal.
5. **Authoritative outside-observer, once, before release**: run the built app under **Little
   Snitch** in "silent" mode (log-only, don't block) for one full session — it is the
   community-recognized gold-standard on macOS for comprehensive outbound-connection visibility
   per-process, and unlike `lsof`/`nettop` it captures the connection **attempt** even when it's
   immediately blocked/hosts-file-redirected, giving you the domain name it tried to resolve,
   not just the socket
   ([Little Snitch — obdev.at](https://www.obdev.at/products/littlesnitch/index.html);
   free alternative for a CI-friendly, scriptable version: **LuLu**, open-source, blocks/logs
   unknown outgoing connections by default). Treat step 5 as the release-gate check; steps 1-4
   are what you run continuously during development.
- No `dtrace`/`dtruss` needed for this — those are for syscall-level tracing when you need to
  know *which line of code* opened a socket, useful for debugging a specific leak once Little
  Snitch/nettop tells you *that* one exists, but overkill as the primary verification method (and
  `dtruss` requires disabling SIP on modern macOS, an operational cost not worth paying unless
  step 4/5 surfaces something to chase down).

## 6. Rust: gutting `crates/client` and `crates/telemetry` without breaking ~37-39 downstream crates

Confirmed dependent counts in this repo (grep, 2026-07-26):
`grep -rl '^client\.workspace = true' crates/*/Cargo.toml` → **37 crates**;
`grep -rl '^telemetry\.workspace = true' crates/*/Cargo.toml` → **29 crates**.

**Recommendation: delete-and-narrow, not stub-and-preserve**, for the large majority of the
API surface — because this is an explicit hard fork with **no upstream-compatibility
requirement** stated by the user, "keep the signature but no-op the body" pattern is pure
YAGNI violation: it preserves API surface area that nothing will ever call meaningfully again,
which is dead weight that (a) still needs maintaining/reading by every future contributor,
(b) hides the true shape of the new architecture behind a compatibility facade, and
(c) contradicts the DRY-adjacent principle that a hard fork's whole point is to stop carrying
upstream's abstractions once they no longer serve a purpose.

Concrete pattern, ranked by minimal total churn given the constraint that the compiler must
guide you (not manual review) through all 37-39 call sites:

1. **Split, don't stub, `crates/client`.** Zed's `client` crate structurally bundles two
   concerns: (a) `proto`/`rpc` message plumbing used for collab/project-sharing message routing
   (likely still wanted if any real-time collab-adjacent feature survives the fork) and
   (b) the actual HTTP/WebSocket auth+cloud connection logic. Keep (a)'s public types/structs
   exactly as-is (many of the 37 dependents likely only touch `proto::Envelope`-style types or
   an `Arc<Client>` handle threaded through for unrelated reasons like action dispatch) — delete
   (b)'s implementation entirely (the connect/auth/reconnect state machine, credential storage
   calls into `credentials_provider`/`askpass`, the async task that maintains the WS
   connection). This is a targeted deletion, not a whole-crate stub.
2. **Let the compiler enumerate breakage, don't grep-guess it.** After deleting the auth/connect
   code from `client`, run `cargo check -p client` first (it'll fail loudly on anything
   internal referencing deleted internals), fix `client` itself to compile standalone, THEN run
   the reverse-topological survivor loop from §2 — each of the 37 dependents will show
   exactly which symbols it used that no longer exist. This is strictly more reliable than
   trying to manually audit 37 files for "what does this crate actually need from client" before
   touching anything.
3. **For call sites that only ever called `client.telemetry().report_event(...)` or similar
   fire-and-forget notification calls**: keep `telemetry`'s **public function signatures**
   (`send_event`, whatever the entry points are — 29 dependents call in) but make the bodies
   true no-ops. This is the one place a "stub" pattern is correct, not YAGNI-violating, because
   telemetry calls are typically scattered one-liners across dozens of unrelated business-logic
   files (editor actions, panel opens, etc.) — deleting the calls themselves at every one of the
   29 call sites is much higher total churn (29+ files touched, each requiring you to verify the
   surrounding code still makes sense without the call) than making `telemetry::send_event(...)`
   a compiled-out or literal-no-op function once, in one file. This is the textbook case for
   "keep API surface, no-op body": the call sites are numerous, trivial, and semantically
   inert (fire-and-forget, no return value consumed) — the calculus flips versus `client`
   specifically because `client`'s API return values (connection state, auth tokens, user
   identity) actually get *used* downstream (branched on, displayed in UI), so a stub there
   would require fake data anyway, which CLAUDE.md explicitly forbids ("never fake it or stub it
   out as a stand-in" — but that rule is about faking *real functionality*; a genuine no-op
   telemetry sink is not faking anything, it's honestly declining to phone home, which is the
   actual goal).
4. **For call sites that check `client.status()` or `client.is_signed_in()` to gate UI
   (e.g. "Sign in to use collab")**: don't stub these as "always false" silently — this is
   exactly the kind of behavior change that needs an explicit decision (delete the gated
   feature entirely vs. always show it enabled vs. always show it disabled with a removed-menu
   item). Flag these specific call sites for the user/architect to resolve during
   implementation planning, don't auto-resolve them as a research/mechanics decision.
5. Compute the split precisely with `cargo tree -i client -e normal` before starting, to get
   the exact 37-crate list, and hand-classify each into "only uses proto/rpc types" vs "uses
   real connection/auth state" — this classification is the actual planning deliverable for
   the implementation phase, not something to solve in this research pass.

## Trade-off matrix (removal-order strategies)

| Approach | Total check rounds | Predictability | Risk of missed dead refs | Fit with `resolver=2` + 240-crate scale |
|---|---|---|---|---|
| Delete all, fix in arbitrary/file-order | Unbounded, errors reappear as "new" after fixing deps | Low — no way to estimate remaining work | Medium (easy to miss a survivor Cargo.toml edit buried in noise) | Poor — noise scales with crate count |
| Delete all, fix reverse-topological (recommended) | Bounded ≈ N (survivor count) | High — walk order known up front from `cargo depgraph`/`cargo tree -i` | Low — machete + tree -i sweep is systematic, done once at the end | Good — this repo already has `cargo depgraph` wired in `script/crate-dep-graph` |
| Incremental crate-by-crate removal | N/A — user explicitly rejected this | N/A | N/A | N/A (out of scope, big-bang chosen) |

## Adoption risk / tooling maturity

- `cargo machete`: mature, stable-Rust, already a first-party convention in this repo
  (`script/clippy`, `Cargo.toml` metadata) — zero adoption risk, zero new dependency.
- `cargo depgraph`: already used via `script/crate-dep-graph`; requires `graphviz`'s `dot`
  binary and network install (`cargo install cargo-depgraph`) if not present — low risk, already
  a repo convention.
- `cargo udeps`: NOT recommended (see §4) — would be a net-new tool, nightly-only, slower;
  don't introduce it for this task.
- `mold`: NOT recommended on macOS — alpha-quality per upstream's own tracking issue, no payoff
  for a check-heavy (not link-heavy) workflow. `sccache`: leave as CI-only, no local change.
- Little Snitch / LuLu: mature, widely used, no code integration risk since they're
  external, observational tools — zero impact on the codebase itself.

## Architectural fit

This repo is a 240-crate Cargo workspace already organized with a flat `[workspace.dependencies]`
alias table and `resolver = "2"` — both of which are exactly what supports clean reverse-
topological crate removal (each crate's deps are explicit and named, nothing implicit to
untangle). The existing tooling (`cargo machete` via `script/clippy`, `cargo depgraph` via
`script/crate-dep-graph`) is already fit for this exact task — no new tool needs to be
introduced into the team's workflow, which minimizes both adoption risk and review burden for
whoever inspects the removal PR.

## Concrete ranked recommendation

1. Before touching anything: `cargo tree -i client -e normal`, `cargo tree -i telemetry -e normal`,
   and `cargo tree -i <each-of-56>` to build the authoritative impact map (this also becomes
   your worklist).
2. Compute reverse-topological order for the ~37-39 survivor crates via
   `./script/crate-dep-graph` (already exists) or `cargo metadata --format-version=1 | jq` if you
   want raw JSON to script the order yourself.
3. One commit: remove 56 lines from `[workspace] members`, remove their matching
   `[workspace.dependencies]` lines, `rm -rf` the 56 directories, gut `crates/client` (delete
   auth/connect impl, keep proto/rpc types) and `crates/telemetry` (no-op `send_event` and
   siblings) in the same commit — these are structurally inseparable (§3).
4. Run `cargo check --workspace --message-format=json | jq` once to get the initial
   crate-by-crate error census (not to fix from, just to confirm your survivor list matches
   reality).
5. Fix survivors leaf-first with `cargo check -p <crate>`, one at a time, in the order from
   step 2. Never run `cargo check --workspace` again until this loop reports zero remaining
   survivors in the census.
6. Gate: `cargo check --workspace` clean → `cargo machete` → ripgrep sweep (§4) →
   `cargo build --workspace` (or default-members smoke test) → `script/clippy` (full lint,
   release profile) → `cargo test`.
7. Runtime privacy verification (§5) as a separate, final release-checklist step, not part of
   the red-period compile loop.

## Limits of this research

- Did not read the actual contents of `crates/client/src/*.rs` or `crates/telemetry/src/*.rs`
  line-by-line to enumerate every public function that must be split vs stubbed — that's
  implementation-phase work, not research; §6's classification (proto/rpc types vs
  auth/connect impl) is inferred from crate naming/convention in this codebase family (Zed),
  not verified against this specific fork's current source.
- Did not identify the exact 56 crates slated for removal (task didn't name them) — the
  35-39-dependent counts for `client`/`telemetry` are measured against the *current* 240-crate
  graph, not the post-removal graph; if any of the 56 removed crates is itself one of the 37/29
  dependents, the real post-removal dependent count is slightly lower. Re-run the `grep`/
  `cargo tree -i` commands in this report after the actual removal list is finalized.
- `mold`/`sccache` guidance is general/community-sourced (Phoronix, GitHub issues) plus this
  repo's own CI config; did not benchmark actual wall-clock times on this specific machine —
  recommend a quick empirical `cargo check -p zed` timing sanity-check before committing to any
  build-speed tooling changes, since 240-crate-workspace behavior can differ from general
  advice.
- Did not verify whether this fork has already removed any Zed-cloud domains from telemetry
  config elsewhere (e.g. a build-time feature flag) — the network-silence recipe in §5 assumes
  a from-scratch verification; if partial silencing already exists, some steps may be
  redundant.
