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
            TreeRow::Agent { entry, .. } => format!("agent:{}", entry.label()),
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

/// Agents under a branch.
///
/// The transcripts record which branch they ran on, so this needs no worktree
/// and no window -- which is also why it can be asserted here rather than only
/// on screen.
mod agents {
    use super::*;
    use crate::branch_panel::tree::AgentEntry;

    fn past(id: &str, label: &str) -> AgentEntry {
        AgentEntry::Past {
            label: label.to_string().into(),
            id: std::sync::Arc::from(id),
        }
    }

    fn repo_with_agents(pairs: Vec<(&str, Vec<AgentEntry>)>) -> RepoData {
        let mut repo = repo(1, "zode");
        repo.branches = vec![branch("main", None, true), branch("feature", None, false)];
        repo.agents = pairs
            .into_iter()
            .map(|(name, entries)| (SharedString::from(name.to_string()), entries))
            .collect();
        repo
    }

    fn all_open(repo: &RepoData) -> Vec<RowKey> {
        vec![
            RowKey::Repo(repo.id),
            RowKey::Section(repo.id, SectionKind::Local),
        ]
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

    /// Closed, a branch with agents is still one row -- the children are not
    /// built, so the list costs what is shown.
    #[test]
    fn a_closed_branch_builds_no_agent_rows() {
        let repo = repo_with_agents(vec![(
            "main",
            vec![past("a", "First"), past("b", "Second")],
        )]);
        let rows = build_rows(std::slice::from_ref(&repo), &opened(all_open(&repo)), "");

        assert!(
            !labels(&rows)
                .iter()
                .any(|label| label.starts_with("agent:")),
            "the branch is closed, so its agents are not rows yet"
        );
        let count = rows.iter().find_map(|row| match row {
            TreeRow::Branch {
                branch,
                agent_count,
                ..
            } if branch.name() == "main" => Some(*agent_count),
            _ => None,
        });
        assert_eq!(count, Some(2), "the count is on the row even when closed");
    }

    #[test]
    fn an_open_branch_lists_its_agents_beneath_it() {
        let repo = repo_with_agents(vec![(
            "main",
            vec![past("a", "First"), past("b", "Second")],
        )]);
        let mut open = all_open(&repo);
        open.push(RowKey::BranchAgents(repo.id, "main".into()));

        let rows = build_rows(std::slice::from_ref(&repo), &opened(open), "");

        let labels = labels(&rows);
        let branch_at = labels.iter().position(|l| l == "branch:main").unwrap();
        assert_eq!(labels[branch_at + 1], "agent:First");
        assert_eq!(labels[branch_at + 2], "agent:Second");
    }

    /// Two branches, two sets. Opening one must not spill the other's agents.
    #[test]
    fn agents_stay_under_the_branch_they_ran_on() {
        let repo = repo_with_agents(vec![
            ("main", vec![past("a", "On main")]),
            ("feature", vec![past("b", "On feature")]),
        ]);
        let mut open = all_open(&repo);
        open.push(RowKey::BranchAgents(repo.id, "feature".into()));

        let labels = labels(&build_rows(std::slice::from_ref(&repo), &opened(open), ""));

        assert!(labels.contains(&"agent:On feature".to_string()));
        assert!(
            !labels.contains(&"agent:On main".to_string()),
            "only the branch that was opened contributes agent rows"
        );
    }

    /// An agent row sits one level in from its branch, so the tree reads as a
    /// tree rather than as a flat list with odd labels in it.
    #[test]
    fn an_agent_row_is_indented_under_its_branch() {
        let repo = repo_with_agents(vec![("main", vec![past("a", "First")])]);
        let mut open = all_open(&repo);
        open.push(RowKey::BranchAgents(repo.id, "main".into()));

        let rows = build_rows(std::slice::from_ref(&repo), &opened(open), "");
        let branch_depth = rows
            .iter()
            .find_map(|row| match row {
                TreeRow::Branch { branch, depth, .. } if branch.name() == "main" => Some(*depth),
                _ => None,
            })
            .unwrap();
        let agent_depth = rows
            .iter()
            .find_map(|row| match row {
                TreeRow::Agent { depth, .. } => Some(*depth),
                _ => None,
            })
            .unwrap();

        assert_eq!(agent_depth, branch_depth + 1);
    }
}
