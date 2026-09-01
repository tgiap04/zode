//! Tests for the two decisions that stand between a click and losing work.
//!
//! Both are pure functions on purpose. The prompt they drive needs a window to
//! exercise; the *rule* it enforces does not, and the rule is the part that
//! must never quietly change.

use git::status::{GitSummary, TrackedSummary};

use super::{local_name_for_remote_branch, summary_is_dirty};

fn summary(
    index: TrackedSummary,
    worktree: TrackedSummary,
    conflict: usize,
    untracked: usize,
) -> GitSummary {
    GitSummary {
        index,
        worktree,
        conflict,
        untracked,
        count: 0,
    }
}

fn tracked(added: usize, modified: usize, deleted: usize) -> TrackedSummary {
    TrackedSummary {
        added,
        modified,
        deleted,
    }
}

/// A clean checkout switches branches without asking. Prompting here would
/// train the user to dismiss the prompt without reading it.
#[test]
fn a_clean_repository_is_not_dirty() {
    assert!(!summary_is_dirty(summary(
        tracked(0, 0, 0),
        tracked(0, 0, 0),
        0,
        0
    )));
}

/// Untracked files alone are NOT a reason to stop. git carries them across a
/// checkout untouched, so stopping for them is a false alarm on every project
/// with a build directory -- and a prompt that cries wolf is worse than none.
#[test]
fn untracked_files_alone_do_not_count_as_dirty() {
    assert!(!summary_is_dirty(summary(
        tracked(0, 0, 0),
        tracked(0, 0, 0),
        0,
        42
    )));
}

/// Anything tracked and modified, staged or not, is work a checkout could
/// destroy.
#[test]
fn tracked_changes_count_as_dirty() {
    assert!(summary_is_dirty(summary(
        tracked(0, 1, 0),
        tracked(0, 0, 0),
        0,
        0
    )));
    assert!(summary_is_dirty(summary(
        tracked(0, 0, 0),
        tracked(0, 1, 0),
        0,
        0
    )));
    assert!(summary_is_dirty(summary(
        tracked(0, 0, 1),
        tracked(0, 0, 0),
        0,
        0
    )));
}

/// A conflict is the state where a careless checkout does the most damage.
#[test]
fn a_conflict_counts_as_dirty() {
    assert!(summary_is_dirty(summary(
        tracked(0, 0, 0),
        tracked(0, 0, 0),
        1,
        0
    )));
}

/// `origin/main` checks out as `main`.
#[test]
fn the_remote_prefix_comes_off() {
    assert_eq!(local_name_for_remote_branch("origin/main"), "main");
}

/// A branch name with slashes keeps them: only the remote segment is removed.
/// Getting this wrong would check out `1.0` instead of `release/1.0`.
#[test]
fn only_the_remote_segment_is_removed() {
    assert_eq!(
        local_name_for_remote_branch("origin/release/1.0"),
        "release/1.0"
    );
    assert_eq!(
        local_name_for_remote_branch("upstream/feature/auth/login"),
        "feature/auth/login"
    );
}

/// A name with no remote prefix is returned unchanged rather than mangled.
#[test]
fn a_name_without_a_remote_is_left_alone() {
    assert_eq!(local_name_for_remote_branch("main"), "main");
}
