//! The installed-extension list as a synced artifact.
//!
//! # Why this is not a file, and not JSON
//!
//! `settings.json` and `keymap.json` are files the user edits, so they travel
//! byte for byte and their comments survive. The extension list is neither: it
//! is derived from what is installed, and it exists to be compared.
//!
//! So it travels as one identifier per line, sorted. That makes the diff the
//! user is shown a per-extension diff — one line added is one extension added
//! — rather than a diff of JSON punctuation.
//!
//! # Why only identifiers
//!
//! No versions and no sources. Pinning a version through sync would add a
//! second thing an attacker who reached the ciphertext could steer, and buys
//! nothing: every install goes through the registry regardless.
//!
//! # The rule that matters
//!
//! **Pulling never installs anything.** This module produces a comparison; the
//! decision to install belongs to the person reading it. A sync payload that
//! could install code is a supply-chain hole with a user's own account as the
//! key.

/// Marks the format, so a future change is a refusal rather than a
/// misinterpretation.
const HEADER: &str = "# zode-extensions v1";

/// Renders the installed set for storage.
///
/// Sorted and deduplicated, because the ordering the extension store happens
/// to return is not stable — and an unstable rendering means every push
/// produces a diff against a list that did not actually change.
pub fn render(installed: impl IntoIterator<Item = String>) -> String {
    let mut ids: Vec<String> = installed.into_iter().collect();
    ids.sort();
    ids.dedup();

    let mut out = String::from(HEADER);
    for id in ids {
        out.push('\n');
        out.push_str(&id);
    }
    out.push('\n');
    out
}

/// Reads a stored list back. Unknown header → `None`, never a guess.
pub fn parse(stored: &str) -> Option<Vec<String>> {
    let mut lines = stored.lines();
    if lines.next()?.trim() != HEADER {
        return None;
    }
    Some(
        lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// What a pulled list means for this machine.
#[derive(Debug, PartialEq, Eq)]
pub struct ExtensionComparison {
    /// Stored elsewhere, not installed here. Offered — never installed.
    pub missing: Vec<String>,
    /// Installed here, not in the stored list.
    pub extra: Vec<String>,
}

pub fn compare(installed: &[String], stored: &[String]) -> ExtensionComparison {
    let here: std::collections::BTreeSet<&String> = installed.iter().collect();
    let there: std::collections::BTreeSet<&String> = stored.iter().collect();

    ExtensionComparison {
        missing: there.difference(&here).map(|id| (*id).clone()).collect(),
        extra: here.difference(&there).map(|id| (*id).clone()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_stable_regardless_of_input_order() {
        let one = render(["b/two".to_string(), "a/one".to_string()]);
        let other = render(["a/one".to_string(), "b/two".to_string()]);
        assert_eq!(
            one, other,
            "an unstable rendering means a phantom diff on every push"
        );
    }

    #[test]
    fn duplicates_collapse() {
        assert_eq!(
            render(["a/one".to_string(), "a/one".to_string()]),
            render(["a/one".to_string()]),
        );
    }

    #[test]
    fn round_trips() {
        let ids = vec!["a/one".to_string(), "b/two".to_string()];
        assert_eq!(parse(&render(ids.clone())).unwrap(), ids);
    }

    #[test]
    fn an_empty_list_round_trips() {
        assert_eq!(
            parse(&render(Vec::<String>::new())).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unknown_format_is_refused_rather_than_guessed() {
        assert_eq!(parse("# zode-extensions v2\nfoo/bar"), None);
        assert_eq!(parse("foo/bar"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn comparison_names_both_directions() {
        let installed = vec!["here/only".to_string(), "both/sides".to_string()];
        let stored = vec!["both/sides".to_string(), "there/only".to_string()];

        assert_eq!(
            compare(&installed, &stored),
            ExtensionComparison {
                missing: vec!["there/only".to_string()],
                extra: vec!["here/only".to_string()],
            },
        );
    }
}
