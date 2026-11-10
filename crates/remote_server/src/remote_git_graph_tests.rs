use crate::{headless_project::HeadlessProject, remote_editing_tests::init_test};
use fs::FakeFs;
use git::{
    Oid,
    repository::{GraphCommitData, InitialGraphCommitData, LogOrder, LogSource},
};
use gpui::{Entity, Subscription, TestAppContext};
use project::{
    Project,
    git_store::{CommitDataState, GitGraphEvent, Repository, RepositoryEvent},
};
use rpc::{AnyProtoClient, proto};
use serde_json::json;
use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};
use util::path;

type GraphKey = (LogSource, LogOrder);
type RecordedGraphEvents = Rc<RefCell<Vec<(GraphKey, GitGraphEvent)>>>;

const DOT_GIT: &str = path!("/code/project1/.git");

fn oid(index: usize) -> Oid {
    let mut bytes = [0u8; 20];
    bytes[..8].copy_from_slice(&(index as u64 + 1).to_be_bytes());
    Oid::from_bytes(&bytes).unwrap()
}

/// Newest first, as `git log` prints it, with refs on a few commits.
fn linear_history(count: usize) -> Vec<Arc<InitialGraphCommitData>> {
    (0..count)
        .map(|index| {
            Arc::new(InitialGraphCommitData {
                sha: oid(index),
                parents: if index + 1 < count {
                    vec![oid(index + 1)].into()
                } else {
                    Default::default()
                },
                ref_names: match index {
                    0 => vec!["HEAD -> main".into(), "origin/main".into()],
                    1_200 => vec!["tag: v1.0".into()],
                    _ => Vec::new(),
                },
            })
        })
        .collect()
}

fn commit_details(commit: &InitialGraphCommitData, index: usize) -> GraphCommitData {
    GraphCommitData {
        sha: commit.sha,
        parents: commit.parents.clone(),
        author_name: format!("Author {index}").into(),
        author_email: format!("author{index}@example.com").into(),
        commit_timestamp: 1_700_000_000 + index as i64,
        subject: format!("Commit number {index}").into(),
    }
}

async fn init_remote_repository(
    fs: &Arc<FakeFs>,
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) -> (Entity<Project>, Entity<HeadlessProject>, Entity<Repository>) {
    let (project, headless) = init_test(fs, cx, server_cx).await;
    project
        .update(cx, |project, cx| {
            project.find_or_create_worktree(path!("/code/project1"), true, cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();
    let repository = project.update(cx, |project, cx| project.active_repository(cx).unwrap());
    (project, headless, repository)
}

async fn new_fake_fs(server_cx: &mut TestAppContext) -> Arc<FakeFs> {
    let fs = FakeFs::new(server_cx.executor());
    fs.insert_tree(
        path!("/code"),
        json!({
            "project1": {
                ".git": {},
                "README.md": "# project 1",
            },
        }),
    )
    .await;
    fs
}

fn record_graph_events(
    repository: &Entity<Repository>,
    cx: &mut TestAppContext,
) -> (RecordedGraphEvents, Subscription) {
    let events = RecordedGraphEvents::default();
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(repository, move |_, event: &RepositoryEvent, _| {
            if let RepositoryEvent::GraphEvent(key, graph_event) = event {
                events.borrow_mut().push((key.clone(), graph_event.clone()));
            }
        })
    });
    (events, subscription)
}

fn graph_events_for(events: &RecordedGraphEvents, key: &GraphKey) -> Vec<GitGraphEvent> {
    events
        .borrow()
        .iter()
        .filter(|(event_key, _)| event_key == key)
        .map(|(_, event)| event.clone())
        .collect()
}

fn load_graph(repository: &Entity<Repository>, key: &GraphKey, cx: &mut TestAppContext) {
    repository.update(cx, |repository, cx| {
        repository.graph_data(key.0.clone(), key.1, 0..usize::MAX, cx);
    });
    cx.run_until_parked();
}

/// The first request only opens the reader, so ask twice before reading the settled state.
fn settled_commit_data(
    repository: &Entity<Repository>,
    sha: Oid,
    cx: &mut TestAppContext,
) -> CommitDataState {
    for _ in 0..2 {
        repository.update(cx, |repository, cx| {
            repository.fetch_commit_data(sha, cx);
        });
        cx.run_until_parked();
    }
    repository.update(cx, |repository, cx| {
        repository.fetch_commit_data(sha, cx).clone()
    })
}

fn proto_client(project: &Entity<Project>, cx: &mut TestAppContext) -> AnyProtoClient {
    project.read_with(cx, |project, cx| {
        project
            .remote_client()
            .expect("a remote project has a remote client")
            .read(cx)
            .proto_client()
    })
}

fn graph_page_request(
    repository_id: u64,
    offset: u64,
    limit: u64,
    generation: Option<u64>,
) -> proto::GitGetGraphPage {
    proto::GitGetGraphPage {
        project_id: proto::REMOTE_SERVER_PROJECT_ID,
        repository_id,
        log_source: Some(proto::GitLogSource {
            source: Some(proto::git_log_source::Source::Branch("main".into())),
        }),
        log_order: proto::GitLogOrder::DateOrder.into(),
        offset,
        limit,
        generation,
    }
}

#[gpui::test]
async fn test_remote_graph_streams_every_commit_in_pages(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    let history = linear_history(2_500);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    let (_project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;
    let (events, _subscription) = record_graph_events(&repository, cx);

    for log_source in [LogSource::Branch("main".into()), LogSource::All] {
        let key = (log_source, LogOrder::default());
        load_graph(&repository, &key, cx);

        repository.read_with(cx, |repository, _| {
            let data = repository
                .get_graph_data(key.0.clone(), key.1)
                .expect("the graph should have an entry once asked for");
            assert_eq!(data.error, None);
            assert!(!data.is_loading());
            assert_eq!(data.commit_data.len(), history.len());
            for (received, expected) in data.commit_data.iter().zip(&history) {
                assert_eq!(received.sha, expected.sha);
                assert_eq!(received.parents, expected.parents);
                assert_eq!(received.ref_names, expected.ref_names);
            }
            assert_eq!(
                data.commit_oid_to_index.get(&history[1_200].sha),
                Some(&1_200)
            );
        });

        let graph_events = graph_events_for(&events, &key);
        let counts = graph_events
            .iter()
            .filter_map(|event| match event {
                GitGraphEvent::CountUpdated(count) => Some(*count),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            counts.len() >= 3,
            "the graph should arrive in several pages, got counts {counts:?}"
        );
        assert!(
            counts.windows(2).all(|pair| pair[0] < pair[1]),
            "counts should only grow, got {counts:?}"
        );
        assert_eq!(counts.last(), Some(&history.len()));
        assert_eq!(graph_events.last(), Some(&GitGraphEvent::FullyLoaded));
        assert_eq!(
            graph_events
                .iter()
                .filter(|event| **event == GitGraphEvent::FullyLoaded)
                .count(),
            1
        );
    }
}

#[gpui::test]
async fn test_remote_commit_details_load_and_report_missing_commits(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    let history = linear_history(3);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    fs.set_graph_commit_details(
        Path::new(DOT_GIT),
        history
            .iter()
            .enumerate()
            .map(|(index, commit)| commit_details(commit, index))
            .collect(),
    );
    let (_project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;

    let CommitDataState::Loaded(details) = settled_commit_data(&repository, history[1].sha, cx)
    else {
        panic!("details of a known commit should load");
    };
    assert_eq!(details.sha, history[1].sha);
    assert_eq!(details.parents, history[1].parents);
    assert_eq!(details.author_name.as_ref(), "Author 1");
    assert_eq!(details.author_email.as_ref(), "author1@example.com");
    assert_eq!(details.commit_timestamp, 1_700_000_001);
    assert_eq!(details.subject.as_ref(), "Commit number 1");

    let unknown_sha = oid(9_999);
    let state = settled_commit_data(&repository, unknown_sha, cx);
    assert!(
        matches!(state, CommitDataState::Failed(_)),
        "an unknown commit should fail rather than load forever"
    );
}

/// A request that failed as a whole says nothing about the commits in it, unlike a commit the server
/// could not find, so those commits have to be asked for again rather than marked failed for good.
#[gpui::test]
async fn test_remote_commit_details_are_asked_again_after_a_failed_request(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    let history = linear_history(3);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    fs.set_graph_commit_details(
        Path::new(DOT_GIT),
        history
            .iter()
            .enumerate()
            .map(|(index, commit)| commit_details(commit, index))
            .collect(),
    );
    fs.set_commit_data_error(Path::new(DOT_GIT), Some("cannot read objects".into()));
    let (_project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;

    let state = settled_commit_data(&repository, history[1].sha, cx);
    assert!(
        !matches!(state, CommitDataState::Loaded(_)),
        "details cannot load while the server fails every request"
    );

    fs.set_commit_data_error(Path::new(DOT_GIT), None);
    let CommitDataState::Loaded(details) = settled_commit_data(&repository, history[1].sha, cx)
    else {
        panic!("once the server answers again, the details should load");
    };
    assert_eq!(details.subject.as_ref(), "Commit number 1");
}

#[gpui::test]
async fn test_remote_graph_reports_git_errors_and_retries(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    fs.set_graph_error(Path::new(DOT_GIT), Some("fatal: bad revision".into()));
    let (_project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;
    let (events, _subscription) = record_graph_events(&repository, cx);
    let key = (LogSource::Branch("main".into()), LogOrder::default());

    load_graph(&repository, &key, cx);
    repository.read_with(cx, |repository, _| {
        let data = repository
            .get_graph_data(key.0.clone(), key.1)
            .expect("the graph should have an entry once asked for");
        let error = data.error.clone().expect("the git error should be kept");
        assert!(
            error.contains("fatal: bad revision"),
            "the error should carry git's message, got: {error}"
        );
        assert!(!data.is_loading());
    });
    assert_eq!(
        graph_events_for(&events, &key),
        vec![GitGraphEvent::LoadingError]
    );

    let history = linear_history(10);
    fs.set_graph_error(Path::new(DOT_GIT), None);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    cx.run_until_parked();
    events.borrow_mut().clear();

    load_graph(&repository, &key, cx);
    assert_eq!(
        graph_events_for(&events, &key),
        Vec::new(),
        "asking for a failed graph again must not refetch it"
    );
    assert_eq!(fs.graph_fetch_count(Path::new(DOT_GIT)), 1);

    repository.update(cx, |repository, cx| {
        repository.retry_graph_data(key.0.clone(), key.1, cx);
    });
    cx.run_until_parked();
    assert_eq!(fs.graph_fetch_count(Path::new(DOT_GIT)), 2);
    repository.read_with(cx, |repository, _| {
        let data = repository
            .get_graph_data(key.0.clone(), key.1)
            .expect("the retried graph should have an entry");
        assert_eq!(data.error, None);
        assert!(!data.is_loading());
        assert_eq!(data.commit_data.len(), history.len());
    });
    assert_eq!(
        graph_events_for(&events, &key).last(),
        Some(&GitGraphEvent::FullyLoaded)
    );
}

#[gpui::test]
async fn test_remote_graph_requests_are_validated(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    fs.set_graph_commits(Path::new(DOT_GIT), linear_history(3));
    let (project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;
    let client = proto_client(&project, cx);
    let repository_id = repository.read_with(cx, |repository, _| repository.id.to_proto());

    let error = client
        .request(graph_page_request(9_999, 0, 10, None))
        .await
        .expect_err("an unknown repository should be refused");
    assert!(
        error.to_string().contains("missing repository handle"),
        "got: {error}"
    );

    let error = client
        .request(proto::GitGetCommitData {
            project_id: proto::REMOTE_SERVER_PROJECT_ID,
            repository_id: 9_999,
            shas: vec![oid(0).to_string()],
        })
        .await
        .expect_err("an unknown repository should be refused");
    assert!(
        error.to_string().contains("missing repository handle"),
        "got: {error}"
    );

    let error = client
        .request(proto::GitGetCommitData {
            project_id: proto::REMOTE_SERVER_PROJECT_ID,
            repository_id,
            shas: vec!["--not-a-sha".into()],
        })
        .await
        .expect_err("a malformed sha should be refused");
    assert!(error.to_string().contains("--not-a-sha"), "got: {error}");

    let error = client
        .request(proto::GitGetCommitData {
            project_id: proto::REMOTE_SERVER_PROJECT_ID,
            repository_id,
            shas: (0..257).map(|index| oid(index).to_string()).collect(),
        })
        .await
        .expect_err("an oversized batch should be refused");
    assert!(error.to_string().contains("257"), "got: {error}");

    let page = client
        .request(graph_page_request(repository_id, 0, 0, None))
        .await
        .expect("a zero limit is raised to one commit");
    assert_eq!(page.commits.len(), 1);
}

#[gpui::test]
async fn test_remote_graph_does_not_hang_across_a_disconnect(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    let history = linear_history(10);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    let (project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;
    let key = (LogSource::Branch("main".into()), LogOrder::default());

    let remote_client = project.read_with(cx, |project, _| {
        project
            .remote_client()
            .expect("a remote project has a remote client")
    });
    remote_client
        .update(cx, |client, cx| client.simulate_disconnect(cx))
        .detach();
    load_graph(&repository, &key, cx);

    repository.read_with(cx, |repository, _| {
        let data = repository
            .get_graph_data(key.0.clone(), key.1)
            .expect("the graph should have an entry once asked for");
        assert!(!data.is_loading(), "the fetch should not hang");
        // The mock connection reconnects, and the paging requests are resent across it.
        assert_eq!(data.error, None);
        assert_eq!(
            data.commit_data
                .iter()
                .map(|commit| commit.sha)
                .collect::<Vec<_>>(),
            history.iter().map(|commit| commit.sha).collect::<Vec<_>>()
        );
    });
}

#[gpui::test]
async fn test_remote_graph_refuses_a_stale_generation(
    cx: &mut TestAppContext,
    server_cx: &mut TestAppContext,
) {
    let fs = new_fake_fs(server_cx).await;
    let history = linear_history(30);
    fs.set_graph_commits(Path::new(DOT_GIT), history.clone());
    let (project, _headless, repository) = init_remote_repository(&fs, cx, server_cx).await;
    let client = proto_client(&project, cx);
    let repository_id = repository.read_with(cx, |repository, _| repository.id.to_proto());

    let first_page = client
        .request(graph_page_request(repository_id, 0, 10, None))
        .await
        .expect("the first page should load");
    assert_eq!(first_page.commits.len(), 10);
    assert_eq!(first_page.commits[0].sha, history[0].sha.to_string());
    assert!(!first_page.is_complete);

    let error = client
        .request(graph_page_request(
            repository_id,
            10,
            10,
            Some(first_page.generation + 1),
        ))
        .await
        .expect_err("a page of another generation should be refused");
    assert!(
        error.to_string().contains("history changed"),
        "got: {error}"
    );

    let second_page = client
        .request(graph_page_request(
            repository_id,
            10,
            10,
            Some(first_page.generation),
        ))
        .await
        .expect("the matching generation should keep paging");
    assert_eq!(second_page.commits[0].sha, history[10].sha.to_string());

    fs.set_branch_name(Path::new(DOT_GIT), Some("other"));
    cx.run_until_parked();

    let error = client
        .request(graph_page_request(
            repository_id,
            20,
            10,
            Some(first_page.generation),
        ))
        .await
        .expect_err("a page of a history the server dropped should be refused");
    assert!(
        error.to_string().contains("history changed"),
        "got: {error}"
    );
}
