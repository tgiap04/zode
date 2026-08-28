//! Guards on the app's own name: nothing that ships may call this editor Zed,
//! in Rust or in `assets/`, and the one place the name is load-bearing rather
//! than cosmetic -- the application menu a keybinding opens by name -- has to
//! agree with itself.
//!
//! `crates/onboarding` keeps its own, narrower version of this check. That one
//! bans the name anywhere in four files, identifiers included; this one covers
//! every crate but only string literals a user could read. Neither subsumes the
//! other, so both stay.

use std::path::{Path, PathBuf};

/// Strings that may still say Zed, because each names something that really is
/// Zed rather than this fork.
///
/// Matched by exact content rather than by file and line, so moving code around
/// does not turn this test red for no reason.
const ALLOWED: &[&str] = &[
    // Upstream services and systems this fork uses rather than replaces.
    "The URL of the Zed server to connect to.",
    "Use LSP tasks over Zed language extension tasks.",
    "Zed AI",
    // Identifiers that happen to read as labels. `Zed Keybind Context` is a
    // registered grammar (`crates/grammars/src/zed-keybind-context/config.toml`)
    // looked up by exact name; `Zed/{}` is the HTTP User-Agent sent to every
    // server; `ZedHeap` is an ETW provider; `Zed X Copilot` names an asset.
    "Zed Keybind Context",
    "Failed to load Zed Keybind Context language",
    "Zed/{} ({}; {})",
    "Zed-Server/{} ({}; {})",
    "Add ZedHeap profile to collection",
    "Zed X Copilot",
    // Font family aliases kept for compatibility: a settings file that already
    // names one of these must keep resolving. See `font_name_with_fallbacks`.
    "Zed Plex Sans",
    "Zed Plex Mono",
    // Placeholder text inside component previews, which no user of the editor
    // reaches through the editor itself.
    "A button component displayed in the title bar to show auto-update status and allow users to restart Zed.",
    "Mix and match Zed's agent with any ACP-compatible agent",
    "a_very_long_username_that_might_overflow is sharing a project in Zed:",
    "Create a Zed account by",
    // Markers this fork wrote before the rename, kept so cleanup can still find
    // the blocks they wrap. See `LEGACY_MARKERS` in `git::repository`.
    "\\n\\n#  ====== Auto-added by Zed: =======\\n",
    "\\n#  ====== End of auto-added by Zed =======\\n",
];

/// Returns every string literal opening on `line`, with escape sequences left
/// in their source form -- `\n` stays two characters, so `ALLOWED` can quote
/// what the source quotes.
fn string_literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buffer = String::new();
    let mut inside = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if !inside {
            if character == '/' && characters.peek() == Some(&'/') {
                break;
            }
            if character == '"' {
                inside = true;
                buffer.clear();
            }
            continue;
        }
        match character {
            '\\' => {
                buffer.push('\\');
                if let Some(escaped) = characters.next() {
                    buffer.push(escaped);
                }
            }
            '"' => {
                inside = false;
                out.push(std::mem::take(&mut buffer));
            }
            other => buffer.push(other),
        }
    }
    out
}

/// The lines of `source` that ship, dropping `#[cfg(test)]` items.
///
/// Splitting the file at its first `#[cfg(test)]` -- which is what the
/// onboarding version of this check does -- silently drops the remainder of any
/// file that registers a test module near the top. `crates/zed/src/zed.rs`
/// registers *this* module on its second line, and under the split rule the
/// other 6400 lines of the app's main module counted as test code: `About Zed`
/// could be reintroduced there and this test still passed.
///
/// So an attribute followed by a `mod name;` declaration skips those two lines
/// and scanning continues. An attribute followed by anything else still ends the
/// file, which is right for the trailing `mod tests { .. }` every crate uses and
/// wrong only for a `#[cfg(test)]` item placed mid-file above shipping code.
/// That shape does not exist in this tree today, and nothing here would notice
/// if it appeared in some crate other than `zed` -- the canary at the end of the
/// test only watches `crates/zed/src/zed.rs`, because that is the one file whose
/// early registration is load-bearing for this module.
fn shipping_lines(source: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut lines = source.lines().enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        if !line.trim_start().starts_with("#[cfg(test)]") {
            out.push((index + 1, line));
            continue;
        }
        match lines.peek() {
            Some((_, next)) if next.trim_end().ends_with(';') => {
                lines.next();
            }
            _ => break,
        }
    }
    out
}

fn shipping_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            // Not shipped to anyone: benchmarks, examples and test trees.
            if !matches!(name.as_str(), "benches" | "examples" | "tests" | "target") {
                shipping_sources(&path, found);
            }
        } else if name.ends_with(".rs") && !name.contains("test") {
            found.push(path);
        }
    }
}

/// Reads the source rather than behaviour, because "calls itself by the right
/// name" has no runtime seam, and it is exactly the kind of thing that drifts
/// back one label at a time -- 66 of them had, across 23 crates, after the
/// onboarding screen alone was cleaned up.
#[test]
fn no_shipping_string_calls_this_editor_zed() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates");
    let mut sources = Vec::new();
    shipping_sources(&crates, &mut sources);
    assert!(
        sources.len() > 100,
        "found only {} source files under {}; the walk is not reaching the tree",
        sources.len(),
        crates.display()
    );

    let mut offenders = Vec::new();
    let mut scanned_in_app_module = 0usize;
    for path in sources {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Test code is not user-facing text, and this very file quotes the name
        // it is banning.
        let lines = shipping_lines(&source);
        if path.ends_with("src/zed.rs") {
            scanned_in_app_module = lines.last().map_or(0, |(number, _)| *number);
        }
        for (number, line) in lines {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for literal in string_literals(line) {
                if !literal.contains("Zed") {
                    continue;
                }
                // A token with no space in it is an identifier, a path or a URL,
                // not a sentence anyone reads.
                if !literal.trim().contains(' ') {
                    continue;
                }
                if ALLOWED.contains(&literal.as_str()) {
                    continue;
                }
                offenders.push(format!("{}:{number}: {literal}", path.display()));
            }
        }
    }

    // A canary against this test going blind rather than green. The app's main
    // module registers a `#[cfg(test)]` item on its second line, so a scanner
    // that stops there reports no offenders because it read almost nothing.
    assert!(
        scanned_in_app_module > 1000,
        "only scanned to line {scanned_in_app_module} of crates/zed/src/zed.rs; \
         the walk is stopping at its `#[cfg(test)]` registration and would miss \
         anything below it"
    );

    assert!(
        offenders.is_empty(),
        "these strings still call the editor Zed. Rename them, or add each to \
         ALLOWED in this file with the reason it really does mean Zed:\n{}",
        offenders.join("\n")
    );
}

/// Paths under `assets/` that legitimately name Zed. Matched as substrings of
/// the path, because each covers a whole file or directory rather than one line.
const ASSET_EXCEPTIONS: &[&str] = &[
    "licenses.md",                // copyright notices, not branding
    "themes/",                    // `"author": "Zed Industries"` is who wrote them
    "badge/",                     // upstream's badge, carrying Zed's own logo
    "initial_user_settings.json", // deliberately points the reader at upstream's docs
];

/// Lines under `assets/` that may name Zed despite living in a swept file.
const ASSET_ALLOWED_LINES: &[&str] = &[
    // A glob matching directory names on disk. `.zed/settings.json` is where a
    // project's settings live, so this is an identifier and renaming it would
    // stop project settings being found at all.
    "\"**/{zed,Zed}/{settings,keymap,tasks,debug}.json\",",
];

/// `assets/settings/default.json` and the keymap templates are opened by users
/// directly -- `zed: open default settings` puts that file in the editor -- so
/// they are as user-facing as any label, and they drift the same way. 52 of
/// their lines still said Zed after the Rust side had been swept, including the
/// same sentence that `settings_ui::page_data` shows in the settings UI.
#[test]
fn no_shipping_asset_calls_this_editor_zed() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let mut files = Vec::new();
    collect_assets(&assets, &mut files);
    assert!(
        files.len() > 20,
        "found only {} asset files under {}; the walk is not reaching the tree",
        files.len(),
        assets.display()
    );

    let mut offenders = Vec::new();
    for path in files {
        let shown = path.to_string_lossy().replace('\\', "/");
        if ASSET_EXCEPTIONS
            .iter()
            .any(|exception| shown.contains(exception))
        {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if !names_zed(line) || ASSET_ALLOWED_LINES.contains(&line.trim()) {
                continue;
            }
            offenders.push(format!("{shown}:{}: {}", index + 1, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "these asset lines still call the editor Zed. Rename them, or add each \
         to ASSET_ALLOWED_LINES with the reason it really does mean Zed:\n{}",
        offenders.join("\n")
    );
}

/// `Zed` as a whole word, ignoring the font aliases and links that mean it on
/// purpose.
fn names_zed(line: &str) -> bool {
    if ["zed.dev", "zed://", "ZedSans", "ZedMono", "Zed Plex"]
        .iter()
        .any(|allowed| line.contains(allowed))
    {
        return false;
    }
    line.match_indices("Zed").any(|(at, _)| {
        let after = line[at + 3..].chars().next();
        !matches!(after, Some(character) if character.is_alphanumeric() || character == '_')
    })
}

fn collect_assets(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_assets(&path, found);
        } else {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") || name.ends_with(".md") {
                found.push(path);
            }
        }
    }
}

/// `f10` dispatches `OpenApplicationMenu` with the menu's name as its argument,
/// and `title_bar::application_menu` resolves it by exact string match. The
/// rename made the menu `Zode` while both keymaps kept saying `Zed`, so f10
/// opened nothing on Linux and Windows: `find` returned `None`, the pending name
/// was consumed, and nothing was logged. Cosmetic drift in one file, a dead
/// keybinding in another -- which is why the two are checked against each other
/// rather than each being eyeballed alone.
#[test]
fn every_keymap_application_menu_names_a_real_menu() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let menus = std::fs::read_to_string(root.join("crates/zed/src/zed/app_menus.rs"))
        .expect("app_menus.rs is readable");
    let declared: Vec<String> = menus
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("name: ")
                .and_then(|rest| string_literals(rest).into_iter().next())
        })
        .collect();
    assert!(
        declared.iter().any(|name| name == "Zode"),
        "app_menus.rs declares no menu named Zode; found {declared:?}"
    );

    let mut broken = Vec::new();
    for keymap in [
        "assets/keymaps/default-linux.json",
        "assets/keymaps/default-windows.json",
        "assets/keymaps/default-macos.json",
    ] {
        let Ok(source) = std::fs::read_to_string(root.join(keymap)) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            if !line.contains("OpenApplicationMenu") || line.trim_start().starts_with("//") {
                continue;
            }
            let literals = string_literals(line);
            let argument = literals
                .iter()
                .skip_while(|literal| !literal.contains("OpenApplicationMenu"))
                .nth(1);
            match argument {
                Some(name) if declared.contains(name) => {}
                Some(name) => broken.push(format!(
                    "{keymap}:{}: opens a menu named {name:?}, which app_menus.rs does not declare",
                    index + 1
                )),
                None => broken.push(format!(
                    "{keymap}:{}: OpenApplicationMenu with no menu name",
                    index + 1
                )),
            }
        }
    }

    assert!(broken.is_empty(), "{}", broken.join("\n"));
}
