//! Where a project's agent sessions are read, and where deleting one happens.
//!
//! The agents keep their histories in files and sqlite databases under the home
//! directory of whichever machine they ran on. For a local project that is this
//! machine, and [`AgentSessionHost::Local`] runs the providers here. For a
//! project opened over a remote server it is the other machine:
//! [`AgentSessionHost::Remote`] asks that server, which runs the same providers
//! against its own home ([`AgentSessionService`]). The providers cannot be
//! pointed at a remote file system instead -- two of the stores are sqlite in
//! WAL mode, which a reader has to open where the database lives.
//!
//! The providers stay synchronous; only the host handle is async. Blocking a
//! provider on a request would stall the deterministic test scheduler, and the
//! providers are what every store-reading test runs against.

use crate::{
    AgentBinary, AgentBinaryMissing, AgentId, AgentServerStore,
    agent_server_store::remote_request_within,
};
use agent_sessions::{
    AgentCommand, AgentKind, Deletion, SessionCounts, SessionProvider, SessionSummary,
    SubagentSummary, TranscriptProgress,
    wire::{
        SessionOp, SessionOpResult, WireAgent, WireDeleteOutcome, WirePathKind, WireSession,
        WireUnavailable,
    },
};
use anyhow::{Context as _, Result, anyhow, bail};
use fs::Fs;
use futures::{FutureExt as _, select_biased};
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use rpc::{AnyProtoClient, TypedEnvelope, proto};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use util::paths::PathStyle;

/// What the history shows when the remote server cannot answer these
/// requests at all. A reconnect fetches the server matching this app.
pub const REMOTE_SESSIONS_OUT_OF_DATE: &str =
    "Remote server is out of date — reconnect, then refresh";

/// What a session request that timed out adds to "did not answer". Hedged: a
/// server too old to know the request drops it silently, but so, for as long
/// as it takes, does a current one that is slow.
const REMOTE_SESSIONS_UNANSWERED: &str = "if it is older than this app, reconnect, then refresh";

/// What a delete that timed out adds instead. The server bounds its own delete
/// below this client's deadline, so a timeout here most likely means the link
/// went quiet -- and the delete may still have happened.
const REMOTE_DELETE_UNANSWERED: &str =
    "the delete may still finish there; refresh to see whether the session is gone";

/// How long a client waits for the remote server to list its sessions. A sweep
/// opens every transcript on the host, which for a heavy user is thousands.
pub const REMOTE_LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a client waits for a read: one session's files, or a handful of
/// directories.
const REMOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the server may spend finding an agent's CLI for a delete. Finding
/// it can mean loading a login shell, which nothing else bounds.
const DELETE_LOCATE_DEADLINE: Duration = Duration::from_secs(30);

/// How long a single `Deletion::Command` invocation may run before the delete
/// gives up on it and treats it as a failure. A wedged CLI call must not stall
/// every trash and delete queued behind it in the same sequential sweep.
pub const COMMAND_DELETE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a client waits for a delete: everything the server may spend on
/// one, and room for the lookup, the trash and the round trip on top. Shorter,
/// and a current server still finishing a delete would be reported as failing.
const REMOTE_DELETE_TIMEOUT: Duration =
    Duration::from_secs(DELETE_LOCATE_DEADLINE.as_secs() + COMMAND_DELETE_TIMEOUT.as_secs() + 20);

fn timeout_for(op: &SessionOp) -> Duration {
    match op {
        SessionOp::List { .. } => REMOTE_LIST_TIMEOUT,
        SessionOp::Delete { .. } => REMOTE_DELETE_TIMEOUT,
        _ => REMOTE_REQUEST_TIMEOUT,
    }
}

/// The agents a listing asks for: every store there is.
const ALL_AGENTS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Copilot,
    AgentKind::OpenCode,
];

/// Every session a host can see, and the stores on it that could not be read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionListing {
    /// Newest first.
    pub sessions: Vec<SessionSummary>,
    pub unavailable: Vec<(AgentKind, String)>,
}

/// The machine a project's agent sessions live on.
#[derive(Clone)]
pub enum AgentSessionHost {
    Local(Vec<Arc<dyn SessionProvider>>),
    Remote(RemoteSessionHost),
}

impl AgentSessionHost {
    /// Registers the server half: the handler that answers a client's session
    /// requests on the host. The [`AgentSessionService`] entity must be
    /// subscribed under the project's id for requests to reach it.
    pub fn init_headless(session: &AnyProtoClient) {
        session.add_entity_request_handler(AgentSessionService::handle_agent_session_request);
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    /// How the host spells its paths.
    pub fn path_style(&self) -> PathStyle {
        match self {
            Self::Local(_) => PathStyle::local(),
            Self::Remote(remote) => remote.path_style,
        }
    }

    pub fn list(&self, cx: &App) -> Task<Result<SessionListing>> {
        match self {
            Self::Local(providers) => {
                let providers = providers.clone();
                cx.background_spawn(async move { Ok(list_here(&providers, &ALL_AGENTS)) })
            }
            Self::Remote(remote) => remote.list(cx),
        }
    }

    pub fn find(
        &self,
        agent: AgentKind,
        id: &str,
        cx: &App,
    ) -> Task<Result<Option<SessionSummary>>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, agent) else {
                    return Task::ready(Ok(None));
                };
                let id = id.to_string();
                cx.background_spawn(async move { provider.find(&id) })
            }
            Self::Remote(remote) => remote.find(agent, id, cx),
        }
    }

    pub fn counts(&self, session: &SessionSummary, cx: &App) -> Task<Result<SessionCounts>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, session.agent) else {
                    return Task::ready(Ok(SessionCounts::default()));
                };
                let session = session.clone();
                cx.background_spawn(async move { provider.counts(&session) })
            }
            Self::Remote(remote) => remote.counts(session, cx),
        }
    }

    pub fn subagents(
        &self,
        session: &SessionSummary,
        cx: &App,
    ) -> Task<Result<Vec<SubagentSummary>>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, session.agent) else {
                    return Task::ready(Ok(Vec::new()));
                };
                let session = session.clone();
                cx.background_spawn(async move { provider.subagents(&session) })
            }
            Self::Remote(remote) => remote.subagents(session, cx),
        }
    }

    pub fn transcript_progress(
        &self,
        session: &SessionSummary,
        from: u64,
        max_bytes: u64,
        cx: &App,
    ) -> Task<Result<TranscriptProgress>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, session.agent) else {
                    return Task::ready(Ok(TranscriptProgress {
                        scanned_to: from,
                        ..TranscriptProgress::default()
                    }));
                };
                let session = session.clone();
                cx.background_spawn(async move {
                    provider.transcript_progress_within(&session, from, max_bytes)
                })
            }
            Self::Remote(remote) => remote.transcript_progress(session, from, max_bytes, cx),
        }
    }

    pub fn successor(
        &self,
        session: &SessionSummary,
        since: SystemTime,
        claimed: &[Arc<str>],
        cx: &App,
    ) -> Task<Result<Option<SessionSummary>>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, session.agent) else {
                    return Task::ready(Ok(None));
                };
                let session = session.clone();
                let claimed = claimed.to_vec();
                cx.background_spawn(async move { provider.successor(&session, since, &claimed) })
            }
            Self::Remote(remote) => remote.successor(session, since, claimed, cx),
        }
    }

    pub fn background_quiet_for(
        &self,
        session: &SessionSummary,
        cx: &App,
    ) -> Task<Result<Option<Duration>>> {
        match self {
            Self::Local(providers) => {
                let Some(provider) = provider_in(providers, session.agent) else {
                    return Task::ready(Ok(None));
                };
                let session = session.clone();
                cx.background_spawn(async move { provider.background_quiet_for(&session) })
            }
            Self::Remote(remote) => remote.background_quiet_for(session, cx),
        }
    }

    /// Whether each of `paths` is a directory on the host, in order.
    pub fn directories_exist(&self, paths: Vec<PathBuf>, cx: &App) -> Task<Result<Vec<bool>>> {
        match self {
            Self::Local(_) => cx.background_spawn(async move {
                Ok(paths.iter().map(|path| directory_exists(path)).collect())
            }),
            Self::Remote(remote) => remote.directories_exist(paths, cx),
        }
    }

    /// Whether each of `paths` is there on the host as a `kind`, in order.
    pub fn paths_exist(
        &self,
        paths: Vec<PathBuf>,
        kind: WirePathKind,
        cx: &App,
    ) -> Task<Result<Vec<bool>>> {
        match self {
            Self::Local(_) => cx.background_spawn(async move {
                Ok(paths.iter().map(|path| path_exists(path, kind)).collect())
            }),
            Self::Remote(remote) => remote.paths_exist(paths, kind, cx),
        }
    }

    /// The session a tab starts from: the host's answer for the id, unless the
    /// tab was pinned to a particular copy of it.
    ///
    /// Asked for an id that two transcripts carry, a store answers with
    /// whichever it reaches first, and the tab would then run the CLI in *that*
    /// one's directory -- possibly outside every folder the person who vetted
    /// the other copy checked. So when the answer is not the pinned copy, the
    /// pinned copy wins, provided its transcript is still on the host: one
    /// deleted since is a session that is gone, and answering with another copy
    /// in its place would be the same substitution.
    ///
    /// The store still gets the first word on whether the session exists at
    /// all. A store that keeps sessions as rows, whose transcript can be missing
    /// while the session is not, is not second-guessed by a file check.
    pub fn find_pinned(
        &self,
        agent: AgentKind,
        id: &str,
        anchor: Option<SessionSummary>,
        cx: &App,
    ) -> Task<Result<Option<SessionSummary>>> {
        let found = self.find(agent, id, cx);
        let host = self.clone();
        cx.spawn(async move |cx| {
            let found = found.await?;
            let pinned_log = match (&anchor, &found) {
                (Some(anchor), Some(found)) if !is_same_copy(anchor, found) => {
                    anchor.log_path.clone()
                }
                _ => None,
            };
            let anchor_log_there = match pinned_log {
                Some(log_path) => cx
                    .update(|cx| host.paths_exist(vec![log_path], WirePathKind::File, cx))
                    .await?
                    .first()
                    .copied()
                    .unwrap_or(false),
                None => true,
            };
            Ok(pinned_choice(anchor, found, anchor_log_there))
        })
    }
}

/// [`AgentSessionHost::find_pinned`]'s decision, once the store has answered
/// and, where it matters, the host has said whether the pinned copy's
/// transcript is still there.
fn pinned_choice(
    anchor: Option<SessionSummary>,
    found: Option<SessionSummary>,
    anchor_log_there: bool,
) -> Option<SessionSummary> {
    match (anchor, found) {
        (None, found) => found,
        // Nothing holds the id any more, whichever copy was pinned.
        (Some(_), None) => None,
        (Some(anchor), Some(found)) => {
            if is_same_copy(&anchor, &found) {
                Some(found)
            } else {
                anchor_log_there.then_some(anchor)
            }
        }
    }
}

fn is_same_copy(anchor: &SessionSummary, found: &SessionSummary) -> bool {
    found.cwd == anchor.cwd && found.log_path == anchor.log_path
}

/// A project's remote server, as the place its sessions are kept.
#[derive(Clone)]
pub struct RemoteSessionHost {
    client: AnyProtoClient,
    project_id: u64,
    path_style: PathStyle,
}

impl RemoteSessionHost {
    pub fn new(client: AnyProtoClient, project_id: u64, path_style: PathStyle) -> Self {
        Self {
            client,
            project_id,
            path_style,
        }
    }

    pub fn path_style(&self) -> PathStyle {
        self.path_style
    }

    pub fn list(&self, cx: &App) -> Task<Result<SessionListing>> {
        let agents = ALL_AGENTS.into_iter().map(WireAgent::from).collect();
        let answer = self.request(SessionOp::List { agents }, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::List {
                    sessions,
                    unavailable,
                } => Ok(SessionListing {
                    sessions: sessions.into_iter().map(Into::into).collect(),
                    unavailable: unavailable
                        .into_iter()
                        .map(|store| (store.agent.into(), store.reason))
                        .collect(),
                }),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn find(
        &self,
        agent: AgentKind,
        id: &str,
        cx: &App,
    ) -> Task<Result<Option<SessionSummary>>> {
        let op = SessionOp::Find {
            agent: agent.into(),
            id: id.to_string(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::Find { session } => Ok(session.map(Into::into)),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn counts(&self, session: &SessionSummary, cx: &App) -> Task<Result<SessionCounts>> {
        let op = SessionOp::Counts {
            session: session.into(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::Counts { counts } => Ok(counts.into()),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn subagents(
        &self,
        session: &SessionSummary,
        cx: &App,
    ) -> Task<Result<Vec<SubagentSummary>>> {
        let op = SessionOp::Subagents {
            session: session.into(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::Subagents { subagents } => {
                    Ok(subagents.into_iter().map(Into::into).collect())
                }
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn transcript_progress(
        &self,
        session: &SessionSummary,
        from: u64,
        max_bytes: u64,
        cx: &App,
    ) -> Task<Result<TranscriptProgress>> {
        let op = SessionOp::TranscriptProgress {
            session: session.into(),
            from,
            max_bytes,
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::TranscriptProgress { progress } => Ok(progress.into()),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn successor(
        &self,
        session: &SessionSummary,
        since: SystemTime,
        claimed: &[Arc<str>],
        cx: &App,
    ) -> Task<Result<Option<SessionSummary>>> {
        let op = SessionOp::Successor {
            session: session.into(),
            since_ms: agent_sessions::wire::time_to_millis(since),
            claimed: claimed.iter().map(|id| id.to_string()).collect(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::Successor { session } => Ok(session.map(Into::into)),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn background_quiet_for(
        &self,
        session: &SessionSummary,
        cx: &App,
    ) -> Task<Result<Option<Duration>>> {
        let op = SessionOp::BackgroundQuietFor {
            session: session.into(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::BackgroundQuietFor { quiet_for_ms } => {
                    Ok(quiet_for_ms.map(Duration::from_millis))
                }
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn directories_exist(&self, paths: Vec<PathBuf>, cx: &App) -> Task<Result<Vec<bool>>> {
        let expected = paths.len();
        let op = SessionOp::DirectoriesExist {
            paths: paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::DirectoriesExist { exists } if exists.len() == expected => {
                    Ok(exists)
                }
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    pub fn paths_exist(
        &self,
        paths: Vec<PathBuf>,
        kind: WirePathKind,
        cx: &App,
    ) -> Task<Result<Vec<bool>>> {
        let expected = paths.len();
        let op = SessionOp::PathsExist {
            paths: paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            kind,
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::PathsExist { exists } if exists.len() == expected => Ok(exists),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    /// Deletes a session on the host. Only its agent and id are sent: the
    /// server finds it in its own store, works out what deleting it takes, and
    /// does that with its own file system and its own copy of the agent's CLI.
    pub fn delete(&self, agent: AgentKind, id: &str, cx: &App) -> Task<Result<WireDeleteOutcome>> {
        let op = SessionOp::Delete {
            agent: agent.into(),
            id: id.to_string(),
        };
        let answer = self.request(op, cx);
        cx.background_spawn(async move {
            match answer.await? {
                SessionOpResult::Delete { outcome } => Ok(outcome),
                other => Err(unexpected_answer(&other)),
            }
        })
    }

    /// One request, with the deadline [`timeout_for`] gives it. A server
    /// built before these requests existed may drop them without a word, so
    /// the deadline is what ends the wait; a newer one answers "no handler
    /// registered", which is named as the out-of-date server it is.
    ///
    /// Everything after the send happens on the background executor: a
    /// listing's answer is every session on the host, and decoding it on the
    /// foreground would hold the window for as long as that takes.
    fn request(&self, op: SessionOp, cx: &App) -> Task<Result<SessionOpResult>> {
        let timeout = timeout_for(&op);
        let if_unanswered = match op {
            SessionOp::Delete { .. } => REMOTE_DELETE_UNANSWERED,
            _ => REMOTE_SESSIONS_UNANSWERED,
        };
        let op_json = match serde_json::to_string(&op) {
            Ok(op_json) => op_json,
            Err(error) => return Task::ready(Err(error.into())),
        };
        let request = self.client.request(proto::AgentSessionRequest {
            project_id: self.project_id,
            op_json,
        });
        let timer = cx.background_executor().timer(timeout);
        cx.background_spawn(async move {
            let response = remote_request_within(
                request,
                timer,
                timeout,
                REMOTE_SESSIONS_OUT_OF_DATE,
                if_unanswered,
            )
            .await
            .map_err(name_a_server_older_than_its_request)?;
            serde_json::from_str(&response.result_json)
                .context("the remote server sent a session answer this app cannot read")
        })
    }
}

/// What a server says of a request whose op it has no variant for, which is
/// what one built before that op reaches. It knows the envelope, so it answers
/// rather than dropping it, and the client reads this to name the fix.
const UNREADABLE_SESSION_REQUEST: &str = "a session request this server cannot read";

fn name_a_server_older_than_its_request(error: anyhow::Error) -> anyhow::Error {
    if format!("{error:#}").contains(UNREADABLE_SESSION_REQUEST) {
        anyhow!(REMOTE_SESSIONS_OUT_OF_DATE)
    } else {
        error
    }
}

fn unexpected_answer(answer: &SessionOpResult) -> anyhow::Error {
    anyhow!("the remote server answered a session request with {answer:?}")
}

fn provider_in(
    providers: &[Arc<dyn SessionProvider>],
    agent: AgentKind,
) -> Option<Arc<dyn SessionProvider>> {
    providers
        .iter()
        .find(|provider| provider.agent() == agent)
        .cloned()
}

/// The listing of `agents` from the stores on this machine. Blocking.
fn list_here(providers: &[Arc<dyn SessionProvider>], agents: &[AgentKind]) -> SessionListing {
    let providers: Vec<Arc<dyn SessionProvider>> = providers
        .iter()
        .filter(|provider| agents.contains(&provider.agent()))
        .cloned()
        .collect();
    let unavailable = providers
        .iter()
        .filter_map(|provider| match provider.availability() {
            agent_sessions::Availability::Ready => None,
            agent_sessions::Availability::Unavailable(reason) => Some((provider.agent(), reason)),
        })
        .collect();
    SessionListing {
        sessions: agent_sessions::list_all(&providers),
        unavailable,
    }
}

/// The server half: the stores in this host's home, and what a delete there
/// goes through.
pub struct AgentSessionService {
    providers: Vec<Arc<dyn SessionProvider>>,
    fs: Arc<dyn Fs>,
    agent_server_store: Entity<AgentServerStore>,
}

impl AgentSessionService {
    pub fn new(
        providers: Vec<Arc<dyn SessionProvider>>,
        fs: Arc<dyn Fs>,
        agent_server_store: Entity<AgentServerStore>,
    ) -> Self {
        Self {
            providers,
            fs,
            agent_server_store,
        }
    }

    /// The service for the stores in this machine's own home directory.
    pub fn for_this_host(fs: Arc<dyn Fs>, agent_server_store: Entity<AgentServerStore>) -> Self {
        Self::new(agent_sessions::default_providers(), fs, agent_server_store)
    }

    /// Points the service at stores of a test's own making instead of the
    /// ones in this machine's home.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_providers_for_test(&mut self, providers: Vec<Arc<dyn SessionProvider>>) {
        self.providers = providers;
    }

    async fn handle_agent_session_request(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::AgentSessionRequest>,
        mut cx: AsyncApp,
    ) -> Result<proto::AgentSessionResponse> {
        let op: SessionOp =
            serde_json::from_str(&envelope.payload.op_json).context(UNREADABLE_SESSION_REQUEST)?;
        let result = Self::answer(&this, op, &mut cx).await?;
        // A listing is every session on the host; encoding it is no work for
        // the thread every other request on this server waits behind.
        let result_json = cx
            .background_spawn(async move { serde_json::to_string(&result) })
            .await?;
        Ok(proto::AgentSessionResponse { result_json })
    }

    /// What the stores on this host say to `op`.
    pub async fn answer(
        this: &Entity<Self>,
        op: SessionOp,
        cx: &mut AsyncApp,
    ) -> Result<SessionOpResult> {
        let (providers, fs, agent_server_store) = this.read_with(cx, |this, _| {
            (
                this.providers.clone(),
                this.fs.clone(),
                this.agent_server_store.clone(),
            )
        });
        let background = cx.background_executor().clone();
        let provider_for = |agent: WireAgent| {
            provider_in(&providers, agent.into())
                .with_context(|| format!("this host reads no {agent:?} sessions"))
        };
        Ok(match op {
            SessionOp::List { agents } => {
                let agents: Vec<AgentKind> = agents.into_iter().map(Into::into).collect();
                let providers = providers.clone();
                // Converted where it was read: a host's whole listing is thousands
                // of rows, and every other request to this server waits behind
                // its foreground thread.
                background
                    .spawn(async move {
                        let listing = list_here(&providers, &agents);
                        SessionOpResult::List {
                            sessions: listing.sessions.iter().map(WireSession::from).collect(),
                            unavailable: listing
                                .unavailable
                                .into_iter()
                                .map(|(agent, reason)| WireUnavailable {
                                    agent: agent.into(),
                                    reason,
                                })
                                .collect(),
                        }
                    })
                    .await
            }
            SessionOp::Find { agent, id } => {
                let provider = provider_for(agent)?;
                let session = background.spawn(async move { provider.find(&id) }).await?;
                SessionOpResult::Find {
                    session: session.as_ref().map(WireSession::from),
                }
            }
            SessionOp::Counts { session } => {
                let provider = provider_for(session.agent)?;
                let session = held_session(&provider, session, &background).await?;
                let counts = background
                    .spawn(async move { provider.counts(&session) })
                    .await?;
                SessionOpResult::Counts {
                    counts: counts.into(),
                }
            }
            SessionOp::Subagents { session } => {
                let provider = provider_for(session.agent)?;
                let session = held_session(&provider, session, &background).await?;
                let subagents = background
                    .spawn(async move { provider.subagents(&session) })
                    .await?;
                SessionOpResult::Subagents {
                    subagents: subagents.iter().map(Into::into).collect(),
                }
            }
            SessionOp::TranscriptProgress {
                session,
                from,
                max_bytes,
            } => {
                let provider = provider_for(session.agent)?;
                let session = held_session(&provider, session, &background).await?;
                let progress =
                    background
                        .spawn(async move {
                            provider.transcript_progress_within(&session, from, max_bytes)
                        })
                        .await?;
                SessionOpResult::TranscriptProgress {
                    progress: (&progress).into(),
                }
            }
            SessionOp::Successor {
                session,
                since_ms,
                claimed,
            } => {
                let provider = provider_for(session.agent)?;
                let session = held_session(&provider, session, &background).await?;
                let since = agent_sessions::wire::millis_to_time(since_ms);
                let claimed: Vec<Arc<str>> = claimed.into_iter().map(Arc::from).collect();
                let successor = background
                    .spawn(async move { provider.successor(&session, since, &claimed) })
                    .await?;
                SessionOpResult::Successor {
                    session: successor.as_ref().map(WireSession::from),
                }
            }
            SessionOp::BackgroundQuietFor { session } => {
                let provider = provider_for(session.agent)?;
                let session = held_session(&provider, session, &background).await?;
                let quiet_for = background
                    .spawn(async move { provider.background_quiet_for(&session) })
                    .await?;
                SessionOpResult::BackgroundQuietFor {
                    quiet_for_ms: quiet_for.map(agent_sessions::wire::duration_to_millis),
                }
            }
            SessionOp::DirectoriesExist { paths } => SessionOpResult::DirectoriesExist {
                exists: paths_exist_on(fs.as_ref(), paths, WirePathKind::Directory).await,
            },
            SessionOp::PathsExist { paths, kind } => SessionOpResult::PathsExist {
                exists: paths_exist_on(fs.as_ref(), paths, kind).await,
            },
            SessionOp::Delete { agent, id } => {
                let provider = provider_for(agent)?;
                let outcome =
                    delete_here(provider, fs, &agent_server_store, agent.into(), id, cx).await?;
                SessionOpResult::Delete { outcome }
            }
        })
    }
}

/// This host's own record of the session a request names.
///
/// A request carries a whole summary, paths included, but only its agent and
/// id are taken from it: the paths a read opens are the ones the store keeps,
/// so a request cannot point a provider at any other file on the host.
async fn held_session(
    provider: &Arc<dyn SessionProvider>,
    session: WireSession,
    background: &gpui::BackgroundExecutor,
) -> Result<SessionSummary> {
    let id = session.id;
    if !agent_sessions::is_safe_component(&id) {
        bail!("refusing a session whose id is not a single path component: {id:?}");
    }
    let provider = provider.clone();
    let lookup_id = id.clone();
    background
        .spawn(async move { provider.find(&lookup_id) })
        .await?
        .with_context(|| format!("this host holds no session {id:?}"))
}

/// Deletes the session `(agent, id)` from the stores on this machine.
///
/// Everything about what to take is decided here, from the store's own record
/// of the session: the request named a session, not paths. A trash that fails
/// is reported and nothing else is tried -- the trash is the one form of
/// delete these stores' files get.
async fn delete_here(
    provider: Arc<dyn SessionProvider>,
    fs: Arc<dyn Fs>,
    agent_server_store: &Entity<AgentServerStore>,
    agent: AgentKind,
    id: String,
    cx: &mut AsyncApp,
) -> Result<WireDeleteOutcome> {
    if !agent_sessions::is_safe_component(&id) {
        bail!("refusing to delete a session whose id is not a single path component: {id:?}");
    }
    let background = cx.background_executor().clone();
    let lookup = {
        let provider = provider.clone();
        let id = id.clone();
        background.spawn(async move { provider.find(&id) })
    };
    let Some(session) = lookup.await? else {
        return Ok(WireDeleteOutcome::Gone);
    };
    let deletion = {
        let provider = provider.clone();
        background
            .spawn(async move { provider.deletion(&session) })
            .await
    };
    match deletion {
        Deletion::Nothing => Ok(WireDeleteOutcome::NothingToDelete),
        Deletion::Trash(paths) => {
            let mut failures = Vec::new();
            for path in &paths {
                let trashed = fs
                    .trash(
                        path,
                        fs::RemoveOptions {
                            recursive: true,
                            ignore_if_not_exists: true,
                        },
                    )
                    .await;
                if let Err(error) = trashed {
                    log::warn!("trashing {} failed: {error:#}", path.display());
                    failures.push(format!("{}: {error:#}", path.display()));
                }
            }
            if failures.is_empty() {
                Ok(WireDeleteOutcome::Gone)
            } else {
                Ok(WireDeleteOutcome::Failed {
                    message: format!(
                        "could not move to the trash on the remote host: {}",
                        failures.join("; ")
                    ),
                })
            }
        }
        Deletion::Command(command) => {
            let id: Arc<str> = Arc::from(id);
            // Bounded here, where nothing else bounds it: finding the CLI can
            // mean loading a login shell, and the client stops waiting after
            // this deadline plus the command's own.
            let resolve = resolve_delete_binary(agent_server_store, agent, cx);
            let resolved = select_biased! {
                resolved = resolve.fuse() => binary_for_delete(resolved),
                _ = background.timer(DELETE_LOCATE_DEADLINE).fuse() => {
                    Err(CommandDeleteOutcome::Failed(format!(
                        "finding the {} CLI on the remote host took longer than {}s",
                        agent.label(),
                        DELETE_LOCATE_DEADLINE.as_secs()
                    )))
                }
            };
            let outcome = match resolved {
                Ok(binary) => {
                    run_resolved_command_delete(&provider, &background, &binary, &id, &command, cx)
                        .await
                }
                Err(outcome) => outcome,
            };
            Ok(match outcome {
                CommandDeleteOutcome::Gone => WireDeleteOutcome::Gone,
                CommandDeleteOutcome::BinaryMissing(missing) => WireDeleteOutcome::BinaryMissing {
                    binary: missing.binary.to_string(),
                },
                CommandDeleteOutcome::Failed(message) => WireDeleteOutcome::Failed { message },
            })
        }
    }
}

/// Whether the directory at `path` is there. Stats the disk, so never from a
/// render.
pub fn directory_exists(path: &Path) -> bool {
    is_local_absolute_path(path) && path.is_dir()
}

/// Whether `path` is there on this machine as a `kind`. Stats the disk, so
/// never from a render.
fn path_exists(path: &Path, kind: WirePathKind) -> bool {
    match kind {
        // Not behind the shape guard a directory is: a transcript path is one
        // this machine's own store wrote down, and one on a UNC share, which
        // the guard refuses, was always checked.
        WirePathKind::File => path.is_file(),
        WirePathKind::Directory => directory_exists(path),
    }
}

/// [`path_exists`] on a server, through its file system, for paths a client
/// sent: each must be an absolute path on the server before it is looked at.
async fn paths_exist_on(fs: &dyn Fs, paths: Vec<String>, kind: WirePathKind) -> Vec<bool> {
    let mut exists = Vec::with_capacity(paths.len());
    for path in paths {
        let path = Path::new(&path);
        exists.push(
            is_local_absolute_path(path)
                && match kind {
                    WirePathKind::File => fs.is_file(path).await,
                    WirePathKind::Directory => fs.is_dir(path).await,
                },
        );
    }
    exists
}

/// Every provider's `cwd` is untrusted data (it is read from a store this
/// editor does not own), and it reaches an `is_dir()` call here, plus, for a
/// `Deletion::Command`, a subprocess `current_dir` in
/// [`run_resolved_command_delete`]. On Windows, merely stat-ing a UNC path
/// (`\\host\share`) is itself a side effect -- it can trigger an outbound SMB
/// authentication attempt -- so the shape has to be rejected *before* any
/// `is_dir()` call, not after. The minimum this rejects: anything empty,
/// relative, or -- on Windows only -- not an ordinary local drive path.
pub fn is_local_absolute_path(path: &Path) -> bool {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(prefix))
                if matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                )
        )
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// What running a `Deletion::Command` once found out.
#[derive(Debug)]
pub enum CommandDeleteOutcome {
    /// The session is gone -- either the command exited zero, or it did not,
    /// but the store no longer holds the id either way (see
    /// [`run_command_delete`]): the goal was already met.
    Gone,
    /// The CLI that owns this session is not on this machine. Kept distinct
    /// from [`Self::Failed`] because a missing binary must not be offered the
    /// same way a `Trash` failure is.
    BinaryMissing(AgentBinaryMissing),
    /// The command ran, or tried to, and failed, and the store still holds
    /// the session. Carries text already stripped of control characters, fit
    /// to put in a toast.
    Failed(String),
}

/// Runs one `Deletion::Command`, resolving the binary through
/// `AgentServerStore` -- never the advisory `command.program` -- and racing it
/// against [`COMMAND_DELETE_TIMEOUT`] so one wedged invocation cannot stall a
/// sequential sweep.
///
/// A non-zero exit is not treated as final: `opencode session delete` on an id
/// already gone exits 1, so a failure re-checks `provider.find` and only keeps
/// the row if the store still holds the session -- the store is the authority,
/// not the exit code.
///
/// Generic over the context so the same logic serves a single delete behind a
/// window, a bulk sweep, and the remote server answering a client.
pub async fn run_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    agent_store: &Entity<AgentServerStore>,
    executor: &gpui::BackgroundExecutor,
    agent: AgentKind,
    id: &Arc<str>,
    command: &AgentCommand,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let resolve = resolve_delete_binary(agent_store, agent, cx);
    match binary_for_delete(resolve.await) {
        Ok(binary) => {
            run_resolved_command_delete(provider, executor, &binary, id, command, cx).await
        }
        Err(outcome) => outcome,
    }
}

fn resolve_delete_binary<C: gpui::AppContext>(
    agent_store: &Entity<AgentServerStore>,
    agent: AgentKind,
    cx: &mut C,
) -> Task<Result<AgentBinary>> {
    let agent_id = AgentId::new(agent.builtin_agent_id());
    agent_store.update(cx, |store, cx| store.resolve_agent_binary(&agent_id, cx))
}

/// The binary a resolve found, or the outcome a delete ends on without one.
fn binary_for_delete(resolved: Result<AgentBinary>) -> Result<PathBuf, CommandDeleteOutcome> {
    match resolved {
        Ok(AgentBinary::Found(path)) => Ok(path),
        Ok(AgentBinary::Missing(missing)) => Err(CommandDeleteOutcome::BinaryMissing(missing)),
        Err(error) => Err(CommandDeleteOutcome::Failed(error.to_string())),
    }
}

/// The half of [`run_command_delete`] that runs once a binary is already in
/// hand: builds the real subprocess and races it through [`race_command_delete`].
///
/// No `--` argv separator is inserted here. Against opencode's real CLI (the
/// only agent that reaches this path today) a `--` placed before its
/// `session`/`delete` subcommand words stops them being recognised at all,
/// falling through to opencode's default action -- which launches its
/// interactive TUI as a detached child process, on every delete. A provider
/// whose id shape needs protecting from being read as a flag carries its own
/// guard in its own `args`, at the position that actually works for its CLI;
/// this spawn site trusts `command.args` verbatim.
async fn run_resolved_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    executor: &gpui::BackgroundExecutor,
    binary: &Path,
    id: &Arc<str>,
    command: &AgentCommand,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let mut process = util::command::Command::new(binary);
    process.args(command.args.iter());
    // `directory_exists` applies the shape guard first: `command.cwd` is
    // untrusted data reaching a subprocess `current_dir`, and on Windows merely
    // stat-ing a UNC path is itself a network side effect. Off the foreground,
    // where a stat on a stalled mount would hold everything else.
    let cwd = command.cwd.clone();
    let cwd_exists = executor
        .spawn({
            let cwd = cwd.clone();
            async move { directory_exists(&cwd) }
        })
        .await;
    if cwd_exists {
        process.current_dir(&cwd);
    }
    race_command_delete(provider, executor, id, process.output(), cx).await
}

/// Races one command-delete attempt against [`COMMAND_DELETE_TIMEOUT`], then
/// resolves the outcome -- checking back with `provider.find` when the attempt
/// failed or ran out of time, since a wedged or non-zero exit
/// for a session the store no longer holds means the goal was already met.
///
/// Generic over the future that actually produces the process's output, which
/// is what makes this independently testable: a real `smol::process` child
/// plus its own OS reactor thread does not compose safely with
/// `#[gpui::test]`'s deterministic dispatcher -- it produces a wrong outcome
/// or a panic inside `blocking::Executor`'s thread pool. Passing the run as a plain
/// `Future` lets a test drive the timeout and the idempotency re-check with a
/// value built by hand (`std::future::ready`/`std::future::pending`), with no
/// subprocess and no reactor involved.
pub async fn race_command_delete<C: gpui::AppContext>(
    provider: &Arc<dyn SessionProvider>,
    executor: &gpui::BackgroundExecutor,
    id: &Arc<str>,
    run: impl Future<Output = std::io::Result<std::process::Output>>,
    cx: &mut C,
) -> CommandDeleteOutcome {
    let spawned = select_biased! {
        output = run.fuse() => Some(output),
        _ = executor.timer(COMMAND_DELETE_TIMEOUT).fuse() => None,
    };

    if let Some(Ok(output)) = &spawned {
        if output.status.success() {
            return CommandDeleteOutcome::Gone;
        }
    }

    let recheck_provider = provider.clone();
    let recheck_id = id.to_string();
    let still_there = cx
        .background_spawn(async move { recheck_provider.find(&recheck_id) })
        .await;
    match still_there {
        Ok(None) => CommandDeleteOutcome::Gone,
        Ok(Some(_)) | Err(_) => {
            let message = match spawned {
                Some(Ok(output)) => {
                    let stderr = strip_ansi_first_line(&output.stderr);
                    if stderr.is_empty() {
                        format!("exited with {}", output.status)
                    } else {
                        stderr
                    }
                }
                Some(Err(error)) => error.to_string(),
                None => format!("timed out after {}s", COMMAND_DELETE_TIMEOUT.as_secs()),
            };
            CommandDeleteOutcome::Failed(message)
        }
    }
}

/// Strips ANSI CSI escape sequences from `bytes`, then returns the first
/// non-empty line of what remains.
///
/// "Trim control characters" is not enough: that removes the `ESC`
/// (`0x1B`) byte alone and leaves e.g. `[91m` behind as ordinary printable
/// text -- exactly the garbage a strip exists to prevent. A CSI sequence is
/// `ESC` `[`, then any number of bytes, ending at the first one in
/// `0x40..=0x7E` (its "final byte"); this scans for that shape and drops the
/// whole run, `ESC` included.
pub fn strip_ansi_first_line(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().copied().peekable();
    while let Some(byte) = iter.next() {
        if byte == 0x1B && iter.peek() == Some(&b'[') {
            iter.next();
            for next in iter.by_ref() {
                if (0x40..=0x7E).contains(&next) {
                    break;
                }
            }
            continue;
        }
        out.push(byte);
    }
    String::from_utf8_lossy(&out)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::Fork;
    use fs::FakeFs;
    use gpui::TestAppContext;
    use serde_json::json;
    use std::time::UNIX_EPOCH;

    /// A store that answers `find` and `deletion` with what it was given, so
    /// what the handler does with an answer can be seen without a real store.
    struct StubProvider {
        agent: AgentKind,
        found: Option<SessionSummary>,
        deletion: Deletion,
    }

    impl SessionProvider for StubProvider {
        fn agent(&self) -> AgentKind {
            self.agent
        }
        fn availability(&self) -> agent_sessions::Availability {
            agent_sessions::Availability::Ready
        }
        fn list(&self) -> Result<Vec<SessionSummary>> {
            Ok(self.found.iter().cloned().collect())
        }
        fn find(&self, _id: &str) -> Result<Option<SessionSummary>> {
            Ok(self.found.clone())
        }
        fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
            None
        }
        fn counts(&self, _session: &SessionSummary) -> Result<SessionCounts> {
            Ok(SessionCounts::default())
        }
        fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
            None
        }
        fn deletion(&self, _session: &SessionSummary) -> Deletion {
            self.deletion.clone()
        }
    }

    fn stub_session(id: &str, agent: AgentKind) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from("/w/one"),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    fn found_only(found: Option<SessionSummary>) -> Arc<dyn SessionProvider> {
        Arc::new(StubProvider {
            agent: AgentKind::OpenCode,
            found,
            deletion: Deletion::Nothing,
        })
    }

    /// Builds a real, local `AgentServerStore` with an empty project
    /// environment -- enough for `resolve_agent_binary` to run its real
    /// logic. `ProjectEnvironment::get_cli_environment` always answers
    /// `Some(HashMap::default())` under `cfg!(test)`/`test-support`, which
    /// short-circuits before any real shell would be spawned, so this never
    /// touches a real login shell.
    fn test_agent_server_store(cx: &mut TestAppContext) -> Entity<AgentServerStore> {
        cx.update(|cx| {
            if !cx.has_global::<settings::SettingsStore>() {
                let settings_store = settings::SettingsStore::test(cx);
                cx.set_global(settings_store);
            }
            let fs: Arc<dyn Fs> = Arc::new(fs::RealFs::new(None, cx.background_executor().clone()));
            let worktree_store = cx.new(|cx| {
                crate::worktree_store::WorktreeStore::local(
                    false,
                    fs,
                    crate::worktree_store::WorktreeIdCounter::get(cx),
                )
            });
            let project_environment = cx.new(|cx| {
                crate::ProjectEnvironment::new(None, worktree_store.downgrade(), None, false, cx)
            });
            cx.new(|cx| AgentServerStore::local(project_environment, cx))
        })
    }

    /// Opencode's real failure output, byte for byte. "Trim control
    /// characters" alone would leave `[91m`/`[1m` behind as ordinary text --
    /// this is the one thing that proves a real CSI scan instead.
    #[test]
    fn the_csi_scanner_leaves_only_the_message() {
        let raw = b"\x1b[91m\x1b[1mError: \x1b[0mSession not found: x";
        assert_eq!(strip_ansi_first_line(raw), "Error: Session not found: x");
    }

    #[test]
    fn the_csi_scanner_is_a_no_op_on_plain_text() {
        assert_eq!(
            strip_ansi_first_line(b"plain error, no escapes"),
            "plain error, no escapes"
        );
    }

    #[test]
    fn the_csi_scanner_skips_a_blank_first_line() {
        assert_eq!(
            strip_ansi_first_line(b"\n\x1b[0mSession not found: x"),
            "Session not found: x"
        );
    }

    /// A fabricated exit, with no subprocess anywhere behind it -- see
    /// `race_command_delete`'s doc comment for why that is exactly the point.
    #[cfg(unix)]
    fn fake_exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        // The raw wait-status encoding: a normal exit packs the code into the
        // high byte.
        std::process::ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    fn fake_exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code as u32)
    }

    /// A command that exits non-zero for a session the store no longer holds
    /// must be treated as success, not failure. `opencode session
    /// delete` on an id already gone is exactly this shape.
    #[gpui::test]
    async fn a_failed_exit_is_forgiven_once_the_store_no_longer_holds_the_session(
        cx: &mut TestAppContext,
    ) {
        let provider = found_only(None);
        let id: Arc<str> = Arc::from("ses_gone");
        let executor = cx.background_executor.clone();
        let run = std::future::ready(Ok(std::process::Output {
            status: fake_exit_status(1),
            stdout: Vec::new(),
            stderr: b"Error: Session not found: ses_gone".to_vec(),
        }));

        let outcome = cx
            .spawn(async move |mut cx| {
                race_command_delete(&provider, &executor, &id, run, &mut cx).await
            })
            .await;

        assert!(
            matches!(outcome, CommandDeleteOutcome::Gone),
            "a non-zero exit for a session the store no longer holds must be \
             forgiven, got {outcome:?}"
        );
    }

    /// The other half: a non-zero exit for a session the store
    /// *still* holds must not be silently forgiven.
    #[gpui::test]
    async fn a_failed_exit_is_reported_when_the_store_still_holds_the_session(
        cx: &mut TestAppContext,
    ) {
        let provider = found_only(Some(stub_session("ses_here", AgentKind::OpenCode)));
        let id: Arc<str> = Arc::from("ses_here");
        let executor = cx.background_executor.clone();
        let run = std::future::ready(Ok(std::process::Output {
            status: fake_exit_status(1),
            stdout: Vec::new(),
            stderr: b"boom".to_vec(),
        }));

        let outcome = cx
            .spawn(async move |mut cx| {
                race_command_delete(&provider, &executor, &id, run, &mut cx).await
            })
            .await;

        match outcome {
            CommandDeleteOutcome::Failed(message) => assert_eq!(message, "boom"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// A command that never returns must be abandoned by the timer rather
    /// than blocking the caller forever. It falls through to the same
    /// idempotency re-check a failed exit does.
    #[gpui::test]
    async fn a_command_that_never_returns_is_abandoned_after_the_timeout(cx: &mut TestAppContext) {
        let provider = found_only(Some(stub_session("ses_stuck", AgentKind::OpenCode)));
        let id: Arc<str> = Arc::from("ses_stuck");
        let executor = cx.background_executor.clone();

        let task = cx.spawn(async move |mut cx| {
            race_command_delete(
                &provider,
                &executor,
                &id,
                std::future::pending::<std::io::Result<std::process::Output>>(),
                &mut cx,
            )
            .await
        });

        cx.run_until_parked();
        cx.executor().advance_clock(COMMAND_DELETE_TIMEOUT);
        cx.run_until_parked();
        let outcome = task.await;

        match outcome {
            CommandDeleteOutcome::Failed(message) => {
                assert!(message.contains("timed out"), "got: {message}");
            }
            other => panic!("expected Failed(timed out...), got {other:?}"),
        }
    }

    /// A `Command` delete with the CLI absent is reported as such, not
    /// as a failure of the command.
    #[gpui::test]
    async fn a_missing_binary_is_reported_distinctly(cx: &mut TestAppContext) {
        let provider = found_only(Some(stub_session(
            "ses_needs_opencode",
            AgentKind::OpenCode,
        )));
        let id: Arc<str> = Arc::from("ses_needs_opencode");
        let agent_store = test_agent_server_store(cx);
        let executor = cx.background_executor.clone();
        let command = AgentCommand {
            program: "opencode".to_string(),
            args: vec![
                "session".to_string(),
                "delete".to_string(),
                "ses_needs_opencode".to_string(),
            ],
            cwd: PathBuf::from("/w/one"),
        };

        let outcome = cx
            .spawn(async move |mut cx| {
                run_command_delete(
                    &provider,
                    &agent_store,
                    &executor,
                    AgentKind::OpenCode,
                    &id,
                    &command,
                    &mut cx,
                )
                .await
            })
            .await;

        match outcome {
            CommandDeleteOutcome::BinaryMissing(missing) => {
                assert_eq!(missing.binary, "opencode");
            }
            other => panic!("expected BinaryMissing, got {other:?}"),
        }
    }

    fn service_with(
        provider: Arc<dyn SessionProvider>,
        fs: Arc<FakeFs>,
        cx: &mut TestAppContext,
    ) -> Entity<AgentSessionService> {
        let agent_server_store = test_agent_server_store(cx);
        cx.new(|_| AgentSessionService::new(vec![provider], fs, agent_server_store))
    }

    /// The id is the one thing a delete request names, and it reaches a store
    /// that joins it onto a directory. A store that would happily answer for
    /// `../x` stands in here, so it is the handler's own check that has to
    /// refuse -- before anything is looked up, let alone trashed.
    #[gpui::test]
    async fn a_delete_for_an_id_that_escapes_its_store_is_refused(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/store", json!({ "x.jsonl": "" })).await;
        let found = SessionSummary {
            log_path: Some(PathBuf::from("/store/x.jsonl")),
            ..stub_session("../x", AgentKind::Claude)
        };
        let provider: Arc<dyn SessionProvider> = Arc::new(StubProvider {
            agent: AgentKind::Claude,
            found: Some(found),
            deletion: Deletion::Trash(vec![PathBuf::from("/store/x.jsonl")]),
        });
        let service = service_with(provider, fs.clone(), cx);

        for id in ["../x", "a/b", "..", ""] {
            let answer = cx
                .spawn({
                    let service = service.clone();
                    async move |mut cx| {
                        AgentSessionService::answer(
                            &service,
                            SessionOp::Delete {
                                agent: WireAgent::Claude,
                                id: id.to_string(),
                            },
                            &mut cx,
                        )
                        .await
                    }
                })
                .await;
            assert!(answer.is_err(), "`{id}` must be refused, got {answer:?}");
        }
        assert!(
            fs.trash_entries().is_empty(),
            "nothing may be trashed for a refused id"
        );
        assert!(fs.is_file(Path::new("/store/x.jsonl")).await);
    }

    /// The server builds the deletion from its own record of the session and
    /// trashes through its own `Fs`.
    #[gpui::test]
    async fn a_delete_trashes_what_the_store_names_through_the_hosts_fs(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/store", json!({ "s.jsonl": "", "s": { "subagents": {} } }))
            .await;
        let provider: Arc<dyn SessionProvider> = Arc::new(StubProvider {
            agent: AgentKind::Claude,
            found: Some(stub_session("s", AgentKind::Claude)),
            deletion: Deletion::Trash(vec![
                PathBuf::from("/store/s"),
                PathBuf::from("/store/s.jsonl"),
            ]),
        });
        let service = service_with(provider, fs.clone(), cx);

        let answer = cx
            .spawn(async move |mut cx| {
                AgentSessionService::answer(
                    &service,
                    SessionOp::Delete {
                        agent: WireAgent::Claude,
                        id: "s".to_string(),
                    },
                    &mut cx,
                )
                .await
            })
            .await
            .expect("a delete of a held session answers");

        assert_eq!(
            answer,
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::Gone
            }
        );
        let mut names: Vec<String> = fs
            .trash_entries()
            .into_iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["s".to_string(), "s.jsonl".to_string()]);
    }

    /// A trash that fails is a failure the client hears about. Nothing falls
    /// back to removing the file outright.
    #[gpui::test]
    async fn a_trash_that_fails_is_reported_and_nothing_is_removed(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/store", json!({ "s.jsonl": "" })).await;
        let provider: Arc<dyn SessionProvider> = Arc::new(StubProvider {
            agent: AgentKind::Claude,
            found: Some(stub_session("s", AgentKind::Claude)),
            // A path with no file name cannot be trashed by any `Fs`.
            deletion: Deletion::Trash(vec![PathBuf::from("/")]),
        });
        let service = service_with(provider, fs.clone(), cx);

        let answer = cx
            .spawn(async move |mut cx| {
                AgentSessionService::answer(
                    &service,
                    SessionOp::Delete {
                        agent: WireAgent::Claude,
                        id: "s".to_string(),
                    },
                    &mut cx,
                )
                .await
            })
            .await
            .expect("a failed trash is an answer, not a dropped request");

        assert!(
            matches!(
                &answer,
                SessionOpResult::Delete {
                    outcome: WireDeleteOutcome::Failed { .. }
                }
            ),
            "got {answer:?}"
        );
        assert!(fs.is_file(Path::new("/store/s.jsonl")).await);
    }

    /// A store that does not hold the session has nothing left to take.
    #[gpui::test]
    async fn a_delete_of_a_session_the_store_no_longer_holds_is_gone(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        let service = service_with(
            Arc::new(StubProvider {
                agent: AgentKind::Claude,
                found: None,
                deletion: Deletion::Trash(vec![PathBuf::from("/never")]),
            }),
            fs.clone(),
            cx,
        );
        let answer = cx
            .spawn(async move |mut cx| {
                AgentSessionService::answer(
                    &service,
                    SessionOp::Delete {
                        agent: WireAgent::Claude,
                        id: "s".to_string(),
                    },
                    &mut cx,
                )
                .await
            })
            .await
            .expect("answers");
        assert_eq!(
            answer,
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::Gone
            }
        );
        assert!(fs.trash_entries().is_empty());
    }

    /// The store still lists the session but owns nothing to take. Answering
    /// "gone" would have the client forget a row that the next sweep brings
    /// back.
    #[gpui::test]
    async fn a_session_with_nothing_left_to_take_is_not_reported_gone(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        let service = service_with(
            Arc::new(StubProvider {
                agent: AgentKind::Claude,
                found: Some(stub_session("s", AgentKind::Claude)),
                deletion: Deletion::Nothing,
            }),
            fs.clone(),
            cx,
        );
        let answer = cx
            .spawn(async move |mut cx| {
                AgentSessionService::answer(
                    &service,
                    SessionOp::Delete {
                        agent: WireAgent::Claude,
                        id: "s".to_string(),
                    },
                    &mut cx,
                )
                .await
            })
            .await
            .expect("answers");
        assert_eq!(
            answer,
            SessionOpResult::Delete {
                outcome: WireDeleteOutcome::NothingToDelete
            }
        );
        assert!(fs.trash_entries().is_empty());
    }

    /// Records the log path each read was asked to follow.
    struct RecordingProvider {
        held: SessionSummary,
        read_paths: Arc<std::sync::Mutex<Vec<Option<PathBuf>>>>,
    }

    impl RecordingProvider {
        fn record(&self, session: &SessionSummary) {
            if let Ok(mut read_paths) = self.read_paths.lock() {
                read_paths.push(session.log_path.clone());
            }
        }
    }

    impl SessionProvider for RecordingProvider {
        fn agent(&self) -> AgentKind {
            AgentKind::Codex
        }
        fn availability(&self) -> agent_sessions::Availability {
            agent_sessions::Availability::Ready
        }
        fn list(&self) -> Result<Vec<SessionSummary>> {
            Ok(vec![self.held.clone()])
        }
        fn find(&self, id: &str) -> Result<Option<SessionSummary>> {
            Ok((id == self.held.id.as_ref()).then(|| self.held.clone()))
        }
        fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
            None
        }
        fn counts(&self, session: &SessionSummary) -> Result<SessionCounts> {
            self.record(session);
            Ok(SessionCounts::default())
        }
        fn transcript_progress(
            &self,
            session: &SessionSummary,
            from: u64,
        ) -> Result<TranscriptProgress> {
            self.record(session);
            Ok(TranscriptProgress {
                scanned_to: from,
                ..TranscriptProgress::default()
            })
        }
        fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
            None
        }
        fn deletion(&self, _session: &SessionSummary) -> Deletion {
            Deletion::Nothing
        }
    }

    /// A read names a session, and the paths it opens are the host's own
    /// record of that session -- never the ones the request carried, which
    /// could name any file on the host (`/dev/zero` for one that reads a whole
    /// transcript to count it).
    #[gpui::test]
    async fn a_read_follows_the_hosts_record_not_the_path_the_client_sent(cx: &mut TestAppContext) {
        let held = SessionSummary {
            log_path: Some(PathBuf::from("/home/u/.codex/sessions/rollout-s.jsonl")),
            ..stub_session("s", AgentKind::Codex)
        };
        let read_paths: Arc<std::sync::Mutex<Vec<Option<PathBuf>>>> = Arc::default();
        let provider: Arc<dyn SessionProvider> = Arc::new(RecordingProvider {
            held: held.clone(),
            read_paths: read_paths.clone(),
        });
        let service = service_with(provider, FakeFs::new(cx.executor()), cx);
        let bogus = WireSession::from(&SessionSummary {
            log_path: Some(PathBuf::from("/dev/zero")),
            ..held.clone()
        });
        let stranger = WireSession::from(&SessionSummary {
            log_path: Some(PathBuf::from("/dev/zero")),
            ..stub_session("not-held", AgentKind::Codex)
        });

        let answers = cx
            .spawn(async move |mut cx| {
                let mut answers = Vec::new();
                for op in [
                    SessionOp::Counts {
                        session: bogus.clone(),
                    },
                    SessionOp::TranscriptProgress {
                        session: bogus,
                        from: 0,
                        max_bytes: 1024,
                    },
                    SessionOp::Counts { session: stranger },
                ] {
                    answers.push(AgentSessionService::answer(&service, op, &mut cx).await);
                }
                answers
            })
            .await;

        assert!(answers[0].is_ok(), "{:?}", answers[0]);
        assert!(answers[1].is_ok(), "{:?}", answers[1]);
        assert!(
            answers[2].is_err(),
            "a session the host does not hold is refused, got {:?}",
            answers[2]
        );
        let read_paths = read_paths
            .lock()
            .map(|read_paths| read_paths.clone())
            .unwrap_or_default();
        assert_eq!(read_paths, vec![held.log_path.clone(), held.log_path]);
    }

    /// The server may spend up to its locate deadline finding the CLI and then
    /// up to the command's own timeout running it. A client that gave up
    /// sooner would report a failure for a delete still under way.
    #[test]
    fn a_delete_waits_longer_than_the_host_may_take_to_answer_it() {
        let delete = SessionOp::Delete {
            agent: WireAgent::OpenCode,
            id: "s".to_string(),
        };
        assert!(
            timeout_for(&delete) > DELETE_LOCATE_DEADLINE + COMMAND_DELETE_TIMEOUT,
            "a client deadline of {:?} ends before the server's",
            timeout_for(&delete)
        );
        assert_eq!(
            timeout_for(&SessionOp::List { agents: Vec::new() }),
            REMOTE_LIST_TIMEOUT
        );
    }

    #[gpui::test]
    async fn the_local_host_lists_what_its_providers_hold(cx: &mut TestAppContext) {
        let newer = SessionSummary {
            updated_at: UNIX_EPOCH + Duration::from_secs(2),
            ..stub_session("newer", AgentKind::Claude)
        };
        let host = AgentSessionHost::Local(vec![Arc::new(StubProvider {
            agent: AgentKind::Claude,
            found: Some(newer.clone()),
            deletion: Deletion::Nothing,
        })]);
        let listing = cx
            .update(|cx| host.list(cx))
            .await
            .expect("a local listing never fails");
        assert_eq!(listing.sessions, vec![newer]);
        assert!(listing.unavailable.is_empty());
    }

    /// Answers every lookup with the copy it was given, whatever the id.
    fn answers_with(found: Option<SessionSummary>) -> AgentSessionHost {
        AgentSessionHost::Local(vec![Arc::new(StubProvider {
            agent: AgentKind::Claude,
            found,
            deletion: Deletion::Nothing,
        })])
    }

    fn copy(cwd: &str, log_path: Option<PathBuf>) -> SessionSummary {
        SessionSummary {
            cwd: PathBuf::from(cwd),
            log_path,
            ..stub_session("one", AgentKind::Claude)
        }
    }

    async fn find_pinned(
        host: &AgentSessionHost,
        anchor: Option<SessionSummary>,
        cx: &mut TestAppContext,
    ) -> Option<SessionSummary> {
        cx.update(|cx| host.find_pinned(AgentKind::Claude, "one", anchor, cx))
            .await
            .expect("a local lookup does not fail")
    }

    /// The same id under two project directories: the tab was opened onto one,
    /// and the store, asked by id, answers with the other.
    #[gpui::test]
    async fn a_pinned_lookup_keeps_the_vetted_copy_while_its_transcript_is_there(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let vetted_log = directory.path().join("vetted.jsonl");
        std::fs::write(&vetted_log, "{}\n").expect("the vetted transcript is written");
        let vetted = copy("/inside/an/open/folder", Some(vetted_log));
        let other = copy("/outside/every/open/folder", None);
        let host = answers_with(Some(other.clone()));

        assert_eq!(
            find_pinned(&host, None, cx).await,
            Some(other),
            "with nothing pinned the store's answer stands"
        );
        assert_eq!(
            find_pinned(&host, Some(vetted.clone()), cx).await,
            Some(vetted)
        );
    }

    #[gpui::test]
    async fn a_pinned_copy_whose_transcript_is_gone_is_not_replaced_by_another(
        cx: &mut TestAppContext,
    ) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let vetted = copy("/inside", Some(directory.path().join("deleted.jsonl")));
        let host = answers_with(Some(copy("/outside", None)));
        assert_eq!(
            find_pinned(&host, Some(vetted), cx).await,
            None,
            "a copy nobody vetted is not what the tab agreed to start"
        );
    }

    /// A store whose sessions are rows can hold a session with no transcript
    /// of its own, and its word on that session is the one that counts.
    #[gpui::test]
    async fn a_pinned_session_with_no_transcript_file_still_stands(cx: &mut TestAppContext) {
        let vetted = copy("/inside", None);
        let host = answers_with(Some(copy("/outside", None)));
        assert_eq!(
            find_pinned(&host, Some(vetted.clone()), cx).await,
            Some(vetted)
        );
    }

    #[gpui::test]
    async fn a_pinned_copy_the_store_agrees_on_is_the_stores_own_summary(cx: &mut TestAppContext) {
        let vetted = copy("/inside", Some(PathBuf::from("/nowhere/at/all.jsonl")));
        let fresher = SessionSummary {
            title: "retitled since".into(),
            ..vetted.clone()
        };
        let host = answers_with(Some(fresher.clone()));
        assert_eq!(
            find_pinned(&host, Some(vetted), cx).await,
            Some(fresher),
            "the same copy, so the store's word on it stands, even with its file out of sight"
        );
    }

    #[gpui::test]
    async fn a_session_the_store_no_longer_holds_is_gone_whatever_was_pinned(
        cx: &mut TestAppContext,
    ) {
        let host = answers_with(None);
        assert_eq!(
            find_pinned(&host, Some(copy("/inside", None)), cx).await,
            None
        );
    }

    #[gpui::test]
    async fn the_local_host_tells_files_from_directories(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let file = directory.path().join("transcript.jsonl");
        std::fs::write(&file, "{}\n").expect("the transcript is written");
        let host = answers_with(None);
        let paths = vec![
            file.clone(),
            directory.path().to_path_buf(),
            directory.path().join("missing.jsonl"),
            PathBuf::from("relative.jsonl"),
        ];

        let files = cx
            .update(|cx| host.paths_exist(paths.clone(), WirePathKind::File, cx))
            .await
            .expect("a local check does not fail");
        let directories = cx
            .update(|cx| host.paths_exist(paths, WirePathKind::Directory, cx))
            .await
            .expect("a local check does not fail");

        assert_eq!(files, vec![true, false, false, false]);
        assert_eq!(directories, vec![false, true, false, false]);
    }

    /// A pinned copy's transcript path comes from this machine's own store,
    /// and was always checked with a plain `is_file`. A test's working
    /// directory is its crate, so a relative manifest path names a file there
    /// that the absolute-path guard alone would refuse.
    #[gpui::test]
    async fn the_local_host_checks_a_transcript_as_this_machines_store_does(
        cx: &mut TestAppContext,
    ) {
        let manifest = PathBuf::from("Cargo.toml");
        assert!(manifest.is_file(), "the test runs in its crate's directory");
        let host = answers_with(None);
        let files = cx
            .update(|cx| host.paths_exist(vec![manifest], WirePathKind::File, cx))
            .await
            .expect("a local check does not fail");
        assert_eq!(files, vec![true]);
    }
}
