use fs::{FakeFs, Fs};
use git::{
    Oid,
    repository::{GraphCommitData, InitialGraphCommitData, LogOrder, LogSource},
};
use gpui::{BackgroundExecutor, TestAppContext};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use util::path;

#[gpui::test]
async fn test_fake_worktree_lifecycle(cx: &mut TestAppContext) {
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/project", json!({".git": {}, "file.txt": "content"}))
        .await;
    let repo = fs
        .open_repo(Path::new("/project/.git"), None)
        .expect("should open fake repo");

    // Initially only the main worktree exists
    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].path, PathBuf::from("/project"));

    fs.create_dir("/my-worktrees".as_ref()).await.unwrap();
    let worktrees_dir = Path::new("/my-worktrees");

    // Create a worktree
    let worktree_1_dir = worktrees_dir.join("feature-branch");
    repo.create_worktree(
        git::repository::CreateWorktreeTarget::NewBranch {
            branch_name: "feature-branch".to_string(),
            base_sha: Some("abc123".to_string()),
        },
        worktree_1_dir.clone(),
    )
    .await
    .unwrap();

    // List worktrees — should have main + one created
    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].path, PathBuf::from("/project"));
    assert_eq!(worktrees[1].path, worktree_1_dir);
    assert_eq!(
        worktrees[1].ref_name,
        Some("refs/heads/feature-branch".into())
    );
    assert_eq!(worktrees[1].sha.as_ref(), "abc123");

    // Directory should exist in FakeFs after create
    assert!(fs.is_dir(&worktrees_dir.join("feature-branch")).await);

    // Create a second worktree (without explicit commit)
    let worktree_2_dir = worktrees_dir.join("bugfix-branch");
    repo.create_worktree(
        git::repository::CreateWorktreeTarget::NewBranch {
            branch_name: "bugfix-branch".to_string(),
            base_sha: None,
        },
        worktree_2_dir.clone(),
    )
    .await
    .unwrap();

    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 3);
    assert!(fs.is_dir(&worktree_2_dir).await);

    // Rename the first worktree
    repo.rename_worktree(worktree_1_dir, worktrees_dir.join("renamed-branch"))
        .await
        .unwrap();

    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 3);
    assert!(
        worktrees
            .iter()
            .any(|w| w.path == worktrees_dir.join("renamed-branch")),
    );
    assert!(
        worktrees
            .iter()
            .all(|w| w.path != worktrees_dir.join("feature-branch")),
    );

    // Directory should be moved in FakeFs after rename
    assert!(!fs.is_dir(&worktrees_dir.join("feature-branch")).await);
    assert!(fs.is_dir(&worktrees_dir.join("renamed-branch")).await);

    // Rename a nonexistent worktree should fail
    let result = repo
        .rename_worktree(PathBuf::from("/nonexistent"), PathBuf::from("/somewhere"))
        .await;
    assert!(result.is_err());

    // Remove a worktree
    repo.remove_worktree(worktrees_dir.join("renamed-branch"), false)
        .await
        .unwrap();

    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].path, PathBuf::from("/project"));
    assert_eq!(worktrees[1].path, worktree_2_dir);

    // Directory should be removed from FakeFs after remove
    assert!(!fs.is_dir(&worktrees_dir.join("renamed-branch")).await);

    // Remove a nonexistent worktree should fail
    let result = repo
        .remove_worktree(PathBuf::from("/nonexistent"), false)
        .await;
    assert!(result.is_err());

    // Remove the last worktree
    repo.remove_worktree(worktree_2_dir.clone(), false)
        .await
        .unwrap();

    let worktrees = repo.worktrees().await.unwrap();
    assert_eq!(worktrees.len(), 1);
    assert_eq!(worktrees[0].path, PathBuf::from("/project"));
    assert!(!fs.is_dir(&worktree_2_dir).await);
}

#[gpui::test]
async fn test_checkpoints(executor: BackgroundExecutor) {
    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/"),
        json!({
            "bar": {
                "baz": "qux"
            },
            "foo": {
                ".git": {},
                "a": "lorem",
                "b": "ipsum",
            },
        }),
    )
    .await;
    fs.with_git_state(Path::new("/foo/.git"), true, |_git| {})
        .unwrap();
    let repository = fs
        .open_repo(Path::new("/foo/.git"), Some("git".as_ref()))
        .unwrap();

    let checkpoint_1 = repository.checkpoint().await.unwrap();
    fs.write(Path::new("/foo/b"), b"IPSUM").await.unwrap();
    fs.write(Path::new("/foo/c"), b"dolor").await.unwrap();
    let checkpoint_2 = repository.checkpoint().await.unwrap();
    let checkpoint_3 = repository.checkpoint().await.unwrap();

    assert!(
        repository
            .compare_checkpoints(checkpoint_2.clone(), checkpoint_3.clone())
            .await
            .unwrap()
    );
    assert!(
        !repository
            .compare_checkpoints(checkpoint_1.clone(), checkpoint_2.clone())
            .await
            .unwrap()
    );

    repository
        .restore_checkpoint(checkpoint_1.clone())
        .await
        .unwrap();
    assert_eq!(
        fs.files_with_contents(Path::new("")),
        [
            (Path::new(path!("/bar/baz")).into(), b"qux".into()),
            (Path::new(path!("/foo/a")).into(), b"lorem".into()),
            (Path::new(path!("/foo/b")).into(), b"ipsum".into())
        ]
    );

    // diff_checkpoints: identical checkpoints produce empty diff
    let diff = repository
        .diff_checkpoints(checkpoint_2.clone(), checkpoint_3.clone())
        .await
        .unwrap();
    assert!(
        diff.is_empty(),
        "identical checkpoints should produce empty diff"
    );

    // diff_checkpoints: different checkpoints produce non-empty diff
    let diff = repository
        .diff_checkpoints(checkpoint_1.clone(), checkpoint_2.clone())
        .await
        .unwrap();
    assert!(diff.contains("b"), "diff should mention changed file 'b'");
    assert!(diff.contains("c"), "diff should mention added file 'c'");
}

#[gpui::test]
async fn test_fake_commit_data_reader_answers_configured_commits(executor: BackgroundExecutor) {
    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "a.txt": "a",
        }),
    )
    .await;
    let dot_git = Path::new(path!("/project/.git"));

    let root_sha = Oid::from_bytes(&[1; 20]).unwrap();
    let child_sha = Oid::from_bytes(&[2; 20]).unwrap();
    let unknown_sha = Oid::from_bytes(&[3; 20]).unwrap();
    fs.set_graph_commit_details(
        dot_git,
        vec![
            GraphCommitData {
                sha: root_sha,
                parents: Default::default(),
                author_name: "Ada".into(),
                author_email: "ada@example.com".into(),
                commit_timestamp: 1_700_000_000,
                subject: "Initial commit".into(),
            },
            GraphCommitData {
                sha: child_sha,
                parents: vec![root_sha].into(),
                author_name: "Grace".into(),
                author_email: "grace@example.com".into(),
                commit_timestamp: 1_700_000_100,
                subject: "Add feature".into(),
            },
        ],
    );

    let repository = fs.open_repo(dot_git, Some("git".as_ref())).unwrap();
    let reader = repository.commit_data_reader();
    assert!(
        reader.is_ok(),
        "the fake should hand out a commit data reader"
    );
    let reader = reader.unwrap();

    let root = reader.read(root_sha).await.unwrap();
    assert_eq!(root.sha, root_sha);
    assert!(root.parents.is_empty());
    assert_eq!(root.author_name.as_ref(), "Ada");
    assert_eq!(root.author_email.as_ref(), "ada@example.com");
    assert_eq!(root.commit_timestamp, 1_700_000_000);
    assert_eq!(root.subject.as_ref(), "Initial commit");

    let child = reader.read(child_sha).await.unwrap();
    assert_eq!(child.sha, child_sha);
    assert_eq!(child.parents.as_slice(), &[root_sha]);
    assert_eq!(child.author_name.as_ref(), "Grace");
    assert_eq!(child.author_email.as_ref(), "grace@example.com");
    assert_eq!(child.commit_timestamp, 1_700_000_100);
    assert_eq!(child.subject.as_ref(), "Add feature");

    let error = reader.read(unknown_sha).await.unwrap_err();
    assert!(
        error.to_string().contains(&unknown_sha.to_string()),
        "the error should name the missing commit, got: {error}"
    );
}

/// A log that dies partway still printed the commits before the failure, so the fake streams the
/// configured commits first and fails after them.
#[gpui::test]
async fn test_fake_graph_streams_its_commits_before_the_simulated_error(
    executor: BackgroundExecutor,
) {
    let fs = FakeFs::new(executor);
    fs.insert_tree(path!("/project"), json!({ ".git": {}, "a.txt": "a" }))
        .await;
    let dot_git = Path::new(path!("/project/.git"));
    let commits = (1..=3u8)
        .map(|byte| {
            Arc::new(InitialGraphCommitData {
                sha: Oid::from_bytes(&[byte; 20]).unwrap(),
                parents: Default::default(),
                ref_names: Vec::new(),
            })
        })
        .collect::<Vec<_>>();
    fs.set_graph_commits(dot_git, commits.clone());
    fs.set_graph_error(dot_git, Some("fatal: early EOF".into()));

    let repository = fs.open_repo(dot_git, Some("git".as_ref())).unwrap();
    let (request_tx, request_rx) = smol::channel::unbounded();
    let error = repository
        .initial_graph_data(LogSource::All, LogOrder::default(), request_tx)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("early EOF"), "got: {error}");

    let mut received = Vec::new();
    while let Ok(chunk) = request_rx.try_recv() {
        received.extend(chunk.iter().map(|commit| commit.sha));
    }
    assert_eq!(
        received,
        commits.iter().map(|commit| commit.sha).collect::<Vec<_>>(),
        "the commits before the failure should have been streamed"
    );
    assert_eq!(fs.graph_fetch_count(dot_git), 1);
}

#[gpui::test]
async fn test_fake_commit_data_reader_fails_while_an_error_is_simulated(
    executor: BackgroundExecutor,
) {
    let fs = FakeFs::new(executor);
    fs.insert_tree(path!("/project"), json!({ ".git": {}, "a.txt": "a" }))
        .await;
    let dot_git = Path::new(path!("/project/.git"));
    let repository = fs.open_repo(dot_git, Some("git".as_ref())).unwrap();

    fs.set_commit_data_error(dot_git, Some("cannot read objects".into()));
    let error = repository.commit_data_reader().err();
    assert!(
        error.is_some_and(|error| error.to_string().contains("cannot read objects")),
        "the simulated error should fail opening the reader"
    );

    fs.set_commit_data_error(dot_git, None);
    assert!(repository.commit_data_reader().is_ok());
}
