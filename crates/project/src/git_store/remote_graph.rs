use super::*;
use git::repository::GRAPH_CHUNK_SIZE;

/// Most shas the server reads in one `GitGetCommitData` request.
const MAX_COMMIT_DATA_SHAS: usize = 256;

const HISTORY_CHANGED: &str = "the history changed while loading the graph";

pub(super) fn log_source_to_proto(log_source: &LogSource) -> proto::GitLogSource {
    let source = match log_source {
        LogSource::All => proto::git_log_source::Source::All(true),
        LogSource::Branch(branch) => proto::git_log_source::Source::Branch(branch.to_string()),
        LogSource::Sha(sha) => proto::git_log_source::Source::Sha(sha.to_string()),
    };
    proto::GitLogSource {
        source: Some(source),
    }
}

pub(super) fn log_source_from_proto(log_source: &proto::GitLogSource) -> Result<LogSource> {
    match log_source.source.as_ref().context("missing log source")? {
        proto::git_log_source::Source::All(_) => Ok(LogSource::All),
        proto::git_log_source::Source::Branch(branch) => {
            // The branch becomes an argument of `git log`, where a leading dash is an option.
            anyhow::ensure!(!branch.starts_with('-'), "invalid branch name {branch:?}");
            Ok(LogSource::Branch(branch.clone().into()))
        }
        proto::git_log_source::Source::Sha(sha) => Ok(LogSource::Sha(parse_sha(sha)?)),
    }
}

pub(super) fn log_order_to_proto(log_order: LogOrder) -> proto::GitLogOrder {
    match log_order {
        LogOrder::DateOrder => proto::GitLogOrder::DateOrder,
        LogOrder::TopoOrder => proto::GitLogOrder::TopoOrder,
        LogOrder::AuthorDateOrder => proto::GitLogOrder::AuthorDateOrder,
        LogOrder::ReverseChronological => proto::GitLogOrder::ReverseChronological,
    }
}

pub(super) fn log_order_from_proto(log_order: proto::GitLogOrder) -> LogOrder {
    match log_order {
        proto::GitLogOrder::DateOrder => LogOrder::DateOrder,
        proto::GitLogOrder::TopoOrder => LogOrder::TopoOrder,
        proto::GitLogOrder::AuthorDateOrder => LogOrder::AuthorDateOrder,
        proto::GitLogOrder::ReverseChronological => LogOrder::ReverseChronological,
    }
}

pub(super) fn graph_commit_to_proto(commit: &InitialGraphCommitData) -> proto::GitGraphCommit {
    proto::GitGraphCommit {
        sha: commit.sha.to_string(),
        parents: commit
            .parents
            .iter()
            .map(|parent| parent.to_string())
            .collect(),
        ref_names: commit
            .ref_names
            .iter()
            .map(|ref_name| ref_name.to_string())
            .collect(),
    }
}

pub(super) fn graph_commit_from_proto(
    commit: proto::GitGraphCommit,
) -> Result<Arc<InitialGraphCommitData>> {
    Ok(Arc::new(InitialGraphCommitData {
        sha: parse_sha(&commit.sha)?,
        parents: commit
            .parents
            .iter()
            .map(|parent| parse_sha(parent))
            .collect::<Result<_>>()?,
        ref_names: commit
            .ref_names
            .into_iter()
            .map(SharedString::from)
            .collect(),
    }))
}

pub(super) fn commit_data_to_proto(commit: &GraphCommitData) -> proto::GitGraphCommitData {
    proto::GitGraphCommitData {
        sha: commit.sha.to_string(),
        parents: commit
            .parents
            .iter()
            .map(|parent| parent.to_string())
            .collect(),
        author_name: commit.author_name.to_string(),
        author_email: commit.author_email.to_string(),
        commit_timestamp: commit.commit_timestamp,
        subject: commit.subject.to_string(),
    }
}

pub(super) fn commit_data_from_proto(commit: proto::GitGraphCommitData) -> Result<GraphCommitData> {
    Ok(GraphCommitData {
        sha: parse_sha(&commit.sha)?,
        parents: commit
            .parents
            .iter()
            .map(|parent| parse_sha(parent))
            .collect::<Result<_>>()?,
        author_name: commit.author_name.into(),
        author_email: commit.author_email.into(),
        commit_timestamp: commit.commit_timestamp,
        subject: commit.subject.into(),
    })
}

/// What the user is told when a graph request to the remote server failed.
fn remote_graph_error_message(error: &anyhow::Error) -> SharedString {
    crate::remote_request_error_message(error, "Reconnect to update it to show the git graph.")
        .into()
}

fn parse_sha(sha: &str) -> Result<Oid> {
    Oid::from_str(sha).with_context(|| format!("invalid commit sha {sha:?}"))
}

/// Asks the server for details of `shas` in one request. A commit the server could not read
/// comes back as an error; a request that failed as a whole is the error of the call.
pub(super) async fn read_remote_commit_data(
    client: &AnyProtoClient,
    project_id: ProjectId,
    repository_id: RepositoryId,
    shas: Vec<Oid>,
) -> Result<Vec<(Oid, Result<GraphCommitData, SharedString>)>> {
    let response = client
        .request(proto::GitGetCommitData {
            project_id: project_id.to_proto(),
            repository_id: repository_id.to_proto(),
            shas: shas.iter().map(|sha| sha.to_string()).collect(),
        })
        .await?;
    let mut commits_by_sha = response
        .commits
        .into_iter()
        .filter_map(|commit| commit_data_from_proto(commit).log_err())
        .map(|commit| (commit.sha, commit))
        .collect::<HashMap<_, _>>();
    Ok(shas
        .into_iter()
        .map(|sha| {
            let commit = commits_by_sha
                .remove(&sha)
                .ok_or_else(|| SharedString::from("commit not found"));
            (sha, commit)
        })
        .collect())
}

pub(super) struct GraphPage {
    commits: Vec<Arc<InitialGraphCommitData>>,
    is_complete: bool,
    generation: u64,
}

impl Repository {
    /// Pages through the server's copy of the graph, appending each page as it arrives.
    pub(super) async fn remote_git_graph_data(
        this: WeakEntity<Self>,
        client: AnyProtoClient,
        project_id: ProjectId,
        repository_id: RepositoryId,
        log_source: LogSource,
        log_order: LogOrder,
        cx: &mut AsyncApp,
    ) -> Result<(), SharedString> {
        let proto_log_source = log_source_to_proto(&log_source);
        let key = (log_source, log_order);
        let mut offset = 0;
        let mut generation = None;
        loop {
            let response = client
                .request(proto::GitGetGraphPage {
                    project_id: project_id.to_proto(),
                    repository_id: repository_id.to_proto(),
                    log_source: Some(proto_log_source.clone()),
                    log_order: log_order_to_proto(log_order).into(),
                    offset: offset as u64,
                    limit: GRAPH_CHUNK_SIZE as u64,
                    generation,
                })
                .await
                .map_err(|error| remote_graph_error_message(&error))?;
            if generation.is_some_and(|generation| generation != response.generation) {
                return Err(HISTORY_CHANGED.into());
            }
            generation = Some(response.generation);

            let commits = response
                .commits
                .into_iter()
                .map(graph_commit_from_proto)
                .collect::<Result<Vec<_>>>()
                .map_err(|error| SharedString::from(error.to_string()))?;
            if commits.is_empty() {
                if response.is_complete {
                    return Ok(());
                }
                // The server answers only once it has a commit past `offset` or the log ended,
                // so asking again would spin.
                return Err("the remote server sent an empty page of the graph".into());
            }

            offset += commits.len();
            this.update(cx, |repository, cx| {
                repository.append_graph_commits(&key, commits, cx)
            })
            .map_err(|error| SharedString::from(error.to_string()))?;
            if response.is_complete {
                return Ok(());
            }
        }
    }

    /// Serves one page of the graph from this repository's own cache, waiting until it holds a
    /// commit past `offset` or its fetch has ended. A first page (`generation` of `None`) starts
    /// the fetch, retrying one that failed; later pages must name the generation they continue.
    fn graph_page(
        &mut self,
        log_source: LogSource,
        log_order: LogOrder,
        offset: usize,
        limit: usize,
        generation: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Task<Result<GraphPage>> {
        let key = (log_source, log_order);
        if generation.is_none() {
            // A client asks for a first page only when it starts a fetch of its own, which it does
            // once per log unless its user asked to reload, so a failed log is fetched again here.
            self.retry_graph_data(key.0.clone(), key.1, cx);
        }
        let waiter = self
            .initial_graph_data
            .get(&key)
            .map(|data| data.waiters.clone());

        let (wake_tx, wake_rx) = smol::channel::bounded::<()>(1);
        let subscription = cx.subscribe_self({
            let key = key.clone();
            move |_, event: &RepositoryEvent, _| {
                let concerns_page = match event {
                    RepositoryEvent::GraphEvent(event_key, _) => *event_key == key,
                    // These may drop the cached graph without any graph event.
                    _ => true,
                };
                if concerns_page {
                    // A full channel already holds a wake-up, which is all the waiter needs.
                    wake_tx.try_send(()).ok();
                }
            }
        });

        cx.spawn(async move |this, cx| {
            let _subscription = subscription;
            let _waiter = waiter;
            loop {
                let page = this.update(cx, |repository, cx| {
                    repository.ready_graph_page(&key, offset, limit, generation, cx)
                })??;
                if let Some(page) = page {
                    return Ok(page);
                }
                wake_rx
                    .recv()
                    .await
                    .context("the repository was released while loading the graph")?;
            }
        })
    }

    fn ready_graph_page(
        &mut self,
        key: &(LogSource, LogOrder),
        offset: usize,
        limit: usize,
        generation: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Result<Option<GraphPage>> {
        if !self.initial_graph_data.contains_key(key) {
            anyhow::ensure!(generation.is_none(), HISTORY_CHANGED);
            self.graph_data(key.0.clone(), key.1, 0..0, cx);
        }
        let used_at = post_inc(&mut self.graph_use_clock);
        let data = self
            .initial_graph_data
            .get_mut(key)
            .context("the graph is missing from the cache")?;
        data.last_used = used_at;
        anyhow::ensure!(
            generation.is_none_or(|generation| generation == data.generation),
            HISTORY_CHANGED
        );

        let commits = data
            .commit_data
            .get(offset..)
            .unwrap_or_default()
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        if !commits.is_empty() {
            let is_complete = !data.is_loading
                && data.error.is_none()
                && offset + commits.len() >= data.commit_data.len();
            return Ok(Some(GraphPage {
                commits,
                is_complete,
                generation: data.generation,
            }));
        }
        if let Some(error) = &data.error {
            bail!("{error}");
        }
        if data.is_loading {
            return Ok(None);
        }
        Ok(Some(GraphPage {
            commits,
            is_complete: true,
            generation: data.generation,
        }))
    }
}

impl GitStore {
    pub(super) async fn handle_get_graph_page(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::GitGetGraphPage>,
        mut cx: AsyncApp,
    ) -> Result<proto::GitGetGraphPageResponse> {
        let repository_id = RepositoryId::from_proto(envelope.payload.repository_id);
        let repository_handle = Self::repository_for_request(&this, repository_id, &mut cx)?;
        let log_source = log_source_from_proto(
            envelope
                .payload
                .log_source
                .as_ref()
                .context("missing log source")?,
        )?;
        let log_order = log_order_from_proto(envelope.payload.log_order());
        let offset = usize::try_from(envelope.payload.offset)?;
        let limit = usize::try_from(envelope.payload.limit)
            .unwrap_or(GRAPH_CHUNK_SIZE)
            .clamp(1, GRAPH_CHUNK_SIZE);
        let generation = envelope.payload.generation;

        let page = repository_handle
            .update(&mut cx, |repository, cx| {
                repository.graph_page(log_source, log_order, offset, limit, generation, cx)
            })
            .await?;

        Ok(proto::GitGetGraphPageResponse {
            commits: page
                .commits
                .iter()
                .map(|commit| graph_commit_to_proto(commit))
                .collect(),
            is_complete: page.is_complete,
            generation: page.generation,
        })
    }

    pub(super) async fn handle_get_commit_data(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::GitGetCommitData>,
        mut cx: AsyncApp,
    ) -> Result<proto::GitGetCommitDataResponse> {
        let repository_id = RepositoryId::from_proto(envelope.payload.repository_id);
        let repository_handle = Self::repository_for_request(&this, repository_id, &mut cx)?;
        let requested = envelope.payload.shas.len();
        anyhow::ensure!(
            requested <= MAX_COMMIT_DATA_SHAS,
            "{requested} commits requested, at most {MAX_COMMIT_DATA_SHAS} are read at once"
        );
        let shas = envelope
            .payload
            .shas
            .iter()
            .map(|sha| parse_sha(sha))
            .collect::<Result<Vec<_>>>()?;

        let repository_state = repository_handle
            .read_with(&cx, |repository, _| repository.repository_state.clone())
            .await
            .map_err(|error| anyhow!(error))?;
        let RepositoryState::Local(LocalRepositoryState { backend, .. }) = repository_state else {
            bail!("commit details are only read where the repository lives");
        };
        let reader = backend.commit_data_reader()?;

        let mut commits = Vec::with_capacity(shas.len());
        for sha in shas {
            match reader.read(sha).await {
                Ok(commit) => commits.push(commit_data_to_proto(&commit)),
                Err(error) => log::warn!("failed to read commit data for {sha}: {error:?}"),
            }
        }
        Ok(proto::GitGetCommitDataResponse { commits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        Oid::from_bytes(&[byte; 20]).unwrap()
    }

    #[test]
    fn log_source_round_trips_through_proto() {
        for log_source in [
            LogSource::All,
            LogSource::Branch("feature/x".into()),
            LogSource::Sha(oid(7)),
        ] {
            let round_tripped = log_source_from_proto(&log_source_to_proto(&log_source)).unwrap();
            assert_eq!(round_tripped, log_source);
        }
    }

    #[test]
    fn a_request_an_old_server_cannot_handle_asks_for_a_reconnect() {
        for request in ["GitGetGraphPage", "GitGetCommitData"] {
            let error =
                anyhow!("RPC request {request} failed: no handler registered for {request}");
            assert_eq!(
                remote_graph_error_message(&error).as_ref(),
                "The remote server is older than this version of Zode. Reconnect to update it to show the git graph.",
                "{request}"
            );
        }
        assert_eq!(
            remote_graph_error_message(&anyhow!("connection lost")).as_ref(),
            "connection lost"
        );
    }

    #[test]
    fn log_order_round_trips_through_proto() {
        for log_order in [
            LogOrder::DateOrder,
            LogOrder::TopoOrder,
            LogOrder::AuthorDateOrder,
            LogOrder::ReverseChronological,
        ] {
            assert_eq!(
                log_order_from_proto(log_order_to_proto(log_order)),
                log_order
            );
        }
    }

    #[test]
    fn log_source_with_a_bad_sha_is_rejected() {
        let log_source = proto::GitLogSource {
            source: Some(proto::git_log_source::Source::Sha("not-a-sha".into())),
        };
        assert!(log_source_from_proto(&log_source).is_err());
    }

    #[test]
    fn log_source_without_a_source_is_rejected() {
        let log_source = proto::GitLogSource { source: None };
        assert!(log_source_from_proto(&log_source).is_err());
    }

    #[test]
    fn branch_that_reads_as_an_option_is_rejected() {
        let log_source = proto::GitLogSource {
            source: Some(proto::git_log_source::Source::Branch(
                "--output=/tmp/x".into(),
            )),
        };
        assert!(log_source_from_proto(&log_source).is_err());
    }

    #[test]
    fn graph_commit_round_trips_through_proto() {
        let commit = InitialGraphCommitData {
            sha: oid(1),
            parents: vec![oid(2), oid(3)].into(),
            ref_names: vec!["HEAD -> main".into(), "tag: v1.0".into()],
        };
        let round_tripped = graph_commit_from_proto(graph_commit_to_proto(&commit)).unwrap();
        assert_eq!(round_tripped.sha, commit.sha);
        assert_eq!(round_tripped.parents, commit.parents);
        assert_eq!(round_tripped.ref_names, commit.ref_names);
    }

    #[test]
    fn graph_commit_with_a_bad_parent_is_rejected() {
        let commit = proto::GitGraphCommit {
            sha: oid(1).to_string(),
            parents: vec!["zz".into()],
            ref_names: Vec::new(),
        };
        assert!(graph_commit_from_proto(commit).is_err());
    }

    #[test]
    fn commit_data_round_trips_through_proto() {
        let commit = GraphCommitData {
            sha: oid(4),
            parents: vec![oid(5)].into(),
            author_name: "Ada".into(),
            author_email: "ada@example.com".into(),
            commit_timestamp: 1_700_000_000,
            subject: "Fix the thing".into(),
        };
        let round_tripped = commit_data_from_proto(commit_data_to_proto(&commit)).unwrap();
        assert_eq!(round_tripped.sha, commit.sha);
        assert_eq!(round_tripped.parents, commit.parents);
        assert_eq!(round_tripped.author_name, commit.author_name);
        assert_eq!(round_tripped.author_email, commit.author_email);
        assert_eq!(round_tripped.commit_timestamp, commit.commit_timestamp);
        assert_eq!(round_tripped.subject, commit.subject);
    }
}
