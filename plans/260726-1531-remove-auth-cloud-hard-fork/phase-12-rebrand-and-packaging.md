---
phase: 12
title: "Rebrand and packaging"
status: "Legal/naming (12a, 12a', 12b) complete; packaging (12c) and full icon/desktop-file rebrand need the user"
effort: "3-5d"
---

# Phase 12: Rebrand and packaging

## Context Links

- [Consultation record](./brainstorm-report.md) — licensing analysis and distribution decision
- [Data-file scout](./reports/scout-data-files.md) §3 — plist/entitlements inventory

## Overview

**Priority:** P2 · **Build state:** ✅ GREEN (must stay green) · **Depends on:** Phase 11

Deliberately separated from the surgery so design work never mixes with a wall of compile errors. Two jobs: **legal compliance** (mandatory before public distribution) and **packaging** (the update mechanism chosen in place of `auto_update`).

**Blocked on the one open question: the new brand name.**

## Key Insights

- **Rebranding is a legal obligation, not a preference.** "Zed" is a trademark. Distributing a modified build under that name, icon, or domain is infringement regardless of GPL compliance. GPL grants copyright permission; it says nothing about trademarks.
- **License obligations are now simpler than at the start.** `collab` was AGPL-3.0; deleting it removes the AGPL obligation entirely. What remains is GPL-3.0-or-later (`zed`, `editor`, `project`, `workspace`, `client`) and Apache-2.0 (`gpui`). Public distribution requires publishing source and preserving copyright notices — both easy.
- **Package-manager distribution *is* the security-patch channel.** This resolves the concern raised during consultation: having dropped both upstream patches and `auto_update`, Homebrew/apt/winget is what remains to deliver fixes. Treat the packaging work as security infrastructure, not convenience.
- `crates/client/src/zed_urls.rs` still exists after Phase 5 with functions like `account_url`, `start_trial_url`, `upgrade_to_zed_pro_url`. Most callers are gone; the file needs rewriting or deleting.
- Bundle identifier is `dev.zed.Zed` — changing it means macOS treats the app as new: fresh keychain scope, fresh preferences path, fresh permission grants. That is desirable here but must be deliberate.

## Requirements

**Functional**
- No "Zed" trademark usage in binary name, bundle identifier, app icon, window title, or user-visible URLs.
- GPL-3.0 compliance: source published, copyright notices intact, license text shipped.
- A working install-and-update path on at least one platform.

**Non-functional**
- Attribution to the upstream Zed project preserved and honest — required by GPL and simply correct.

## Architecture

```
LEGAL (mandatory)                      PACKAGING (the patch channel)
─────────────────                      ─────────────────────────────
binary name / [[bin]]                  Homebrew cask (macOS first)
bundle id dev.zed.Zed → <new>          release artifact build + notarization
app icon + assets/icons/               versioning scheme
window title / About dialog            source-publication repo
zed_urls.rs                            apt / winget (later)
LICENSE files + NOTICE
legal/*.md (already inaccurate)
```

## Related Code Files

**To modify**
- `crates/zed/Cargo.toml` — package name, `[[bin]]` name
- `crates/zed/resources/` — `info/*.plist` (bundle id, display name), `zed.entitlements`, app icon set
- `crates/client/src/zed_urls.rs` — rewrite or delete
- `crates/zed/src/zed.rs` — About dialog, window title strings
- `assets/icons/` — app icon and any branded SVG
- `crates/zed/resources/zed.desktop.in` (Linux), flatpak/snap/winget manifests
- `README.md`, `CONTRIBUTING.md`, `docs/` branding
- `legal/privacy-policy.md`, `legal/subprocessors.md`, `legal/third-party-terms.md` — **already inaccurate after Phase 6**; rewrite or remove
- `script/bundle-mac`, `script/bundle-linux` and related release scripts

**To create**
- `NOTICE` — attribution to the upstream Zed project
- Homebrew cask formula (separate tap repo)

## Implementation Steps

### 12a′. Decide the extension registry (blocking, red team finding 2)

**The fork currently ships pointed at Zed Industries' production API.** `assets/settings/default.json:2472` `server_url` → `HttpClientWithUrl::build_zed_api_url` (`http_client.rs:214-224`) → `ExtensionStore::install_extension` (`extension_host.rs:693`, `:792`, `:837`). Every extension browse and install under the new brand hits `api.zed.dev`.

This is permitted egress under "vẫn cho phép tải về", but shipping it **undisclosed** is not acceptable for a product marketed on privacy, and it is an uncontrolled supply-chain and availability dependency on a third party who never agreed to serve this client.

Pick one before release:

| Option | Cost | Consequence |
|---|---|---|
| **Disclose and keep** | ~0 | Document in `legal/third-party-terms.md` and the README. **Check Zed's API terms first** — a rebranded third-party client may not be permitted. Zed can throttle or block at any time. |
| **Mirror** | Low-medium | Cache the registry; still originates from Zed. Same ToS question, better availability. |
| **Independent registry** | High | Full control, no third-party dependency — but you now run and moderate a marketplace. |
| **Drop the marketplace** | Low | Extensions install from local files only. Honest and self-contained; loses the ecosystem. |

Whatever is chosen must be reflected in Phase 11's `network-verification.md` and in the rewritten `legal/`.

### 12a. Decide the name (blocking)

1. Choose the product name. Check: not trademarked in the software space, domain available if a site is planned, no collision with an existing editor.
2. Derive: binary name (lowercase, no spaces), bundle identifier (reverse-DNS, e.g. `com.example.<name>`), display name, short description.

### 12b. Legal compliance

3. Rename the package and `[[bin]]` in `crates/zed/Cargo.toml`. Expect a wide but mechanical diff across scripts and CI.
4. Change the bundle identifier in `crates/zed/resources/info/*.plist`. **Note:** macOS will treat this as a new application — new preferences path, new keychain scope, new permission grants. Intended, but document it for users migrating.
5. Replace the app icon and every branded asset. Remove Zed logos from `assets/icons/`.
6. Rewrite or delete `crates/client/src/zed_urls.rs`. After Phase 5 most callers are gone — prefer deletion over repointing dead URLs.
7. Sweep all user-visible strings:
   ```sh
   rg -n '\bZed\b' crates/*/src/ assets/ --type rust --type json | grep -v '^crates/zed/'
   ```
   Distinguish product name (change) from historical attribution (keep).
8. **GPL compliance:**
   - Keep every existing copyright header. Do not strip or replace them.
   - Ship `LICENSE-GPL` and `LICENSE-APACHE` in the bundle. Delete `LICENSE-AGPL` — no AGPL code remains after `collab` was removed. **Verify this** with a licence scan before deleting.
   - Add a `NOTICE` file crediting the upstream Zed project with a link.
   - Publish the source at the same version as any binary released.
9. **Rewrite `legal/`.** `privacy-policy.md` and `subprocessors.md` describe data collection and vendors that no longer exist. Leaving them is a false statement to users about what the software does — an accuracy obligation, not housekeeping. `third-party-terms.md` likewise references LiveKit and LLM providers that are gone.
10. Update the About dialog and window title.

### 12c. Packaging — the patch channel

11. Define a versioning scheme independent of upstream Zed's, so a fork version is never confused with an upstream one.
12. Build a signed, notarized macOS release artifact. Adapt `script/bundle-mac`; expect the removed `visual-tests` feature and deleted crates to have left dead references in the release scripts.
13. Create the Homebrew cask in a tap repo: artifact URL, SHA256, version, `auto_updates false` (there is no in-app updater by design).
14. Test the full user journey on a clean machine: `brew install --cask <tap>/<name>` → launch → `brew upgrade` delivers a new version.
15. Document the release process — with no `auto_update`, this checklist *is* the security-patch delivery mechanism. Treat it accordingly.
16. Optional, later: apt repository (Linux) and winget manifest (Windows).

### 12d. Source publication

17. Publish the repository (or confirm the existing public repo carries the fork).
18. README: state plainly that this is a fork of Zed, what was removed and why, and how it differs. Link the upstream project.
19. Confirm the published source matches the released binary version.

## Todo List

- [x] 12a′ **Extension registry decision made** (disclose and keep) and reflected in
      `legal/third-party-terms.md` + `research/network-verification.md`
- [x] 12a Brand name chosen ("Zode"); binary name `zode`, bundle id
      `io.github.tgiap04.zode` (+ per-channel suffixes), display name "Zode" derived
- [x] 12b Package + `[[bin]]` renamed (`zed` → `zode`); version reset to 0.1.0 so a fork
      version is never mistaken for an upstream one
- [x] 12b Bundle identifier changed in all four `[package.metadata.bundle-*]` blocks and
      `release_channel::app_id()` (the single source both macOS bundling and Wayland/X11
      app-id/WM_CLASS read from) — **no user-migration note written yet**, since no
      release has shipped under the old identity to migrate away from
- [ ] 12b App icon and branded assets replaced — **not done, needs a designer/design
      tool**: the actual Zed logo PNGs/ICNS/ICO files are still in `assets/`/
      `crates/zed/resources/`; I have no image-generation capability to replace them
- [x] 12b `zed_urls.rs` rewritten (cut to its one surviving caller, `acp_registry_blog`)
- [ ] 12b User-visible string sweep — **partial, not exhaustive**: fixed the macOS menu
      bar title, About/Hide/Quit menu items, the Help menu (including a real bug —
      "File Bug Report..."/"Request Feature..."/"Zed Repository" pointed at
      zed-industries/zed's own tracker, now repointed at this fork's), `Info.plist`
      permission-prompt and document-type strings, and `release_channel::display_name()`
      (the shared source most other window-title/About-dialog text reads from). **Not
      swept**: onboarding/welcome-screen copy, tooltips, empty states, and the Linux
      `.desktop`/`snapcraft.yaml.in`/flatpak-manifest branding (left for the packaging
      pass below, since those files also embed unbuilt/untestable packaging logic)
- [x] 12b Copyright headers preserved (verified: this codebase doesn't use per-file
      headers; nothing was touched); `NOTICE` added crediting upstream Zed
- [x] 12b `LICENSE-AGPL` removal verified by scan (0 of 169 crates declare AGPL) —
      also found and fixed two stray AGPL/Apache symlinks on `ztracing`/`ztracing_macro`
      inconsistent with their own GPL-3.0-or-later `Cargo.toml` declaration
- [x] 12b `legal/*.md` rewritten to match reality (privacy-policy, subprocessors, terms,
      third-party-terms all replaced; Phase 10's warning banners removed since the
      content underneath them is now true)
- [x] 12b About dialog and window title updated (via `release_channel::display_name()`)
- [ ] 12c Versioning scheme defined — version reset to 0.1.0; a full scheme (tagging,
      branch strategy) is not defined, `script/bump-zed-minor-versions`' branching
      workflow was renamed but not redesigned
- [ ] 12c Signed + notarized macOS artifact builds — **needs the user**: requires an
      Apple Developer ID certificate this environment doesn't have
- [ ] 12c Homebrew cask published — **needs the user**: requires a separate tap
      repository
- [ ] 12c Clean-machine install **and upgrade** verified — blocked on the above
- [ ] 12c Release process documented as the patch channel — blocked on the above;
      what exists instead is a corrected build-from-source path in `README.md`
- [x] 12d Source published; README states the fork relationship (repo already exists at
      `github.com/tgiap04/zode` — confirmed via `git remote -v`)
- [x] Build still green: `cargo check --workspace` (+`--all-features`) and the final
      `./script/clippy` (`--release --all-targets --all-features -- --deny warnings` +
      `cargo machete`) both confirmed green after every change in this phase

## Success Criteria

- No "Zed" trademark usage in binary name, bundle id, icon, window title, or user-visible URLs.
- The extension-registry dependency is either removed or **explicitly disclosed** in `legal/third-party-terms.md`.
- `LICENSE-GPL` + `LICENSE-APACHE` + `NOTICE` shipped; copyright headers intact.
- `legal/*.md` accurately describes a product that collects nothing.
- `brew install --cask` and `brew upgrade` both work on a clean machine.
- Published source matches the released binary.
- Build still green.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Distributing under the Zed name/icon | **Trademark infringement** | Steps 3–7 are mandatory before any public artifact |
| Copyright headers stripped during rename | **GPL violation** | Step 8 states it; use targeted renames, never a blanket find-replace over source headers |
| `legal/privacy-policy.md` left as-is | Users told the software collects data it does not — false statement | Step 9 |
| `LICENSE-AGPL` deleted while AGPL code remains | Licence violation | Step 8 requires a scan, not an assumption |
| Bundle-id change silently orphans user settings | Users appear to lose their configuration | Step 4 requires a documented migration note |
| Release process undocumented | No repeatable way to ship a security patch — the very risk package-manager distribution was chosen to solve | Step 15 |
| Blanket find-replace of "Zed" | Breaks attribution and internal identifiers | Step 7 distinguishes product name from attribution |

## Security Considerations

- **This phase delivers the security-patch channel.** With upstream patches abandoned and `auto_update` deleted, the Homebrew cask is the only route from a fix to a user. Step 15 is security work.
- Release artifacts must be signed and notarized — an unsigned build trains users to bypass Gatekeeper.
- Publish the artifact SHA256 alongside the cask.
- Retain a documented process for issuing an out-of-band patch release.

## Next Steps

Plan complete. Follow-ups, all outside this plan:
1. Regenerate `docs/system/*` and `docs/generated/*` via `/tkm:rebuild-spec` (scheduled in Phase 10c).
2. Optional settings migration to strip dead keys from users' `settings.json` (~80 lines; the migrator already supports key removal — precedent at `m_2025_11_25`).
3. Optional cleanup refactor: remove the now-inert `edit_prediction_types` UI paths from `editor` and the `context_server` store from `project` — deliberately deferred to keep them out of the red period.
