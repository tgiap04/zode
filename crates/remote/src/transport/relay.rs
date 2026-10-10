//! Reaching another Zode's project server through the zodekit relay.
//!
//! The other Zode runs `remote_server proxy` for us and carries its stdio on
//! one stream of the encrypted session, so everything above this file -- the
//! project, language servers, git -- works as it does over SSH. What cannot
//! work is anything that needs a local command to reach the machine:
//! terminals are driven with terminal messages instead (see
//! [`RemoteConnection::terminals_over_rpc`]), and a binary cannot be uploaded,
//! which is why both Zodes must run the same version.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
use gpui::{App, AppContext as _, AsyncApp, Task};
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
    stream_id: u32,
    session: gpui::WeakEntity<RelaySession>,
    /// Taken by the one `start_proxy` that may use this stream.
    inbound: Mutex<Option<StreamReceiver>>,
    facts: HostFacts,
    ended: Arc<AtomicBool>,
    /// Closing the session when this connection is let go, or killed.
    close_signal: smol::channel::Sender<()>,
    closer: Mutex<Option<Task<()>>>,
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
        let initiator = cx.update(RelayInitiator::get_or_create)?;
        let session = initiator
            .update(cx, |initiator, cx| {
                initiator.connect(&options.host_device_id, cx)
            })
            .await
            .map_err(|error| anyhow!("{error}"))?;

        match Self::open_project_server(&options, &session, delegate.as_ref(), cx).await {
            Ok(connection) => Ok(connection),
            Err(error) => {
                session.update(cx, |session, cx| session.close(cx));
                Err(error)
            }
        }
    }

    async fn open_project_server(
        options: &RelayConnectionOptions,
        session: &gpui::Entity<RelaySession>,
        delegate: &dyn RemoteClientDelegate,
        cx: &mut AsyncApp,
    ) -> Result<Self> {
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

        let ended = Arc::new(AtomicBool::new(false));
        // Subscribed here, not inside the task below: a task may not run until
        // after the session has closed, and a close nobody was listening for
        // would leave `has_been_killed` false for good.
        let watch = cx.update(|cx| {
            let ended = ended.clone();
            cx.subscribe(session, move |_, event: &RelaySessionEvent, _| {
                if matches!(event, RelaySessionEvent::Closed(_)) {
                    ended.store(true, Ordering::SeqCst);
                }
            })
        });
        if session.read_with(cx, |session, _| session.is_closed()) {
            ended.store(true, Ordering::SeqCst);
        }
        let (close_signal, close_requested) = smol::channel::bounded(1);
        let closer = cx.spawn({
            let strong = session.clone();
            let session = session.downgrade();
            async move |cx| {
                // The task is what keeps the session alive: the connection
                // itself holds only a weak handle.
                let _strong = strong;
                let _watch = watch;
                // Both a signal and the sender being dropped mean the same:
                // nobody is using the connection any more.
                close_requested.recv().await.ok();
                session.update(cx, |session, cx| session.close(cx)).ok();
            }
        });
        Ok(Self {
            options: options.clone(),
            stream_id,
            session: session.downgrade(),
            inbound: Mutex::new(Some(inbound)),
            facts: HostFacts {
                path_style,
                default_shell: default_shell.unwrap_or_else(|| "/bin/sh".to_string()),
                shell: shell.unwrap_or_else(|| "/bin/sh".to_string()),
            },
            ended,
            close_signal,
            closer: Mutex::new(Some(closer)),
        })
    }
}

impl Drop for RelayRemoteConnection {
    fn drop(&mut self) {
        self.close_signal.try_send(()).ok();
        // Left to finish: cancelling it here would skip the very close it was
        // woken for.
        if let Some(closer) = self.closer.lock().take() {
            closer.detach();
        }
    }
}

#[async_trait(?Send)]
impl RemoteConnection for RelayRemoteConnection {
    fn start_proxy(
        &self,
        _unique_identifier: String,
        _reconnect: bool,
        incoming_tx: UnboundedSender<Envelope>,
        mut outgoing_rx: UnboundedReceiver<Envelope>,
        mut connection_activity_tx: Sender<()>,
        _delegate: Arc<dyn RemoteClientDelegate>,
        cx: &mut AsyncApp,
    ) -> Task<Result<i32>> {
        // The project server on the other Zode belongs to this stream. A second
        // call means the first one ended, and that server is gone with it.
        let Some(mut inbound) = self.inbound.lock().take() else {
            return Task::ready(Err(anyhow!(
                "the connection to {} was lost; open the project again",
                self.options.host_name
            )));
        };
        let session = self.session.clone();
        let stream_id = self.stream_id;

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

        cx.spawn(async move |cx| {
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

    async fn kill(&self) -> Result<()> {
        self.ended.store(true, Ordering::SeqCst);
        // A full queue already holds a pending signal.
        self.close_signal.try_send(()).ok();
        Ok(())
    }

    fn has_been_killed(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
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
