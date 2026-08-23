# Research: Colouring zode's file icons with Material Icon Theme

Scope: is `img()` a viable substitute for `svg()` to render coloured file icons, where every
render site is, the icon-theme format gap vs Material's source, how to strip the picker, and
asset weight. Read-only investigation, no edits made to the zode tree.

## 0. The headline finding, up front

**The colour rendering path already exists and already ships in production.** `Icon::from_path`
(`crates/ui/src/components/icon.rs:151-164, 201-224`) already branches on the path string:

```rust
pub fn from_path(path: impl Into<SharedString>) -> Self {
    let source = if path.starts_with("icons/") {
        IconSource::Embedded(path)       // -> svg() -> monochrome mask
    } else {
        IconSource::External(Arc::from(PathBuf::from(path.as_ref())))  // -> img() -> full colour
    };
    ...
}
```
and `RenderOnce for Icon` renders `IconSource::External` as `img(path).size(self.size)...` — full
colour, sized in rems exactly like `IconSize`. This is not hypothetical; it is the exact path
already used whenever a user installs a *third-party* icon theme extension (which ships real
files on disk with non-`"icons/"` paths). zode's own bundled 95-icon theme is the only reason
today's default rendering looks monochrome: its `IconDefinition.path` values are hardcoded as
`"icons/file_icons/*.svg"` (`crates/theme/src/icon_theme.rs:312-404`), which trips the `"icons/"`
prefix check and routes through `svg()`.

So the real question is not "can gpui show colour" (yes, already wired) — it is "what does it cost
to get Material's SVGs into that already-working path, and what breaks along the way." Answered
below.

## 1. Is `img()` a viable substitute?

### 1.1 Resolution: embedded asset vs external file — and a real gap

`img(source)` accepts anything convertible to `ImageSource` (`crates/gpui/src/elements/img.rs:41-125`):
- `&str`/`String` → `is_uri()` check → `Resource::Uri` or `Resource::Embedded` (looked up through
  the app's `AssetSource`, i.e. the same `rust-embed` `Assets` struct `svg()` uses).
- `&Path`/`Arc<Path>`/`PathBuf` → **always** `Resource::Path` → `fs::read(path)` at load time —
  a literal filesystem read, bypassing the embedded `AssetSource` entirely
  (`ImageAssetLoader::load`, img.rs:613-614).

**The gap:** `Icon::from_path`'s `External` branch constructs `Arc<Path>`, not a `SharedString`.
So today, `IconSource::External` is hard-wired to `Resource::Path` (disk read) — it does **not**
go through the embedded `AssetSource`, even though `img()` itself is fully capable of loading
`Resource::Embedded` assets (img.rs:637-646 handles that arm). This matches the doc comment on
`IconSource::External`: "*An image file located at the specified path* ... in order to support
icon themes, we render icons as images instead" — this was built for **user-installed extension
icon themes on disk**, not for a bundled-in-the-binary set.

**Consequence for this plan:** shipping Material's SVGs as `rust-embed` assets (the natural choice,
see §5) and getting them through the *already-working* colour path is **not free** — it needs a
small, targeted change to `crates/ui/src/components/icon.rs`: either loosen the `"icons/"` prefix
heuristic to route a new prefix (e.g. `"icons/file_icons_color/"`) to `img(SharedString)` instead
of `svg()`, or add a third `IconSource` variant that carries a `SharedString` and constructs
`Resource::Embedded` explicitly. This is a few lines, not a redesign — but it is not "just point
the JSON at new paths," which is the assumption worth correcting before scoping the work.

Alternative: ship the SVGs as real files on disk next to the binary (matching `External`'s
existing contract verbatim, zero gpui/ui code changes) — but then path resolution has to survive
macOS `.app` bundling, Linux packaging, and Windows install layouts reliably. That question was
**not** investigated here (see Limits) — it's the one alternative to the code-change route and
needs its own check before ruling either way.

### 1.2 DPI / Retina sharpness — confirmed soft, not a maybe

`svg()`'s monochrome path (`Window::paint_svg`, window.rs:3722-3782) rasterizes on demand, keyed by
`RenderSvgParams { path, size }` where `size` is computed from the **actual paint bounds**
`* window.scale_factor() * SMOOTH_SVG_SCALE_FACTOR(2x)` (window.rs:3736-3741) — i.e. every distinct
on-screen size gets its own correctly-scaled rasterization, cached in the GPU sprite atlas.

`img()`'s path does **not** do this. `ImageAssetLoader::load` calls
`svg_renderer.render_single_frame(&bytes, 1.0)` — the scale factor is **hardcoded to `1.0`**
(img.rs:738, svg_renderer.rs:173-195), which internally becomes `1.0 * SMOOTH_SVG_SCALE_FACTOR(2.0)`
i.e. a flat 2x of the SVG's own intrinsic viewBox size, decided once at load time and **independent
of window scale factor or the bounds the icon is actually painted at**. The code even flags this
itself:

```rust
// TODO: Can we make SVGs always rescale?
// let scale_factor = cx.scale_factor();
```
(img.rs:608-609, commented out). This is an acknowledged, unresolved limitation in this exact
codebase — not something the zode fork already solved. Material's SVGs use a 24×24 viewBox
(VS Code convention); rasterizing at flat 2x = 48×48 px. At `IconSize::XLarge` (48px logical) on a
2x Retina display that's 96 physical px needed — the 48px raster gets upscaled 2x further and will
look visibly soft. At `IconSize::Small`/`Medium` (14–16px logical, ≤32px physical even at 2x) the
48px raster is plenty and will look crisp. **Verdict: fine for tree/tab-sized icons, soft for any
XLarge/2x-Retina-XLarge presentation** (e.g. file finder's larger previews, if any use XLarge).

### 1.3 Caching — confirmed not re-rasterized per frame

Two independent cache layers, neither keyed on paint-time size:
- `App::fetch_asset` (app.rs:2252-2269) caches the decode `Task` keyed on
  `(TypeId::of::<Asset>(), hash(Resource))` — global, app-lifetime, no eviction beyond explicit
  `remove_asset`. One rasterization per unique icon path, ever.
- `Window::paint_image`'s sprite-atlas tile is keyed on `RenderImageParams { image_id, frame_index }`
  (window.rs:3800-3816) — also not keyed on size; the GPU just billboards/scales the cached tile to
  `bounds`. Confirms: ~50 visible tree icons cost 50 quad draws referencing already-resident atlas
  tiles, not 50 rasterizations per frame. Good on performance; this is the same reason DPI isn't
  re-derived per size (§1.2) — the two properties are the same tradeoff.

### 1.4 Sizing in rems — already done, verbatim

`IconSource::External(path) => img(path).size(self.size)...` (icon.rs:218-222) — `self.size` is the
same `Rems` `IconSize::rems()` produces. No work needed here; this is exactly the desired behaviour
and it's already shipping code, not a new pattern to build.

### 1.5 usvg feature coverage — low risk, checked against the actual files

`Cargo.lock`: `usvg 0.45.1` (modern, actively maintained branch of resvg). Static scan of all 909
hand-authored Material SVGs (`scratchpad/mit/icons/*.svg`):
- `<style>` blocks: **0 files** — no CSS-in-SVG risk at all.
- `class="..."` attributes: 5 files (cosmetic, not selector-driven; irrelevant without `<style>`).
- `linearGradient`/`radialGradient`: 18 files — usvg has supported gradients (incl. `xlink:href`
  gradient stops, `gradientTransform`) since well before 0.45; sampled one multi-gradient icon and
  the markup (nested `<linearGradient xlink:href>` chains, `gradientUnits="userSpaceOnUse"`) is
  squarely inside usvg's supported surface.
- `<mask>`, `<filter>`, `<pattern>`, `<text>`, `<foreignObject>`: **0 files each.**

No feature red flags. This is the one part of the investigation resting on static inspection
rather than an actual render — see Limits.

## 2. Every place a file icon is drawn

Traced from `FileIcons::get_icon` / `get_folder_icon` / `get_chevron_icon`
(`crates/file_icons/src/file_icons.rs`) through all 32 `Icon::from_path(...)` call sites:

| Surface | File:line | Notes |
|---|---|---|
| Project panel (file rows) | `crates/project_panel/src/project_panel.rs:5896, 5923, 7297, 6256/6263/6265` | `.color(Color::Muted)` applied at 3 sites |
| Tab bar (`Item::tab_icon`) | `crates/editor/src/items.rs:686-695` → drawn at `crates/workspace/src/pane.rs:2818-2847` | Also `crates/editor/src/split.rs:1702` (split-pane variant) |
| Tab switcher | `crates/tab_switcher/src/tab_switcher.rs:267` | Reuses `Item::tab_icon` |
| File finder (fuzzy picker) | `crates/file_finder/src/file_finder.rs:1632-1633` | `.color(Color::Muted)` |
| Open-path prompt | `crates/open_path_prompt/src/open_path_prompt.rs:736-741` | file + folder icons |
| Outline panel | `crates/outline_panel/src/outline_panel.rs:2231-2483` (6 call sites) | file, folder, chevron |
| Git panel (changed-files list) | `crates/git_ui/src/git_panel/render_entries.rs:267-574` | file, folder, chevron |
| Editor inline (folded-code / inlay icon) | `crates/editor/src/element.rs:8361, 8463-8467` | chevron for code folding, file icon for e.g. inline diff hunks |
| Editor code-context menu | `crates/editor/src/code_context_menus.rs:967` | completion-item file icon |
| Language selector | `crates/language_selector/src/language_selector.rs:192-193` | icon next to language name |
| Snippets UI | `crates/snippets_ui/src/snippets_ui.rs:192-194, 336` | snippet-scope icon |
| Debugger new-process modal | `crates/debugger_ui/src/new_process_modal.rs:1552-1572` | 3 sites |
| Tasks UI modal | `crates/tasks_ui/src/modal.rs:492` | |
| REPL kernel picker | `crates/repl/src/kernels/mod.rs:376` | |
| Image viewer | `crates/image_viewer/src/image_viewer.rs:516-518` | tab icon for image files |
| SVG preview | `crates/svg_preview/src/svg_preview_view.rs:323-324` | |
| Agent UI (thread view, mention crease, `@`-mention chips) | `crates/agent_ui/src/conversation_view/thread_view.rs:2162-2163, 6738-6739`, `crates/agent_ui/src/ui/mention_crease.rs:115`, `crates/acp_thread/src/mention.rs:301-304` | |
| Context menu custom icons | `crates/ui/src/components/context_menu.rs:1760-1769` | generic, not always file icons |

**Corrections to the brief's guessed list:** breadcrumbs (`crates/breadcrumbs/src/breadcrumbs.rs`)
render **no icon at all** — only text/font — so that's not a render site to touch. Conversely, the
brief undercounted: debugger, tasks, REPL, snippets, agent/mention UI, and image/SVG preview tabs
all draw file icons too. **Total: 32 `Icon::from_path` call sites across ~19 files** (some files
have multiple sites for file/folder/chevron variants of the same panel).

## 3. The icon-theme format gap

### 3.1 Material's own build DOES emit a fully-expanded VS Code manifest — verified by running it

```bash
cd <mit-clone> && npm ci
npx tsx ./src/scripts/svg/generateOpenFolderIcons.ts   # materializes 270 "-open" folder variants
npx tsx ./src/scripts/icons/generateClones.ts          # materializes 72 configured colour clones
npx tsx ./src/scripts/icons/generateJson.ts > material-icons.json   # the expanded manifest
```
All three ran clean with `node v24.14.1` / `npm 11.11.0` (both present on this machine; `bun`,
`pnpm`, `yarn` also available as fallbacks). Output: `material-icons.json`, 450 KB, with
`iconDefinitions` (1251), `fileExtensions` (1377), `fileNames` (2135), `folderNames`/
`folderNamesExpanded` (4654 each), `languageIds` (200), plus `light`/`highContrast` override maps.
**This is a far safer conversion input than regex-parsing the TypeScript source** — use it, don't
hand-roll a parser.

### 3.2 The true icon count is 1251, not 904 — and 342 of them don't exist until you build them

`ls icons/*.svg` on the raw clone shows 909 files (close to the "904" in the brief — same order of
magnitude, off by a rounding/counting method, not a material discrepancy). But the manifest
references **1251** distinct icon-definition keys, and running the two generator scripts above
**writes 342 new physical SVG files** into `icons/` (270 open-folder variants + 72 recoloured
clones) — after which file count and manifest-key count match exactly (1251 = 1251). **Any
conversion plan must run the full generator pipeline, not just copy the checked-in `icons/`
directory, or ~27% of the reachable icon set will silently be missing.**

### 3.3 Reachability: base config vs opt-in icon packs

Of the 1251 icon-definition keys, **1185 are reachable from the base (non-opt-in) mapping tables**
(`fileExtensions`, `fileNames`, `languageIds`, `folderNames`/`folderNamesExpanded`,
`rootFolderNames`, plus the `light`/`highContrast` override tables). The remaining **66 keys are
reachable only through optional VS Code "icon packs"** (`nest-*`, `redux-*`, `ngrx-*`,
`folder-vue-directives`, `php_elephant_pink`, etc.) that VS Code users opt into via a separate
`material-icon-theme.activeIconPack` setting. **Recommendation: ignore the 66 icon-pack-only
entries** — they require a settings surface zode doesn't need (YAGNI) and add ~130 KB of icons no
default configuration will ever show.

### 3.4 Does zode's schema support everything Material needs?

`crates/theme/src/icon_theme_schema.rs` fields: `file_stems`, `file_suffixes`, `file_icons`,
`directory_icons`, `named_directory_icons`, `chevron_icons`. Checked against Material's manifest:

- **`fileExtensions` + `fileNames`** → map onto `file_suffixes` + `file_stems`. Clean fit.
- **`folderNames`/`folderNamesExpanded`** → map onto `named_directory_icons`. Clean fit.
- **`languageIds` (200 entries) → no equivalent in zode's schema.** zode's `FileIcons::get_icon`
  only ever looks at the path (stem/suffix); it has no concept of an LSP/VS-Code "language id"
  independent of the filename. This is a **real, unrepresentable gap** — files Material recognizes
  purely by content-detected language (no matching extension/filename rule) cannot get a Material
  icon in zode's current lookup model. In practice this overlaps heavily with the
  extension/filename tables already, so the *practical* loss is probably small, but it is not
  zero and was not enumerated further here (see Limits).
- **`light`/`highContrast` per-appearance variants** → zode's `IconThemeContent.appearance` is a
  single `AppearanceContent` value per theme instance (`icon_theme_schema.rs:20`), not a set of
  inline overrides. But `IconThemeFamilyContent.themes: Vec<IconThemeContent>` already supports
  multiple appearance variants as sibling theme entries (exactly how zode's colour-theme families
  do light+dark today). Material's `light` override is small (31 file-extension keys re-pointed
  at `*_light` icon variants; `highContrast` is **empty** in the generated manifest — nothing to
  port there). **Verdict: no schema change needed** — ship a second `IconThemeContent` (appearance
  = Light) that duplicates the same 1185-key base table and overrides only those ~31 keys to point
  at the `_light` SVG variants.

## 4. Making it non-configurable

**Everything a user can use to change the icon theme today:**
1. Command palette / menu action `zed_actions::icon_theme_selector::Toggle`
   (`crates/zed_actions/src/lib.rs:389-396`), wired into the app menu at
   `crates/zed/src/zed/app_menus.rs:83`, opening the picker modal at
   `crates/theme_selector/src/icon_theme_selector.rs` (a full fuzzy-picker `ModalView`, ~340 lines).
2. `settings.json` → `theme.icon_theme` field (`Option<IconThemeSelection>`,
   `crates/settings_content/src/theme.rs:151, 299-310`) — `Static(name)` or
   `Dynamic { mode, light, dark }`.
3. Settings UI dropdown + picker row (`crates/settings_ui/src/settings_ui.rs:540-541, 4306-4339`;
   wiring in `crates/settings_ui/src/page_data.rs:550-710`).
4. Extensions marketplace filtered to `ExtensionCategoryFilter::IconThemes` ("Install Icon Themes"
   button in the picker's footer) — lets a user install a *third-party* icon theme package, which
   is exactly the code path that made `IconSource::External`/`img()` exist in the first place
   (`crates/zed/src/zed.rs:2194-2268`, `path_to_extension_icon_theme`).

**What reads the field:** `crates/theme_settings/src/theme_settings.rs::configured_icon_theme()`
(line 150) resolves `ThemeSettings.icon_theme` → `ThemeRegistry::get_icon_theme(name)` → falls back
to the hardcoded default on lookup failure → pushed into the `GlobalTheme` global
(`crates/theme/src/theme.rs:311-322`), which `FileIcons::get(cx)` reads on every icon lookup.

**Removal vs keep-but-ignore:** `grep -rn deny_unknown_fields crates/settings_content` found
nothing for the theme settings struct — zode's settings.json parser does **not** reject unknown
keys. So dropping `icon_theme` from `ThemeSettingsContent` entirely would **not** break existing
users' settings files (the key just becomes inert/ignored), which lowers the risk of a full
removal. Given that, **recommend full removal** over keep-but-ignore: drop the field, delete
`icon_theme_selector` (action, menu entry, modal, delegate), delete the two settings-UI rows, and
drop the `IconThemes` extension category filter entry from the "Install Icon Themes" button (or
leave the button pointed at a category that will always be empty — messier; better to remove it).
`ThemeRegistry::list_icon_themes()`/`load_icon_theme()` can stay as dead-but-harmless plumbing if
full removal of the extension-icon-theme loading path is out of scope for this change.

## 5. Asset weight

- Current zode: `assets/icons/file_icons/` = 95 files, **114,378 bytes** raw SVG (392 KB on-disk
  with filesystem block overhead).
- Material, fully built (1251 files, §3.2): **1,031,423 bytes** raw SVG (~1.0 MB; 5.0 MB on-disk
  with block overhead — many small files inflate the block-rounded total far more than the byte
  total, worth knowing for the "AI, is this fine" gut check).
- **Net: file_icons payload grows from ~112 KB to ~1.0 MB raw (+~900 KB), file count from 95 to
  1251 (+1156 files, ~13x).**

**Embedding:** confirmed via `crates/assets/src/assets.rs` — `#[derive(RustEmbed)] #[folder =
"../../assets"] #[include = "icons/**/*"] ...` embeds the entire `assets/icons/` tree into the
binary at compile time. The crate's own top-of-file comment: *"This crate was essentially pulled
out verbatim from main `zed` crate to avoid having to run RustEmbed macro whenever zed has to be
rebuilt. It saves a second or two on an incremental build."* — i.e. the team has **already flagged
the `RustEmbed` macro's per-file compile cost** as worth isolating. Current `assets/icons/` is 360
files / 1.4 MB total; adding 1156 more files is a ~4x jump in *file count* for that macro to
enumerate (file count drives macro-expansion cost more than raw bytes do), on top of whatever the
crate's own incremental-build isolation already buys back. Binary size impact (~+1 MB compressed
SVG, likely more once packed into the binary's data section) is not itself a real concern for a
modern desktop app — the **build-time cost of the embed macro over 1156 extra files is the one to
actually watch**, and this report did not measure it empirically (see Limits).

## Recommendation

**The coloured path is worth pursuing — the plumbing already exists and mostly works — but there
are three real catches, not zero:**

1. **`Icon::from_path`'s `External` branch is hard-wired to a filesystem `Arc<Path>` read, not the
   embedded `AssetSource`.** To keep Material's SVGs embedded in the binary (the natural choice
   given §5), this needs a small, deliberate code change in `crates/ui/src/components/icon.rs` —
   a new source variant or a loosened prefix check that routes to `img(SharedString)` /
   `Resource::Embedded`. Not a redesign; a few lines. But "just change the JSON paths" is not
   sufficient on its own — flag this before scoping the implementation phase.
2. **DPI is not free.** `img()`'s SVG rasterization is a flat, load-time 2x of the SVG's intrinsic
   size, decided once and never re-derived per paint bounds or window scale factor (confirmed via
   an explicit, still-unresolved `TODO` in `img.rs`). Fine at `IconSize::Small`/`Medium` (14–16px,
   the overwhelming majority of file-icon call sites in §2); will look soft at `IconSize::XLarge`
   on a 2x/Retina display. Audit the 32 call sites in §2 for any that use `XLarge` before shipping.
3. **The build pipeline, not the checked-in repo, is the source of truth.** 342 of Material's 1251
   icons (open-folder states + recoloured clones) don't exist as files until
   `generateOpenFolderIcons.ts` + `generateClones.ts` are run. Naively vendoring `icons/` as
   checked out will silently drop ~27% of the reachable set.

None of these three is a blocker on its own, and none argues for staying monochrome — but each
changes the actual scope of "convert to Material icons" from a data-swap into a small, well-defined
set of code and build-pipeline changes. Budget for those three items explicitly in the plan.

## Limits — what this didn't cover

- **No actual render was performed.** usvg-feature-support (§1.5) rests on static grep of the SVG
  markup plus knowledge of usvg 0.45's supported feature set, not an empirical
  `render_single_frame()` call against the real bytes. Recommend a quick smoke test (render every
  Material SVG through the zode `SvgRenderer` once, assert no `Err`) before committing to the
  conversion — cheap, and closes the one part of this report that isn't first-hand evidence.
- **Packaging/path-resolution for the "external file, no code change" alternative (§1.1) was not
  investigated** — i.e. whether a real-files-on-disk approach can reliably resolve an absolute
  path across macOS `.app` bundle / Linux / Windows install layouts. Only the embedded-asset route
  was analyzed in depth, because it's the one that avoids a packaging dependency.
- **`languageIds` gap impact (§3.4) was not enumerated file-by-file** — didn't cross-reference
  which of Material's 200 language-id-only associations have zero corresponding
  extension/filename rule elsewhere in the same manifest. Likely small, unquantified here.
- **Binary-size / incremental-build-time delta from the extra 1156 embedded files (§5) was not
  measured** — flagged as the real cost to watch, but no before/after build timing was taken.

---
**Status:** DONE
**Summary:** Coloured rendering is viable — `img()`'s colour path already exists and ships in `Icon::from_path` today — but needs one small code change (`External` source is disk-path-only, not embedded-asset-aware) plus a DPI caveat at large sizes; 32 render sites across ~19 files touch file icons.
**Concerns/Blockers:** (1) `IconSource::External` must be extended to support `Resource::Embedded`, not just filesystem paths — small code change, not zero. (2) `img()` SVG rasterization is DPI-static (flat 2x, no window-scale-factor awareness) — soft at `IconSize::XLarge` on Retina; audit call sites. (3) Material's real, buildable icon set is 1251 files (909 checked-in + 342 generated via `generateOpenFolderIcons.ts`/`generateClones.ts`) — vendoring the raw repo alone loses ~27% of it. None of these blocks the work; all three must be budgeted into the implementation plan.
