# Cross-repo parallel work, proved only by booting the real backend

**Date**: 2026-09-12
**Severity**: Medium — feature complete and reviewed; one high finding found and fixed post-review; nothing deployed
**Component**: `database` (`install/{endpoint,download,manifest,unpack}.rs`), `database_ui` (`driver_registry.rs`), `tooling/xtask` (`workflows/{release,vars}.rs`), `script/publish-drivers-to-web`; and in the web repo `backend/src/drivers/`
**Status**: Resolved in code. `zode-web#1` merged; `zode#36` open against `develop`. Tag-cutting still blocked — see "What was deliberately not done".

## What Happened

The four database driver sidecars moved off public GitHub release assets and onto the Zode web
backend: a Docker named volume, public unauthenticated download, token-guarded upload from the
release CI, and the driver assets removed from the release entirely. Six phases across two repos,
run in three waves.

The feature itself is not what is worth recording. What is worth recording is that two halves of
one protocol were written in parallel by different agents in different repositories, agreeing only
on a paragraph of prose — and that nothing in the plan, the tests, or the review could have told
us whether they actually agreed.

## The contract that existed only in text

Phase 04 built the receiver (`backend/src/drivers/drivers-admin.controller.ts`): upload an archive
to staging, verify a manifest against what is staged, publish by renaming the whole directory into
`versions/`. Phase 05 built the sender (`script/publish-drivers-to-web`): POST each archive, then
POST the manifest. They ran at the same time, in different repos, and each was handed the same
written contract — URL shape, header names, body encoding, status codes.

Every one of those could have disagreed silently:

- `{base}/drivers/{version}/{asset}` versus any other segment order
- `application/octet-stream` streaming versus multipart
- `X-Driver-Sha256` versus a checksum in the body
- 422 on a hash mismatch versus 400

None of it would surface in either repo's test suite. `cargo test -p xtask` checks the generated
YAML against the generator; `npm test` checks the controller against a fake request. Both pass in a
world where the two sides speak different protocols.

So before delivery, a real backend was booted — `mongo:7.0` in Docker on 27099, `backend/dist/main.js`
on 8099, a real store directory — and the real `script/build-driver-manifest` and
`script/publish-drivers-to-web` were pointed at it with real tarballs:

```
published 4 drivers for version 0.1.7
GET /api/drivers/0.1.7/zode-db-drivers-manifest.json   (no credential) → 4 drivers
GET /api/drivers/0.1.7/zode-db-mongodb-...tar.gz       → sha256 12941a55… == 12941a55…
```

It worked on the first attempt. That is the uncomfortable part: had it not, the discovery would
have come during a release, with the GitHub assets already removed. The lesson is not "we got away
with it" — the check was run, and it is cheap. It is that **a contract between two parallel work
streams needs one integration step owned by neither of them**, scheduled from the start rather than
remembered at the end.

## Three checks built so they could fail

A green check proves nothing unless the same check goes red against the unfixed code. Three were
built that way.

**Volume ownership.** The backend runs as `USER node` and writes to a named volume. Docker seeds a
fresh volume's ownership from what the image carries at container-create time, so the `chown` has
to precede `USER node` in `backend/Dockerfile`. Two images, differing only in that line, each with
a brand-new volume:

```
with chown:     drwxr-xr-x node node …/versions  → WRITE-OK
without chown:  drwxr-xr-x root root …/versions  → EACCES
```

**The concurrency regression test.** After the fix for H1 below, the fix was reverted and the new
test re-run: it failed (`rejected` where `fulfilled` was expected), then passed once restored.

**Retention.** Rather than reading `prune()`, seven versions were published against the live backend
with `DRIVER_RETENTION_VERSIONS=5`. Five survived (`0.2.4`–`0.2.8`), the pruned `0.2.2` returned
404, and the just-published `0.2.8` returned 200 — proving it does not prune itself.

## H1 — two uploads, one `.part` file

The review found it and reproduced it live. `drivers-upload.service.ts` derived the staging path
from the asset name alone, so two concurrent uploads of one asset opened two write descriptors onto
`<asset>.part`. Their writes interleaved into a file matching neither payload, and the loser's
`rename` then failed `ENOENT` outside the `try/catch`, surfacing as an unlabelled 500. The hash is
computed from bytes in transit, so neither request could notice.

Fixed with `${asset}.${randomUUID()}.part` and a wrapped final rename. Both uploads now rename
distinct temp files onto one destination; `rename` is atomic, so the later one wins whole, and
`verifyStaging` re-hashes from disk before anything publishes.

Blast radius was contained by that publish-time re-hash — corrupt bytes could never reach a
download. It was still worth fixing: a 500 with no diagnostic is worse than the 409 every other
mismatch produces.

## A fix that read correctly and did nothing

The same review flagged that `script/publish-drivers-to-web` swallowed curl's exit code: on DNS
failure or a refused connection, `--write-out` never fires, so the script printed
`failed with status ` with an empty status and lost the real cause.

The fix captured `curl_exit` and added a diagnostic, gated on whether a real status had arrived:

```bash
if [[ "${status}" =~ ^[0-9]{3}$ ]]; then
    return 0    # a response arrived; nothing to diagnose
fi
```

curl writes `000` — three digits — when no response ever arrives. The guard therefore skipped the
diagnostic in precisely the case it was written for. The review caught it by running both failure
modes rather than reading the patch, which had looked right to its author. The condition now
excludes `000` explicitly, and both modes were reproduced to confirm it fires:

```
unresolvable host  → curl exit 6, "host could not be resolved -- check ZODE_DRIVER_UPLOAD_URL"
connection refused → curl exit 7, "connection refused -- is the backend up and reachable?"
```

Two rounds on a six-line function. Review-by-reading passed it; only review-by-running did not.

## When a trust boundary moves, re-examine what sits behind it

The review filed unvalidated manifest `entry` and `target` as not currently exploitable — a field
that _could_ someday be used to build a path.

It already was. `install/unpack.rs` does `staging.join(entry)`, and `Path::join` is not a
containment operation: an absolute value discards the directory it is joined to, and `..` walks out
of it. That reaches `store::make_executable`. The real impact stops at a chmod +x on a
manifest-chosen path, because the final `installed.is_file()` check then fails and the install
errors out — so not critical, and not code execution.

What made it worth closing now is that this change moved where the manifest comes from. It used to
be an asset of a fixed GitHub release; it is now a self-hosted endpoint fed by a CI token. The
trust anchor moved, so what depends on it deserved re-reading. Closed on both sides:
`ENTRY_PATTERN`/`TARGET_PATTERN` in `drivers.paths.ts`, and `ensure_entry_stays_inside_staging` in
`unpack.rs` requiring every component to be `Component::Normal` before anything is unpacked.

## Two subagent reports that were wrong

**Retention "not implemented".** The tester graded it critical and wrote that "MongoDB grows
unbounded". It had grepped `drivers.service.ts` — phase 03's read-side file — instead of
`drivers-publish.service.ts`, where `prune()` lives and is called on line 71 after every publish.
Drivers are not in MongoDB at all; they are on a filesystem volume. The same report concluded
"critical gap" and "safe to merge" in one breath, which is the tell worth remembering: an
internally inconsistent verdict is a prompt to go read the code.

**The resolution order in our own docs.** While checking documentation against source,
`driver_registry.rs`'s own doc comment and `docs/src/development.md` both described step 3 of
driver resolution as "a bare name on `PATH`". `path_override` does no `PATH` search — it requires
`ZODE_DB_<ID>` to be a full path to an existing file, and logs a warning when it is not. Anyone
following the docs and setting `ZODE_DB_POSTGRES=zode-db-postgres` would get nothing and no
explanation. Both were corrected.

## Process failures worth recording

Three subagents wrote their reports into the worktree's `plans/`, which is `.gitignore:68` and
would have vanished with the worktree. They were moved to the main checkout beside the plan. Any
agent writing a durable artifact from a worktree needs to be told where the durable directory is.

Every subagent also stamped its own model into the `Co-Authored-By` trailer rather than the one the
session specified — Haiku 4.5 on seven commits, Sonnet 5 on one. Caught before any push, so a
`filter-branch --msg-filter` over the local range was enough; the trees were untouched and the
suites re-run clean afterwards.

## Verification

| Gate                                                 | Result                                        |
| ---------------------------------------------------- | --------------------------------------------- |
| `cargo test -p database --all-features`              | 69 passed                                     |
| `cargo test -p database_ui`                          | 60 passed                                     |
| `cargo test -p xtask`                                | 11 passed                                     |
| `cargo check -p database --no-default-features`      | green — the sidecar still builds without gpui |
| `./script/clippy`, `shellcheck` ×2                   | clean                                         |
| web `npm run lint` / `npm test` / `npm run test:e2e` | clean / 179 / 65                              |

Exit codes were captured by the script that wrote `plans/…/evidence/temper-results.json`, not typed
by hand. `git diff --stat crates/http_client/` is empty, which is what keeps `auto_update`'s
GitHub-only host allowlist out of this change.

## What was deliberately not done

The backend side is merged (`zode-web#1`); this side is still a PR. Nothing is deployed or tagged.

`client_max_body_size 64m;` still has to be added by hand to nginx-ui's `location /api/` on the
server. That config lives in neither repo. Without it every CI upload gets a 413 whose body is
nginx's own HTML page, and the natural instinct is to debug NestJS — where nothing is wrong,
because the request never arrived.

`script/strip-driver-assets-from-releases` exists, defaults to dry-run, and **has never been run**.
The owner decided to strip driver assets from already-published releases knowing the consequence:
an already-shipped binary has the GitHub URL compiled into it and will never ask the web backend,
so those versions lose driver downloads permanently. That script runs last, after the backend is
live and serving a real manifest — never before.
