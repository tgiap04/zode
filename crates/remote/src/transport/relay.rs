//! Reaching another Zode's project server through the zodekit relay.
//!
//! The other Zode runs `remote_server proxy` for us and carries its stdio on
//! one stream of the encrypted session, so everything above this file -- the
//! project, language servers, git -- works as it does over SSH. What cannot
//! work is anything that needs a local command to reach the machine:
//! terminals are driven with terminal messages instead (see
//! [`RemoteConnection::terminals_over_rpc`]), and a binary cannot be uploaded,
//! which is why both Zodes must run the same version.
//!
//! Every window that opens a project gets a relay session of its own, with its
//! own project server on the other Zode. That server lives exactly as long as
//! its session, so a connection that is lost cannot be resumed: the project has
//! to be opened again, which is what [`ProxyLaunchError::ServerNotRunning`]
//! tells the client.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use async_trait::async_trait;
use collections::HashMap;
use futures::{
    FutureExt as _, StreamExt as _,
    channel::mpsc::{Sender, UnboundedReceiver, UnboundedSender},
    select_biased,
};
use gpui::{App, AppContext as _, AsyncApp, Entity, Task, WeakEntity};
use parking_lot::Mutex;
use release_channel::AppVersion;
use remote_relay_client::{
    RelayInitiator, RelaySession, RelaySessionEvent, SessionError, StreamReceiver,
    send_data_waiting,
};
use remote_relay_protocol::Control;
use rpc::proto::Envelope;
use util::paths::{PathStyle, RemotePathBuf};

use crate::{
    CommandTemplate, Interactive, RemoteClientDelegate, RemoteConnection, RemoteConnectionOptions,
    proxy::ProxyLaunchError,
    transport::relay_stream::{MessageDecoder, encode_message},
};

/// A Zode reached through the zodekit relay, named by the account's id for
/// that device. The display name is only for people: two options naming the
/// same device are the same connection whatever the host is called today.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RelayConnectionOptions {
    pub host_device_id: String,
    pub host_name: String,
}

/// What the other Zode reports about itself before the project server starts.
struct HostFacts {
    path_style: PathStyle,
    shell: String,
    default_shell: String,
}

pub(crate) struct RelayRemoteConnection {
    options: RelayConnectionOptions,
    facts: HostFacts,
    /// Opened with the connection, so that a host that cannot serve projects
    /// is known before anything is shown, and taken by the first `start_proxy`.
    /// Every other window opens its own when it asks.
    first_stream: Mutex<Option<ProjectStream>>,
    sessions: Arc<Sessions>,
}

/// One project server on the other Zode, reached through a session that
/// belongs to whoever runs the stream.
struct ProjectStream {
    stream_id: u32,
    session: WeakEntity<RelaySession>,
    inbound: StreamReceiver,
    release: SessionRelease,
}

/// Asks the task that holds a session to end it, which dropping this does too.
struct SessionRelease(smol::channel::Sender<()>);

impl Drop for SessionRelease {
    fn drop(&mut self) {
        // A full queue already holds a request, and a closed one means the
        // session has ended.
        self.0.try_send(()).ok();
    }
}

/// The relay sessions a connection has opened, which it ends when it is let go.
#[derive(Default)]
struct Sessions {
    /// Set once nothing used the connection when it was killed, or it was let
    /// go. Nothing new is opened through it from then on.
    killed: AtomicBool,
    live: AtomicUsize,
    /// Sessions a `start_proxy` is still opening.
    opening: AtomicUsize,
    releases: Mutex<Vec<smol::channel::Sender<()>>>,
}

/// Counts a session that is being opened for as long as it is held.
struct Opening(Arc<Sessions>);

impl Drop for Opening {
    fn drop(&mut self) {
        self.0.opening.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Sessions {
    fn is_killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// What the pool asks: whether to open a new connection rather than hand
    /// this one out. A connection with no session left has nothing to offer a
    /// window but the way to open one.
    fn is_ended(&self) -> bool {
        self.is_killed() || self.live.load(Ordering::SeqCst) == 0
    }

    fn opening(self: &Arc<Self>) -> Opening {
        self.opening.fetch_add(1, Ordering::SeqCst);
        Opening(self.clone())
    }

    /// Ends every session. One that is added afterwards is ended as it is
    /// added.
    fn end_all(&self) {
        self.killed.store(true, Ordering::SeqCst);
        for release in self.releases.lock().iter() {
            release.try_send(()).ok();
        }
    }

    /// Ends the connection unless a window is using it or opening a session.
    /// The window that is giving it up has already let go of its own session,
    /// so one still in use belongs to another window, whose project must not
    /// be taken down by this one's reconnect.
    fn end_if_unused(&self) {
        if self.live.load(Ordering::SeqCst) == 0 && self.opening.load(Ordering::SeqCst) == 0 {
            self.end_all();
        }
    }

    fn session_ended(&self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }

    /// Keeps `session` alive until it is released or ends, then closes it.
    fn hold(self: &Arc<Self>, session: &Entity<RelaySession>, cx: &mut AsyncApp) -> SessionRelease {
        let (release, release_requested) = smol::channel::bounded(1);
        let (closed_sender, closed) = smol::channel::bounded(1);
        self.live.fetch_add(1, Ordering::SeqCst);
        // Subscribed here, not inside the task below: a task may not run until
        // after the session has closed, and a close nobody was listening for
        // would leave the connection looking alive for good.
        let watch = cx.update(|cx| {
            let sessions = self.clone();
            let closed_sender = closed_sender.clone();
            cx.subscribe(session, move |_, event: &RelaySessionEvent, _| {
                if matches!(event, RelaySessionEvent::Closed(_)) {
                    sessions.session_ended();
                    closed_sender.try_send(()).ok();
                }
            })
        });
        if session.read_with(cx, |session, _| session.is_closed()) {
            self.session_ended();
            closed_sender.try_send(()).ok();
        }
        {
            let mut releases = self.releases.lock();
            releases.retain(|release| !release.is_closed());
            releases.push(release.clone());
        }
        if self.is_killed() {
            release.try_send(()).ok();
        }
        cx.spawn({
            let strong = session.clone();
            let session = session.downgrade();
            async move |cx| {
                // The task is what keeps the session alive: the connection
                // itself holds only a weak handle. It lets go as soon as the
                // session is over, and with it the relay connection the
                // session holds.
                let _strong = strong;
                let _watch = watch;
                let requested = release_requested.recv().fuse();
                let ended = closed.recv().fuse();
                futures::pin_mut!(requested, ended);
                // Both a request and the sender being dropped mean the same:
                // nobody is using the session any more.
                select_biased! {
                    _ = requested => {}
                    _ = ended => {}
                }
                session.update(cx, |session, cx| session.close(cx)).ok();
            }
        })
        .detach();
        SessionRelease(release)
    }
}

/// The sentence shown when the two Zodes are not the same build.
fn version_mismatch_message(local: &str, host_name: &str, remote: &str) -> String {
    format!(
        "This Zode is version {local} but {host_name} runs {remote}. Update both Zodes to the \
         same version to open its projects."
    )
}

impl RelayRemoteConnection {
    pub async fn new(
        options: RelayConnectionOptions,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
        delegate.set_status(Some("Connecting through zodekit"), cx);
        let sessions = Arc::new(Sessions::default());
        let (stream, facts) =
            Self::open_project_stream(&options, &sessions, delegate.as_ref(), cx).await?;
        Ok(Self {
            options,
            facts,
            first_stream: Mutex::new(Some(stream)),
            sessions,
        })
    }

    /// A session of its own to the host, with a project server started on a
    /// stream of it.
    async fn open_project_stream(
        options: &RelayConnectionOptions,
        sessions: &Arc<Sessions>,
        delegate: &dyn RemoteClientDelegate,
        cx: &mut AsyncApp,
    ) -> Result<(ProjectStream, HostFacts)> {
        let initiator = cx.update(RelayInitiator::get_or_create)?;
        let session = initiator
            .update(cx, |initiator, cx| {
                initiator.connect(&options.host_device_id, cx)
            })
            .await
            .map_err(|error| anyhow!("{error}"))?;

        match Self::start_project_server(options, &session, delegate, cx).await {
            Ok((stream_id, inbound, facts)) => {
                let release = sessions.hold(&session, cx);
                let stream = ProjectStream {
                    stream_id,
                    session: session.downgrade(),
                    inbound,
                    release,
                };
                Ok((stream, facts))
            }
            Err(error) => {
                session.update(cx, |session, cx| session.close(cx));
                Err(error)
            }
        }
    }

    async fn start_project_server(
        options: &RelayConnectionOptions,
        session: &Entity<RelaySession>,
        delegate: &dyn RemoteClientDelegate,
        cx: &mut AsyncApp,
    ) -> Result<(u32, StreamReceiver, HostFacts)> {
        let local_version = cx.update(|cx| AppVersion::global(cx).to_string());
        let (host_version, can_serve) = session.read_with(cx, |session, _| {
            (
                session.host().app_version.clone(),
                session.host().can("ide"),
            )
        });
        if host_version != local_version {
            return Err(anyhow!(version_mismatch_message(
                &local_version,
                &options.host_name,
                &host_version
            )));
        }
        if !can_serve {
            return Err(anyhow!(
                "{} cannot serve projects: it was built without the project server",
                options.host_name
            ));
        }

        delegate.set_status(Some("Starting the project server"), cx);
        let (inbound_sender, inbound) = RelaySession::stream_channel();
        let (stream_id, reply) = session.update(cx, |session, cx| {
            let stream_id = session.allocate_stream_id();
            // Registered before asking: the server's first bytes may follow the
            // answer immediately.
            session.register_stream(stream_id, inbound_sender)?;
            let version = local_version.clone();
            let reply = session.request(
                |request_id| Control::IdeOpen {
                    request_id,
                    path: String::new(),
                    line: None,
                    stream_id: Some(stream_id),
                    app_version: Some(version),
                    proto_version: Some(rpc::PROTOCOL_VERSION),
                },
                cx,
            );
            Ok::<_, SessionError>((stream_id, reply))
        })?;
        let reply = reply
            .await
            .context("the other Zode did not answer")?
            .map_err(|error| match error {
                SessionError::Remote { code, .. } if code == "version" => {
                    anyhow!(version_mismatch_message(
                        &local_version,
                        &options.host_name,
                        &host_version
                    ))
                }
                other => anyhow!("{other}"),
            })?;
        let Control::IdeOpened {
            path_style,
            shell,
            default_shell,
            ..
        } = reply
        else {
            return Err(anyhow!("the other Zode answered with something unexpected"));
        };
        let path_style = match path_style.as_deref() {
            Some("posix") => PathStyle::Posix,
            Some("windows") => PathStyle::Windows,
            other => {
                return Err(anyhow!(
                    "the other Zode reported an unknown path style {other:?}"
                ));
            }
        };
        let facts = HostFacts {
            path_style,
            default_shell: default_shell.unwrap_or_else(|| "/bin/sh".to_string()),
            shell: shell.unwrap_or_else(|| "/bin/sh".to_string()),
        };
        Ok((stream_id, inbound, facts))
    }

    /// Moves the project server's messages between the stream and the client
    /// until one side ends, and ends the session with them.
    async fn run_project_stream(
        stream: ProjectStream,
        incoming_tx: UnboundedSender<Envelope>,
        mut outgoing_rx: UnboundedReceiver<Envelope>,
        mut connection_activity_tx: Sender<()>,
        cx: &mut AsyncApp,
    ) -> Result<i32> {
        let ProjectStream {
            stream_id,
            session,
            mut inbound,
            release: _release,
        } = stream;

        let writing = cx.spawn({
            let session = session.clone();
            async move |cx| {
                while let Some(envelope) = outgoing_rx.next().await {
                    let bytes = encode_message(envelope).await?;
                    send_data_waiting(&session, stream_id, &bytes, cx).await?;
                }
                anyhow::Ok(())
            }
        });
        let reading = cx.background_spawn(async move {
            let mut decoder = MessageDecoder::default();
            while let Some(chunk) = inbound.next().await {
                connection_activity_tx.try_send(()).ok();
                for envelope in decoder.push(&chunk)? {
                    incoming_tx.unbounded_send(envelope).ok();
                }
            }
            anyhow::Ok(())
        });

        let outcome = select_biased! {
            result = reading.fuse() => result.context("reading from the other Zode"),
            result = writing.fuse() => result.context("writing to the other Zode"),
        };
        // Tells the other Zode to stop its project server; best effort,
        // because the session may be the thing that ended.
        session
            .update(cx, |session, cx| session.send_end_of_stream(stream_id, cx))
            .ok()
            .transpose()
            .map_err(|error| log::debug!("could not end the project stream: {error}"))
            .ok();
        outcome?;
        Err(anyhow!("the other Zode closed the project connection"))
    }
}

impl Drop for RelayRemoteConnection {
    fn drop(&mut self) {
        self.sessions.end_all();
    }
}

#[async_trait(?Send)]
impl RemoteConnection for RelayRemoteConnection {
    fn start_proxy(
        &self,
        _unique_identifier: String,
        reconnect: bool,
        incoming_tx: UnboundedSender<Envelope>,
        outgoing_rx: UnboundedReceiver<Envelope>,
        connection_activity_tx: Sender<()>,
        delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Task<Result<i32>> {
        // The project server on the other Zode ended with the stream it served,
        // and a new one would know nothing of what the client holds. Saying so
        // is what lets the client report that the server is gone, as it does
        // for a proxy that finds none.
        if reconnect {
            return Task::ready(Ok(ProxyLaunchError::ServerNotRunning.to_exit_code()));
        }
        let first = self.first_stream.lock().take();
        let options = self.options.clone();
        let sessions = self.sessions.clone();
        // Counted from here, not from when the task first runs: another
        // window's reconnect must not end the connection in between.
        let opening = first.is_none().then(|| sessions.opening());
        cx.spawn(async move |cx| {
            let stream = match first {
                Some(stream) => stream,
                None => {
                    if sessions.is_killed() {
                        return Err(anyhow!(
                            "the connection to {} was lost; open the project again",
                            options.host_name
                        ));
                    }
                    Self::open_project_stream(&options, &sessions, delegate.as_ref(), cx)
                        .await?
                        .0
                }
            };
            drop(opening);
            Self::run_project_stream(stream, incoming_tx, outgoing_rx, connection_activity_tx, cx)
                .await
        })
    }

    fn upload_directory(
        &self,
        _src_path: PathBuf,
        _dest_path: RemotePathBuf,
        _cx: &App,
    ) -> Task<Result<()>> {
        Task::ready(Err(anyhow!(
            "files cannot be uploaded over the relay: both Zodes must run the same version"
        )))
    }

    /// Ends the connection if no window is using it. A window that reconnects
    /// has dropped its own proxy, and with it its own session, by the time it
    /// kills the connection; whatever session is still live is another
    /// window's, and ending it would end that window's project server.
    async fn kill(&self) -> Result<()> {
        self.sessions.end_if_unused();
        Ok(())
    }

    fn has_been_killed(&self) -> bool {
        self.sessions.is_ended()
    }

    fn build_command(
        &self,
        _program: Option<String>,
        _args: &[String],
        _env: &HashMap<String, String>,
        _working_dir: Option<String>,
        _port_forward: Option<(u16, String, u16)>,
        _interactive: Interactive,
    ) -> Result<CommandTemplate> {
        Err(anyhow!(
            "there is no local command that reaches another Zode; its terminals run through the project server"
        ))
    }

    fn build_forward_ports_command(
        &self,
        _forwards: Vec<(u16, String, u16)>,
    ) -> Result<CommandTemplate> {
        Err(anyhow!("ports cannot be forwarded through the relay"))
    }

    fn connection_options(&self) -> RemoteConnectionOptions {
        RemoteConnectionOptions::Relay(self.options.clone())
    }

    fn path_style(&self) -> PathStyle {
        self.facts.path_style
    }

    fn shell(&self) -> String {
        self.facts.shell.clone()
    }

    fn default_system_shell(&self) -> String {
        self.facts.default_shell.clone()
    }

    fn has_wsl_interop(&self) -> bool {
        false
    }

    fn terminals_over_rpc(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
