//! Holds the test harness's init list against the one the app actually runs.
//!
//! These are two separate lists in two separate files, and a crate registered in
//! only one of them is invisible: every test passes, and the shipped app has a
//! button wired to an action nothing handles. That is not hypothetical -- it is
//! how `container_ui` shipped a dead rail button, caught by a person clicking it
//! rather than by any test here.
//!
//! Scans source text on purpose. The two lists are code, not data, so there is
//! nothing to compare at runtime; the only place they can be held together is
//! where they are written.

use std::collections::BTreeSet;

/// Crates whose `init` genuinely belongs to only one of the two lists.
///
/// Each needs a reason. "It was already like that" is not one -- an entry here
/// is a claim that the app or the harness is right to differ.
const ALLOWED_TO_DIFFER: &[(&str, &str)] = &[
    // The harness builds its own `AppState` and drives windows itself, so the
    // app's startup-only wiring has no counterpart in it.
    ("auto_update", "checks GitHub on a timer; a test must not"),
    (
        "auto_update_ui",
        "surfaces auto_update, which the harness omits",
    ),
    (
        "reliability",
        "installs panic hooks around the real process",
    ),
    ("session", "restores windows from disk"),
    ("zlog_settings", "reconfigures the process logger"),
];

fn init_calls(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        // `foo::init(cx);` and `foo_ui::init(cx);`, but not `foo::bar::init(..)`
        // -- the second segment is what makes it a crate-level init.
        let Some(rest) = line.strip_suffix("::init(cx);") else {
            continue;
        };
        if rest.contains("::") || rest.is_empty() {
            continue;
        }
        if rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            found.insert(rest.to_string());
        }
    }
    found
}

#[test]
fn every_ui_crate_the_harness_initialises_is_also_initialised_by_the_app() {
    let app = init_calls(include_str!("../main.rs"));
    let harness = init_calls(include_str!("../zed.rs"));

    assert!(
        app.len() > 10,
        "only found {} init calls in main.rs; the scanner is not matching the \
         real list and would pass no matter what",
        app.len()
    );
    assert!(
        harness.len() > 10,
        "only found {} init calls in zed.rs; the scanner is not matching the \
         harness list and would pass no matter what",
        harness.len()
    );

    let excused: BTreeSet<&str> = ALLOWED_TO_DIFFER.iter().map(|(name, _)| *name).collect();

    let harness_only: Vec<&String> = harness
        .difference(&app)
        .filter(|name| !excused.contains(name.as_str()))
        .collect();
    assert!(
        harness_only.is_empty(),
        "these crates are initialised by the test harness in zed.rs but not by \
         the app in main.rs: {harness_only:?}\n\
         Every test touching them passes while the shipped app never registers \
         their actions -- a button wired to nothing. Add them to main.rs, or to \
         ALLOWED_TO_DIFFER with a reason."
    );
}
