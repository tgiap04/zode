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
/// file that registers a test item near the top: a `mod tests;` declaration, a
/// `#[cfg(test)]` struct field, or a test-only impl above shipping code. Under
/// the split rule everything below it counted as test code, so a label could be
/// reintroduced there and this test still passed.
///
/// So only the attributed item is skipped and scanning continues after it, and
/// `test_item_end` finds where it stops. The canaries in the test below watch
/// how far the scan gets in files known to have this shape.
fn shipping_lines(source: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if line.trim_start().starts_with("#[cfg(test)]") {
            at = test_item_end(&lines, at);
        } else {
            out.push((at + 1, line));
            at += 1;
        }
    }
    out
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `line` up to a trailing `//` comment. A `//` inside a string literal is not
/// a comment, and URLs in test fixtures are full of them.
fn without_line_comment(line: &str) -> &str {
    let mut inside = false;
    let mut characters = line.char_indices();
    while let Some((at, character)) = characters.next() {
        match character {
            '\\' if inside => {
                characters.next();
            }
            '"' => inside = !inside,
            '/' if !inside && line[at..].starts_with("//") => return &line[..at],
            _ => {}
        }
    }
    line
}

fn ends_an_item(line: &str) -> bool {
    without_line_comment(line)
        .trim_end()
        .ends_with([';', ',', '}'])
}

fn opens_a_block(line: &str) -> bool {
    without_line_comment(line).trim_end().ends_with('{')
}

/// The index of the first line after the `#[cfg(test)]` item whose attribute is
/// on `lines[attribute]`.
///
/// The item starts on the attribute's own line when code follows the attribute
/// there (`#[cfg(test)] flag: bool,`), and otherwise on the first line after the
/// attribute stack and comments. One that ends in `;`, `,` or `}` is the whole
/// item. Anything else runs on, and rustfmt decides where it stops by
/// indentation, which unlike counting brackets is not fooled by the braces
/// inside the string and char literals a test module is full of.
///
/// Once a line opens a block -- `mod tests {`, or the `) -> T {` closing a
/// signature -- only a `}` at the attribute's own indentation ends the item,
/// because anything inside a block may start in any column, raw strings
/// included. Before that, while an item is still a signature or an initializer,
/// a line at the attribute's own indentation is either a closer (`];`, `);`,
/// `),`, `},`) that ends it, a continuation (`) -> T`, `where`, `{`), or the next
/// item, which the item ends short of. A line indented less than the attribute
/// belongs to the parent and also ends it.
fn test_item_end(lines: &[&str], attribute: usize) -> usize {
    let attribute_indentation = indentation(lines[attribute]);
    let after_attribute = lines[attribute]
        .trim_start()
        .trim_start_matches("#[cfg(test)]")
        .trim();
    let item_start = if after_attribute.is_empty()
        || after_attribute.starts_with("//")
        || after_attribute.starts_with("#[")
    {
        (attribute + 1..lines.len()).find(|&index| {
            let trimmed = lines[index].trim();
            !(trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("//"))
        })
    } else {
        Some(attribute)
    };
    let Some(item_start) = item_start else {
        return lines.len();
    };
    if ends_an_item(lines[item_start]) {
        return item_start + 1;
    }
    let mut in_block = opens_a_block(lines[item_start]);
    for (index, line) in lines.iter().enumerate().skip(item_start + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let line_indentation = indentation(line);
        if in_block {
            if line_indentation == attribute_indentation && trimmed.starts_with('}') {
                return index + 1;
            }
        } else if line_indentation < attribute_indentation {
            return index;
        } else if line_indentation == attribute_indentation {
            if trimmed.starts_with(['}', ']', ')', '>']) {
                if opens_a_block(line) {
                    in_block = true;
                } else if ends_an_item(line) {
                    return index + 1;
                }
            } else if trimmed == "{" {
                in_block = true;
            } else if trimmed != "where" {
                return index;
            }
        }
    }
    lines.len()
}

/// Files with a `#[cfg(test)]` item above shipping code, and a shipping line in
/// each that the scan has to reach.
///
/// Matched by content rather than by how far the scan got: a scan that swallows
/// the middle of a file and picks up again at the end still ends on a late line,
/// so only a line it is known to cross proves it did not.
const MID_FILE_TEST_ITEM_FILES: &[(&str, &str)] = &[
    // Registers `#[cfg(test)] mod` declarations on its second line.
    (
        "zed/src/zed.rs",
        "pub(crate) fn eager_load_active_theme_and_icon_theme(",
    ),
    // A test module, then shipping functions below it.
    ("cli/src/main.rs", "fn parse_path_in_wsl("),
    // Test-only field and `mod` declarations near the top, a test module near the
    // bottom, and this function after it.
    ("editor/src/editor.rs", "pub fn multibuffer_context_lines("),
    // A test-only `const` array closed by `];`, above every item that ships.
    ("zed/src/zed/status_bar_items.rs", "pub(crate) fn register("),
    // A test-only `static` whose initializer sits on the next line.
    (
        "remote_server/src/headless_terminals.rs",
        "const OUTPUT_QUEUE_CHUNKS: usize",
    ),
];

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
    let mut reached = Vec::new();
    for path in sources {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Test code is not user-facing text, and this very file quotes the name
        // it is banning.
        let lines = shipping_lines(&source);
        for (suffix, shipping_line) in MID_FILE_TEST_ITEM_FILES {
            if path.ends_with(suffix)
                && lines
                    .iter()
                    .any(|(_, line)| line.trim_start().starts_with(shipping_line))
            {
                reached.push(*suffix);
            }
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

    // A canary against this test going blind rather than green: each file below
    // has a `#[cfg(test)]` item above code that ships, so a scanner that stops at
    // the item, or swallows past it, reports no offenders because it read almost
    // nothing.
    let missed: Vec<String> = MID_FILE_TEST_ITEM_FILES
        .iter()
        .filter(|(suffix, _)| !reached.contains(suffix))
        .map(|(suffix, shipping_line)| format!("{suffix}: `{shipping_line}`"))
        .collect();
    assert!(
        missed.is_empty(),
        "the scan never reached these shipping lines, so it is skipping past a \
         `#[cfg(test)]` item and would miss anything below it:\n{}",
        missed.join("\n")
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

#[test]
fn shipping_lines_keeps_scanning_after_a_mid_file_test_item() {
    let source = "\
struct Holder {
    #[cfg(test)]
    test_only: Vec<String>,
    shipped: String,
}

#[cfg(test)]
#[derive(Debug)]
struct Helper {
    label: \"hidden Zed label\",
}

#[cfg(test)]
fn helper(
    argument: usize,
) -> usize {
    argument
}

#[cfg(test)]
use std::fmt;
const PLANTED: &str = \"Welcome to Zed\";
";
    let shown: Vec<&str> = shipping_lines(source)
        .into_iter()
        .map(|(_, line)| line)
        .collect();
    assert!(
        shown.contains(&"const PLANTED: &str = \"Welcome to Zed\";"),
        "the planted string after the test items was not scanned: {shown:?}"
    );
    assert!(shown.contains(&"    shipped: String,"));
    assert!(!shown.iter().any(|line| line.contains("hidden Zed label")));
    assert!(!shown.iter().any(|line| line.contains("test_only")));
}

/// Every line mentioning `hidden` belongs to a test item and must not be
/// scanned; every line mentioning `shipped` follows or sits beside one and must
/// be. Each shape is one way rustfmt closes a test item other than a lone `}`.
#[test]
fn shipping_lines_ends_a_test_item_where_its_layout_ends_it() {
    let shapes = [
        (
            "array closed by `];`",
            "#[cfg(test)]\nconst LABELS: [&str; 2] = [\n    \"hidden Zed label\",\n    \"hidden\",\n];",
        ),
        (
            "static whose initializer is on the next line",
            "#[cfg(test)]\nstatic LABEL: Label =\n    Label::new(\"hidden Zed label\");",
        ),
        (
            "initializer closed by `});`",
            "#[cfg(test)]\nstatic LABEL: LazyLock<Label> = LazyLock::new(|| {\n    Label::new(\"hidden Zed label\")\n});",
        ),
        (
            "call closed by `);`",
            "#[cfg(test)]\nconst LABEL: Label = Label::new(\n    \"hidden Zed label\",\n);",
        ),
        (
            "array element closed by `),`",
            "const LABELS: [Label; 2] = [\n    #[cfg(test)]\n    Label::new(\n        \"hidden Zed label\",\n    ),\n    Label::new(\"shipped\"),\n];",
        ),
        (
            "variant closed by `},`",
            "enum Label {\n    #[cfg(test)]\n    Variant {\n        hidden: \"Zed label\",\n    },\n    Other(&'static str), // shipped\n}",
        ),
        (
            "field with a trailing comment",
            "struct Holder {\n    #[cfg(test)]\n    only: &'static str, // hidden\n    shipped: String,\n}",
        ),
        (
            "`//` inside a string, then a trailing comment",
            "#[cfg(test)]\nconst LINK: &str = \"http://example.test/hidden Zed label\"; // test only",
        ),
        (
            "attribute and parameter on one line",
            "fn spawn(\n    path: &str,\n    #[cfg(test)] hidden: Checks,\n) -> Task<()> {\n    let shipped = \"Zed label\";\n    run(path)\n}",
        ),
        (
            "attribute and import on one line",
            "#[cfg(test)] use hidden::Label;",
        ),
        (
            "function with a where clause",
            "#[cfg(test)]\nfn gated<T>(\n    value: T,\n) -> T\nwhere\n    T: Clone,\n{\n    let hidden = \"Zed label\";\n    value.clone()\n}",
        ),
    ];
    let after = "const SHIPPED_AFTER: &str = \"Welcome to Zed\";";
    let mut wrong = Vec::new();
    for (shape, item) in shapes {
        let source = format!("{item}\n{after}\n");
        let shown: Vec<&str> = shipping_lines(&source)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        if !shown.contains(&after) {
            wrong.push(format!("{shape}: the line after the item was not scanned"));
        }
        for line in source.lines() {
            if line.contains("shipped") && !shown.contains(&line) {
                wrong.push(format!("{shape}: `{line}` was not scanned"));
            }
            if line.contains("hidden") && shown.contains(&line) {
                wrong.push(format!("{shape}: `{line}` was scanned"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
