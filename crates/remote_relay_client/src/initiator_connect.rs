//! Opening a session to a host and running the handshake from the initiator's
//! end. Shared by connecting to a paired host and by the last step of pairing.

use std::{cell::Cell, future::Future, sync::Arc, time::Duration};

use futures::{FutureExt as _, select_biased};
use gpui::{AppContext as _, AsyncApp, Context, Entity, Task, WeakEntity};
use remote_relay_protocol::{
    Control, DeviceKeypair, HANDSHAKE_TIMEOUT, Handshake, HandshakeParameters, InnerKind, KEY_LEN,
    RELAY_PROTOCOL_VERSION, RPC_PROTOCOL_VERSION, build_prologue, reject_unknown_version,
};
use smol::channel::Receiver;

use crate::{
    ConnectError, ConnectionState, OpenMode, RelayInitiator, RelaySession, RelayWriter,
    initiator::{CAPABILITIES, CONNECT_WAIT, OPEN_WAIT, Route, SidEvent},
    secure_session::{SecureChannel, read_control},
    session::{HostFacts, RetiredSessions},
};

/// Runs `future` unless `limit` passes first.
pub(crate) async fn with_timeout<T>(
    cx: &AsyncApp,
    limit: Duration,
    future: impl Future<Output = T>,
) -> Option<T> {
    let timer = cx.background_executor().timer(limit).fuse();
    let future = future.fuse();
    futures::pin_mut!(future, timer);
    select_biased! {
        value = future => Some(value),
        _ = timer => None,
    }
}

/// A session the relay has opened that nothing has been said on yet.
pub(crate) struct OpenedSession {
    pub(crate) session_id: u32,
    pub(crate) events: Receiver<SidEvent>,
    pub(crate) writer: RelayWriter,
    pub(crate) keypair: Arc<DeviceKeypair>,
    pub(crate) user_id: String,
    pub(crate) local_device_id: String,
    pub(crate) app_version: String,
    pub(crate) initiator: Entity<RelayInitiator>,
    pub(crate) retired: RetiredSessions,
    /// Set once the session has been closed, or passed on to something that
    /// closes it, so that dropping this does not close it again.
    released: Cell<bool>,
}

impl Drop for OpenedSession {
    /// A handshake or pairing that ends early, or is cancelled, still leaves
    /// no session behind at the relay or a route here.
    fn drop(&mut self) {
        if self.released.get() {
            return;
        }
        if let Err(error) = self.writer.close_session(self.session_id) {
            log::debug!("could not tell the relay to close session: {error}");
        }
        self.retired.lock().push(self.session_id);
    }
}

impl OpenedSession {
    /// Ends the session at the relay and forgets it here.
    pub(crate) fn abandon(&self, cx: &mut AsyncApp) {
        self.released.set(true);
        let session_id = self.session_id;
        self.initiator.update(cx, |this, cx| {
            this.routes.remove(&session_id);
            this.close_at_relay(session_id, cx);
        });
    }

    /// The session has ended or now belongs to something else; dropping this
    /// has nothing left to close.
    pub(crate) fn release(&self) {
        self.released.set(true);
    }

    /// The next frame, which is what a handshake waits for. A pairing message
    /// is not one, and is skipped.
    pub(crate) async fn next_frame(
        &self,
        cx: &AsyncApp,
        limit: Duration,
    ) -> Result<Vec<u8>, ConnectError> {
        loop {
            let event = with_timeout(cx, limit, self.events.recv())
                .await
                .ok_or(ConnectError::TimedOut)?
                .map_err(|_| ConnectError::Relay("the session was dropped".into()))?;
            match event {
                SidEvent::Frame(bytes) => return Ok(bytes),
                SidEvent::Pairing(_) => {}
                SidEvent::Closed(reason) => return Err(closed_error(&reason)),
                SidEvent::LinkLost(reason) => return Err(ConnectError::Relay(reason)),
            }
        }
    }
}

/// What a session the relay ended before the handshake finished means. Only
/// the host closing it, or no reason at all, says it does not trust this Zode.
fn closed_error(reason: &str) -> ConnectError {
    match reason {
        "peer_gone" => ConnectError::HostOffline,
        "rate_limited" | "quota" | "quota_exceeded" => ConnectError::RateLimited,
        "" | "closed" => ConnectError::NotPaired,
        other => ConnectError::Relay(other.to_string()),
    }
}

impl RelayInitiator {
    /// Opens a session of `mode` to `host_device_id` and returns it with the
    /// pieces needed to speak on it.
    pub(crate) async fn open_awaiting(
        this: &WeakEntity<Self>,
        host_device_id: &str,
        mode: OpenMode,
        cx: &mut AsyncApp,
    ) -> Result<OpenedSession, ConnectError> {
        let gone = || ConnectError::Unavailable("the relay connection was closed".into());
        let startup = this
            .read_with(cx, |this, _| this.startup.clone())
            .map_err(|_| gone())?;
        startup
            .await
            .map_err(|error| ConnectError::Unavailable(error.to_string()))?;
        let lock = this
            .read_with(cx, |this, _| this.open_lock.clone())
            .map_err(|_| gone())?;
        let _one_open_at_a_time = lock.lock().await;

        Self::wait_connected(this, cx).await?;

        let host = host_device_id.to_string();
        let (receiver, writer, keypair, user_id, local_device_id, app_version, initiator, retired) =
            this.update(cx, |this, cx| {
                let writer = this
                    .writer(cx)
                    .ok_or_else(|| ConnectError::Relay("not connected".into()))?;
                let identity = this.identity.as_ref().ok_or_else(|| {
                    ConnectError::Unavailable("this device's key is not ready".into())
                })?;
                let (respond, receiver) = futures::channel::oneshot::channel();
                this.pending_open = Some(crate::initiator::PendingOpen {
                    host_device_id: host.clone(),
                    mode,
                    respond,
                });
                if let Err(error) = writer.request_open(&host, mode) {
                    this.pending_open = None;
                    return Err(ConnectError::Relay(error.to_string()));
                }
                Ok((
                    receiver,
                    writer,
                    identity.keypair.clone(),
                    identity.user_id.clone(),
                    identity.device_id.clone(),
                    this.app_version.clone(),
                    cx.entity(),
                    this.retired.clone(),
                ))
            })
            .map_err(|_| gone())??;

        let answer = with_timeout(cx, OPEN_WAIT, receiver).await;
        let (session_id, events) = match answer {
            Some(Ok(Ok(opened))) => opened,
            Some(Ok(Err(error))) => return Err(error),
            Some(Err(_)) => return Err(gone()),
            None => {
                this.update(cx, |this, _| this.pending_open = None).ok();
                return Err(ConnectError::TimedOut);
            }
        };
        Ok(OpenedSession {
            session_id,
            events,
            writer,
            keypair,
            user_id,
            local_device_id,
            app_version,
            initiator,
            retired,
            released: Cell::new(false),
        })
    }

    async fn wait_connected(
        this: &WeakEntity<Self>,
        cx: &mut AsyncApp,
    ) -> Result<(), ConnectError> {
        let environment = this
            .read_with(cx, |this, _| this.environment.clone())
            .map_err(|_| ConnectError::Unavailable("the relay connection was closed".into()))?;
        let credential = environment
            .credentials
            .credential(cx)
            .await
            .map_err(|error| ConnectError::Unavailable(error.to_string()))?;
        let same_account = this
            .update(cx, |this, cx| this.check_account(&credential, cx))
            .map_err(|_| ConnectError::Unavailable("the relay connection was closed".into()))?;
        if !same_account {
            return Err(ConnectError::Unavailable(
                "the signed-in account changed".into(),
            ));
        }
        loop {
            let waiting = this
                .update(cx, |this, cx| {
                    if let Some(reason) = this.stopped {
                        return Err(ConnectError::Relay(format!("{reason:?}")));
                    }
                    let connected = this.client.as_ref().is_some_and(|client| {
                        client.read(cx).state() == ConnectionState::Connected
                    });
                    if connected {
                        return Ok(None);
                    }
                    let (sender, receiver) = futures::channel::oneshot::channel();
                    this.connect_waiters.push(sender);
                    Ok(Some(receiver))
                })
                .map_err(|_| {
                    ConnectError::Unavailable("the relay connection was closed".into())
                })??;
            let Some(receiver) = waiting else {
                return Ok(());
            };
            match with_timeout(cx, CONNECT_WAIT, receiver).await {
                Some(_) => {}
                None => {
                    return Err(ConnectError::Relay("the relay could not be reached".into()));
                }
            }
        }
    }

    /// Opens a `session` to the host and brings the encrypted channel up with
    /// `host_key`, which must be a key a person has agreed to trust. The
    /// session is returned only once the host has answered `hello` under the
    /// channel's keys, which only the holder of the matching private key can.
    pub(crate) async fn establish(
        this: &WeakEntity<Self>,
        host_device_id: &str,
        host_name: &str,
        host_key: [u8; KEY_LEN],
        cx: &mut AsyncApp,
    ) -> Result<Entity<RelaySession>, ConnectError> {
        let opened = Self::open_awaiting(this, host_device_id, OpenMode::Session, cx).await?;
        // One deadline for the whole handshake: a host that keeps sending
        // frames that are not its answer must not hold this open forever.
        let brought_up = with_timeout(
            cx,
            HANDSHAKE_TIMEOUT,
            Self::bring_up_channel(&opened, host_device_id, host_key, cx),
        )
        .await
        .unwrap_or(Err(ConnectError::TimedOut));
        let (channel, facts) = match brought_up {
            Ok(brought_up) => brought_up,
            Err(error) => {
                opened.abandon(cx);
                return Err(error);
            }
        };
        let host_device_id = host_device_id.to_string();
        let host_name = host_name.to_string();
        let session_id = opened.session_id;
        opened.initiator.update(cx, |this, cx| {
            let writer = this
                .writer(cx)
                .ok_or_else(|| ConnectError::Relay("not connected".into()))?;
            let initiator = cx.entity();
            let retired = this.retired.clone();
            let session = cx.new(|_| {
                RelaySession::new(
                    session_id,
                    host_device_id,
                    host_name,
                    facts,
                    channel,
                    writer,
                    retired,
                    initiator,
                )
            });
            // From here the session closes itself, or the relay already did.
            opened.release();
            // Frames that arrived right behind the host's answer, such as its
            // lists, were queued for the handshake and belong to the session.
            while let Ok(event) = opened.events.try_recv() {
                match event {
                    SidEvent::Frame(payload) => {
                        session.update(cx, |session, cx| session.on_frame(&payload, cx));
                    }
                    SidEvent::Closed(reason) | SidEvent::LinkLost(reason) => {
                        session.update(cx, |session, cx| session.finish(reason, cx));
                    }
                    SidEvent::Pairing(_) => {}
                }
            }
            if let Some(reason) = session.read(cx).closed_reason() {
                this.routes.remove(&session_id);
                return Err(ConnectError::Relay(reason.to_string()));
            }
            this.routes
                .insert(session_id, Route::Session(session.downgrade()));
            Ok(session)
        })
    }

    async fn bring_up_channel(
        opened: &OpenedSession,
        host_device_id: &str,
        host_key: [u8; KEY_LEN],
        cx: &AsyncApp,
    ) -> Result<(SecureChannel, HostFacts), ConnectError> {
        let handshake_error =
            |error: remote_relay_protocol::NoiseError| ConnectError::Handshake(error.to_string());
        let prologue = build_prologue(&opened.user_id, &opened.local_device_id, host_device_id)
            .map_err(handshake_error)?;
        let mut handshake = Handshake::initiator(&HandshakeParameters {
            local_private_key: opened.keypair.private_key(),
            remote_public_key: &host_key,
            prologue: &prologue,
        })
        .map_err(handshake_error)?;
        let first = handshake.write_message(&[]).map_err(handshake_error)?;
        opened
            .writer
            .send_frame(opened.session_id, &first)
            .map_err(|error| ConnectError::Relay(error.to_string()))?;

        let reply = opened.next_frame(cx, HANDSHAKE_TIMEOUT).await?;
        handshake.read_message(&reply).map_err(handshake_error)?;
        let mut channel = SecureChannel::new(handshake.into_session().map_err(handshake_error)?);

        // Checked before sealing: a sealed hello that is then not sent would
        // leave a gap in the counter the host could never read past.
        if !opened.writer.has_capacity_for(1) {
            return Err(ConnectError::RateLimited);
        }
        let hello = channel
            .seal_control(&Control::Hello {
                relay_protocol: RELAY_PROTOCOL_VERSION,
                app_version: opened.app_version.clone(),
                rpc_protocol: RPC_PROTOCOL_VERSION,
                capabilities: CAPABILITIES.iter().map(|name| name.to_string()).collect(),
            })
            .map_err(|error| ConnectError::Handshake(error.to_string()))?;
        opened
            .writer
            .send_frame(opened.session_id, &hello)
            .map_err(|error| ConnectError::Relay(error.to_string()))?;

        loop {
            let sealed = opened.next_frame(cx, HANDSHAKE_TIMEOUT).await?;
            let frame = channel
                .open(&sealed)
                .map_err(|error| ConnectError::Handshake(error.to_string()))?;
            if frame.kind != InnerKind::Control {
                continue;
            }
            match read_control(&frame).map_err(|error| ConnectError::Protocol(error.to_string()))? {
                Control::HelloAck {
                    relay_protocol,
                    app_version,
                    rpc_protocol,
                    capabilities,
                } => {
                    if reject_unknown_version(relay_protocol).is_some() {
                        return Err(ConnectError::Protocol(format!(
                            "it speaks relay protocol {relay_protocol}"
                        )));
                    }
                    return Ok((
                        channel,
                        HostFacts {
                            app_version,
                            relay_protocol,
                            rpc_protocol,
                            capabilities,
                        },
                    ));
                }
                Control::Error { code, message, .. } => {
                    return Err(ConnectError::Refused { code, message });
                }
                _ => {}
            }
        }
    }

    /// A session to a host that was paired earlier, using the key pinned for it.
    pub fn connect(
        &mut self,
        host_device_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Entity<RelaySession>, ConnectError>> {
        let host_device_id = host_device_id.to_string();
        cx.spawn(async move |this, cx| {
            let startup = this
                .read_with(cx, |this, _| this.startup.clone())
                .map_err(|_| ConnectError::Unavailable("the relay connection was closed".into()))?;
            startup
                .await
                .map_err(|error| ConnectError::Unavailable(error.to_string()))?;
            let pinned = this
                .read_with(cx, |this, _| {
                    this.hosts_trust
                        .as_ref()
                        .and_then(|trust| trust.get(&host_device_id).cloned())
                })
                .map_err(|_| ConnectError::Unavailable("the relay connection was closed".into()))?
                .ok_or(ConnectError::NotPaired)?;
            Self::establish(&this, &host_device_id, &pinned.name, pinned.public_key, cx).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_ended_by_the_relay_is_told_apart_by_its_reason() {
        assert_eq!(closed_error("peer_gone"), ConnectError::HostOffline);
        assert_eq!(closed_error("rate_limited"), ConnectError::RateLimited);
        assert_eq!(closed_error("quota"), ConnectError::RateLimited);
        assert_eq!(closed_error("closed"), ConnectError::NotPaired);
        assert_eq!(closed_error(""), ConnectError::NotPaired);
        assert_eq!(
            closed_error("shutting_down"),
            ConnectError::Relay("shutting_down".into())
        );
    }
}
