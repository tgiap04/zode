# The brand was orange, and half the chrome stayed blue

**Date**: 2026-10-24 01:03
**Severity**: Medium — no functional breakage, but the brand read as inconsistent on most active surfaces
**Component**: theme / theme_settings (brand palette and overrides), ui (`Color`, `TintColor`, `IconButton`)
**Status**: resolved — shipped to `feat.release-v0.1.6`; visual check in the running app still open

## What Happened

Active icons, loading indicators and many other surfaces were still blue although the brand is orange. Commit `0688314` (2026-09-17) had added `crates/ui/src/styles/brand.rs` and routed `Color::Selected`, switches, `TintColor::Accent` background/border and `ButtonStyle::Brand` to the brand. It deliberately left `Color::Accent` (the theme's `text_accent`) with the theme, for links and hints.

The user then set the scope: active icons, loading, focus borders, the unsaved-changes dot, local cursor and selection, highlighted search matches, and links/hints too ("đổi hết sang cam"). The last item overturned the `0688314` rule.

Shipped as `6f5587d` and `98dc338` (the override is documented in `docs/src/themes.md`), pushed to `origin/feat.release-v0.1.6`.

## The Brutal Truth

The galling part is that the "active icon is blue" bug was not a theme problem at all. `impl From<TintColor> for Color` mapped `TintColor::Accent => Color::Accent`, and `IconButton` uses that conversion for a selected icon. A selected button got an orange tint with a blue glyph inside it.

The brand had also been applied per helper, not per token. Every site that read a theme token stayed blue.

## Technical Details

- `.selected_style(ButtonStyle::Tinted(TintColor::Accent))` appears at about 20 call sites.
- `Color::Accent` is used directly in about 100 places (spinners, tab busy bar, state markers).
- `ProgressBar` and `CircularProgress` defaulted to `status().info`.
- Focus rings read `border_focused` (about 29 sites).
- The editor cursor and selection come from `players[0]`, the local player colour.
- Red evidence: with the brand step removed, Ayu Dark's `text_accent` came out at hue 0.56 instead of 0.0745.
- Dark selection at 0x3d measured about 1.65:1 against the `#121314` canvas. Dark now uses 0x66 (about 2.50:1). Light keeps 0x3d.
- Pre-existing, not this change: `cargo check --tests -p title_bar` fails with E0004 on `RemoteConnectionOptions::Mock`. It fails identically at HEAD.

## What We Tried

- **Edit about 150 call sites.** Considered at planning and not attempted. The palette moved into the `theme` crate instead (`crates/theme/src/brand.rs`), because `theme_settings` cannot depend on `ui`.
- **Brand applied in `apply_theme_overrides`.** Kept. It is the one path every active theme takes. The brand runs first, then `experimental.theme_overrides` and per-theme overrides, so a user can still take any token back.
- **Keep dark selection at 0x3d.** The review flagged it as too faint on the default dark theme; the user chose 0x66.
- **Leave `element_selection_background` alone.** The review flagged it and the user chose to brand it too. It is derived from the original selection at load time and no bundled theme sets it, so markdown, hover popovers and notifications stayed blue. It is now set to the brand selection.

## Root Cause Analysis

Two accent tokens (`Color::Accent` and `Color::Selected`) were split by `0688314`, and the one that the theme owns stayed blue. The conversion in `From<TintColor>` picked the wrong one. Fixing it per call site would have been a rewrite across roughly 150 places. Every theme already passes through one function, so the fix went there.

## Lessons Learned

- A type conversion can carry a colour across the wrong boundary as easily as a theme can. Check the `From` impls before blaming the palette.
- One choke point beats 150 call sites when every theme already passes through it.
- A token derived at load time from another token does not follow when you override the source.
- Compare timestamps in one timezone. A delivery-tracker without a shell compared UTC run times with +07:00 file mtimes and reported the test record as older than the last edit when it was not.
- Never name a zsh variable `path`. It is tied to `PATH`, so every command in the loop became "command not found", and a checksum check reported every file as mismatched until the name changed.
- Process: two background agents died on network errors mid-run, and one tester never wrote its evidence. The retry tester was told to append evidence after each command.

## Next Steps

- **Owner: user.** Visual check in the running app. Not verified here.
- **Side effects to watch:** terminal and debugger-console cursors are orange now. Collaborator #1 in `vscode-2026` is already `#cd861a` and may look like the local cursor. The local player background is 0x4d alpha in One, Ayu and Gruvbox.
- **Not verified:** Linux and Windows (CI only). Compile-only check of seven consumer crates is clean.
- **Flaky, not stably red:** the workspace hibernate test, previously noted as red on develop, passed in every run this time.
