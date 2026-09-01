//! Tests for the row builder.
//!
//! Every assertion here is about what the reader sees: which rows exist, in what
//! order, and what a collapsed or filtered tree hides. Nothing here touches
//! GPUI or git, so a regression in the panel's shape fails in milliseconds
//! rather than only on screen.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use git::repository::{Branch, Worktree as GitWorktree};
use git::stash::StashEntry;
use gpui::SharedString;
use project::git_store::RepositoryId;

use super::{RepoData, RowKey, SectionKind, TreeRow, build_rows};

fn repo_id(n: u64) -> RepositoryId {
    RepositoryId(n)
}

fn branch(name: &str, remote: Option<&str>, is_head: bool) -> Branch {
    let ref_name = match remote {
        Some(remote) => format!("refs/remotes/{remote}/{name}"),
        None => format!("refs/heads/{name}"),
    };
    Branch {
        is_head,
        ref_name: ref_name.into(),
        upstream: None,
        most_recent_commit: None,
    }
}

fn worktree(name: &str) -> GitWorktree {
    GitWorktree {
        path: PathBuf::from(format!("/tmp/{name}")),
        ref_name: Some(format!("refs/heads/{name}").into()),
        sha: "abc123".into(),
        is_main: false,
        is_bare: false,
    }
}

fn tag(name: &str) -> git::repository::Tag {
    git::repository::Tag {
        name: name.to_string().into(),
        sha: "abc123".into(),
        is_annotated: false,
        message: None,
    }
}

fn stash(index: usize, message: &str) -> StashEntry {
    StashEntry {
        index,
        oid: Default::default(),
        message: message.to_string(),
        branch: None,
        timestamp: 0,
    }
}

fn repo(id: u64, name: &str) -> RepoData {
    RepoData {
        id: repo_id(id),
        path: Arc::from(PathBuf::from(format!("/repos/{name}")).as_path()),
        name: name.to_string().into(),
        current_branch: Some("main".into()),
        branches: vec![branch("main", None, true)],
        worktrees: Arc::from([]),
        stashes: Arc::from([]),
        tags: Arc::from([]),
        agents: Default::default(),
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
            TreeRow::Section { kind, count, .. } => format!("section:{}:{count}", kind.label()),
            TreeRow::RemoteGroup { remote, count, .. } => format!("group:{remote}:{count}"),
            TreeRow::Branch { branch, .. } => format!("branch:{}", branch.name()),
            TreeRow::Worktree { worktree, .. } => {
                format!("worktree:{}", super::worktree_label(worktree))
            }
            TreeRow::Stash { entry, .. } => format!("stash:{}", entry.message),
            TreeRow::Tag { tag, .. } => format!("tag:{}", tag.name),
            TreeRow::Empty { label } => format!("empty:{label}"),
        })
        .collect()
}

/// A repository the user has not opened contributes exactly one row. Anything
/// more and a monorepo with a dozen submodules is a wall of text on open.
#[test]
fn a_collapsed_repo_shows_only_its_own_row() {
    let rows = build_rows(&[repo(1, "app")], &opened(vec![]), "");
    assert_eq!(labels(&rows), vec!["repo:app"]);
}

/// Opening a repository reveals all four sections even when some are empty --
/// a missing "Stashes" header reads as a bug, not as "there are no stashes".
#[test]
fn an_opened_repo_shows_every_section() {
    let rows = build_rows(
        &[repo(1, "app")],
        &opened(vec![RowKey::Repo(repo_id(1))]),
        "",
    );
    assert_eq!(
        labels(&rows),
        vec![
            "repo:app",
            "section:Local:1",
            "section:Remote:0",
            "section:Worktrees:0",
            "section:Stashes:0",
            "section:Tags:0",
        ]
    );
}

/// An open but empty section says so in words. Without this row the user cannot
/// tell an empty section from one that failed to load.
#[test]
fn an_open_but_empty_section_says_so() {
    let rows = build_rows(
        &[repo(1, "app")],
        &opened(vec![
            RowKey::Repo(repo_id(1)),
            RowKey::Section(repo_id(1), SectionKind::Stashes),
        ]),
        "",
    );
    assert!(labels(&rows).contains(&"empty:No stashes".to_string()));
}

/// Two repositories are two independent subtrees: opening one must not open the
/// other, or a monorepo becomes unusable.
#[test]
fn repos_expand_independently() {
    let repos = vec![repo(1, "app"), repo(2, "lib")];
    let rows = build_rows(&repos, &opened(vec![RowKey::Repo(repo_id(2))]), "");
    let labels = labels(&rows);

    assert_eq!(labels[0], "repo:app");
    assert_eq!(
        labels[1], "repo:lib",
        "the first repo contributed one row only"
    );
    assert!(labels.iter().any(|l| l == "section:Local:1"));
}

/// Remote branches group under their remote, so `origin/main` and
/// `upstream/main` never sit side by side as two identical-looking rows.
#[test]
fn remote_branches_group_by_remote() {
    let mut data = repo(1, "app");
    data.branches = vec![
        branch("main", Some("origin"), false),
        branch("dev", Some("origin"), false),
        branch("main", Some("upstream"), false),
    ];

    let rows = build_rows(
        &[data],
        &opened(vec![
            RowKey::Repo(repo_id(1)),
            RowKey::Section(repo_id(1), SectionKind::Remote),
            RowKey::RemoteGroup(repo_id(1), SharedString::from("origin")),
        ]),
        "",
    );
    let labels = labels(&rows);

    assert!(labels.contains(&"group:origin:2".to_string()));
    assert!(labels.contains(&"group:upstream:1".to_string()));
    assert!(
        labels.contains(&"branch:origin/main".to_string()),
        "the opened origin group lists its branches"
    );
    assert!(
        !labels.contains(&"branch:upstream/main".to_string()),
        "the closed upstream group lists none"
    );
}

/// A filter opens everything it needs to: a match hidden behind a collapsed
/// section would make the search look broken.
#[test]
fn a_filter_reveals_matches_through_collapsed_sections() {
    let mut data = repo(1, "app");
    data.branches = vec![
        branch("main", None, true),
        branch("feature-login", None, false),
    ];

    let rows = build_rows(&[data], &opened(vec![]), "login");
    let labels = labels(&rows);

    assert!(labels.contains(&"branch:feature-login".to_string()));
    assert!(!labels.contains(&"branch:main".to_string()));
}

/// Under a filter, a section with no match contributes nothing at all -- the
/// panel should read as the answer to the query, not the whole tree with empty
/// headers.
#[test]
fn a_filter_drops_sections_with_no_match() {
    let mut data = repo(1, "app");
    data.branches = vec![branch("feature-login", None, false)];
    data.stashes = Arc::from([stash(0, "wip on something else")]);

    let rows = build_rows(&[data], &opened(vec![]), "login");
    let labels = labels(&rows);

    assert!(labels.contains(&"section:Local:1".to_string()));
    assert!(
        !labels.iter().any(|l| l.starts_with("section:Stashes")),
        "no stash matched, so the section is not drawn"
    );
}

/// Worktrees read by their branch name, not by their directory path.
#[test]
fn worktrees_read_by_branch_name() {
    let mut data = repo(1, "app");
    data.worktrees = Arc::from([worktree("hotfix")]);

    let rows = build_rows(
        &[data],
        &opened(vec![
            RowKey::Repo(repo_id(1)),
            RowKey::Section(repo_id(1), SectionKind::Worktrees),
        ]),
        "",
    );
    assert!(labels(&rows).contains(&"worktree:hotfix".to_string()));
}

/// The filter is case-insensitive: a user typing lowercase must find a branch
/// named in mixed case.
#[test]
fn the_filter_ignores_case() {
    let mut data = repo(1, "app");
    data.branches = vec![branch("Feature-LOGIN", None, false)];

    let rows = build_rows(&[data], &opened(vec![]), "login");
    assert!(labels(&rows).contains(&"branch:Feature-LOGIN".to_string()));
}

/// No repositories at all is a legitimate state (a project that is not a git
/// checkout), and must not panic or produce phantom rows.
#[test]
fn no_repositories_yields_no_rows() {
    assert!(build_rows(&[], &opened(vec![]), "").is_empty());
}

/// Tags are lazily loaded, so an unopened Tags section reports zero even for a
/// repository that has tags on disk. The count is of what is loaded, and that
/// is the honest thing to show before anyone asked for them.
#[test]
fn the_tags_section_reports_only_what_was_loaded() {
    let rows = build_rows(
        &[repo(1, "app")],
        &opened(vec![RowKey::Repo(repo_id(1))]),
        "",
    );
    assert!(labels(&rows).contains(&"section:Tags:0".to_string()));
}

/// Once tags are loaded they list under their own section, and the filter
/// reaches them like anything else.
#[test]
fn loaded_tags_list_and_filter() {
    let mut data = repo(1, "app");
    data.tags = Arc::from([tag("v1.0.0"), tag("v2.0.0")]);

    let rows = build_rows(
        &[data.clone()],
        &opened(vec![
            RowKey::Repo(repo_id(1)),
            RowKey::Section(repo_id(1), SectionKind::Tags),
        ]),
        "",
    );
    let all = labels(&rows);
    assert!(all.contains(&"tag:v1.0.0".to_string()));
    assert!(all.contains(&"tag:v2.0.0".to_string()));

    let filtered = labels(&build_rows(&[data], &opened(vec![]), "v2"));
    assert!(filtered.contains(&"tag:v2.0.0".to_string()));
    assert!(!filtered.contains(&"tag:v1.0.0".to_string()));
}

/// Agents on a branch.
///
/// They ride on the branch row rather than being rows of their own, because
/// the card's border has to enclose them -- a list that begins after the border
/// closes says they belong to something else. So the assertions here are about
/// what a branch row carries, not about how many rows exist.
mod agents {
    use super::*;
    use crate::branch_panel::tree::AgentEntry;
    use std::sync::Arc;
    use std::time::SystemTime;

    fn past(id: &str, label: &str) -> AgentEntry {
        AgentEntry::Past {
            label: label.to_string().into(),
            agent: "claude-acp".into(),
            id: Arc::from(id),
            updated_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn repo_with_agents(pairs: Vec<(&str, Vec<AgentEntry>)>) -> RepoData {
        let mut repo = repo(1, "zode");
        repo.branches = vec![branch("main", None, true), branch("feature", None, false)];
        repo.agents = pairs
            .into_iter()
            .map(|(name, entries)| (SharedString::from(name.to_string()), Arc::from(entries)))
            .collect();
        repo
    }

    fn all_open(repo: &RepoData) -> Vec<RowKey> {
        vec![
            RowKey::Repo(repo.id),
            RowKey::Section(repo.id, SectionKind::Local),
        ]
    }

    fn branch_row(rows: &[TreeRow], name: &str) -> (Arc<[AgentEntry]>, bool) {
        rows.iter()
            .find_map(|row| match row {
                TreeRow::Branch {
                    branch,
                    agents,
                    expanded,
                    ..
                } if branch.name() == name => Some((agents.clone(), *expanded)),
                _ => None,
            })
            .expect("the branch is listed")
    }

    /// No agents, no disclosure. A control that opens on nothing reads as
    /// broken.
    #[test]
    fn a_branch_with_no_agents_has_nothing_to_open() {
        let repo = repo_with_agents(vec![]);
        let rows = build_rows(std::slice::from_ref(&repo), &opened(all_open(&repo)), "");

        let branch_rows: Vec<_> = rows
            .iter()
            .filter(|row| matches!(row, TreeRow::Branch { .. }))
            .collect();
        assert!(!branch_rows.is_empty(), "the branches are still listed");
        for row in branch_rows {
            assert!(
                row.toggle_key().is_none(),
                "a branch with no agents must not offer a disclosure"
            );
        }
    }

    /// Closed is closed: the row knows how many there are so the card can say
    /// so, but it is not marked open, and the card draws none of them.
    #[test]
    fn a_closed_branch_knows_its_count_without_being_open() {
        let repo = repo_with_agents(vec![(
            "main",
            vec![past("a", "First"), past("b", "Second")],
        )]);
        let rows = build_rows(std::slice::from_ref(&repo), &opened(all_open(&repo)), "");

        let (agents, expanded) = branch_row(&rows, "main");
        assert_eq!(agents.len(), 2, "the count is on the row even when closed");
        assert!(!expanded);
    }

    #[test]
    fn an_open_branch_is_marked_open_and_carries_its_agents() {
        let repo = repo_with_agents(vec![(
            "main",
            vec![past("a", "First"), past("b", "Second")],
        )]);
        let mut open = all_open(&repo);
        open.push(RowKey::BranchAgents(repo.id, "main".into()));

        let rows = build_rows(std::slice::from_ref(&repo), &opened(open), "");

        let (agents, expanded) = branch_row(&rows, "main");
        assert!(expanded);
        let labels: Vec<_> = agents
            .iter()
            .map(|entry| entry.label().to_string())
            .collect();
        assert_eq!(labels, vec!["First", "Second"]);
    }

    /// Two branches, two sets. One must never carry the other's.
    #[test]
    fn agents_stay_on_the_branch_they_ran_on() {
        let repo = repo_with_agents(vec![
            ("main", vec![past("a", "On main")]),
            ("feature", vec![past("b", "On feature")]),
        ]);
        let rows = build_rows(std::slice::from_ref(&repo), &opened(all_open(&repo)), "");

        let (on_main, _) = branch_row(&rows, "main");
        let (on_feature, _) = branch_row(&rows, "feature");
        assert_eq!(on_main.len(), 1);
        assert_eq!(on_main[0].label().as_ref(), "On main");
        assert_eq!(on_feature.len(), 1);
        assert_eq!(on_feature[0].label().as_ref(), "On feature");
    }

    /// The row clones a refcount, never the entries. Gathering happens once per
    /// rebuild in `collect_repos`; a branch row deep-copying that list would
    /// put the cost back per row.
    #[test]
    fn a_branch_row_shares_the_gathered_list_rather_than_copying_it() {
        let repo = repo_with_agents(vec![("main", vec![past("a", "First")])]);
        let gathered = repo.agents.get("main").expect("gathered").clone();

        let rows = build_rows(std::slice::from_ref(&repo), &opened(all_open(&repo)), "");
        let (on_row, _) = branch_row(&rows, "main");

        assert!(
            Arc::ptr_eq(&gathered, &on_row),
            "the row must point at the gathered list, not a copy of it"
        );
    }
}
