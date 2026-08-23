#!/usr/bin/env python3
"""Regenerate crates/theme/src/icon_theme_material.rs from an upstream clone of the
Material Icon Theme project (https://github.com/material-extensions/vscode-material-icon-theme).

Usage:
    1. Clone the upstream repo and build its full, buildable icon set (this materializes
       open-folder variants and colour clones that are NOT checked into their repo):

           git clone https://github.com/material-extensions/vscode-material-icon-theme mit
           cd mit && npm ci
           npx tsx ./src/scripts/svg/generateOpenFolderIcons.ts
           npx tsx ./src/scripts/icons/generateClones.ts
           npx tsx ./src/scripts/icons/generateJson.ts > material-icons.json

    2. Run this script:

           python3 gen-icon-theme-material.py <path-to-mit-clone> <output.rs>

       It reads `<mit-clone>/material-icons.json` + `<mit-clone>/icons/*.svg`, copies the
       reachable SVGs into assets/icons/file_icons/material/ (relative to the repo root
       this script is run from), and writes the generated Rust module to <output.rs>.

Design notes (see plans/reports/researcher-260822-coloured-file-icons.md for the full
rationale):
    - Only icon-definition keys reachable from the BASE (non-opt-in) mapping tables are
      included: `fileExtensions`, `fileNames`, `folderNames`/`folderNamesExpanded`, plus
      the base `file`/`folder`/`folderExpanded` keys. VS Code "icon pack" opt-ins
      (nest-*, redux-*, ngrx-*, folder-vue-directives, etc.) are deliberately excluded
      (YAGNI -- zode has no icon-pack settings surface).
    - `languageIds` is NOT ported: zode's `FileIcons::get_icon` only ever looks at the
      path (stem/suffix), it has no concept of an LSP/VS-Code "language id" independent
      of the filename.
    - Only a single (Dark-appearance) icon theme is shipped. Material's `light` override
      tables and its (empty) `highContrast` tables are NOT ported -- wiring a second,
      appearance-selected icon theme instance requires changes to `theme_settings`/
      `settings_content` resolution logic, which is outside this module's scope.
    - The icon key "file" (Material's base/default file icon) is aliased under the key
      "default" too, because `FileIcons::get_icon_for_type` falls back to looking up
      the literal key "default" when nothing else matches.
    - Every file-name and file-suffix association is emitted lowercased, with exactly
      one canonical spelling per association. `FileIcons::get_icon`
      (crates/file_icons/src/file_icons.rs) lowercases the real filename before it
      looks the result up in `file_stems`/`file_suffixes`, so the table only ever
      needs to hold the lowercase spelling -- do not "helpfully" add back
      UPPERCASE/Titlecase variants here, that lookup already covers them.
"""

import collections
import json
import os
import shutil
import subprocess
import sys


def rust_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rust_list_of_strs(items) -> str:
    return "&[" + ", ".join(rust_str(i) for i in items) + "]"


def filename_for(icon_definitions, key: str) -> str:
    return icon_definitions[key]["iconPath"].split("/")[-1]


def main() -> None:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <path-to-mit-clone> <output.rs>", file=sys.stderr)
        raise SystemExit(1)

    mit_dir, out_path = sys.argv[1], sys.argv[2]
    manifest_path = os.path.join(mit_dir, "material-icons.json")
    d = json.load(open(manifest_path))
    all_keys = set(d["iconDefinitions"].keys())

    mit_commit = subprocess.run(
        ["git", "-C", mit_dir, "rev-parse", "HEAD"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()

    # --- reachability: base tables only, no languageIds, no light/highContrast ---
    reachable = {d["file"], d["folder"], d["folderExpanded"]}
    reachable.update(d["fileExtensions"].values())
    reachable.update(d["fileNames"].values())
    reachable.update(d["folderNames"].values())
    reachable.update(d["folderNamesExpanded"].values())
    reachable &= all_keys

    # --- vendor the reachable SVGs into assets/icons/file_icons/material/ ---
    repo_root = os.getcwd()
    dest_dir = os.path.join(repo_root, "assets", "icons", "file_icons", "material")
    os.makedirs(dest_dir, exist_ok=True)
    for key in reachable:
        fname = filename_for(d["iconDefinitions"], key)
        src = os.path.join(mit_dir, "icons", fname)
        if not os.path.isfile(src):
            raise FileNotFoundError(f"icon key {key!r} references missing file {src}")
        shutil.copy2(src, os.path.join(dest_dir, fname))

    # --- file_suffixes / file_stems: assoc -> key, filtered to reachable keys ---
    file_suffixes = {a: k for a, k in d["fileExtensions"].items() if k in reachable}
    file_stems = {a: k for a, k in d["fileNames"].items() if k in reachable}

    # `FileIcons::get_icon` (crates/file_icons/src/file_icons.rs) lowercases the real
    # filename before looking it up in `file_stems`/`file_suffixes`, matching VS
    # Code's own case-insensitive behaviour. So the table only needs one canonical,
    # lowercase spelling per association -- fold every association to lowercase here
    # rather than carrying multiple case variants. If two distinct associations fold
    # to the same lowercase spelling but point at different icons, that is a genuine
    # ambiguity: refuse to silently pick one and report it instead.
    def lowercase_table(table: dict, name: str) -> dict:
        lowered: dict[str, str] = {}
        collisions: list[tuple[str, str, str]] = []
        for assoc, key in table.items():
            lk = assoc.lower()
            if lk in lowered and lowered[lk] != key:
                collisions.append((lk, lowered[lk], key))
            else:
                lowered[lk] = key
        if collisions:
            print(f"COLLISIONS while lowercasing {name}:", file=sys.stderr)
            for lk, key_a, key_b in collisions:
                print(f"  {lk!r}: {key_a!r} vs {key_b!r}", file=sys.stderr)
            raise SystemExit(
                f"refusing to silently resolve {len(collisions)} lowercase collision(s) "
                f"in {name}; see stderr for the colliding associations and their icons"
            )
        return lowered

    file_suffixes = lowercase_table(file_suffixes, "file_suffixes")
    file_stems = lowercase_table(file_stems, "file_stems")

    # VS Code lets an association name a relative path (`.config/babelrc`,
    # `src/bashly.yaml`). `FileIcons::get_icon` only ever matches against
    # `Path::file_name`, so a key holding a `/` can never be reached -- emitting it
    # ships dead data that reads as working coverage. Drop them; each has a
    # bare-filename sibling that does match.
    def drop_path_qualified(table: dict, name: str) -> dict:
        kept = {a: k for a, k in table.items() if "/" not in a}
        dropped = len(table) - len(kept)
        if dropped:
            print(f"  dropped {dropped} path-qualified association(s) from {name}")
        return kept

    file_suffixes = drop_path_qualified(file_suffixes, "file_suffixes")
    file_stems = drop_path_qualified(file_stems, "file_stems")

    suffixes_by_key = collections.defaultdict(list)
    for assoc, key in file_suffixes.items():
        suffixes_by_key[key].append(assoc)
    stems_by_key = collections.defaultdict(list)
    for assoc, key in file_stems.items():
        stems_by_key[key].append(assoc)

    used_keys = {k: filename_for(d["iconDefinitions"], k) for k in
                 (set(file_suffixes.values()) | set(file_stems.values()) | {d["file"]})}
    used_keys.setdefault("default", used_keys[d["file"]])

    named_dirs = {}
    for name, key in d["folderNames"].items():
        if key not in reachable:
            continue
        open_key = d["folderNamesExpanded"][name]
        named_dirs[name] = (
            filename_for(d["iconDefinitions"], key),
            filename_for(d["iconDefinitions"], open_key),
        )

    folder_file = filename_for(d["iconDefinitions"], d["folder"])
    folder_open_file = filename_for(d["iconDefinitions"], d["folderExpanded"])

    # --- emit Rust ---
    lines = []
    lines.append("//! Material Icon Theme mapping tables.")
    lines.append("//!")
    lines.append("//! Generated by `plans/reports/gen-icon-theme-material.py` from the")
    lines.append("//! Material Icon Theme project (https://github.com/material-extensions/vscode-material-icon-theme),")
    lines.append(f"//! commit `{mit_commit}`.")
    lines.append("//!")
    lines.append("//! Source: that project's own `generateOpenFolderIcons.ts` + `generateClones.ts` +")
    lines.append("//! `generateJson.ts` scripts were run to produce a fully-expanded VS Code icon-theme")
    lines.append("//! manifest (`material-icons.json`), which was then converted into these tables. Only")
    lines.append("//! icon-definition keys reachable from the base (non-opt-in) mapping tables are")
    lines.append("//! included -- VS Code \"icon pack\" opt-ins (nest-*, redux-*, ngrx-*, etc.), the")
    lines.append("//! `languageIds` table (zode has no language-id concept independent of the filename),")
    lines.append("//! and the `light`/`highContrast` appearance overrides were all deliberately left out.")
    lines.append("//! See `plans/reports/researcher-260822-coloured-file-icons.md` for the full rationale.")
    lines.append("//!")
    lines.append("//! Every file-name and file-suffix association below is lowercased, with exactly one")
    lines.append("//! canonical spelling per association: `FileIcons::get_icon`")
    lines.append("//! (crates/file_icons/src/file_icons.rs) lowercases the real filename before it looks")
    lines.append("//! the result up here, matching VS Code's own case-insensitive matching. Do not add")
    lines.append("//! UPPERCASE/Titlecase variants back in -- that lookup already covers them.")
    lines.append("//!")
    lines.append("//! To regenerate: see the usage docstring at the top of")
    lines.append("//! `plans/reports/gen-icon-theme-material.py`.")
    lines.append("")
    lines.append("use collections::HashMap;")
    lines.append("")
    lines.append("/// Base path under which every Material icon asset is embedded.")
    lines.append('pub const MATERIAL_ICON_PREFIX: &str = "icons/file_icons/material/";')
    lines.append("")

    lines.append("const FILE_STEMS_BY_ICON_KEY: &[(&str, &[&str])] = &[")
    for key in sorted(stems_by_key):
        lines.append(f"    ({rust_str(key)}, {rust_list_of_strs(sorted(stems_by_key[key]))}),")
    lines.append("];")
    lines.append("")

    lines.append("const FILE_SUFFIXES_BY_ICON_KEY: &[(&str, &[&str])] = &[")
    for key in sorted(suffixes_by_key):
        lines.append(f"    ({rust_str(key)}, {rust_list_of_strs(sorted(suffixes_by_key[key]))}),")
    lines.append("];")
    lines.append("")

    lines.append("/// A mapping of a file type identifier to its corresponding Material icon asset.")
    lines.append("const FILE_ICONS: &[(&str, &str)] = &[")
    for key in sorted(used_keys):
        path = f"icons/file_icons/material/{used_keys[key]}"
        lines.append(f"    ({rust_str(key)}, {rust_str(path)}),")
    lines.append("];")
    lines.append("")

    lines.append("/// A mapping of a named directory to its collapsed/expanded Material icon paths.")
    lines.append("const NAMED_DIRECTORY_ICONS: &[(&str, &str, &str)] = &[")
    for name in sorted(named_dirs):
        collapsed_file, expanded_file = named_dirs[name]
        collapsed = f"icons/file_icons/material/{collapsed_file}"
        expanded = f"icons/file_icons/material/{expanded_file}"
        lines.append(f"    ({rust_str(name)}, {rust_str(collapsed)}, {rust_str(expanded)}),")
    lines.append("];")
    lines.append("")

    lines.append("/// The path to the default (unnamed) collapsed directory icon.")
    lines.append(f'pub(crate) const DIRECTORY_ICON_COLLAPSED: &str = "icons/file_icons/material/{folder_file}";')
    lines.append("/// The path to the default (unnamed) expanded directory icon.")
    lines.append(f'pub(crate) const DIRECTORY_ICON_EXPANDED: &str = "icons/file_icons/material/{folder_open_file}";')
    lines.append("")

    lines.append("/// Returns a mapping of file associations to icon keys.")
    lines.append("fn icon_keys_by_association(")
    lines.append("    associations_by_icon_key: &[(&str, &[&str])],")
    lines.append(") -> HashMap<String, String> {")
    lines.append("    let mut icon_keys_by_association = HashMap::default();")
    lines.append("    for (icon_key, associations) in associations_by_icon_key {")
    lines.append("        for association in *associations {")
    lines.append("            icon_keys_by_association.insert(association.to_string(), icon_key.to_string());")
    lines.append("        }")
    lines.append("    }")
    lines.append("")
    lines.append("    icon_keys_by_association")
    lines.append("}")
    lines.append("")
    lines.append("/// Returns the Material icon theme's file-stem -> icon-key table.")
    lines.append("pub(crate) fn material_file_stems() -> HashMap<String, String> {")
    lines.append("    icon_keys_by_association(FILE_STEMS_BY_ICON_KEY)")
    lines.append("}")
    lines.append("")
    lines.append("/// Returns the Material icon theme's file-suffix -> icon-key table.")
    lines.append("pub(crate) fn material_file_suffixes() -> HashMap<String, String> {")
    lines.append("    icon_keys_by_association(FILE_SUFFIXES_BY_ICON_KEY)")
    lines.append("}")
    lines.append("")
    lines.append("/// Returns the Material icon theme's icon-key -> asset-path table.")
    lines.append("pub(crate) fn material_file_icons() -> HashMap<String, String> {")
    lines.append("    HashMap::from_iter(")
    lines.append("        FILE_ICONS")
    lines.append("            .iter()")
    lines.append("            .map(|(key, path)| (key.to_string(), path.to_string())),")
    lines.append("    )")
    lines.append("}")
    lines.append("")
    lines.append("/// Returns the Material icon theme's named-directory -> (collapsed, expanded) table.")
    lines.append("pub(crate) fn material_named_directory_icons() -> HashMap<String, (String, String)> {")
    lines.append("    HashMap::from_iter(NAMED_DIRECTORY_ICONS.iter().map(|(name, collapsed, expanded)| {")
    lines.append("        (name.to_string(), (collapsed.to_string(), expanded.to_string()))")
    lines.append("    }))")
    lines.append("}")
    lines.append("")

    open(out_path, "w").write("\n".join(lines))
    print(f"wrote {out_path} ({len(lines)} lines); vendored {len(reachable)} SVGs into {dest_dir}")


if __name__ == "__main__":
    main()
