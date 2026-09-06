//! Tests for the push-target decision.
//!
//! Pushing to the wrong remote is public and awkward to walk back, so the rule
//! that picks one is worth pinning down. All of it is decidable without I/O,
//! which is why it lives in a pure function.

use super::{PushTarget, choose_push_target};

fn remotes(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

/// A branch that already tracks something pushes there, whatever else exists.
#[test]
fn an_existing_upstream_wins() {
    let target = choose_push_target(
        Some("refs/remotes/upstream/main"),
        &remotes(&["origin", "upstream"]),
    );
    assert_eq!(
        target,
        PushTarget::Existing {
            remote: "upstream".into()
        }
    );
}

/// A tracked branch whose name contains slashes still resolves to the remote,
/// not to the first path segment of the branch.
#[test]
fn the_remote_is_read_from_the_upstream_ref_not_the_branch_name() {
    let target = choose_push_target(
        Some("refs/remotes/origin/release/1.0"),
        &remotes(&["origin"]),
    );
    assert_eq!(
        target,
        PushTarget::Existing {
            remote: "origin".into()
        }
    );
}

/// One remote and no upstream is the ordinary "publish this branch" case.
#[test]
fn a_single_remote_is_the_publish_target() {
    let target = choose_push_target(None, &remotes(&["origin"]));
    assert_eq!(
        target,
        PushTarget::Publish {
            remote: "origin".into()
        }
    );
}

/// Two remotes and no upstream: refuse to guess. Defaulting to `origin` here
/// would silently publish a fork's branch to the wrong place.
#[test]
fn two_remotes_and_no_upstream_is_undecidable() {
    let target = choose_push_target(None, &remotes(&["origin", "upstream"]));
    assert_eq!(
        target,
        PushTarget::Undecidable {
            remotes: remotes(&["origin", "upstream"])
        }
    );
}

/// No remote at all is also undecidable -- and the caller turns that into
/// "this repository has no remote", not a prompt with no options.
#[test]
fn no_remotes_is_undecidable_with_an_empty_list() {
    assert_eq!(
        choose_push_target(None, &[]),
        PushTarget::Undecidable { remotes: vec![] }
    );
}

/// An upstream ref that is not under `refs/remotes/` cannot name a remote, so
/// it falls through to the remote list rather than producing a bogus name.
#[test]
fn a_malformed_upstream_falls_through() {
    let target = choose_push_target(Some("refs/heads/main"), &remotes(&["origin"]));
    assert_eq!(
        target,
        PushTarget::Publish {
            remote: "origin".into()
        }
    );
}
