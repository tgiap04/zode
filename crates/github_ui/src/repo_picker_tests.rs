use github::{
    DetectedRepository, RemoteName, RepositoryId, RepositoryRemotes, detect_repositories,
};

use project::git_store::{GitStoreEvent, RepositoryEvent, RepositoryId as GitRepositoryId};

use crate::repo_picker::{can_change_repositories, choose_active};

fn repository(owner: &str, name: &str) -> RepositoryId {
    RepositoryId::new(owner, name).expect("a valid repository")
}

fn choice(owner: &str, name: &str, remote: RemoteName) -> DetectedRepository {
    DetectedRepository {
        repository: repository(owner, name),
        remote,
    }
}

#[test]
fn the_pick_is_kept_while_offered_and_falls_back_to_the_first() {
    let choices = vec![
        choice("me", "repo", RemoteName::Origin),
        choice("octo", "repo", RemoteName::Upstream),
    ];
    assert_eq!(
        choose_active(&choices, Some(&repository("octo", "repo"))),
        Some(repository("octo", "repo"))
    );
    assert_eq!(
        choose_active(&choices, Some(&repository("gone", "repo"))),
        Some(repository("me", "repo"))
    );
    assert_eq!(
        choose_active(&choices, None),
        Some(repository("me", "repo"))
    );
    assert_eq!(choose_active(&[], Some(&repository("me", "repo"))), None);
}

/// Two git worktrees of one repository are one choice, and an SSH host alias
/// is not read as GitHub, so the tab explains instead of guessing.
#[test]
fn worktrees_of_one_repository_are_offered_once_and_aliases_not_at_all() {
    let worktree = RepositoryRemotes {
        origin_url: Some("git@github.com:me/repo.git".into()),
        upstream_url: None,
    };
    assert_eq!(
        detect_repositories(&[worktree.clone(), worktree]),
        vec![choice("me", "repo", RemoteName::Origin)]
    );
    let alias = RepositoryRemotes {
        origin_url: Some("git@github.com-work:corp/repo.git".into()),
        upstream_url: None,
    };
    assert!(detect_repositories(&[alias]).is_empty());
}

#[test]
fn only_events_that_can_change_the_repositories_rebuild_the_list() {
    let id = GitRepositoryId(1);
    let rebuilds = [
        GitStoreEvent::RepositoryAdded,
        GitStoreEvent::RepositoryRemoved(id),
        GitStoreEvent::RepositoryUpdated(id, RepositoryEvent::HeadChanged, true),
        GitStoreEvent::RepositoryUpdated(id, RepositoryEvent::BranchListChanged, false),
        GitStoreEvent::RepositoryUpdated(id, RepositoryEvent::GitWorktreeListChanged, false),
        // A reload that only moved a remote ends in a finished job.
        GitStoreEvent::JobsUpdated,
    ];
    for event in rebuilds {
        assert!(can_change_repositories(&event), "{event:?}");
    }
    let ignored = [
        GitStoreEvent::ActiveRepositoryChanged(None),
        GitStoreEvent::ConflictsUpdated,
        GitStoreEvent::IndexWriteError(anyhow::anyhow!("locked")),
        GitStoreEvent::RepositoryUpdated(id, RepositoryEvent::StatusesChanged, true),
        GitStoreEvent::RepositoryUpdated(id, RepositoryEvent::StashEntriesChanged, true),
    ];
    for event in ignored {
        assert!(!can_change_repositories(&event), "{event:?}");
    }
}
