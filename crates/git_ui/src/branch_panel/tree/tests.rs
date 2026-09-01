//! Tests for the row builder.
//!
//! Every assertion here is about what the reader sees: which rows exist, in
//! what order, and what a collapsed or filtered panel hides. Nothing here
//! touches GPUI or git, so a regression in the panel's shape fails in
//! milliseconds rather than only on screen.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use git::repository::{Branch, Worktree as GitWorktree};
use project::git_store::RepositoryId;

use super::{AgentEntry, RepoData, RowKey, TreeRow, build_rows};

fn repo_id(n: u64) -> RepositoryId {
    RepositoryId(n)
}

fn branch(name: &str, is_head: bool) -> Branch {
    Branch {
        is_head,
        ref_name: format!("refs/heads/{name}").into(),
        upstream: None,
        most_recent_commit: None,
    }
}

fn worktree(path: &str, branch: Option<&str>, is_main: bool) -> GitWorktree {
    GitWorktree {
        path: PathBuf::from(path),
        ref_name: branch.map(|name| format!("refs/heads/{name}").into()),
        sha: "abc123".into(),
        is_main,
        is_bare: false,
    }
}

fn agent(label: &str) -> AgentEntry {
    AgentEntry::Past {
        label: label.to_string().into(),
        agent: "claude-acp".into(),
        id: Arc::from(label),
        updated_at: SystemTime::UNIX_EPOCH,
    }
}

fn repo(id: u64, name: &str, worktrees: Vec<GitWorktree>) -> RepoData {
    RepoData {
        id: repo_id(id),
        path: Arc::from(PathBuf::from(format!("/repos/{name}")).as_path()),
        name: name.to_string().into(),
        current_branch: Some("main".into()),
        branches: vec![branch("main", true)],
        agents: Default::default(),
        worktrees: Arc::from(worktrees),
    }
}

/// A closure over a set, matching what the panel passes in.
fn opened(keys: Vec<RowKey>) -> impl Fn(&RowKey) -> bool {
    let set: HashSet<RowKey> = keys.into_iter().collect();
    move |key: &RowKey| set.contains(key)
}

fn labels(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match row {
            TreeRow::Repo { name, .. } => format!("repo:{name}"),
            TreeRow::Worktree { worktree, .. } => {
                format!("checkout:{}", super::worktree_label(worktree))
            }
            TreeRow::Empty { label } => format!("empty:{label}"),
        })
        .collect()
}

fn card<'a>(rows: &'a [TreeRow], label: &str) -> &'a TreeRow {
    rows.iter()
        .find(|row| match row {
            TreeRow::Worktree { worktree, .. } => super::worktree_label(worktree) == label,
            _ => false,
        })
        .expect("the checkout is listed")
}

fn agents_of(row: &TreeRow) -> (Arc<[AgentEntry]>, bool) {
    match row {
        TreeRow::Worktree {
            agents, expanded, ..
        } => (agents.clone(), *expanded),
        _ => panic!("not a checkout row"),
    }
}

/// The main checkout is a checkout. `git worktree list` names it, so it gets a
/// card like any other -- a panel that only showed *linked* worktrees would be
/// empty for every repository nobody has branched yet.
#[test]
fn the_main_checkout_gets_a_card() {
    let repo = repo(1, "zode", vec![worktree("/repos/zode", Some("main"), true)]);
    let rows = build_rows(
        std::slice::from_ref(&repo),
        &opened(vec![RowKey::Repo(repo.id)]),
        "",
    );

    assert_eq!(labels(&rows), vec!["repo:zode", "checkout:main"]);
}

#[test]
fn every_checkout_gets_its_own_card() {
    let repo = repo(
        1,
        "zode",
        vec![
            worktree("/repos/zode", Some("main"), true),
            worktree("/wt/feature", Some("feature"), false),
        ],
    );
    let rows = build_rows(
        std::slice::from_ref(&repo),
        &opened(vec![RowKey::Repo(repo.id)]),
        "",
    );

    assert_eq!(
        labels(&rows),
        vec!["repo:zode", "checkout:main", "checkout:feature"]
    );
}

/// Luật 3: a closed repository contributes exactly one row, however many
/// checkouts it holds. The children are not built, not merely not drawn.
#[test]
fn a_collapsed_repository_is_one_row() {
    let repo = repo(
        1,
        "zode",
        vec![
            worktree("/repos/zode", Some("main"), true),
            worktree("/wt/a", Some("a"), false),
            worktree("/wt/b", Some("b"), false),
        ],
    );

    let rows = build_rows(std::slice::from_ref(&repo), &opened(vec![]), "");

    assert_eq!(labels(&rows), vec!["repo:zode"]);
}

/// A detached checkout has no branch to name it, so the directory does.
#[test]
fn a_detached_checkout_falls_back_to_its_directory_name() {
    let repo = repo(1, "zode", vec![worktree("/wt/west-isle", None, false)]);
    let rows = build_rows(
        std::slice::from_ref(&repo),
        &opened(vec![RowKey::Repo(repo.id)]),
        "",
    );

    assert_eq!(labels(&rows), vec!["repo:zode", "checkout:west-isle"]);
}

/// `git worktree list` always names the main checkout, so an empty list means
/// something went wrong. Saying so beats a blank panel.
#[test]
fn a_repository_with_no_checkouts_says_so() {
    let repo = repo(1, "zode", vec![]);
    let rows = build_rows(
        std::slice::from_ref(&repo),
        &opened(vec![RowKey::Repo(repo.id)]),
        "",
    );

    assert_eq!(labels(&rows), vec!["repo:zode", "empty:No checkouts found"]);
}

mod filtering {
    use super::*;

    #[test]
    fn a_filter_matches_the_branch_name() {
        let repo = repo(
            1,
            "zode",
            vec![
                worktree("/repos/zode", Some("main"), true),
                worktree("/wt/feature", Some("feature"), false),
            ],
        );
        let rows = build_rows(std::slice::from_ref(&repo), &opened(vec![]), "feat");

        assert_eq!(labels(&rows), vec!["repo:zode", "checkout:feature"]);
    }

    /// The path is what a checkout is, so it is searchable too -- two
    /// checkouts of the same branch differ only by where they are.
    #[test]
    fn a_filter_matches_the_path() {
        let repo = repo(
            1,
            "zode",
            vec![
                worktree("/repos/zode", Some("main"), true),
                worktree("/wt/west-isle", Some("main"), false),
            ],
        );
        let rows = build_rows(std::slice::from_ref(&repo), &opened(vec![]), "west");

        assert_eq!(labels(&rows), vec!["repo:zode", "checkout:main"]);
    }

    /// A filter forces the repository open, or a match would sit hidden behind
    /// a collapsed row and the search would look broken.
    #[test]
    fn a_filter_opens_a_collapsed_repository() {
        let repo = repo(
            1,
            "zode",
            vec![worktree("/wt/feature", Some("feature"), false)],
        );

        let rows = build_rows(std::slice::from_ref(&repo), &opened(vec![]), "feature");

        assert_eq!(labels(&rows), vec!["repo:zode", "checkout:feature"]);
    }

    /// A repository with no match drops out entirely rather than sitting there
    /// as a header over nothing.
    #[test]
    fn a_repository_with_no_match_disappears() {
        let repos = vec![
            repo(1, "zode", vec![worktree("/repos/zode", Some("main"), true)]),
            repo(2, "other", vec![worktree("/other", Some("feature"), true)]),
        ];

        let rows = build_rows(&repos, &opened(vec![]), "feature");

        assert_eq!(labels(&rows), vec!["repo:other", "checkout:feature"]);
    }
}

mod agents {
    use super::*;

    fn repo_with_agents(pairs: Vec<(&str, Vec<AgentEntry>)>) -> RepoData {
        let mut repo = repo(
            1,
            "zode",
            vec![
                worktree("/repos/zode", Some("main"), true),
                worktree("/wt/feature", Some("feature"), false),
            ],
        );
        repo.agents = pairs
            .into_iter()
            .map(|(path, entries)| {
                (
                    Arc::from(std::path::Path::new(path)),
                    Arc::from(entries) as Arc<[AgentEntry]>,
                )
            })
            .collect();
        repo
    }

    /// No agents, no disclosure. A control that opens on nothing reads as
    /// broken.
    #[test]
    fn a_checkout_with_no_agents_has_nothing_to_open() {
        let repo = repo_with_agents(vec![]);
        let rows = build_rows(
            std::slice::from_ref(&repo),
            &opened(vec![RowKey::Repo(repo.id)]),
            "",
        );

        for row in &rows {
            if matches!(row, TreeRow::Worktree { .. }) {
                assert!(row.toggle_key().is_none());
            }
        }
    }

    /// Closed is closed: the row knows how many there are so the card can say
    /// so, but it is not marked open.
    #[test]
    fn a_closed_checkout_knows_its_count_without_being_open() {
        let repo = repo_with_agents(vec![("/repos/zode", vec![agent("First"), agent("Second")])]);
        let rows = build_rows(
            std::slice::from_ref(&repo),
            &opened(vec![RowKey::Repo(repo.id)]),
            "",
        );

        let (agents, expanded) = agents_of(card(&rows, "main"));
        assert_eq!(agents.len(), 2);
        assert!(!expanded);
    }

    #[test]
    fn an_opened_checkout_is_marked_open() {
        let repo = repo_with_agents(vec![("/repos/zode", vec![agent("First")])]);
        let open = vec![
            RowKey::Repo(repo.id),
            RowKey::WorktreeAgents(repo.id, Arc::from(std::path::Path::new("/repos/zode"))),
        ];

        let rows = build_rows(std::slice::from_ref(&repo), &opened(open), "");

        let (_, expanded) = agents_of(card(&rows, "main"));
        assert!(expanded);
    }

    /// Two checkouts, two sets. One must never carry the other's -- that is the
    /// whole reason the key is the path.
    #[test]
    fn agents_stay_in_the_checkout_they_ran_in() {
        let repo = repo_with_agents(vec![
            ("/repos/zode", vec![agent("On main")]),
            ("/wt/feature", vec![agent("On feature")]),
        ]);
        let rows = build_rows(
            std::slice::from_ref(&repo),
            &opened(vec![RowKey::Repo(repo.id)]),
            "",
        );

        let (on_main, _) = agents_of(card(&rows, "main"));
        let (on_feature, _) = agents_of(card(&rows, "feature"));
        assert_eq!(on_main.len(), 1);
        assert_eq!(on_main[0].label().as_ref(), "On main");
        assert_eq!(on_feature.len(), 1);
        assert_eq!(on_feature[0].label().as_ref(), "On feature");
    }

    /// The row clones a refcount, never the entries. Gathering happens once per
    /// rebuild; a row deep-copying that list would put the cost back per row.
    #[test]
    fn a_card_shares_the_gathered_list_rather_than_copying_it() {
        let repo = repo_with_agents(vec![("/repos/zode", vec![agent("First")])]);
        let gathered = repo
            .agents
            .get(std::path::Path::new("/repos/zode"))
            .expect("gathered")
            .clone();

        let rows = build_rows(
            std::slice::from_ref(&repo),
            &opened(vec![RowKey::Repo(repo.id)]),
            "",
        );
        let (on_card, _) = agents_of(card(&rows, "main"));

        assert!(Arc::ptr_eq(&gathered, &on_card));
    }
}

/// Which checkouts the panel lists.
///
/// The panel showed only the *other* checkouts for a while, because
/// `linked_worktrees` leaves out the one the repository is open at and nothing
/// put it back. With one worktree that meant a single card that switched you to
/// it, whereupon the list showed the one you had just left -- two one-item
/// lists bouncing off each other.
mod checkouts {
    use super::*;
    use crate::branch_panel::tree::all_checkouts;
    use std::path::Path;

    #[test]
    fn the_checkout_this_window_is_in_is_listed_first() {
        let linked = vec![worktree("/wt/feature", Some("feature"), false)];

        let all = all_checkouts(
            Path::new("/repos/zode"),
            Some(&branch("develop", true)),
            Some("abc123".into()),
            &linked,
        );

        assert_eq!(all.len(), 2);
        assert_eq!(all[0].path, PathBuf::from("/repos/zode"));
        assert_eq!(all[1].path, PathBuf::from("/wt/feature"));
    }

    /// Standing in the main checkout: nothing else claims `is_main`, so this
    /// one is it.
    #[test]
    fn the_current_checkout_is_main_when_no_other_claims_it() {
        let linked = vec![worktree("/wt/feature", Some("feature"), false)];

        let all = all_checkouts(Path::new("/repos/zode"), None, None, &linked);

        assert!(all[0].is_main);
    }

    /// Standing in a linked worktree: the main one is in the list and says so,
    /// which is how we know this one is not.
    #[test]
    fn the_current_checkout_is_not_main_when_another_claims_it() {
        let linked = vec![worktree("/repos/zode", Some("develop"), true)];

        let all = all_checkouts(Path::new("/wt/feature"), None, None, &linked);

        assert!(!all[0].is_main, "the checkout we are in is the linked one");
        assert!(all[1].is_main);
    }

    /// A repository with no linked worktrees still has one checkout, and the
    /// panel has to show it -- otherwise every project without a worktree gets
    /// an empty panel.
    #[test]
    fn a_repository_with_no_linked_worktrees_still_lists_one() {
        let all = all_checkouts(
            Path::new("/repos/zode"),
            Some(&branch("develop", true)),
            None,
            &[],
        );

        assert_eq!(all.len(), 1);
        assert!(all[0].is_main);
        assert_eq!(super::super::worktree_label(&all[0]), "develop");
    }

    /// A detached checkout has no branch to name it, so the directory does.
    #[test]
    fn a_detached_current_checkout_falls_back_to_its_directory() {
        let all = all_checkouts(Path::new("/wt/glad-prism"), None, None, &[]);

        assert_eq!(super::super::worktree_label(&all[0]), "glad-prism");
    }
}
