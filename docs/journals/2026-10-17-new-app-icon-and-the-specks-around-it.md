# New app icon and logo, with the source's alpha specks removed

**Date**: 2026-10-17 02:40
**Severity**: Low — a branding change
**Component**: app icons, in-app logo, favicons, Windows and macOS file icons
**Status**: ongoing — committed and pushed; no check in the running app yet

## What Happened

Replaced the app icon and logo with the user-supplied 1254x1254 RGBA PNG. The user decided: full icon as the in-app logo, fitted to Apple's grid (body 824 of 1024), and docs, web, Windows updater and macOS document icons change. Committed as `7d5aec1` (PR #43) and `ac2476c` (web `main`, deploys on push).

## The Brutal Truth

The source was not clean. Its body was alpha 253, slightly see-through, and about 36,000 stray pixels at alpha 1-2 surrounded the square. The specks showed only faintly on white, and the translucency showed nowhere. A small sting: a source that looks right can still carry both.

## Technical Details

- Repair: ImageMagick `-channel A -level 1.2%,98%` made the body opaque and the specks transparent, keeping the anti-aliased edge. A detector counting alpha 1-2 pixels well outside the body found **32,145** in the source (36,412 across the whole image) and **0** in every output.
- One 1024 master (`assets/images/zode_app_icon.png`) copied out, so the four channels' PNGs and Windows `.ico` files are byte-identical. Upstream Zed's `Document.icns` and updater `.ico` are now replaced.
- `zode_logo.png` deleted. Test `the_app_logo_is_embedded` (`crates/assets`) pins `images/zode_app_icon.png`.
- Review fixes: 64 px favicons, opaque 180 px `apple-touch-icon.png`, stale `script/bundle-mac` comment corrected.
- Gates, exit 0: `cargo test` for `assets`, `zode` (71), `onboarding` (4), `workspace` (342); `./script/clippy`; `cargo fmt --check`; `./script/prettier`; `shellcheck`; web build, lint, test (457).

## What We Tried

- **Level clamp on alpha:** `-level 1.2%,98%` cleaned both defects and kept the edge. Confirmed by histograms.
- **Two logo files:** set aside. One file now serves both.

## Root Cause Analysis

- **Specks and translucency:** both live in the alpha channel. The specks are near-transparent, and alpha 253 looks opaque, so a visual check passes either.
- **Logo path:** no test resolved it, so a rename would have left an empty space with every gate green.

## Lessons Learned

- Check the alpha channel, not just the look: the specks were faint even on white, and the translucency invisible everywhere.
- When two assets become identical, delete one rather than embed both, and pin the surviving path with a test.

## Next Steps

- **Owner not recorded:** check the Dock, Finder, the Welcome page and Windows and Linux builds in a running app. Not done; no deadline recorded.
- Windows and Linux packaging cannot be built here; their icons keep the Apple margin, so the 16 px frame is small.
- The About window's separate 512 px embed (about 142 KB) is deferred.
