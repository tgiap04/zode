//! An established, confirmed session to a host, seen from the Zode that
//! controls it.
//!
//! Frames are sealed and written in one synchronous step, because the cipher's
//! counter is implicit: a frame sealed and then dropped, or two frames sealed
//! in one order and written in another, leaves the host unable to read on.

use std::{
    collections::HashMap,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use futures::{
    Stream,
    channel::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
};
use gpui::{AsyncApp, Context, Entity, EventEmitter, Task, WeakEntity};
use remote_relay_protocol::{
    AgentSummary, Control, InnerFrame, InnerKind, MAX_INNER_PAYLOAD_LEN, TerminalSummary,
    decode_control,
};

use crate::{RelayInitiator, RelayWriter, SendError, secure_session::SecureChannel};

/// Requests that may wait for an answer at once. A host that never answers
/// must not make this grow without bound.
pub const MAX_PENDING_REQUESTS: usize = 64;

/// Streams one session may have registered at once.
pub const MAX_OPEN_STREAMS: usize = 64;

/// Agents and terminals remembered from the host's lists.
const MAX_MIRRORED_ITEMS: usize = 256;

/// Bytes a stream may hold that its reader has not taken yet. A host that
/// sends faster than the reader drains ends that stream instead of growing
/// memory without bound.
pub const MAX_STREAM_QUEUE_BYTES: usize = 1024 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_RETRY: Duration = Duration::from_millis(25);
pub(crate) const WRITE_DEADLINE: Duration = Duration::from_secs(30);

/// Sessions to close at the relay whose routes the initiator has yet to drop.
/// Filled by code that has no app context, such as a `Drop`.
pub(crate) type RetiredSessions = Arc<parking_lot::Mutex<Vec<u32>>>;

/// The sending half of a stream's queue, held by the session.
pub struct StreamSender {
    sender: UnboundedSender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
}

/// The reading half of a stream's queue.
pub struct StreamReceiver {
    receiver: UnboundedReceiver<Vec<u8>>,
    queued: Arc<AtomicUsize>,
}

enum StreamSendError {
    Full,
    Closed,
}

impl StreamSender {
    fn try_send(&self, bytes: Vec<u8>) -> Result<(), StreamSendError> {
        let length = bytes.len();
        if self.queued.load(Ordering::Acquire).saturating_add(length) > MAX_STREAM_QUEUE_BYTES {
            return Err(StreamSendError::Full);
        }
        self.queued.fetch_add(length, Ordering::AcqRel);
        if self.sender.unbounded_send(bytes).is_err() {
            self.queued.fetch_sub(length, Ordering::AcqRel);
            return Err(StreamSendError::Closed);
        }
        Ok(())
    }
}

impl Stream for StreamReceiver {
    type Item = Vec<u8>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let polled = Pin::new(&mut self.receiver).poll_next(cx);
        if let Poll::Ready(Some(bytes)) = &polled {
            self.queued.fetch_sub(bytes.len(), Ordering::AcqRel);
        }
        polled
    }
}

struct PendingRequest {
    sender: oneshot::Sender<Result<Control, SessionError>>,
    /// Dropped with the request, which is what cancels the timeout.
    _timeout: Task<()>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum SessionError {
    /// The connection to the relay has no room; try again shortly.
    #[error("the connection to the relay is full")]
    Backpressure,
    #[error("the session has ended: {0}")]
    Closed(String),
    #[error("the other Zode did not answer in time")]
    TimedOut,
    #[error("too many requests are waiting for an answer")]
    TooManyRequests,
    #[error("too many streams are open")]
    TooManyStreams,
    /// The other Zode answered with an error.
    #[error("{message}")]
    Remote { code: String, message: String },
    #[error("that cannot be sent: {0}")]
    Unsendable(String),
}

/// What the host said about itself in its `hello_ack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    pub app_version: String,
    pub relay_protocol: u32,
    pub rpc_protocol: u32,
    pub capabilities: Vec<String>,
}

impl HostFacts {
    pub fn can(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|name| name == capability)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelaySessionEvent {
    /// The agents or terminals the host listed have changed.
    Changed,
    /// A control message nobody was waiting for by request id.
    Control(Control),
    /// The session is over. Nothing more will be sent or received.
    Closed(String),
}

pub struct RelaySession {
    session_id: u32,
    host_device_id: String,
    host_name: String,
    host: HostFacts,
    channel: SecureChannel,
    writer: RelayWriter,
    agents: Vec<AgentSummary>,
    terminals: Vec<TerminalSummary>,
    streams: HashMap<u32, StreamSender>,
    pending: HashMap<u32, PendingRequest>,
    next_stream_id: u32,
    next_request_id: u32,
    closed: Option<String>,
    retired: RetiredSessions,
    /// Keeps the relay connection alive for as long as the session is used.
    _initiator: Entity<RelayInitiator>,
}

impl EventEmitter<RelaySessionEvent> for RelaySession {}

fn reply_request_id(control: &Control) -> Option<u32> {
    match control {
        Control::Error { request_id, .. } => *request_id,
        Control::IdeOpened { request_id, .. }
        | Control::FilesListReply { request_id, .. }
        | Control::FileReadReply { request_id, .. }
        | Control::DiffReply { request_id, .. } => Some(*request_id),
        _ => None,
    }
}

impl RelaySession {
    pub(crate) fn new(
        session_id: u32,
        host_device_id: String,
        host_name: String,
        host: HostFacts,
        channel: SecureChannel,
        writer: RelayWriter,
        retired: RetiredSessions,
        initiator: Entity<RelayInitiator>,
    ) -> Self {
        Self {
            session_id,
            host_device_id,
            host_name,
            host,
            channel,
            writer,
            agents: Vec::new(),
            terminals: Vec::new(),
            streams: HashMap::new(),
            pending: HashMap::new(),
            next_stream_id: 1,
            next_request_id: 1,
            closed: None,
            retired,
            _initiator: initiator,
        }
    }

    pub fn session_id(&self) -> u32 {
        self.session_id
    }

    pub fn host_device_id(&self) -> &str {
        &self.host_device_id
    }

    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    pub fn host(&self) -> &HostFacts {
        &self.host
    }

    pub fn agents(&self) -> &[AgentSummary] {
        &self.agents
    }

    pub fn terminals(&self) -> &[TerminalSummary] {
        &self.terminals
    }

    pub fn is_closed(&self) -> bool {
        self.closed.is_some()
    }

    pub fn closed_reason(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    fn ensure_open(&self) -> Result<(), SessionError> {
        match &self.closed {
            Some(reason) => Err(SessionError::Closed(reason.clone())),
            None => Ok(()),
        }
    }

    /// Hands out stream ids for streams this side opens. Zero is the control
    /// stream and is never used.
    pub fn allocate_stream_id(&mut self) -> u32 {
        let stream_id = self.next_stream_id;
        self.next_stream_id = self.next_stream_id.checked_add(1).unwrap_or(1);
        stream_id
    }

    /// A queue for one stream: give the sender to
    /// [`register_stream`](Self::register_stream) and read the receiver.
    pub fn stream_channel() -> (StreamSender, StreamReceiver) {
        let (sender, receiver) = mpsc::unbounded();
        let queued = Arc::new(AtomicUsize::new(0));
        (
            StreamSender {
                sender,
                queued: queued.clone(),
            },
            StreamReceiver { receiver, queued },
        )
    }

    /// Delivers the bytes that arrive on `stream_id` to `sender`. The stream
    /// ends, and the sender is dropped, on the host's end-of-stream frame, when
    /// the session closes, or when the reader falls
    /// [`MAX_STREAM_QUEUE_BYTES`] behind.
    pub fn register_stream(
        &mut self,
        stream_id: u32,
        sender: StreamSender,
    ) -> Result<(), SessionError> {
        self.ensure_open()?;
        if !self.streams.contains_key(&stream_id) && self.streams.len() >= MAX_OPEN_STREAMS {
            return Err(SessionError::TooManyStreams);
        }
        self.streams.insert(stream_id, sender);
        Ok(())
    }

    /// Stops delivering `stream_id` and drops its sender. Does nothing for a
    /// stream that is not registered.
    pub fn unregister_stream(&mut self, stream_id: u32) {
        self.streams.remove(&stream_id);
    }

    fn write_sealed(
        &mut self,
        frames: usize,
        seal: impl FnOnce(&mut SecureChannel) -> Vec<Result<Vec<u8>, String>>,
        cx: &mut Context<Self>,
    ) -> Result<(), SessionError> {
        self.ensure_open()?;
        // Checked before sealing: see the module note on the counter.
        if !self.writer.has_capacity_for(frames) {
            return Err(SessionError::Backpressure);
        }
        for sealed in seal(&mut self.channel) {
            let sealed = match sealed {
                Ok(sealed) => sealed,
                Err(error) => {
                    self.finish(format!("the channel failed: {error}"), cx);
                    return Err(SessionError::Unsendable(error));
                }
            };
            match self.writer.send_frame(self.session_id, &sealed) {
                Ok(()) => {}
                Err(SendError::Backpressure) => return Err(SessionError::Backpressure),
                Err(error) => {
                    self.finish(error.to_string(), cx);
                    return Err(SessionError::Closed(error.to_string()));
                }
            }
        }
        Ok(())
    }

    pub fn send_control(
        &mut self,
        control: &Control,
        cx: &mut Context<Self>,
    ) -> Result<(), SessionError> {
        self.write_sealed(
            1,
            |channel| {
                vec![
                    channel
                        .seal_control(control)
                        .map_err(|error| error.to_string()),
                ]
            },
            cx,
        )
    }

    /// Sends `bytes` on `stream_id`, cut into frames, or none of it: when the
    /// relay connection has no room for every frame, nothing is sealed and the
    /// answer is [`SessionError::Backpressure`]. `bytes` should be small; see
    /// [`send_data_waiting`] for a write of any size. Empty input sends
    /// nothing, because an empty frame would end the stream.
    pub fn try_send_data(
        &mut self,
        stream_id: u32,
        bytes: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<(), SessionError> {
        if stream_id == 0 {
            return Err(SessionError::Unsendable(
                "stream 0 is reserved for control messages".into(),
            ));
        }
        let chunks = bytes.chunks(MAX_INNER_PAYLOAD_LEN);
        let frames = chunks.len();
        if frames == 0 {
            return Ok(());
        }
        self.write_sealed(
            frames,
            |channel| {
                chunks
                    .map(|chunk| {
                        channel
                            .seal_data(stream_id, chunk.to_vec())
                            .map_err(|error| error.to_string())
                    })
                    .collect()
            },
            cx,
        )
    }

    pub fn send_end_of_stream(
        &mut self,
        stream_id: u32,
        cx: &mut Context<Self>,
    ) -> Result<(), SessionError> {
        self.write_sealed(
            1,
            |channel| {
                vec![
                    channel
                        .seal_end_of_stream(stream_id)
                        .map_err(|error| error.to_string()),
                ]
            },
            cx,
        )
    }

    /// Sends the control message `make` builds from a fresh request id, and
    /// answers with the reply that repeats it. An error reply from the host is
    /// [`SessionError::Remote`].
    pub fn request(
        &mut self,
        make: impl FnOnce(u32) -> Control,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<Control, SessionError>> {
        let (sender, receiver) = oneshot::channel();
        let outcome = (|| {
            self.ensure_open()?;
            if self.pending.len() >= MAX_PENDING_REQUESTS {
                return Err(SessionError::TooManyRequests);
            }
            let request_id = self.next_request_id;
            self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);
            self.send_control(&make(request_id), cx)?;
            Ok(request_id)
        })();
        match outcome {
            Ok(request_id) => {
                let timeout = cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(REQUEST_TIMEOUT).await;
                    this.update(cx, |this, _| {
                        if let Some(request) = this.pending.remove(&request_id)
                            && request.sender.send(Err(SessionError::TimedOut)).is_err()
                        {
                            log::debug!("nobody was waiting for request {request_id}");
                        }
                    })
                    .ok();
                });
                self.pending.insert(
                    request_id,
                    PendingRequest {
                        sender,
                        _timeout: timeout,
                    },
                );
            }
            Err(error) => {
                if sender.send(Err(error)).is_err() {
                    log::debug!("nobody was waiting for a refused request");
                }
            }
        }
        receiver
    }

    /// Ends the session from this side and tells the relay.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.is_closed() {
            return;
        }
        self.release_at_relay();
        self.finish("closed here".to_string(), cx);
    }

    /// Tells the relay this side is done with the session and asks the
    /// initiator to forget its route. Needs no app context, so a `Drop` can
    /// use it.
    fn release_at_relay(&self) {
        if let Err(error) = self.writer.close_session(self.session_id) {
            log::debug!("could not tell the relay to close session: {error}");
        }
        self.retired.lock().push(self.session_id);
    }

    /// The relay or the connection ended the session. Idempotent.
    pub(crate) fn finish(&mut self, reason: String, cx: &mut Context<Self>) {
        if self.closed.is_some() {
            return;
        }
        self.closed = Some(reason.clone());
        self.streams.clear();
        for (_, request) in self.pending.drain() {
            if request
                .sender
                .send(Err(SessionError::Closed(reason.clone())))
                .is_err()
            {
                log::debug!("nobody was waiting for a request that ended with the session");
            }
        }
        cx.emit(RelaySessionEvent::Closed(reason));
        cx.notify();
    }

    /// A binary frame arrived for this session.
    pub(crate) fn on_frame(&mut self, payload: &[u8], cx: &mut Context<Self>) {
        if self.is_closed() {
            return;
        }
        let frame = match self.channel.open(payload) {
            Ok(frame) => frame,
            Err(error) => {
                // A frame that does not authenticate ends the session for
                // good: there is no skipping it.
                log::warn!("ending a session: {error}");
                self.release_at_relay();
                self.finish(format!("a message failed to authenticate: {error}"), cx);
                return;
            }
        };
        match frame.kind {
            InnerKind::Control => self.on_control(frame, cx),
            InnerKind::Data => self.on_data(frame, cx),
        }
    }

    fn on_control(&mut self, frame: InnerFrame, cx: &mut Context<Self>) {
        let control = match decode_control(&frame.payload) {
            Ok(control) => control,
            Err(error) => {
                log::debug!("the host sent a control message that does not parse: {error}");
                return;
            }
        };
        if let Some(request_id) = reply_request_id(&control)
            && let Some(request) = self.pending.remove(&request_id)
        {
            let sender = request.sender;
            let answer = match control {
                Control::Error { code, message, .. } => Err(SessionError::Remote { code, message }),
                other => Ok(other),
            };
            if sender.send(answer).is_err() {
                log::debug!("nobody was waiting for request {request_id}");
            }
            return;
        }
        match &control {
            Control::Ping => {
                if let Err(error) = self.send_control(&Control::Pong, cx) {
                    log::debug!("could not answer a ping: {error}");
                }
                return;
            }
            Control::Pong => return,
            Control::AgentList { agents } => {
                self.agents = agents.iter().take(MAX_MIRRORED_ITEMS).cloned().collect();
                cx.emit(RelaySessionEvent::Changed);
            }
            Control::AgentUpdate { agent } => {
                if let Some(known) = self.agents.iter_mut().find(|known| known.id == agent.id) {
                    *known = agent.clone();
                } else if self.agents.len() < MAX_MIRRORED_ITEMS {
                    self.agents.push(agent.clone());
                }
                cx.emit(RelaySessionEvent::Changed);
            }
            Control::TerminalList { terminals } => {
                self.terminals = terminals.iter().take(MAX_MIRRORED_ITEMS).cloned().collect();
                cx.emit(RelaySessionEvent::Changed);
            }
            Control::Error { code, .. } if code == "version" => {
                cx.emit(RelaySessionEvent::Control(control.clone()));
                self.release_at_relay();
                self.finish("the other Zode runs a different version".to_string(), cx);
                return;
            }
            _ => {}
        }
        cx.emit(RelaySessionEvent::Control(control));
        cx.notify();
    }

    fn on_data(&mut self, frame: InnerFrame, cx: &mut Context<Self>) {
        if frame.payload.is_empty() {
            self.streams.remove(&frame.stream_id);
            return;
        }
        let Some(sender) = self.streams.get(&frame.stream_id) else {
            log::debug!("data on a stream nobody registered");
            return;
        };
        let stopped = match sender.try_send(frame.payload) {
            Ok(()) => return,
            Err(StreamSendError::Closed) => "nobody reads",
            Err(StreamSendError::Full) => {
                log::warn!(
                    "ending stream {}: its reader fell too far behind",
                    frame.stream_id
                );
                "is read too slowly"
            }
        };
        // Tell the host so it stops sending.
        self.streams.remove(&frame.stream_id);
        if let Err(error) = self.send_end_of_stream(frame.stream_id, cx) {
            log::debug!("could not end a stream that {stopped}: {error}");
        }
    }
}

impl Drop for RelaySession {
    fn drop(&mut self) {
        if self.closed.is_none() {
            self.release_at_relay();
        }
    }
}

/// Writes `bytes` of any size to `stream_id`, a frame at a time, waiting while
/// the relay connection has no room. Gives up with
/// [`SessionError::Backpressure`] after waiting [`WRITE_DEADLINE`] in all.
pub async fn send_data_waiting(
    session: &WeakEntity<RelaySession>,
    stream_id: u32,
    bytes: &[u8],
    cx: &mut AsyncApp,
) -> Result<(), SessionError> {
    let mut waited = Duration::ZERO;
    for chunk in bytes.chunks(MAX_INNER_PAYLOAD_LEN) {
        loop {
            let outcome = session
                .update(cx, |session, cx| {
                    session.try_send_data(stream_id, chunk, cx)
                })
                .map_err(|_| SessionError::Closed("the session was dropped".into()))?;
            match outcome {
                Ok(()) => break,
                Err(SessionError::Backpressure) => {
                    waited += WRITE_RETRY;
                    if waited >= WRITE_DEADLINE {
                        return Err(SessionError::Backpressure);
                    }
                    cx.background_executor().timer(WRITE_RETRY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}
