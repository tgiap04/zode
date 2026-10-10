//! Pairing from the requesting side: the Zode that wants to control another.
//!
//! Nothing is trusted until a person at this screen says the digits match, and
//! even then the host is pinned only after an encrypted session to it has
//! been brought up with the key the pairing produced. The key must also be the
//! one the account's directory lists for that device: a relay that sits in the
//! middle of a pairing can show each end a key of its own, but cannot change
//! what the directory says.

use std::time::Duration;

use futures::{FutureExt as _, channel::oneshot, select_biased};
use gpui::{AppContext as _, AsyncApp, Context, EventEmitter, Task, WeakEntity};
use remote_relay_protocol::{PAIRING_EXPIRY, PairingMessage, PairingRequester, random_nonce};

use crate::{
    ConnectError, OpenMode, PinnedDevice, RelayHost, RelayInitiator, RelayInitiatorEvent,
    initiator::SidEvent,
    initiator_connect::{OpenedSession, with_timeout},
};

/// The host answers a pairing request at once; waiting longer means it is not
/// going to.
const ACCEPT_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingFailure {
    #[error("That Zode is not online.")]
    HostOffline,
    #[error(
        "That Zode is busy with another pairing, or has locked pairing after failed attempts. \
         Unlock it there and try again."
    )]
    Busy,
    #[error("Another pairing is already in progress here.")]
    AnotherAttempt,
    #[error("The numbers did not match. Nothing was paired.")]
    DigitsDiffer,
    #[error("Pairing was cancelled.")]
    Cancelled,
    /// The key that arrived is not the one the account lists for that device.
    #[error(
        "That Zode's key is not the one your account lists for it, so it was not paired. \
         Someone may be intercepting the connection."
    )]
    KeyChanged,
    #[error(
        "That Zode has not published its key yet. Open Zode there while signed in, then try again."
    )]
    HostNotReady,
    #[error("That Zode did not accept the pairing.")]
    NotAccepted,
    #[error("Pairing took too long and was stopped.")]
    TimedOut,
    #[error("{0}")]
    Relay(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingStep {
    Requesting,
    /// The digits to compare with the ones the other Zode shows.
    Comparing {
        digits: String,
    },
    /// Digits confirmed here; waiting for the person at the other Zode.
    WaitingForHost,
    /// The other Zode answered; proving it holds the key.
    Verifying,
    Done,
    Failed(PairingFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingAttemptEvent {
    Changed,
}

pub struct PairingAttempt {
    host: RelayHost,
    step: PairingStep,
    decision: Option<oneshot::Sender<bool>>,
    task: Option<Task<()>>,
}

impl EventEmitter<PairingAttemptEvent> for PairingAttempt {}

impl PairingAttempt {
    pub fn host(&self) -> &RelayHost {
        &self.host
    }

    pub fn step(&self) -> &PairingStep {
        &self.step
    }

    /// Whether the attempt has ended, one way or the other.
    pub fn is_finished(&self) -> bool {
        matches!(self.step, PairingStep::Done | PairingStep::Failed(_))
    }

    /// The person says the digits here match the ones on the other Zode.
    pub fn confirm_digits(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.step, PairingStep::Comparing { .. }) {
            return;
        }
        let Some(decision) = self.decision.take() else {
            return;
        };
        if decision.send(true).is_err() {
            log::debug!("the pairing ended before the digits were confirmed");
            return;
        }
        self.set_step(PairingStep::WaitingForHost, cx);
    }

    /// The person says the digits differ. Treated as an attack until shown
    /// otherwise: nothing is pinned and the exchange is dropped.
    pub fn digits_differ(&mut self, cx: &mut Context<Self>) {
        if self.is_finished() {
            return;
        }
        log::warn!("the digits shown while pairing did not match");
        self.end(PairingFailure::DigitsDiffer, cx);
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.is_finished() {
            return;
        }
        self.end(PairingFailure::Cancelled, cx);
    }

    fn end(&mut self, failure: PairingFailure, cx: &mut Context<Self>) {
        // Dropping the task stops the exchange wherever it is waiting.
        self.task = None;
        self.decision = None;
        self.set_step(PairingStep::Failed(failure), cx);
    }

    fn set_step(&mut self, step: PairingStep, cx: &mut Context<Self>) {
        self.step = step;
        cx.emit(PairingAttemptEvent::Changed);
        cx.notify();
    }
}

fn failure_for(error: ConnectError) -> PairingFailure {
    match error {
        ConnectError::HostOffline => PairingFailure::HostOffline,
        ConnectError::TimedOut => PairingFailure::TimedOut,
        other => PairingFailure::Relay(other.to_string()),
    }
}

impl RelayInitiator {
    /// Starts pairing with `host`. The returned attempt is already running;
    /// watch it for [`PairingStep::Comparing`] and then call
    /// [`PairingAttempt::confirm_digits`] or [`PairingAttempt::digits_differ`].
    pub fn pair(
        &mut self,
        host: RelayHost,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<PairingAttempt> {
        let initiator = cx.weak_entity();
        let busy = self
            .active_pairing
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .is_some_and(|attempt| !attempt.read(cx).is_finished());
        if busy {
            return cx.new(|_| PairingAttempt {
                host,
                step: PairingStep::Failed(PairingFailure::AnotherAttempt),
                decision: None,
                task: None,
            });
        }
        let attempt = cx.new(|cx| {
            let (decision_sender, decision) = oneshot::channel();
            let task = cx.spawn({
                let host = host.clone();
                let initiator = initiator.clone();
                async move |this, cx| Self::run_pairing(this, initiator, host, decision, cx).await
            });
            PairingAttempt {
                host,
                step: PairingStep::Requesting,
                decision: Some(decision_sender),
                task: Some(task),
            }
        });
        self.active_pairing = Some(attempt.downgrade());
        attempt
    }

    async fn run_pairing(
        attempt: WeakEntity<PairingAttempt>,
        initiator: WeakEntity<RelayInitiator>,
        host: RelayHost,
        decision: oneshot::Receiver<bool>,
        cx: &mut AsyncApp,
    ) {
        let outcome = Self::pairing_steps(&attempt, &initiator, &host, decision, cx).await;
        let finished = attempt.update(cx, |attempt, cx| {
            attempt.task = None;
            let step = match outcome {
                Ok(()) => PairingStep::Done,
                Err(failure) => PairingStep::Failed(failure),
            };
            attempt.set_step(step, cx);
        });
        if finished.is_err() {
            log::debug!("the pairing attempt was dropped before it finished");
        }
    }

    async fn pairing_steps(
        attempt: &WeakEntity<PairingAttempt>,
        initiator: &WeakEntity<RelayInitiator>,
        host: &RelayHost,
        decision: oneshot::Receiver<bool>,
        cx: &mut AsyncApp,
    ) -> Result<(), PairingFailure> {
        let host_key = host.public_key.ok_or(PairingFailure::HostNotReady)?;
        initiator
            .read_with(cx, |initiator, _| {
                initiator
                    .hosts_trust
                    .as_ref()
                    .ok_or_else(|| PairingFailure::Relay("pinned hosts are not loaded".into()))?
                    .check_can_pin(&host.device_id)
                    .map_err(|error| PairingFailure::Relay(error.to_string()))
            })
            .map_err(|_| PairingFailure::Cancelled)??;
        let opened = Self::open_awaiting(initiator, &host.device_id, OpenMode::Pair, cx)
            .await
            .map_err(failure_for)?;
        let mut relay_ended = false;
        let result = Self::exchange(
            attempt,
            initiator,
            host,
            host_key,
            decision,
            &opened,
            &mut relay_ended,
            cx,
        )
        .await;
        if result.is_err() && !relay_ended {
            opened.abandon(cx);
        } else {
            // A pairing that finished left no session worth keeping, and the
            // host has already closed its end.
            opened.release();
            opened.initiator.update(cx, |initiator, _| {
                initiator.routes.remove(&opened.session_id);
            });
        }
        result
    }

    #[expect(clippy::too_many_arguments)]
    async fn exchange(
        attempt: &WeakEntity<PairingAttempt>,
        initiator: &WeakEntity<RelayInitiator>,
        host: &RelayHost,
        host_key: [u8; 32],
        decision: oneshot::Receiver<bool>,
        opened: &OpenedSession,
        relay_ended: &mut bool,
        cx: &mut AsyncApp,
    ) -> Result<(), PairingFailure> {
        let nonce = random_nonce().map_err(|error| PairingFailure::Relay(error.to_string()))?;
        let (requester, request) = PairingRequester::start(*opened.keypair.public_key(), nonce);
        opened
            .writer
            .send_pairing(opened.session_id, &request)
            .map_err(|error| PairingFailure::Relay(error.to_string()))?;

        let (acceptor_key, acceptor_nonce) = loop {
            let event = with_timeout(cx, ACCEPT_WAIT, opened.events.recv())
                .await
                .ok_or(PairingFailure::TimedOut)?
                .map_err(|_| PairingFailure::Relay("the session was dropped".into()))?;
            match event {
                SidEvent::Pairing(PairingMessage::PairAccept { public_key, nonce }) => {
                    break (public_key, nonce);
                }
                SidEvent::Pairing(_) | SidEvent::Frame(_) => {}
                SidEvent::Closed(_) => {
                    *relay_ended = true;
                    return Err(PairingFailure::Busy);
                }
                SidEvent::LinkLost(reason) => {
                    *relay_ended = true;
                    return Err(PairingFailure::Relay(reason));
                }
            }
        };

        // Checked before our nonce is revealed: revealing it is what puts the
        // digits on the other screen, and a person should not be asked to
        // compare digits for a key that is already known to be wrong.
        if acceptor_key != host_key {
            return Err(PairingFailure::KeyChanged);
        }
        let (reveal, outcome) = requester
            .receive_accept(acceptor_key, acceptor_nonce)
            .map_err(|error| PairingFailure::Relay(error.to_string()))?;
        opened
            .writer
            .send_pairing(opened.session_id, &reveal)
            .map_err(|error| PairingFailure::Relay(error.to_string()))?;
        attempt
            .update(cx, |attempt, cx| {
                attempt.set_step(
                    PairingStep::Comparing {
                        digits: outcome.short_authentication_string.clone(),
                    },
                    cx,
                );
            })
            .map_err(|_| PairingFailure::Cancelled)?;

        let expiry = cx.background_executor().timer(PAIRING_EXPIRY).fuse();
        futures::pin_mut!(expiry);
        let mut decision = decision.fuse();
        let mut host_closed = false;
        loop {
            let already_closed = host_closed;
            let event = async move {
                if already_closed {
                    futures::future::pending::<Option<SidEvent>>().await
                } else {
                    opened.events.recv().await.ok()
                }
            }
            .fuse();
            futures::pin_mut!(event);
            select_biased! {
                answer = decision => match answer {
                    Ok(true) => break,
                    Ok(false) | Err(_) => return Err(PairingFailure::Cancelled),
                },
                event = event => match event {
                    // The host may well have decided first; whether it said
                    // yes is learned from the session below.
                    Some(SidEvent::Closed(_)) | None => {
                        host_closed = true;
                        *relay_ended = true;
                    }
                    Some(SidEvent::LinkLost(reason)) => {
                        *relay_ended = true;
                        return Err(PairingFailure::Relay(reason));
                    }
                    Some(SidEvent::Pairing(_) | SidEvent::Frame(_)) => {}
                },
                _ = expiry => return Err(PairingFailure::TimedOut),
            }
        }

        while !host_closed {
            let event = with_timeout(cx, PAIRING_EXPIRY, opened.events.recv())
                .await
                .ok_or(PairingFailure::TimedOut)?;
            match event {
                Ok(SidEvent::Closed(_)) | Err(_) => {
                    host_closed = true;
                    *relay_ended = true;
                }
                Ok(SidEvent::LinkLost(reason)) => {
                    *relay_ended = true;
                    return Err(PairingFailure::Relay(reason));
                }
                Ok(SidEvent::Pairing(_) | SidEvent::Frame(_)) => {}
            }
        }

        attempt
            .update(cx, |attempt, cx| {
                attempt.set_step(PairingStep::Verifying, cx);
            })
            .map_err(|_| PairingFailure::Cancelled)?;
        let session =
            match Self::establish(initiator, &host.device_id, &host.name, host_key, cx).await {
                Ok(session) => session,
                Err(ConnectError::NotPaired) => return Err(PairingFailure::NotAccepted),
                Err(error) => return Err(failure_for(error)),
            };
        session.update(cx, |session, cx| session.close(cx));

        let device = PinnedDevice {
            device_id: host.device_id.clone(),
            public_key: host_key,
            name: host.name.clone(),
            paired_at: crate::initiator::unix_now(),
        };
        initiator
            .update(cx, |initiator, cx| {
                let pinned = initiator
                    .hosts_trust
                    .as_mut()
                    .ok_or_else(|| PairingFailure::Relay("pinned hosts are not loaded".into()))?
                    .pin(device, cx)
                    .map_err(|error| PairingFailure::Relay(error.to_string()));
                cx.emit(RelayInitiatorEvent::Changed);
                cx.notify();
                pinned
            })
            .map_err(|_| PairingFailure::Cancelled)?
    }
}
