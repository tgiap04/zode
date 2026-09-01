//! Tests for the branch service.
//!
//! `process_branches` decides the order and membership of every branch list the
//! user ever sees, in both the modal and the panel. It was untested while it
//! lived inside the picker; these pin the behaviour before anything else leans
//! on it.

use super::*;
use git::repository::{CommitSummary, Upstream, UpstreamTracking};

fn branch(name: &str, is_head: bool, remote: Option<&str>, timestamp: Option<i64>) -> Branch {
    let ref_name = match remote {
        Some(remote) => format!("refs/remotes/{remote}/{name}"),
        None => format!("refs/heads/{name}"),
    };
    Branch {
        is_head,
        ref_name: ref_name.into(),
        upstream: None,
        most_recent_commit: timestamp.map(|commit_timestamp| CommitSummary {
            sha: "abc123".into(),
            commit_timestamp,
            author_name: "Test Author".into(),
            subject: "Test commit".into(),
            has_parent: true,
        }),
    }
}

fn tracking(mut local: Branch, remote_ref: &str) -> Branch {
    local.upstream = Some(Upstream {
        ref_name: remote_ref.to_string().into(),
        tracking: UpstreamTracking::Gone,
    });
    local
}

/// The head is what the reader is looking for first, whatever its commit
/// date says. Sorting purely by recency would bury it under any branch that
/// happened to be committed to later.
#[test]
fn the_head_branch_sorts_first_regardless_of_recency() {
    let branches: Arc<[Branch]> = vec![
        branch("newer", false, None, Some(200)),
        branch("head", true, None, Some(100)),
    ]
    .into();

    let processed = process_branches(&branches);

    assert_eq!(processed[0].name(), "head");
    assert_eq!(processed[1].name(), "newer");
}

/// Among non-head branches the most recently committed comes first, so the
/// list reads as "what I was just working on" rather than alphabetically.
#[test]
fn the_rest_sort_most_recent_first() {
    let branches: Arc<[Branch]> = vec![
        branch("older", false, None, Some(100)),
        branch("newest", false, None, Some(300)),
        branch("middle", false, None, Some(200)),
    ]
    .into();

    let processed = process_branches(&branches);

    let names: Vec<_> = processed.iter().map(|b| b.name().to_string()).collect();
    assert_eq!(names, vec!["newest", "middle", "older"]);
}

/// A remote branch that a local branch already tracks is redundant: showing
/// both `main` and `origin/main` makes the reader pick between two names for
/// one thing. The local one wins and the remote is dropped.
#[test]
fn a_remote_branch_already_tracked_by_a_local_one_is_dropped() {
    let local = tracking(
        branch("main", true, None, Some(100)),
        "refs/remotes/origin/main",
    );
    let branches: Arc<[Branch]> = vec![
        local,
        branch("main", false, Some("origin"), Some(100)),
        branch("solo", false, Some("origin"), Some(50)),
    ]
    .into();

    let processed = process_branches(&branches);

    let names: Vec<_> = processed.iter().map(|b| b.ref_name.to_string()).collect();
    assert_eq!(
        names,
        vec!["refs/heads/main", "refs/remotes/origin/solo"],
        "the tracked remote ref is folded into its local branch; an untracked one survives"
    );
}

/// A branch with no commit data must not vanish or panic the sort -- a fresh
/// orphan branch is exactly the case a user is most likely to be looking for.
#[test]
fn a_branch_without_commit_data_survives() {
    let branches: Arc<[Branch]> = vec![
        branch("with-commit", false, None, Some(100)),
        branch("no-commit", false, None, None),
    ]
    .into();

    let processed = process_branches(&branches);

    assert_eq!(processed.len(), 2);
}

#[test]
fn an_empty_ref_list_yields_an_empty_result() {
    let branches: Arc<[Branch]> = Vec::new().into();
    assert!(process_branches(&branches).is_empty());
}
