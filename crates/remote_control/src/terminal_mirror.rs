//! Mirrors terminals to the devices attached to them.
//!
//! A terminal's raw pty output is tapped on the reader thread, numbered, and
//! queued once per attached session. A newly attached session starts from a
//! snapshot of the screen taken under the same lock the tap runs under, so the
//! snapshot's sequence number says exactly which queued bytes it already
//! contains. Nothing is ever cut to fit: a queue that outgrows its limit is
//! thrown away whole and replaced by a fresh snapshot, which is also how a
//! resize and a tap that had to drop a chunk are answered.
//!
//! Finding terminals is always on and costs a weak handle each. Following them
//! -- subscribing to their events, and above all tapping them -- happens only
//! while remote control is on and a device is attached, so a terminal nobody
//! is watching is untouched.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use gpui::{App, Context, Entity, EntityId, EventEmitter, Subscription, Task, WeakEntity};
use remote_relay_protocol::{MAX_INNER_PAYLOAD_LEN, TerminalSummary};
use terminal::{RemoteOutputChunk, RemoteTap, Terminal};
use util::ResultExt as _;

use crate::agent_mirror::terminal_id;

/// How much live output may wait for one slow session before it is given a
/// fresh snapshot instead.
pub const SESSION_QUEUE_LIMIT: usize = 1024 * 1024;

/// The least time between two snapshots for one session of one terminal. A
/// flood of output keeps overflowing the queue, and without this every
/// overflow would render the whole screen again under the emulator lock.
pub(crate) const MIN_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);

/// How long after a snapshot taken while output was flowing the screen is sent
/// once more. The tap sees bytes before the emulator parses them, so such a
/// snapshot can stand for output the screen does not show yet; by now it does.
pub(crate) const FOLLOW_UP_SNAPSHOT_DELAY: Duration = Duration::from_millis(400);

/// Most chunks taken from the tap in one go, so one burst cannot hold the
/// foreground for long.
const DRAIN_BATCH: usize = 256;

/// What waits to be sent to one session for one terminal.
#[derive(Default)]
pub(crate) struct OutputQueue {
    /// The not-yet-sent frames of a snapshot, which go out before anything
    /// else.
    snapshot: VecDeque<Vec<u8>>,
    chunks: VecDeque<(u64, Vec<u8>)>,
    chunk_bytes: usize,
    needs_snapshot: bool,
    /// The sequence of the newest snapshot installed. Output at or below it is
    /// drawn in that snapshot, wherever it turns up.
    snapshot_sequence: u64,
}

impl OutputQueue {
    pub(crate) fn awaiting_snapshot() -> Self {
        Self {
            needs_snapshot: true,
            ..Self::default()
        }
    }

    pub(crate) fn needs_snapshot(&self) -> bool {
        self.needs_snapshot
    }

    pub(crate) fn has_output(&self) -> bool {
        self.needs_snapshot || !self.snapshot.is_empty() || !self.chunks.is_empty()
    }

    /// Queues live output. Past the limit the queue is dropped and a snapshot
    /// requested instead; a queue that is already waiting for one drops the
    /// bytes, because the snapshot will be taken later than they were.
    pub(crate) fn push(&mut self, sequence: u64, bytes: Vec<u8>) {
        // The snapshot is taken lazily and counts reads the foreground has not
        // been handed yet, so those arrive after it and are already drawn.
        if self.needs_snapshot || sequence <= self.snapshot_sequence {
            return;
        }
        if self.chunk_bytes + bytes.len() > SESSION_QUEUE_LIMIT {
            self.invalidate();
            return;
        }
        self.chunk_bytes += bytes.len();
        self.chunks.push_back((sequence, bytes));
    }

    /// Throws away everything queued and asks for a fresh snapshot.
    pub(crate) fn invalidate(&mut self) {
        self.snapshot.clear();
        self.chunks.clear();
        self.chunk_bytes = 0;
        self.needs_snapshot = true;
    }

    /// Replaces whatever was queued with a snapshot taken at `sequence`. Live
    /// output up to that sequence is already drawn in it.
    pub(crate) fn install_snapshot(&mut self, sequence: u64, bytes: &[u8]) {
        self.needs_snapshot = false;
        self.snapshot_sequence = sequence;
        self.snapshot = bytes
            .chunks(MAX_INNER_PAYLOAD_LEN)
            .map(<[u8]>::to_vec)
            .collect();
        while self
            .chunks
            .front()
            .is_some_and(|(chunk_sequence, _)| *chunk_sequence <= sequence)
        {
            self.chunks.pop_front();
        }
        self.chunk_bytes = self.chunks.iter().map(|(_, bytes)| bytes.len()).sum();
    }

    /// The next frame's worth of bytes: snapshot first, then live output
    /// joined up to one frame. `None` while a snapshot is still to be taken.
    pub(crate) fn pop(&mut self) -> Option<Vec<u8>> {
        if self.needs_snapshot {
            return None;
        }
        if let Some(frame) = self.snapshot.pop_front() {
            return Some(frame);
        }
        let (sequence, mut frame) = self.chunks.pop_front()?;
        let mut taken = frame.len();
        if frame.len() > MAX_INNER_PAYLOAD_LEN {
            // The rest keeps the sequence of the read it came from: a snapshot
            // either contains all of a read or none of it.
            let rest = frame.split_off(MAX_INNER_PAYLOAD_LEN);
            taken = frame.len();
            self.chunks.push_front((sequence, rest));
        }
        self.chunk_bytes = self.chunk_bytes.saturating_sub(taken);
        while let Some((_, next)) = self.chunks.front() {
            if frame.len() + next.len() > MAX_INNER_PAYLOAD_LEN {
                break;
            }
            let Some((_, next)) = self.chunks.pop_front() else {
                break;
            };
            self.chunk_bytes = self.chunk_bytes.saturating_sub(next.len());
            frame.extend_from_slice(&next);
        }
        Some(frame)
    }
}

struct Attachment {
    stream_id: u32,
    queue: OutputQueue,
    last_snapshot: Option<Instant>,
    /// The next snapshot is the follow-up to one taken while output flowed, and
    /// does not earn another.
    next_snapshot_is_follow_up: bool,
    /// Wakes the session at a later time: to retry a snapshot that was too
    /// soon, or to send the follow-up. One at a time.
    wake: Option<Task<()>>,
}

impl Attachment {
    fn new(stream_id: u32) -> Self {
        Self {
            stream_id,
            queue: OutputQueue::awaiting_snapshot(),
            last_snapshot: None,
            next_snapshot_is_follow_up: false,
            wake: None,
        }
    }

    /// How much longer a snapshot has to wait to keep clear of the last one.
    fn snapshot_wait(&self, now: Instant) -> Option<Duration> {
        let elapsed = now.saturating_duration_since(self.last_snapshot?);
        MIN_SNAPSHOT_INTERVAL
            .checked_sub(elapsed)
            .filter(|wait| !wait.is_zero())
    }
}

enum Wake {
    /// A snapshot was held back; ask again.
    Retry,
    /// Send the screen once more, now that the output has settled.
    FollowUp,
}

struct Mirrored {
    terminal: WeakEntity<Terminal>,
    id: String,
    attachments: HashMap<u32, Attachment>,
    last_dimensions: (u16, u16),
    last_sequence: u64,
    /// Drains the tap. Exists only while something is attached.
    tap_task: Option<Task<()>>,
    /// Present only while mirroring is on.
    following: Vec<Subscription>,
    _release: Subscription,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalMirrorEvent {
    /// A terminal appeared, went away or changed its title.
    ListChanged,
    /// A session has output waiting; ask for it with
    /// [`TerminalMirror::pop_output`].
    OutputReady { session_id: u32 },
    /// The terminal's size changed. Sessions attached to it are being resent a
    /// snapshot at the new size.
    Resized {
        terminal_id: String,
        columns: u16,
        rows: u16,
        sessions: Vec<u32>,
    },
    Closed {
        terminal_id: String,
        sessions: Vec<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachError {
    /// No terminal by that id, or one that is not a process.
    NotFound,
    /// Mirroring is off.
    Inactive,
}

pub struct TerminalMirror {
    terminals: HashMap<EntityId, Mirrored>,
    active: bool,
    /// Which terminal each session was last served from, so one busy terminal
    /// cannot starve another attached to the same session.
    last_served: HashMap<u32, EntityId>,
    _observe_new: Subscription,
}

impl EventEmitter<TerminalMirrorEvent> for TerminalMirror {}

impl TerminalMirror {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = cx.entity();
        let observe_new = cx.observe_new::<Terminal>(move |terminal, _window, cx| {
            if !terminal.is_pty_backed() {
                return;
            }
            let entity = cx.entity();
            this.update(cx, |this, cx| this.track(entity, cx));
        });
        Self {
            terminals: HashMap::default(),
            active: false,
            last_served: HashMap::default(),
            _observe_new: observe_new,
        }
    }

    fn track(&mut self, terminal: Entity<Terminal>, cx: &mut Context<Self>) {
        let id = terminal.entity_id();
        if self.terminals.contains_key(&id) {
            return;
        }
        let release = cx.observe_release(&terminal, move |this, _terminal, cx| {
            this.untrack(id, cx);
        });
        self.terminals.insert(
            id,
            Mirrored {
                terminal: terminal.downgrade(),
                id: terminal_id(id),
                attachments: HashMap::default(),
                last_dimensions: (0, 0),
                last_sequence: 0,
                tap_task: None,
                following: Vec::new(),
                _release: release,
            },
        );
        if self.active {
            // `observe_new` fires while the terminal is still being built, and
            // reading it then panics. Following it, and announcing it, wait
            // until the build is over.
            let weak = cx.weak_entity();
            let terminal = terminal.downgrade();
            cx.defer(move |cx| {
                weak.update(cx, |this, cx| {
                    if !this.active {
                        return;
                    }
                    if let Some(terminal) = terminal.upgrade() {
                        this.follow(id, &terminal, cx);
                        cx.emit(TerminalMirrorEvent::ListChanged);
                    }
                })
                .log_err();
            });
        }
    }

    fn untrack(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.remove(&id) else {
            return;
        };
        if !self.active {
            return;
        }
        let sessions: Vec<u32> = mirrored.attachments.keys().copied().collect();
        cx.emit(TerminalMirrorEvent::Closed {
            terminal_id: mirrored.id,
            sessions,
        });
        cx.emit(TerminalMirrorEvent::ListChanged);
    }

    /// Starts following terminals, existing and future. Idempotent.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        if self.active {
            return;
        }
        self.active = true;
        let known: Vec<(EntityId, WeakEntity<Terminal>)> = self
            .terminals
            .iter()
            .map(|(id, mirrored)| (*id, mirrored.terminal.clone()))
            .collect();
        for (id, terminal) in known {
            if let Some(terminal) = terminal.upgrade() {
                self.follow(id, &terminal, cx);
            }
        }
    }

    /// Stops following and puts every terminal back as it was: no tap, no
    /// attachments, no subscriptions.
    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.active = false;
        self.last_served.clear();
        for mirrored in self.terminals.values_mut() {
            mirrored.following.clear();
            mirrored.attachments.clear();
            mirrored.tap_task = None;
            mirrored.last_sequence = 0;
            if let Some(terminal) = mirrored.terminal.upgrade() {
                terminal.update(cx, |terminal, _| terminal.detach_remote_tap());
            }
        }
    }

    fn follow(&mut self, id: EntityId, terminal: &Entity<Terminal>, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        if !mirrored.following.is_empty() {
            return;
        }
        mirrored.last_dimensions = terminal.read(cx).remote_dimensions();
        mirrored.following = vec![cx.subscribe(
            terminal,
            move |this, _terminal, event: &terminal::Event, cx| match event {
                terminal::Event::Wakeup => this.check_size(id, cx),
                terminal::Event::TitleChanged | terminal::Event::BreadcrumbsChanged => {
                    cx.emit(TerminalMirrorEvent::ListChanged);
                }
                _ => {}
            },
        )];
    }

    fn check_size(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        if mirrored.attachments.is_empty() {
            return;
        }
        let Some(terminal) = mirrored.terminal.upgrade() else {
            return;
        };
        let (columns, rows) = terminal.read(cx).remote_dimensions();
        if (columns, rows) == mirrored.last_dimensions {
            return;
        }
        mirrored.last_dimensions = (columns, rows);
        let sessions: Vec<u32> = mirrored.attachments.keys().copied().collect();
        for attachment in mirrored.attachments.values_mut() {
            attachment.queue.invalidate();
        }
        let terminal_id = mirrored.id.clone();
        // The host's size wins: the client is told, and resent a snapshot at
        // the new size, and never asked what size it would prefer.
        cx.emit(TerminalMirrorEvent::Resized {
            terminal_id,
            columns,
            rows,
            sessions: sessions.clone(),
        });
        for session_id in sessions {
            cx.emit(TerminalMirrorEvent::OutputReady { session_id });
        }
    }

    fn find(&self, terminal_id: &str) -> Option<EntityId> {
        self.terminals
            .iter()
            .find(|(_, mirrored)| mirrored.id == terminal_id)
            .map(|(id, _)| *id)
    }

    /// Every terminal, in a stable order.
    pub fn summaries(&self, cx: &App) -> Vec<TerminalSummary> {
        let mut listed: Vec<(EntityId, TerminalSummary)> = self
            .terminals
            .iter()
            .filter_map(|(id, mirrored)| {
                let terminal = mirrored.terminal.upgrade()?;
                let terminal = terminal.read(cx);
                let (columns, rows) = terminal.remote_dimensions();
                Some((
                    *id,
                    TerminalSummary {
                        id: mirrored.id.clone(),
                        title: terminal.title(false),
                        columns,
                        rows,
                    },
                ))
            })
            .collect();
        listed.sort_by_key(|(id, _)| id.as_u64());
        listed.into_iter().map(|(_, summary)| summary).collect()
    }

    /// Attaches `session_id` to a terminal, tapping it if nobody was attached
    /// yet. Returns the terminal's size; the screen itself follows through
    /// [`Self::pop_output`], so the first thing sent after the reply is its
    /// snapshot.
    pub fn attach(
        &mut self,
        session_id: u32,
        terminal_id: &str,
        stream_id: u32,
        cx: &mut Context<Self>,
    ) -> Result<(u16, u16), AttachError> {
        if !self.active {
            return Err(AttachError::Inactive);
        }
        let id = self.find(terminal_id).ok_or(AttachError::NotFound)?;
        let Some(mirrored) = self.terminals.get(&id) else {
            return Err(AttachError::NotFound);
        };
        let terminal = mirrored.terminal.upgrade().ok_or(AttachError::NotFound)?;

        if mirrored.tap_task.is_none() {
            let tap = terminal
                .update(cx, |terminal, _| terminal.attach_remote_tap())
                .ok_or(AttachError::NotFound)?;
            let task = Self::drain(id, tap, cx);
            if let Some(mirrored) = self.terminals.get_mut(&id) {
                mirrored.tap_task = Some(task);
                mirrored.last_sequence = 0;
            }
        }
        let dimensions = terminal.read(cx).remote_dimensions();
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return Err(AttachError::NotFound);
        };
        mirrored.last_dimensions = dimensions;
        mirrored
            .attachments
            .insert(session_id, Attachment::new(stream_id));
        cx.emit(TerminalMirrorEvent::OutputReady { session_id });
        Ok(dimensions)
    }

    fn drain(id: EntityId, tap: RemoteTap, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            while let Ok(first) = tap.recv().await {
                let mut chunks = vec![first];
                while chunks.len() < DRAIN_BATCH {
                    match tap.try_recv() {
                        Ok(chunk) => chunks.push(chunk),
                        Err(_) => break,
                    }
                }
                let dropped = tap.take_overflowed();
                if this
                    .update(cx, |this, cx| this.on_chunks(id, chunks, dropped, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    fn on_chunks(
        &mut self,
        id: EntityId,
        chunks: Vec<RemoteOutputChunk>,
        dropped: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        let Some(first) = chunks.first() else {
            return;
        };
        // Sequence numbers only skip where the tap dropped a chunk, so a jump
        // is as good a sign of loss as the flag, and survives the flag being
        // read between two drains.
        let gap = first.sequence != mirrored.last_sequence + 1;
        if let Some(last) = chunks.last() {
            mirrored.last_sequence = last.sequence;
        }
        let lost = dropped || gap;
        for attachment in mirrored.attachments.values_mut() {
            if lost {
                attachment.queue.invalidate();
                continue;
            }
            for chunk in &chunks {
                attachment.queue.push(chunk.sequence, chunk.bytes.clone());
            }
        }
        let ready: Vec<u32> = mirrored
            .attachments
            .iter()
            .filter(|(_, attachment)| attachment.queue.has_output())
            .map(|(session_id, _)| *session_id)
            .collect();
        for session_id in ready {
            cx.emit(TerminalMirrorEvent::OutputReady { session_id });
        }
    }

    pub fn detach(&mut self, session_id: u32, terminal_id: &str, cx: &mut Context<Self>) {
        let Some(id) = self.find(terminal_id) else {
            return;
        };
        self.detach_from(id, session_id, cx);
    }

    pub fn detach_session(&mut self, session_id: u32, cx: &mut Context<Self>) {
        let attached: Vec<EntityId> = self
            .terminals
            .iter()
            .filter(|(_, mirrored)| mirrored.attachments.contains_key(&session_id))
            .map(|(id, _)| *id)
            .collect();
        for id in attached {
            self.detach_from(id, session_id, cx);
        }
        self.last_served.remove(&session_id);
    }

    fn detach_from(&mut self, id: EntityId, session_id: u32, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        mirrored.attachments.remove(&session_id);
        if !mirrored.attachments.is_empty() {
            return;
        }
        // Nobody left: the terminal goes back to costing nothing.
        mirrored.tap_task = None;
        mirrored.last_sequence = 0;
        if let Some(terminal) = mirrored.terminal.upgrade() {
            terminal.update(cx, |terminal, _| terminal.detach_remote_tap());
        }
    }

    /// The terminal ids `session_id` is attached to.
    pub fn attached_terminals(&self, session_id: u32) -> Vec<String> {
        self.terminals
            .values()
            .filter(|mirrored| mirrored.attachments.contains_key(&session_id))
            .map(|mirrored| mirrored.id.clone())
            .collect()
    }

    /// Types into a terminal on behalf of a session. `false` when there is no
    /// such terminal or the session is not attached to it: a device types only
    /// into what it was shown.
    pub fn input(
        &mut self,
        session_id: u32,
        terminal_id: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(id) = self.find(terminal_id) else {
            return false;
        };
        let Some(terminal) = self
            .terminals
            .get(&id)
            .filter(|mirrored| mirrored.attachments.contains_key(&session_id))
            .and_then(|mirrored| mirrored.terminal.upgrade())
        else {
            return false;
        };
        terminal.update(cx, |terminal, _| terminal.input(bytes));
        true
    }

    /// The next frame of output for `session_id` and the stream it belongs on.
    pub fn pop_output(
        &mut self,
        session_id: u32,
        cx: &mut Context<Self>,
    ) -> Option<(u32, Vec<u8>)> {
        let mut candidates: Vec<EntityId> = self
            .terminals
            .iter()
            .filter(|(_, mirrored)| mirrored.attachments.contains_key(&session_id))
            .map(|(id, _)| *id)
            .collect();
        candidates.sort_by_key(|id| id.as_u64());
        if let Some(served) = self.last_served.get(&session_id)
            && let Some(position) = candidates.iter().position(|id| id == served)
        {
            let length = candidates.len();
            candidates.rotate_left((position + 1) % length);
        }
        for id in candidates {
            let now = cx.background_executor().now();
            let Some(mirrored) = self.terminals.get_mut(&id) else {
                continue;
            };
            let last_sequence = mirrored.last_sequence;
            let terminal = mirrored.terminal.upgrade();
            let Some(attachment) = mirrored.attachments.get_mut(&session_id) else {
                continue;
            };
            let mut wake = None;
            if attachment.queue.needs_snapshot() {
                if let Some(wait) = attachment.snapshot_wait(now) {
                    wake = Some((wait, Wake::Retry));
                } else if let Some(terminal) = terminal {
                    let snapshot = terminal.read(cx).remote_snapshot();
                    attachment
                        .queue
                        .install_snapshot(snapshot.sequence, &snapshot.bytes);
                    attachment.last_snapshot = Some(now);
                    // Output the foreground has not been handed yet means the
                    // child was writing as the screen was read.
                    let output_was_flowing = snapshot.sequence > last_sequence;
                    if output_was_flowing && !attachment.next_snapshot_is_follow_up {
                        wake = Some((FOLLOW_UP_SNAPSHOT_DELAY, Wake::FollowUp));
                    }
                    attachment.next_snapshot_is_follow_up = false;
                }
            }
            let popped = if attachment.queue.needs_snapshot() {
                None
            } else {
                attachment.queue.pop()
            };
            let stream_id = attachment.stream_id;
            if let Some((delay, kind)) = wake {
                self.schedule_wake(id, session_id, delay, kind, cx);
            }
            if let Some(bytes) = popped {
                self.last_served.insert(session_id, id);
                return Some((stream_id, bytes));
            }
        }
        None
    }

    fn schedule_wake(
        &mut self,
        id: EntityId,
        session_id: u32,
        delay: Duration,
        kind: Wake,
        cx: &mut Context<Self>,
    ) {
        let Some(attachment) = self
            .terminals
            .get_mut(&id)
            .and_then(|mirrored| mirrored.attachments.get_mut(&session_id))
        else {
            return;
        };
        if attachment.wake.is_some() {
            return;
        }
        attachment.wake = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| this.wake(id, session_id, kind, cx))
                .log_err();
        }));
    }

    fn wake(&mut self, id: EntityId, session_id: u32, kind: Wake, cx: &mut Context<Self>) {
        let Some(attachment) = self
            .terminals
            .get_mut(&id)
            .and_then(|mirrored| mirrored.attachments.get_mut(&session_id))
        else {
            return;
        };
        attachment.wake = None;
        if matches!(kind, Wake::FollowUp) {
            attachment.queue.invalidate();
            attachment.next_snapshot_is_follow_up = true;
        }
        cx.emit(TerminalMirrorEvent::OutputReady { session_id });
    }

    /// Whether any terminal still has a tap. For tests and for the host's own
    /// check that switching off left nothing behind.
    pub fn any_tapped(&self, cx: &App) -> bool {
        self.terminals.values().any(|mirrored| {
            mirrored
                .terminal
                .upgrade()
                .is_some_and(|terminal| terminal.read(cx).has_remote_tap())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(queue: &mut OutputQueue) -> Vec<Vec<u8>> {
        std::iter::from_fn(|| queue.pop()).collect()
    }

    #[test]
    fn a_new_queue_gives_nothing_until_it_has_a_snapshot() {
        let mut queue = OutputQueue::awaiting_snapshot();
        assert!(queue.has_output());
        assert_eq!(queue.pop(), None);
        queue.install_snapshot(0, b"screen");
        assert_eq!(drain(&mut queue), vec![b"screen".to_vec()]);
        assert!(!queue.has_output());
    }

    #[test]
    fn output_that_arrives_while_waiting_for_a_snapshot_is_not_queued_twice() {
        let mut queue = OutputQueue::awaiting_snapshot();
        queue.push(1, b"already on screen".to_vec());
        queue.install_snapshot(1, b"screen");
        assert_eq!(drain(&mut queue), vec![b"screen".to_vec()]);
    }

    #[test]
    fn output_the_snapshot_already_drew_is_ignored_when_it_arrives_late() {
        // The snapshot is taken lazily, so reads it already contains can still
        // be on their way from the tap when it is installed.
        let mut queue = OutputQueue::awaiting_snapshot();
        queue.install_snapshot(5, b"screen");
        queue.push(4, b"four".to_vec());
        queue.push(5, b"five".to_vec());
        queue.push(6, b"six".to_vec());
        assert_eq!(
            drain(&mut queue),
            vec![b"screen".to_vec(), b"six".to_vec()],
            "reads 4 and 5 are in the snapshot and must not be sent again"
        );
        assert_eq!(queue.chunk_bytes, 0);
    }

    #[test]
    fn a_later_snapshot_moves_the_line_forward() {
        let mut queue = OutputQueue::default();
        queue.install_snapshot(5, b"one");
        queue.push(6, b"six".to_vec());
        queue.invalidate();
        queue.install_snapshot(9, b"two");
        queue.push(8, b"eight".to_vec());
        queue.push(10, b"ten".to_vec());
        assert_eq!(drain(&mut queue), vec![b"two".to_vec(), b"ten".to_vec()]);
    }

    #[test]
    fn only_output_after_the_snapshot_follows_it() {
        let mut queue = OutputQueue::default();
        queue.push(1, b"one".to_vec());
        queue.push(2, b"two".to_vec());
        queue.push(3, b"three".to_vec());
        queue.install_snapshot(2, b"screen");
        assert_eq!(
            drain(&mut queue),
            vec![b"screen".to_vec(), b"three".to_vec()],
            "chunks 1 and 2 are drawn in the snapshot and must not be replayed"
        );
    }

    #[test]
    fn small_chunks_are_joined_and_nothing_is_reordered() {
        let mut queue = OutputQueue::default();
        for (sequence, text) in [(1, "a"), (2, "b"), (3, "c")] {
            queue.push(sequence, text.as_bytes().to_vec());
        }
        assert_eq!(drain(&mut queue), vec![b"abc".to_vec()]);
    }

    #[test]
    fn nothing_exceeds_one_frame() {
        let mut queue = OutputQueue::default();
        let big: Vec<u8> = (0..MAX_INNER_PAYLOAD_LEN * 2 + 10)
            .map(|index| (index % 251) as u8)
            .collect();
        queue.push(1, big.clone());
        queue.push(2, b"tail".to_vec());
        let frames = drain(&mut queue);
        assert!(
            frames
                .iter()
                .all(|frame| frame.len() <= MAX_INNER_PAYLOAD_LEN)
        );
        let mut expected = big;
        expected.extend_from_slice(b"tail");
        assert_eq!(
            frames.concat(),
            expected,
            "a split must lose and reorder nothing"
        );
    }

    #[test]
    fn a_snapshot_larger_than_a_frame_is_cut_into_frames_in_order() {
        let mut queue = OutputQueue::default();
        let snapshot: Vec<u8> = (0..MAX_INNER_PAYLOAD_LEN + 5)
            .map(|index| (index % 7) as u8)
            .collect();
        queue.install_snapshot(0, &snapshot);
        let frames = drain(&mut queue);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames.concat(), snapshot);
    }

    #[test]
    fn past_the_limit_the_queue_is_dropped_and_a_snapshot_requested() {
        let mut queue = OutputQueue::default();
        let chunk = vec![b'x'; SESSION_QUEUE_LIMIT / 2];
        queue.push(1, chunk.clone());
        queue.push(2, chunk);
        assert!(!queue.needs_snapshot(), "exactly at the limit still fits");
        queue.push(3, b"one byte too many".to_vec());
        assert!(queue.needs_snapshot());
        assert_eq!(queue.pop(), None, "no stale bytes survive the overflow");

        queue.install_snapshot(3, b"fresh screen");
        queue.push(4, b"after".to_vec());
        assert_eq!(
            drain(&mut queue),
            vec![b"fresh screen".to_vec(), b"after".to_vec()]
        );
    }

    #[test]
    fn invalidating_drops_a_snapshot_still_being_sent() {
        let mut queue = OutputQueue::default();
        queue.install_snapshot(0, &vec![1u8; MAX_INNER_PAYLOAD_LEN * 3]);
        assert!(queue.pop().is_some());
        queue.invalidate();
        assert_eq!(queue.pop(), None);
        assert!(queue.needs_snapshot());
    }

    #[test]
    fn the_byte_accounting_returns_to_zero() {
        let mut queue = OutputQueue::default();
        for sequence in 1..=50 {
            queue.push(sequence, vec![7; 10_000]);
        }
        drain(&mut queue);
        assert_eq!(queue.chunk_bytes, 0);
        assert!(!queue.has_output());
    }

    mod with_terminals {
        use super::super::*;
        use crate::remote_control_tests::shell_terminal;
        use gpui::{AppContext as _, TestAppContext};
        use std::{cell::RefCell, rc::Rc, time::Duration};

        struct Recorded {
            events: Rc<RefCell<Vec<TerminalMirrorEvent>>>,
            _subscription: Subscription,
        }

        fn mirror(cx: &mut TestAppContext) -> (Entity<TerminalMirror>, Recorded) {
            cx.executor().allow_parking();
            cx.update(|cx| {
                let store = settings::SettingsStore::test(cx);
                cx.set_global(store);
                theme_settings::init(theme::LoadThemes::JustBase, cx);
            });
            let mirror = cx.new(TerminalMirror::new);
            let events = Rc::new(RefCell::new(Vec::new()));
            let subscription = cx.update(|cx| {
                let events = events.clone();
                cx.subscribe(&mirror, move |_, event: &TerminalMirrorEvent, _| {
                    events.borrow_mut().push(event.clone());
                })
            });
            (
                mirror,
                Recorded {
                    events,
                    _subscription: subscription,
                },
            )
        }

        async fn wait_for(terminal: &Entity<Terminal>, needle: &str, cx: &mut TestAppContext) {
            for _ in 0..500 {
                if terminal
                    .update(cx, |terminal, _| terminal.get_content())
                    .contains(needle)
                {
                    return;
                }
                cx.background_executor
                    .timer(Duration::from_millis(10))
                    .await;
                cx.run_until_parked();
            }
            panic!("timed out waiting for {needle:?}");
        }

        fn only_terminal_id(mirror: &Entity<TerminalMirror>, cx: &mut TestAppContext) -> String {
            let summaries = mirror.read_with(cx, |mirror, cx| mirror.summaries(cx));
            assert_eq!(summaries.len(), 1, "{summaries:?}");
            summaries[0].id.clone()
        }

        fn pop_all(
            mirror: &Entity<TerminalMirror>,
            session_id: u32,
            cx: &mut TestAppContext,
        ) -> Vec<Vec<u8>> {
            let mut frames = Vec::new();
            while let Some((_, bytes)) =
                mirror.update(cx, |mirror, cx| mirror.pop_output(session_id, cx))
            {
                frames.push(bytes);
            }
            frames
        }

        #[gpui::test]
        async fn attaching_taps_a_terminal_and_the_last_detach_untaps_it(cx: &mut TestAppContext) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));

            let (columns, rows) = mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert!(columns > 0 && rows > 0);
            mirror
                .update(cx, |mirror, cx| mirror.attach(2, &id, 6, cx))
                .expect("a second device attaches to the same terminal");
            assert!(terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
            assert_eq!(
                mirror.read_with(cx, |mirror, _| mirror.attached_terminals(1)),
                vec![id.clone()]
            );

            mirror.update(cx, |mirror, cx| mirror.detach(1, &id, cx));
            assert!(
                terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()),
                "one device is still watching"
            );
            mirror.update(cx, |mirror, cx| mirror.detach_session(2, cx));
            assert!(!terminal.read_with(cx, |terminal, _| terminal.has_remote_tap()));
            assert!(!mirror.read_with(cx, |mirror, cx| mirror.any_tapped(cx)));
        }

        #[gpui::test]
        async fn attaching_something_that_is_not_there_or_while_asleep_is_refused(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            let _terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            assert_eq!(
                mirror.update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx)),
                Err(AttachError::Inactive),
                "a mirror nobody switched on taps nothing"
            );
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            assert_eq!(
                mirror.update(cx, |mirror, cx| mirror.attach(1, "terminal-0", 5, cx)),
                Err(AttachError::NotFound)
            );
        }

        #[gpui::test]
        async fn a_snapshot_comes_first_and_live_output_follows_without_repeating_it(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal =
                shell_terminal("printf 'AAA\\n'; read line; printf 'BBB\\n'; sleep 30", cx).await;
            wait_for(&terminal, "AAA", cx).await;
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");

            let first = pop_all(&mirror, 1, cx).concat();
            assert!(first.starts_with(b"\x1bc"));
            assert!(String::from_utf8_lossy(&first).contains("AAA"));

            assert!(mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"go\r".to_vec(), cx)));
            let mut live = Vec::new();
            for _ in 0..500 {
                cx.background_executor
                    .timer(Duration::from_millis(10))
                    .await;
                cx.run_until_parked();
                live.extend(pop_all(&mirror, 1, cx).concat());
                if String::from_utf8_lossy(&live).contains("BBB") {
                    break;
                }
            }
            let live = String::from_utf8_lossy(&live).into_owned();
            assert!(live.contains("BBB"), "{live:?}");
            assert!(
                !live.contains("AAA"),
                "output the snapshot already drew was sent again: {live:?}"
            );
            assert!(
                !live.contains('\x1b') || !live.starts_with("\x1bc"),
                "no second reset"
            );
        }

        const MID_STREAM_SCRIPT: &str = "i=0; while [ $i -lt 40 ]; do printf 'PRE%03d\\n' $i; i=$((i+1)); done; \
             read line; \
             i=0; while [ $i -lt 40 ]; do printf 'POST%03d\\n' $i; i=$((i+1)); done; \
             printf 'DONE\\n'; sleep 30";

        /// A terminal that has already printed, is attached to, and then prints
        /// a lot more while the task that drains the tap is not getting to run:
        /// the foreground is blocked on this thread, and the pty reader keeps
        /// numbering output into the tap.
        async fn attached_while_output_is_in_flight(
            cx: &mut TestAppContext,
        ) -> (Entity<TerminalMirror>, Recorded, Entity<Terminal>, String) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal(MID_STREAM_SCRIPT, cx).await;
            wait_for(&terminal, "PRE039", cx).await;
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert!(mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"go\r".to_vec(), cx)));
            let give_up = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                let screen = terminal.read_with(cx, |terminal, _| terminal.remote_snapshot());
                if String::from_utf8_lossy(&screen.bytes).contains("DONE") {
                    break;
                }
                assert!(
                    std::time::Instant::now() < give_up,
                    "the script never finished"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            (mirror, recorded, terminal, id)
        }

        fn marker_counts(bytes: &[u8]) -> Vec<(String, usize)> {
            let text = String::from_utf8_lossy(bytes);
            (0..40)
                .flat_map(|number| [format!("PRE{number:03}"), format!("POST{number:03}")])
                .map(|marker| {
                    let count = text.matches(marker.as_str()).count();
                    (marker, count)
                })
                .collect()
        }

        #[gpui::test]
        async fn attaching_in_the_middle_of_a_stream_shows_every_line_exactly_once(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded, _terminal, _id) = attached_while_output_is_in_flight(cx).await;

            let mut seen = pop_all(&mirror, 1, cx).concat();
            // Only now does the task that drains the tap get to hand over the
            // reads the snapshot already drew.
            cx.run_until_parked();
            seen.extend(pop_all(&mirror, 1, cx).concat());

            let wrong: Vec<_> = marker_counts(&seen)
                .into_iter()
                .filter(|(_, count)| *count != 1)
                .collect();
            assert!(
                wrong.is_empty(),
                "every line must arrive exactly once across the snapshot and what follows: {wrong:?}"
            );
        }

        #[gpui::test]
        async fn a_snapshot_taken_while_output_flows_is_followed_by_one_more_once_it_settles(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded, _terminal, _id) = attached_while_output_is_in_flight(cx).await;
            pop_all(&mirror, 1, cx);
            cx.run_until_parked();
            pop_all(&mirror, 1, cx);
            recorded.events.borrow_mut().clear();

            cx.executor()
                .advance_clock(FOLLOW_UP_SNAPSHOT_DELAY + Duration::from_millis(10));
            cx.run_until_parked();
            assert!(
                recorded.events.borrow().iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::OutputReady { session_id: 1 }
                )),
                "the host is told there is a screen to send"
            );
            let again = pop_all(&mirror, 1, cx).concat();
            assert!(again.starts_with(b"\x1bc"), "a full redraw, not a delta");
            let wrong: Vec<_> = marker_counts(&again)
                .into_iter()
                .filter(|(_, count)| *count != 1)
                .collect();
            assert!(wrong.is_empty(), "{wrong:?}");

            recorded.events.borrow_mut().clear();
            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(
                pop_all(&mirror, 1, cx).is_empty() && recorded.events.borrow().is_empty(),
                "the follow-up is not itself followed up"
            );
        }

        #[gpui::test]
        async fn a_terminal_that_was_quiet_gets_no_follow_up_snapshot(cx: &mut TestAppContext) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _terminal = shell_terminal("printf 'quiet\\n'; sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert!(!pop_all(&mirror, 1, cx).is_empty());
            recorded.events.borrow_mut().clear();

            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(recorded.events.borrow().is_empty());
            assert!(pop_all(&mirror, 1, cx).is_empty());
        }

        #[gpui::test]
        async fn snapshots_for_one_session_are_spaced_out_and_still_arrive(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert!(
                !pop_all(&mirror, 1, cx).is_empty(),
                "the first is immediate"
            );

            // An overflow right after: the queue wants a fresh snapshot.
            mirror.update(cx, |mirror, _| {
                for mirrored in mirror.terminals.values_mut() {
                    if let Some(attachment) = mirrored.attachments.get_mut(&1) {
                        attachment.queue.invalidate();
                    }
                }
            });
            assert!(
                pop_all(&mirror, 1, cx).is_empty(),
                "a second snapshot this soon would render the screen again under the lock"
            );
            recorded.events.borrow_mut().clear();

            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            assert!(
                recorded.events.borrow().iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::OutputReady { session_id: 1 }
                )),
                "the held-back snapshot is asked for again by itself"
            );
            assert!(pop_all(&mirror, 1, cx)[0].starts_with(b"\x1bc"));
        }

        #[gpui::test]
        async fn a_deferred_follow_that_runs_after_switching_off_subscribes_to_nothing(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            mirror.update(cx, |mirror, cx| mirror.deactivate(cx));
            // What the deferred closure of `track` does if switching off got
            // there first.
            mirror.update(cx, |mirror, cx| {
                mirror.follow(terminal.entity_id(), &terminal, cx)
            });
            assert!(
                mirror.read_with(cx, |mirror, _| mirror
                    .terminals
                    .values()
                    .all(|mirrored| mirrored.following.is_empty())),
                "a mirror that is off subscribes to nothing"
            );
        }

        #[gpui::test]
        async fn typing_reaches_the_terminal(cx: &mut TestAppContext) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            assert!(
                !mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"ls\r".to_vec(), cx)),
                "a device that is not attached types nothing"
            );
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert!(mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"ls\r".to_vec(), cx)));
            assert!(
                !mirror.update(cx, |mirror, cx| mirror.input(2, &id, b"no\r".to_vec(), cx)),
                "another session is not attached to it either"
            );
            assert_eq!(
                terminal.update(cx, |terminal, _| terminal.take_input_log()),
                vec![b"ls\r".to_vec()]
            );
            assert!(!mirror.update(cx, |mirror, cx| mirror.input(
                1,
                "terminal-0",
                b"x".to_vec(),
                cx
            )));
        }

        #[gpui::test]
        async fn a_flood_of_output_is_answered_with_a_fresh_snapshot_not_an_unbounded_queue(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal(
                "read line; head -c 4000000 /dev/zero | tr '\\000' 'x'; printf '\\nFLOODEND\\n'; sleep 30",
                cx,
            )
            .await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            // The first snapshot goes out, so what follows is live output
            // queueing behind a reader that has stopped reading.
            let initial = pop_all(&mirror, 1, cx);
            assert!(initial[0].starts_with(b"\x1bc"));
            assert!(mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"go\r".to_vec(), cx)));
            wait_for(&terminal, "FLOODEND", cx).await;
            // Snapshots are spaced out, so the replacement comes a moment later.
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();

            let frames = pop_all(&mirror, 1, cx);
            let total: usize = frames.iter().map(Vec::len).sum();
            assert!(
                frames[0].starts_with(b"\x1bc"),
                "a queue that overflowed is replaced by a snapshot"
            );
            assert!(
                total < 2 * SESSION_QUEUE_LIMIT,
                "{total} bytes were queued for a slow reader; the limit is {SESSION_QUEUE_LIMIT}"
            );
            assert!(String::from_utf8_lossy(&frames.concat()).contains("FLOODEND"));
            assert!(
                recorded.events.borrow().iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::OutputReady { session_id: 1 }
                )),
                "the host is told there is something to send"
            );
        }

        #[gpui::test]
        async fn a_new_size_is_announced_and_followed_by_a_fresh_snapshot(cx: &mut TestAppContext) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            pop_all(&mirror, 1, cx);
            recorded.events.borrow_mut().clear();

            let entity_id = terminal.entity_id();
            let window = cx.add_empty_window();
            terminal.update_in(window, |terminal, window, cx| {
                let bounds = terminal::TerminalBounds::new(
                    gpui::px(10.),
                    gpui::px(5.),
                    gpui::Bounds {
                        origin: gpui::Point::default(),
                        size: gpui::Size {
                            width: gpui::px(500.),
                            height: gpui::px(200.),
                        },
                    },
                );
                terminal.set_size(bounds);
                terminal.sync(window, cx);
            });
            mirror.update(cx, |mirror, cx| mirror.check_size(entity_id, cx));
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();

            let events = recorded.events.borrow().clone();
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::Resized { columns: 100, rows: 20, sessions, .. } if sessions == &vec![1]
                )),
                "{events:?}"
            );
            let frames = pop_all(&mirror, 1, cx);
            assert!(
                frames[0].starts_with(b"\x1bc"),
                "the screen is redrawn at the new size"
            );

            recorded.events.borrow_mut().clear();
            mirror.update(cx, |mirror, cx| mirror.check_size(entity_id, cx));
            assert!(
                recorded.events.borrow().is_empty(),
                "an unchanged size says nothing"
            );
        }

        #[gpui::test]
        async fn switching_off_removes_every_tap_and_attachment(cx: &mut TestAppContext) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let first = shell_terminal("sleep 30", cx).await;
            let second = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let ids: Vec<String> = mirror
                .read_with(cx, |mirror, cx| mirror.summaries(cx))
                .into_iter()
                .map(|summary| summary.id)
                .collect();
            assert_eq!(ids.len(), 2);
            for id in &ids {
                mirror
                    .update(cx, |mirror, cx| mirror.attach(1, id, 5, cx))
                    .expect("attaches");
            }
            assert!(first.read_with(cx, |terminal, _| terminal.has_remote_tap()));
            assert!(second.read_with(cx, |terminal, _| terminal.has_remote_tap()));

            mirror.update(cx, |mirror, cx| mirror.deactivate(cx));
            assert!(!first.read_with(cx, |terminal, _| terminal.has_remote_tap()));
            assert!(!second.read_with(cx, |terminal, _| terminal.has_remote_tap()));
            assert!(mirror.read_with(cx, |mirror, _| mirror.attached_terminals(1).is_empty()));
            assert_eq!(
                mirror.update(cx, |mirror, cx| mirror.pop_output(1, cx)),
                None
            );
        }

        #[gpui::test]
        async fn a_terminal_that_goes_away_is_announced_with_who_was_watching(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(3, &id, 5, cx))
                .expect("attaches");
            recorded.events.borrow_mut().clear();

            // The release observer is gpui's to fire; what is this module's is
            // what it does when it does.
            mirror.update(cx, |mirror, cx| mirror.untrack(terminal.entity_id(), cx));
            let events = recorded.events.borrow().clone();
            assert!(events.contains(&TerminalMirrorEvent::Closed {
                terminal_id: id,
                sessions: vec![3]
            }));
            assert!(events.contains(&TerminalMirrorEvent::ListChanged));
        }

        #[gpui::test]
        async fn terminals_that_are_not_processes_are_not_listed(cx: &mut TestAppContext) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _display_only = cx.new(|cx| {
                terminal::TerminalBuilder::new_display_only(
                    terminal::terminal_settings::CursorShape::default(),
                    terminal::terminal_settings::AlternateScroll::On,
                    None,
                    0,
                    cx.background_executor(),
                    util::paths::PathStyle::local(),
                )
                .expect("a display-only terminal")
                .subscribe(cx)
            });
            cx.run_until_parked();
            assert!(mirror.read_with(cx, |mirror, cx| mirror.summaries(cx).is_empty()));
        }
    }
}
