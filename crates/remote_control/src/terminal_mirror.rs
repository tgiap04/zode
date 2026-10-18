//! Mirrors terminals to the devices attached to them.
//!
//! A terminal's raw pty output is tapped on the reader thread, numbered, and
//! queued once per attached session. A newly attached session starts from a
//! snapshot of the screen taken under the same lock the tap runs under, so the
//! snapshot's sequence number says exactly which queued bytes it already
//! contains. Nothing is ever cut to fit: a queue that outgrows its limit is
//! thrown away whole and replaced by a fresh snapshot, which is also how a
//! resize, a tap that had to drop a chunk, and a program leaving the alternate
//! screen are answered. A snapshot that is still being sent when the live
//! output behind it overflows is sent whole, and the fresh one follows it:
//! cutting it off would leave a device that cannot keep up with a blank screen
//! for as long as the output lasts. Such a repair carries the screen only, not
//! the scrollback that makes up most of a snapshot, and the time between
//! repairs grows while the device keeps falling behind.
//!
//! A new size is announced once it has stopped changing, so dragging a pane
//! edge costs one announcement and one snapshot instead of one per step.
//!
//! A snapshot can be ahead of the screen: the tap sees bytes before the emulator
//! has drawn them, so one taken while output was flowing, or inside a
//! synchronized update, may stand for output the screen does not show yet. Such
//! a snapshot is taken once more when the output has been quiet for a while, or
//! after a cap if it never is, and waits for an open update to end first. That
//! repeat is never itself repeated.
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
use terminal::{REMOTE_SNAPSHOT_HISTORY_LINES, RemoteOutputChunk, RemoteTap, Terminal};
use util::ResultExt as _;

use crate::agent_mirror::terminal_id;

/// How much live output may wait for one slow session before it is given a
/// fresh snapshot instead.
pub const SESSION_QUEUE_LIMIT: usize = 1024 * 1024;

/// The least time between two snapshots for one session of one terminal. A
/// flood of output keeps overflowing the queue, and without this every
/// overflow would render the whole screen again under the emulator lock.
pub(crate) const MIN_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);

/// The longest the time between two repair snapshots of one session grows to
/// while it keeps falling behind. A repair is wanted when output was lost, which
/// a flood on a slow link does again and again, and each one costs the link a
/// whole screen.
pub(crate) const MAX_REPAIR_SPACING: Duration = Duration::from_secs(2);

/// How long a session has to go without losing output, counted from its last
/// repair, before the time between repairs starts over at
/// [`MIN_SNAPSHOT_INTERVAL`]. Longer than [`MAX_REPAIR_SPACING`], or a flood that
/// goes on would look like a link that had recovered.
pub(crate) const REPAIR_BACKOFF_RESET: Duration = Duration::from_secs(4);

/// How long a terminal's size has to stay as it is before devices are told.
/// Dragging a pane edge changes it at every step, and each announcement throws
/// away what is being sent and has the screen sent again.
pub(crate) const RESIZE_SETTLE: Duration = Duration::from_millis(120);

/// How long the output has to stay quiet before a snapshot that may have been
/// taken ahead of the screen is sent once more. The tap sees bytes before the
/// emulator parses them, so such a snapshot can stand for output the screen
/// does not show yet; once the output stops, it does. Every chunk that arrives
/// restarts the wait.
pub(crate) const SETTLE_QUIET: Duration = Duration::from_millis(400);

/// The most the quiet period can be stretched, counted from the snapshot, so a
/// program that never stops writing -- a spinner -- still gets its repair.
pub(crate) const SETTLE_MAX_WAIT: Duration = Duration::from_secs(2);

/// How long to wait before looking again when the repair is due but the
/// emulator is still inside a synchronized update, which holds output back until
/// it ends or times out.
pub(crate) const SETTLE_SYNC_RECHECK_INTERVAL: Duration = Duration::from_millis(50);

/// How many times to look again before taking the repair anyway. The wait is
/// bounded because a child that exits inside an update never ends it.
pub(crate) const SETTLE_SYNC_RECHECKS: u32 = 20;

/// Most chunks taken from the tap in one go, so one burst cannot hold the
/// foreground for long.
const DRAIN_BATCH: usize = 256;

/// How much of the terminal a snapshot carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) enum SnapshotKind {
    /// The screen only, for a device that lost output. Its scrollback is not
    /// worth what it costs to send again at the rate losses come.
    Repair,
    /// The screen and the scrollback behind it, as a device that has just
    /// attached or been given a new size needs it.
    #[default]
    Full,
}

/// What waits to be sent to one session for one terminal.
#[derive(Default)]
pub(crate) struct OutputQueue {
    /// The not-yet-sent frames of a snapshot, which go out before anything
    /// else.
    snapshot: VecDeque<Vec<u8>>,
    chunks: VecDeque<(u64, Vec<u8>)>,
    chunk_bytes: usize,
    needs_snapshot: bool,
    /// Live output was dropped while the frames of a snapshot were still
    /// being sent, so a fresh snapshot is wanted as soon as they are out.
    snapshot_after_current: bool,
    /// What the snapshot that is wanted has to carry.
    wanted_kind: SnapshotKind,
    /// Output was lost since this was last asked, which is what the time
    /// between repairs backs off on.
    loss_pending: bool,
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

    /// Whether a snapshot is to be taken, now or once the one being sent is
    /// out.
    pub(crate) fn wants_snapshot(&self) -> bool {
        self.needs_snapshot || self.snapshot_after_current
    }

    /// What the snapshot that is wanted has to carry.
    pub(crate) fn wanted_kind(&self) -> SnapshotKind {
        self.wanted_kind
    }

    /// Whether output was lost since this was last asked. Losses that come
    /// while a snapshot is already wanted are the same loss, and are not
    /// counted again.
    pub(crate) fn take_loss(&mut self) -> bool {
        std::mem::take(&mut self.loss_pending)
    }

    /// Whether [`Self::pop`] has something to give.
    pub(crate) fn is_sendable(&self) -> bool {
        !self.needs_snapshot && (!self.snapshot.is_empty() || !self.chunks.is_empty())
    }

    /// Whether live output with `sequence` would be queued. A queue that waits
    /// for a snapshot drops it, because the snapshot will be taken later than
    /// the output was, and so is the output the snapshot already drew.
    pub(crate) fn accepts(&self, sequence: u64) -> bool {
        !self.wants_snapshot() && sequence > self.snapshot_sequence
    }

    /// Queues live output. Past the limit the queue is dropped and a snapshot
    /// requested instead.
    pub(crate) fn push(&mut self, sequence: u64, bytes: Vec<u8>) {
        // The snapshot is taken lazily and counts reads the foreground has not
        // been handed yet, so those arrive after it and are already drawn.
        if !self.accepts(sequence) {
            return;
        }
        if self.chunk_bytes + bytes.len() > SESSION_QUEUE_LIMIT {
            self.invalidate_after_loss();
            return;
        }
        self.chunk_bytes += bytes.len();
        self.chunks.push_back((sequence, bytes));
    }

    /// Throws away the live output queued and asks for a fresh snapshot. The
    /// frames of a snapshot that are still to be sent stay: the device has
    /// started on that screen, and a half of it is worse than all of it. The
    /// fresh snapshot is taken once they are out.
    pub(crate) fn invalidate(&mut self) {
        self.want_snapshot(SnapshotKind::Full);
    }

    /// [`Self::invalidate`] for output that was lost, which only the screen
    /// has to be put right after.
    pub(crate) fn invalidate_after_loss(&mut self) {
        if !self.wants_snapshot() {
            self.loss_pending = true;
        }
        self.want_snapshot(SnapshotKind::Repair);
    }

    fn want_snapshot(&mut self, kind: SnapshotKind) {
        self.chunks.clear();
        self.chunk_bytes = 0;
        // A snapshot that was already wanted for another reason keeps what
        // that needed.
        self.wanted_kind = if self.wants_snapshot() {
            self.wanted_kind.max(kind)
        } else {
            kind
        };
        if self.snapshot.is_empty() {
            self.needs_snapshot = true;
        } else {
            self.snapshot_after_current = true;
        }
    }

    /// Throws away everything queued, a snapshot being sent included, and
    /// asks for a fresh one. For when what is queued describes a screen that
    /// no longer exists, as after a resize.
    pub(crate) fn restart(&mut self) {
        self.snapshot.clear();
        self.chunks.clear();
        self.chunk_bytes = 0;
        self.snapshot_after_current = false;
        self.wanted_kind = SnapshotKind::Full;
        self.needs_snapshot = true;
    }

    /// Replaces whatever was queued with a snapshot taken at `sequence`. Live
    /// output up to that sequence is already drawn in it.
    pub(crate) fn install_snapshot(&mut self, sequence: u64, bytes: &[u8]) {
        self.needs_snapshot = false;
        self.snapshot_after_current = false;
        self.wanted_kind = SnapshotKind::Full;
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
            if self.snapshot.is_empty() && self.snapshot_after_current {
                self.snapshot_after_current = false;
                self.needs_snapshot = true;
            }
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

/// A snapshot that may have been taken ahead of the screen, and so is to be
/// taken again once the output has settled.
struct Settle {
    armed_at: Instant,
    /// Pushed out by every chunk the tap delivers, so a burst keeps it waiting.
    quiet_until: Instant,
    /// Set once the wait is over but the emulator is still inside a
    /// synchronized update: when to look again.
    recheck_at: Option<Instant>,
    rechecks: u32,
}

impl Settle {
    fn armed(now: Instant) -> Self {
        Self {
            armed_at: now,
            quiet_until: now + SETTLE_QUIET,
            recheck_at: None,
            rechecks: 0,
        }
    }

    fn on_output(&mut self, now: Instant) {
        // Once the quiet period has been given up on, all that is left to wait
        // for is the update.
        if self.recheck_at.is_none() {
            self.quiet_until = now + SETTLE_QUIET;
        }
    }

    fn due_at(&self) -> Instant {
        self.recheck_at
            .unwrap_or_else(|| self.quiet_until.min(self.armed_at + SETTLE_MAX_WAIT))
    }

    /// Whether the snapshot is to be taken again now. `sync_pending` is asked
    /// only when it is due, and an update that is still open pushes it back a
    /// little, up to [`SETTLE_SYNC_RECHECKS`] times.
    fn ready(&mut self, now: Instant, sync_pending: impl FnOnce() -> bool) -> bool {
        if now < self.due_at() {
            return false;
        }
        if self.rechecks < SETTLE_SYNC_RECHECKS && sync_pending() {
            self.rechecks += 1;
            self.recheck_at = Some(now + SETTLE_SYNC_RECHECK_INTERVAL);
            return false;
        }
        true
    }
}

struct ScheduledWake {
    at: Instant,
    _task: Task<()>,
}

struct Attachment {
    stream_id: u32,
    queue: OutputQueue,
    last_snapshot: Option<Instant>,
    /// The least time between two repair snapshots at the moment, which grows
    /// while the session keeps losing output.
    repair_spacing: Duration,
    last_repair: Option<Instant>,
    /// Set when the snapshot just taken may have been ahead of the screen.
    settle: Option<Settle>,
    /// The next snapshot is the repeat for a settle, and does not earn another.
    next_snapshot_is_settle: bool,
    /// The snapshot last sent showed the alternate screen, which it carries
    /// without the primary screen behind it, so the device has none to go back
    /// to when the program leaves.
    primary_screen_missing: bool,
    /// Wakes the session at a later time: to retry a snapshot that was too
    /// soon, or to take the settle. One at a time, whichever is wanted first.
    wake: Option<ScheduledWake>,
}

impl Attachment {
    fn new(stream_id: u32) -> Self {
        Self {
            stream_id,
            queue: OutputQueue::awaiting_snapshot(),
            last_snapshot: None,
            repair_spacing: MIN_SNAPSHOT_INTERVAL,
            last_repair: None,
            settle: None,
            next_snapshot_is_settle: false,
            primary_screen_missing: false,
            wake: None,
        }
    }

    /// How much longer a snapshot has to wait to keep clear of the last one.
    fn snapshot_wait(&self, now: Instant) -> Option<Duration> {
        let spacing = match self.queue.wanted_kind() {
            SnapshotKind::Repair => self.repair_spacing,
            SnapshotKind::Full => MIN_SNAPSHOT_INTERVAL,
        };
        let elapsed = now.saturating_duration_since(self.last_snapshot?);
        spacing.checked_sub(elapsed).filter(|wait| !wait.is_zero())
    }

    /// Output was lost. A loss soon after the last repair means the session is
    /// still not keeping up, so the next repair waits twice as long, up to
    /// [`MAX_REPAIR_SPACING`]; after a good while without one it starts over.
    fn note_loss(&mut self, now: Instant) {
        let still_behind = self
            .last_repair
            .is_some_and(|at| now.saturating_duration_since(at) < REPAIR_BACKOFF_RESET);
        self.repair_spacing = if still_behind {
            (self.repair_spacing * 2).min(MAX_REPAIR_SPACING)
        } else {
            MIN_SNAPSHOT_INTERVAL
        };
    }

    /// Whether asking for output now would yield some: frames are queued, or a
    /// snapshot is wanted and may be taken.
    fn has_output_now(&self, now: Instant) -> bool {
        self.queue.is_sendable()
            || (self.queue.needs_snapshot() && self.snapshot_wait(now).is_none())
    }

    /// When this session next needs waking: to retry a snapshot that was held
    /// back, or to take the settle, whichever comes first.
    fn next_wake(&self, now: Instant) -> Option<Instant> {
        let retry = self
            .queue
            .needs_snapshot()
            .then(|| self.snapshot_wait(now))
            .flatten()
            .map(|wait| now + wait);
        let settle = self.settle.as_ref().map(Settle::due_at);
        retry.into_iter().chain(settle).min()
    }
}

struct PendingResize {
    dimensions: (u16, u16),
    _task: Task<()>,
}

struct Mirrored {
    terminal: WeakEntity<Terminal>,
    id: String,
    attachments: HashMap<u32, Attachment>,
    last_dimensions: (u16, u16),
    last_sequence: u64,
    /// When sessions that detached last had a snapshot, for as long as that
    /// still holds them back: a device that attaches again is owed no sooner a
    /// snapshot than one that stayed.
    recent_snapshots: HashMap<u32, Instant>,
    /// A size devices have not been told of, and the wait for it to stop
    /// changing.
    pending_resize: Option<PendingResize>,
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
    /// A session has output it can be given now; ask for it with
    /// [`TerminalMirror::pop_output`]. Not sent for one that is waiting out the
    /// time between two snapshots, which is announced when that is over.
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
        // Weak, because the subscription lives in the mirror and a strong
        // handle here would keep the mirror alive for good.
        let this = cx.weak_entity();
        let observe_new = cx.observe_new::<Terminal>(move |terminal, _window, cx| {
            if !terminal.is_pty_backed() {
                return;
            }
            let entity = cx.entity();
            this.update(cx, |this, cx| this.track(entity, cx)).log_err();
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
                recent_snapshots: HashMap::default(),
                pending_resize: None,
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
            mirrored.recent_snapshots.clear();
            mirrored.pending_resize = None;
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
                terminal::Event::Wakeup => this.on_wakeup(id, cx),
                // The breadcrumb is the title a program set with an escape
                // sequence, which no summary shows, so announcing it would
                // send every device the same list again.
                terminal::Event::TitleChanged => cx.emit(TerminalMirrorEvent::ListChanged),
                _ => {}
            },
        )];
    }

    fn on_wakeup(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.check_size(id, cx);
        self.check_alternate_screen(id, cx);
    }

    fn check_size(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get(&id) else {
            return;
        };
        if mirrored.attachments.is_empty() {
            return;
        }
        let Some(terminal) = mirrored.terminal.upgrade() else {
            return;
        };
        let dimensions = terminal.read(cx).remote_dimensions();
        self.note_size(id, dimensions, cx);
    }

    /// The terminal has `dimensions` now. If devices have not been told of
    /// that size they are, once it has stopped changing for [`RESIZE_SETTLE`].
    /// Hearing of the same size again does not restart the wait, or a terminal
    /// that keeps printing would never get its resize announced.
    fn note_size(&mut self, id: EntityId, dimensions: (u16, u16), cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        if dimensions == mirrored.last_dimensions {
            // Back at the size devices know, so there is nothing to tell.
            mirrored.pending_resize = None;
            return;
        }
        if mirrored
            .pending_resize
            .as_ref()
            .is_some_and(|pending| pending.dimensions == dimensions)
        {
            return;
        }
        mirrored.pending_resize = Some(PendingResize {
            dimensions,
            _task: cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RESIZE_SETTLE).await;
                this.update(cx, |this, cx| this.settle_size(id, cx))
                    .log_err();
            }),
        });
    }

    fn settle_size(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        mirrored.pending_resize = None;
        let Some(terminal) = mirrored.terminal.upgrade() else {
            return;
        };
        let dimensions = terminal.read(cx).remote_dimensions();
        if mirrored.attachments.is_empty() {
            mirrored.last_dimensions = dimensions;
        } else if dimensions != mirrored.last_dimensions {
            self.announce_resize(id, dimensions, cx);
        }
    }

    /// Tells every session attached to a terminal that it has a new size, and
    /// has each of them resent the screen at it.
    fn announce_resize(
        &mut self,
        id: EntityId,
        (columns, rows): (u16, u16),
        cx: &mut Context<Self>,
    ) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        mirrored.last_dimensions = (columns, rows);
        mirrored.pending_resize = None;
        let mut sessions: Vec<u32> = mirrored.attachments.keys().copied().collect();
        sessions.sort_unstable();
        for attachment in mirrored.attachments.values_mut() {
            attachment.queue.restart();
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
        self.announce(id, &sessions, cx);
    }

    /// A snapshot of the alternate screen carries no primary screen, so a
    /// device that was sent one has nothing to return to when the program
    /// leaves it. Sessions in that state are sent a fresh snapshot then.
    fn check_alternate_screen(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        if !mirrored
            .attachments
            .values()
            .any(|attachment| attachment.primary_screen_missing)
        {
            return;
        }
        let Some(terminal) = mirrored.terminal.upgrade() else {
            return;
        };
        if terminal.read(cx).remote_in_alternate_screen() {
            return;
        }
        let mut sessions = Vec::new();
        for (session_id, attachment) in &mut mirrored.attachments {
            if attachment.primary_screen_missing {
                attachment.queue.invalidate();
                sessions.push(*session_id);
            }
        }
        self.announce(id, &sessions, cx);
    }

    /// Tells the host about every one of `sessions` that has something to
    /// send now, and sets a wake for the rest to say so when they do. A session
    /// that is only waiting out the time between snapshots is not announced:
    /// asking would get nothing.
    fn announce(&mut self, id: EntityId, sessions: &[u32], cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some(mirrored) = self.terminals.get(&id) else {
            return;
        };
        let mut ready = Vec::new();
        let mut wakes = Vec::new();
        for session_id in sessions {
            let Some(attachment) = mirrored.attachments.get(session_id) else {
                continue;
            };
            if attachment.has_output_now(now) {
                ready.push(*session_id);
            }
            if let Some(at) = attachment.next_wake(now) {
                wakes.push((*session_id, at));
            }
        }
        for (session_id, at) in wakes {
            self.schedule_wake(id, session_id, at, cx);
        }
        for session_id in ready {
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

    /// The size of one terminal in columns and rows, without describing the
    /// others. `None` when there is no such terminal or it is gone.
    pub fn dimensions_of(&self, terminal_id: &str, cx: &App) -> Option<(u16, u16)> {
        let id = self.find(terminal_id)?;
        let terminal = self.terminals.get(&id)?.terminal.upgrade()?;
        Some(terminal.read(cx).remote_dimensions())
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
        // With nobody attached there is nobody who has not heard of this size.
        // Otherwise a size sessions have not been told of stays what they are
        // measured against, and is announced in due course to them all.
        if mirrored.attachments.is_empty() {
            mirrored.last_dimensions = dimensions;
        }
        let mut attachment = Attachment::new(stream_id);
        attachment.last_snapshot = mirrored
            .attachments
            .get(&session_id)
            .and_then(|replaced| replaced.last_snapshot)
            .or_else(|| mirrored.recent_snapshots.remove(&session_id));
        mirrored.attachments.insert(session_id, attachment);
        self.note_size(id, dimensions, cx);
        self.announce(id, &[session_id], cx);
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
        let now = cx.background_executor().now();
        let mut queues = Vec::with_capacity(mirrored.attachments.len());
        for attachment in mirrored.attachments.values_mut() {
            if let Some(settle) = attachment.settle.as_mut() {
                settle.on_output(now);
            }
            if lost {
                attachment.queue.invalidate_after_loss();
            } else {
                queues.push(&mut attachment.queue);
            }
        }
        // The last queue is handed the bytes themselves and the others a copy,
        // and only a queue that would keep them is given any: one waiting for
        // its snapshot drops them, so copying for it is wasted.
        if let Some((last, others)) = queues.split_last_mut() {
            for chunk in chunks {
                for queue in others.iter_mut() {
                    if queue.accepts(chunk.sequence) {
                        queue.push(chunk.sequence, chunk.bytes.clone());
                    }
                }
                if last.accepts(chunk.sequence) {
                    last.push(chunk.sequence, chunk.bytes);
                }
            }
        }
        // An overflow shows up inside the queues above, a loss in the tap here;
        // either way the session is one that is not keeping up.
        for attachment in mirrored.attachments.values_mut() {
            if attachment.queue.take_loss() {
                attachment.note_loss(now);
            }
        }
        let sessions: Vec<u32> = mirrored.attachments.keys().copied().collect();
        self.announce(id, &sessions, cx);
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
        if let Some(detached) = mirrored.attachments.remove(&session_id) {
            let now = cx.background_executor().now();
            let recent = |at: Instant| now.saturating_duration_since(at) < MIN_SNAPSHOT_INTERVAL;
            mirrored.recent_snapshots.retain(|_, at| recent(*at));
            if let Some(at) = detached.last_snapshot.filter(|at| recent(*at)) {
                mirrored.recent_snapshots.insert(session_id, at);
            }
        }
        if !mirrored.attachments.is_empty() {
            return;
        }
        // Nobody left: the terminal goes back to costing nothing.
        mirrored.tap_task = None;
        mirrored.last_sequence = 0;
        mirrored.pending_resize = None;
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
            self.take_snapshot_if_due(id, session_id, now, cx);
            let Some(attachment) = self
                .terminals
                .get_mut(&id)
                .and_then(|mirrored| mirrored.attachments.get_mut(&session_id))
            else {
                continue;
            };
            let popped = attachment.queue.pop();
            let stream_id = attachment.stream_id;
            let next_wake = attachment.next_wake(now);
            if let Some(at) = next_wake {
                self.schedule_wake(id, session_id, at, cx);
            }
            if let Some(bytes) = popped {
                self.last_served.insert(session_id, id);
                return Some((stream_id, bytes));
            }
        }
        None
    }

    /// Takes the snapshot `session_id` is waiting for, unless it is too soon
    /// after the last one or the terminal is gone.
    fn take_snapshot_if_due(
        &mut self,
        id: EntityId,
        session_id: u32,
        now: Instant,
        cx: &mut Context<Self>,
    ) {
        let Some(mirrored) = self.terminals.get(&id) else {
            return;
        };
        let Some(attachment) = mirrored.attachments.get(&session_id) else {
            return;
        };
        if !attachment.queue.needs_snapshot() || attachment.snapshot_wait(now).is_some() {
            return;
        }
        let Some(terminal) = mirrored.terminal.upgrade() else {
            return;
        };
        let (last_dimensions, last_sequence) = (mirrored.last_dimensions, mirrored.last_sequence);
        let kind = attachment.queue.wanted_kind();
        let history_lines = match kind {
            SnapshotKind::Full => REMOTE_SNAPSHOT_HISTORY_LINES,
            SnapshotKind::Repair => 0,
        };
        let snapshot = terminal
            .read(cx)
            .remote_snapshot_with_history(history_lines);
        let dimensions = (snapshot.columns, snapshot.rows);
        if dimensions != last_dimensions {
            // The terminal was resized and nobody has been told yet, so every
            // session still believes in the old size, and this one must hear
            // of the new one before it is shown a screen drawn at it. The
            // snapshot is not kept, and does not count against the time
            // between snapshots: it is taken again once the news is out, which
            // is when the size has stopped changing.
            self.note_size(id, dimensions, cx);
            return;
        }
        let Some(attachment) = self
            .terminals
            .get_mut(&id)
            .and_then(|mirrored| mirrored.attachments.get_mut(&session_id))
        else {
            return;
        };
        attachment
            .queue
            .install_snapshot(snapshot.sequence, &snapshot.bytes);
        attachment.last_snapshot = Some(now);
        if kind == SnapshotKind::Repair {
            attachment.last_repair = Some(now);
        }
        attachment.primary_screen_missing = snapshot.alternate_screen;
        // Output the foreground has not been handed yet means the child
        // was writing as the screen was read, and an open synchronized
        // update means the screen is behind what the tap has seen.
        let may_be_ahead = snapshot.sequence > last_sequence || snapshot.sync_pending;
        attachment.settle =
            (may_be_ahead && !attachment.next_snapshot_is_settle).then(|| Settle::armed(now));
        attachment.next_snapshot_is_settle = false;
    }

    /// Wakes the session at `at`, unless it is already set to wake no later.
    fn schedule_wake(
        &mut self,
        id: EntityId,
        session_id: u32,
        at: Instant,
        cx: &mut Context<Self>,
    ) {
        let Some(attachment) = self
            .terminals
            .get_mut(&id)
            .and_then(|mirrored| mirrored.attachments.get_mut(&session_id))
        else {
            return;
        };
        if attachment
            .wake
            .as_ref()
            .is_some_and(|scheduled| scheduled.at <= at)
        {
            return;
        }
        let delay = at.saturating_duration_since(cx.background_executor().now());
        attachment.wake = Some(ScheduledWake {
            at,
            _task: cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update(cx, |this, cx| this.wake(id, session_id, cx))
                    .log_err();
            }),
        });
    }

    fn wake(&mut self, id: EntityId, session_id: u32, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Some(mirrored) = self.terminals.get_mut(&id) else {
            return;
        };
        let terminal = mirrored.terminal.upgrade();
        let Some(attachment) = mirrored.attachments.get_mut(&session_id) else {
            return;
        };
        attachment.wake = None;
        if let Some(settle) = attachment.settle.as_mut()
            && settle.ready(now, || {
                terminal.is_some_and(|terminal| terminal.read(cx).remote_sync_pending())
            })
        {
            attachment.settle = None;
            // A snapshot that is already wanted is a fresh one and decides for
            // itself whether it needs a repeat.
            if !attachment.queue.wants_snapshot() {
                attachment.queue.invalidate();
                attachment.next_snapshot_is_settle = true;
            }
        }
        self.announce(id, &[session_id], cx);
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
        assert!(queue.wants_snapshot());
        assert!(!queue.is_sendable(), "there is nothing to pop yet");
        assert_eq!(queue.pop(), None);
        queue.install_snapshot(0, b"screen");
        assert!(queue.is_sendable());
        assert_eq!(drain(&mut queue), vec![b"screen".to_vec()]);
        assert!(!queue.is_sendable());
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
    fn a_snapshot_still_being_sent_goes_out_whole_and_the_fresh_one_follows_it() {
        let mut queue = OutputQueue::default();
        let snapshot: Vec<u8> = (0..MAX_INNER_PAYLOAD_LEN * 3)
            .map(|index| (index % 13) as u8)
            .collect();
        queue.install_snapshot(4, &snapshot);
        let mut sent = queue.pop().expect("the first frame");
        queue.push(5, b"behind the snapshot".to_vec());

        queue.invalidate();
        assert!(queue.wants_snapshot());
        assert!(
            !queue.needs_snapshot(),
            "the frames still to be sent are not thrown away"
        );
        assert!(queue.is_sendable());

        while let Some(frame) = queue.pop() {
            sent.extend_from_slice(&frame);
            if queue.needs_snapshot() {
                break;
            }
        }
        assert_eq!(sent, snapshot, "no frame of the snapshot is lost");
        assert!(
            queue.needs_snapshot(),
            "once the snapshot is out, the fresh one is wanted"
        );
        assert!(!queue.is_sendable());
        assert_eq!(queue.pop(), None);
        assert_eq!(
            queue.chunk_bytes, 0,
            "the live output behind it was dropped"
        );
    }

    #[test]
    fn live_output_is_dropped_while_a_fresh_snapshot_waits_behind_the_current_one() {
        let mut queue = OutputQueue::default();
        queue.install_snapshot(0, &vec![1u8; MAX_INNER_PAYLOAD_LEN * 2]);
        queue.invalidate();
        assert!(!queue.accepts(1));
        queue.push(1, b"too early to be of use".to_vec());
        assert_eq!(queue.chunk_bytes, 0);

        queue.install_snapshot(9, b"fresh");
        assert!(!queue.wants_snapshot());
        assert!(queue.accepts(10));
        assert!(!queue.accepts(9), "the snapshot already drew it");
    }

    #[test]
    fn an_overflow_behind_a_snapshot_does_not_cut_it_off() {
        let mut queue = OutputQueue::default();
        queue.install_snapshot(0, &vec![1u8; MAX_INNER_PAYLOAD_LEN * 2]);
        let chunk = vec![b'x'; SESSION_QUEUE_LIMIT / 2];
        queue.push(1, chunk.clone());
        queue.push(2, chunk);
        queue.push(3, b"one byte too many".to_vec());
        assert!(!queue.needs_snapshot());
        assert!(queue.wants_snapshot());
        assert_eq!(
            queue.pop().map(|frame| frame.len()),
            Some(MAX_INNER_PAYLOAD_LEN)
        );
        assert_eq!(
            queue.pop().map(|frame| frame.len()),
            Some(MAX_INNER_PAYLOAD_LEN)
        );
        assert!(queue.needs_snapshot());
    }

    #[test]
    fn restarting_drops_a_snapshot_still_being_sent() {
        let mut queue = OutputQueue::default();
        queue.install_snapshot(0, &vec![1u8; MAX_INNER_PAYLOAD_LEN * 3]);
        assert!(queue.pop().is_some());
        queue.restart();
        assert_eq!(queue.pop(), None);
        assert!(queue.needs_snapshot());
        assert!(!queue.is_sendable());
    }

    #[test]
    fn a_queue_only_accepts_what_is_newer_than_its_snapshot_and_not_while_one_is_wanted() {
        let mut queue = OutputQueue::awaiting_snapshot();
        assert!(!queue.accepts(1), "the snapshot will be taken later");
        queue.install_snapshot(5, b"screen");
        assert!(!queue.accepts(5));
        assert!(queue.accepts(6));
    }

    #[test]
    fn the_byte_accounting_returns_to_zero() {
        let mut queue = OutputQueue::default();
        for sequence in 1..=50 {
            queue.push(sequence, vec![7; 10_000]);
        }
        drain(&mut queue);
        assert_eq!(queue.chunk_bytes, 0);
        assert!(!queue.is_sendable());
    }

    const MILLISECOND: Duration = Duration::from_millis(1);

    #[test]
    fn a_settle_is_due_once_the_output_has_been_quiet_for_a_while() {
        let start = Instant::now();
        let mut settle = Settle::armed(start);
        assert_eq!(settle.due_at(), start + SETTLE_QUIET);
        assert!(!settle.ready(start + SETTLE_QUIET - MILLISECOND, || {
            panic!("the emulator is not asked before it is due")
        }));
        assert!(settle.ready(start + SETTLE_QUIET, || false));
    }

    #[test]
    fn every_chunk_restarts_the_quiet_period_but_the_cap_still_holds() {
        let start = Instant::now();
        let mut settle = Settle::armed(start);
        let step = SETTLE_QUIET / 2;
        let mut now = start;
        while now + step < start + SETTLE_MAX_WAIT {
            now += step;
            settle.on_output(now);
            assert_eq!(
                settle.due_at(),
                (now + SETTLE_QUIET).min(start + SETTLE_MAX_WAIT)
            );
        }
        assert_eq!(settle.due_at(), start + SETTLE_MAX_WAIT);
        assert!(!settle.ready(start + SETTLE_MAX_WAIT - MILLISECOND, || false));
        assert!(settle.ready(start + SETTLE_MAX_WAIT, || false));
    }

    #[test]
    fn a_settle_waits_for_a_synchronized_update_to_end() {
        let start = Instant::now();
        let due = start + SETTLE_QUIET;
        let mut settle = Settle::armed(start);

        assert!(!settle.ready(due, || true), "the update is still open");
        assert_eq!(settle.due_at(), due + SETTLE_SYNC_RECHECK_INTERVAL);
        assert!(
            !settle.ready(due + SETTLE_SYNC_RECHECK_INTERVAL - MILLISECOND, || {
                panic!("the emulator is only asked when the recheck is due")
            })
        );
        assert!(!settle.ready(due + SETTLE_SYNC_RECHECK_INTERVAL, || true));
        assert!(
            settle.ready(due + SETTLE_SYNC_RECHECK_INTERVAL * 2, || false),
            "the update ended, so the screen has caught up"
        );
    }

    #[test]
    fn an_update_that_never_ends_is_waited_for_a_bounded_time() {
        let start = Instant::now();
        let due = start + SETTLE_QUIET;
        let mut settle = Settle::armed(start);
        let mut now = due;
        let mut waits = 0;
        while !settle.ready(now, || true) {
            waits += 1;
            assert!(waits <= SETTLE_SYNC_RECHECKS, "it must give up");
            now = settle.due_at();
        }
        assert_eq!(waits, SETTLE_SYNC_RECHECKS);
        assert_eq!(
            now,
            due + SETTLE_SYNC_RECHECK_INTERVAL * SETTLE_SYNC_RECHECKS
        );
    }

    #[test]
    fn output_while_waiting_out_an_update_does_not_restart_the_wait() {
        let start = Instant::now();
        let due = start + SETTLE_QUIET;
        let mut settle = Settle::armed(start);
        assert!(!settle.ready(due, || true));
        settle.on_output(due + MILLISECOND * 10);
        assert_eq!(
            settle.due_at(),
            due + SETTLE_SYNC_RECHECK_INTERVAL,
            "the quiet period already passed; only the update is being waited for"
        );
    }

    #[test]
    fn a_session_wakes_for_whichever_of_a_retry_and_a_settle_comes_first() {
        let start = Instant::now();
        let now = start + MILLISECOND * 100;
        let mut attachment = Attachment::new(1);
        attachment.last_snapshot = Some(start);
        attachment.settle = Some(Settle::armed(start));
        assert!(attachment.queue.needs_snapshot());
        assert_eq!(
            attachment.next_wake(now),
            Some(start + MIN_SNAPSHOT_INTERVAL),
            "the held-back snapshot is due before the settle"
        );

        attachment.queue.install_snapshot(0, b"screen");
        assert_eq!(attachment.next_wake(now), Some(start + SETTLE_QUIET));

        attachment.settle = None;
        assert_eq!(attachment.next_wake(now), None);
    }

    #[cfg(unix)]
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

        fn announced(recorded: &Recorded, session_id: u32) -> usize {
            recorded
                .events
                .borrow()
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        TerminalMirrorEvent::OutputReady { session_id: announced }
                            if *announced == session_id
                    )
                })
                .count()
        }

        /// Asks for a fresh snapshot for the session, as an overflow does.
        fn invalidate_session(
            mirror: &Entity<TerminalMirror>,
            session_id: u32,
            cx: &mut TestAppContext,
        ) {
            mirror.update(cx, |mirror, _| {
                for mirrored in mirror.terminals.values_mut() {
                    if let Some(attachment) = mirrored.attachments.get_mut(&session_id) {
                        attachment.queue.invalidate();
                    }
                }
            });
        }

        /// Makes the mirror deaf to what the terminal does, so a test can
        /// change the terminal without the mirror hearing of it.
        fn stop_following(mirror: &Entity<TerminalMirror>, cx: &mut TestAppContext) {
            mirror.update(cx, |mirror, _| {
                for mirrored in mirror.terminals.values_mut() {
                    mirrored.following.clear();
                }
            });
        }

        /// Gives the terminal a new size of `width` pixels across and 20 rows,
        /// as its view would when it is laid out.
        fn resize_terminal(terminal: &Entity<Terminal>, width: f32, cx: &mut TestAppContext) {
            let window = cx.add_empty_window();
            terminal.update_in(window, |terminal, window, cx| {
                let bounds = terminal::TerminalBounds::new(
                    gpui::px(10.),
                    gpui::px(5.),
                    gpui::Bounds {
                        origin: gpui::Point::default(),
                        size: gpui::Size {
                            width: gpui::px(width),
                            height: gpui::px(200.),
                        },
                    },
                );
                terminal.set_size(bounds);
                terminal.sync(window, cx);
            });
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
                .advance_clock(SETTLE_QUIET + Duration::from_millis(10));
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
                "the repeat is not itself repeated"
            );
        }

        #[gpui::test]
        async fn a_terminal_that_was_quiet_gets_no_repeat_snapshot(cx: &mut TestAppContext) {
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

        fn push_output(
            mirror: &Entity<TerminalMirror>,
            entity_id: EntityId,
            cx: &mut TestAppContext,
        ) {
            mirror.update(cx, |mirror, cx| {
                let sequence = mirror
                    .terminals
                    .get(&entity_id)
                    .map_or(0, |mirrored| mirrored.last_sequence)
                    + 1;
                let chunk = RemoteOutputChunk {
                    sequence,
                    bytes: b"x".to_vec(),
                };
                mirror.on_chunks(entity_id, vec![chunk], false, cx);
            });
        }

        fn full_redraws(frames: &[Vec<u8>]) -> usize {
            frames
                .iter()
                .filter(|frame| frame.starts_with(b"\x1bc"))
                .count()
        }

        #[gpui::test]
        async fn output_that_never_goes_quiet_still_gets_exactly_one_repeat_at_the_cap(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded, terminal, _id) = attached_while_output_is_in_flight(cx).await;
            let entity_id = terminal.entity_id();
            pop_all(&mirror, 1, cx);
            cx.run_until_parked();
            pop_all(&mirror, 1, cx);

            // Something arrives more often than the quiet period, for longer
            // than the cap.
            let step = SETTLE_QUIET / 4;
            let mut waited = Duration::ZERO;
            let mut repeats = 0;
            while waited < SETTLE_MAX_WAIT + SETTLE_QUIET {
                push_output(&mirror, entity_id, cx);
                cx.executor().advance_clock(step);
                cx.run_until_parked();
                waited += step;
                repeats += full_redraws(&pop_all(&mirror, 1, cx));
                if waited < SETTLE_MAX_WAIT {
                    assert_eq!(repeats, 0, "still inside the cap at {waited:?}");
                }
            }
            assert_eq!(repeats, 1, "one repeat once the cap was reached");

            recorded.events.borrow_mut().clear();
            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(
                full_redraws(&pop_all(&mirror, 1, cx)) == 0 && recorded.events.borrow().is_empty(),
                "the repeat is not itself repeated"
            );
        }

        /// A terminal whose emulator is inside a synchronized update and stays
        /// there: the child leaves inside it, so nothing ends it. An update
        /// that ends by itself after a fraction of a second would make a test
        /// of what happens inside one a race.
        async fn terminal_inside_an_update(cx: &mut TestAppContext) -> Entity<Terminal> {
            let terminal = shell_terminal("printf 'before\\n\\033[?2026hheld back'", cx).await;
            for _ in 0..500 {
                if terminal.read_with(cx, |terminal, _| terminal.remote_sync_pending()) {
                    break;
                }
                cx.background_executor
                    .timer(Duration::from_millis(10))
                    .await;
                cx.run_until_parked();
            }
            assert!(terminal.read_with(cx, |terminal, _| terminal.remote_sync_pending()));
            terminal
        }

        #[gpui::test]
        async fn a_snapshot_taken_inside_a_synchronized_update_is_repeated_when_the_wait_ends(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _terminal = terminal_inside_an_update(cx).await;
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");

            let first = pop_all(&mirror, 1, cx).concat();
            let first_text = String::from_utf8_lossy(&first).into_owned();
            assert!(first.starts_with(b"\x1bc"));
            assert!(first_text.contains("before"), "{first_text:?}");
            assert!(
                !first_text.contains("held back"),
                "the update holds those bytes back: {first_text:?}"
            );
            recorded.events.borrow_mut().clear();

            // Nothing is flowing, yet the snapshot is not trusted: it is due
            // after the quiet period, but the update is still open. Here it
            // never ends, so the repeat waits a bounded number of looks and
            // then goes ahead anyway.
            let look = SETTLE_SYNC_RECHECK_INTERVAL + Duration::from_millis(1);
            cx.executor()
                .advance_clock(SETTLE_QUIET + Duration::from_millis(10));
            cx.run_until_parked();
            for number in 1..SETTLE_SYNC_RECHECKS {
                assert!(
                    pop_all(&mirror, 1, cx).is_empty() && recorded.events.borrow().is_empty(),
                    "still waiting for the update at look {number}"
                );
                cx.executor().advance_clock(look);
                cx.run_until_parked();
            }
            assert!(pop_all(&mirror, 1, cx).is_empty());
            cx.executor().advance_clock(look);
            cx.run_until_parked();
            assert!(
                recorded.events.borrow().iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::OutputReady { session_id: 1 }
                )),
                "the host is told there is a screen to send"
            );
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);

            recorded.events.borrow_mut().clear();
            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(
                pop_all(&mirror, 1, cx).is_empty() && recorded.events.borrow().is_empty(),
                "the repeat is not itself repeated, even though it was taken inside the update too"
            );
        }

        #[gpui::test]
        async fn a_held_back_snapshot_is_not_delayed_by_a_repeat_that_is_further_off(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded, _terminal, _id) = attached_while_output_is_in_flight(cx).await;
            pop_all(&mirror, 1, cx);
            cx.run_until_parked();
            pop_all(&mirror, 1, cx);

            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            mirror.update(cx, |mirror, _| {
                for mirrored in mirror.terminals.values_mut() {
                    if let Some(attachment) = mirrored.attachments.get_mut(&1) {
                        attachment.queue.invalidate();
                    }
                }
            });
            assert!(
                pop_all(&mirror, 1, cx).is_empty(),
                "too soon after the last"
            );
            recorded.events.borrow_mut().clear();

            // The retry is due before the repeat is, and must not wait for it.
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL - Duration::from_millis(90));
            cx.run_until_parked();
            assert!(
                recorded.events.borrow().iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::OutputReady { session_id: 1 }
                )),
                "the held-back snapshot is asked for again on time"
            );
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);

            // Taken at rest, that snapshot is trustworthy: the repeat that was
            // waiting for the older one is no longer needed.
            recorded.events.borrow_mut().clear();
            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(pop_all(&mirror, 1, cx).is_empty() && recorded.events.borrow().is_empty());
        }

        /// Whether the session has a repeat waiting, whether its next snapshot
        /// is that repeat, and whether a snapshot is wanted.
        fn session_state(
            mirror: &Entity<TerminalMirror>,
            cx: &mut TestAppContext,
        ) -> (bool, bool, bool) {
            mirror.read_with(cx, |mirror, _| {
                let attachment = mirror
                    .terminals
                    .values()
                    .find_map(|mirrored| mirrored.attachments.get(&1))
                    .expect("session 1 is attached");
                (
                    attachment.settle.is_some(),
                    attachment.next_snapshot_is_settle,
                    attachment.queue.needs_snapshot(),
                )
            })
        }

        #[gpui::test]
        async fn a_repeat_that_comes_due_while_a_snapshot_is_still_wanted_leaves_that_snapshot_to_decide_for_itself(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _terminal = terminal_inside_an_update(cx).await;
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);
            assert_eq!(
                session_state(&mirror, cx),
                (true, false, false),
                "taken inside an update, so it is not trusted"
            );

            // The session is asked for a fresh snapshot and nobody takes it, as
            // when the relay is backed up.
            mirror.update(cx, |mirror, _| {
                for mirrored in mirror.terminals.values_mut() {
                    if let Some(attachment) = mirrored.attachments.get_mut(&1) {
                        attachment.queue.invalidate();
                    }
                }
            });

            // The update never ends, so the repeat gives up waiting for it
            // after a bounded number of looks.
            let look = SETTLE_SYNC_RECHECK_INTERVAL + Duration::from_millis(1);
            cx.executor()
                .advance_clock(SETTLE_QUIET + Duration::from_millis(10));
            cx.run_until_parked();
            for _ in 0..=SETTLE_SYNC_RECHECKS {
                if !session_state(&mirror, cx).0 {
                    break;
                }
                cx.executor().advance_clock(look);
                cx.run_until_parked();
            }
            assert_eq!(
                session_state(&mirror, cx),
                (false, false, true),
                "the repeat is spent, and the snapshot that is wanted is not marked as one"
            );

            // That snapshot is taken inside the update too, and earns a repeat
            // of its own, which it could not if it had been taken for the old one.
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);
            assert_eq!(session_state(&mirror, cx), (true, false, false));
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
        async fn output_is_announced_when_there_is_something_to_send_and_not_while_a_snapshot_is_held_back(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            assert_eq!(
                announced(&recorded, 1),
                1,
                "the first snapshot can be taken at once"
            );
            assert!(!pop_all(&mirror, 1, cx).is_empty());
            recorded.events.borrow_mut().clear();

            let entity_id = terminal.entity_id();
            push_output(&mirror, entity_id, cx);
            assert_eq!(announced(&recorded, 1), 1, "output is waiting behind it");
            pop_all(&mirror, 1, cx);
            recorded.events.borrow_mut().clear();

            // An overflow right after: the fresh snapshot may not be taken for
            // a while, and until then there is nothing to ask for.
            invalidate_session(&mirror, 1, cx);
            for _ in 0..3 {
                push_output(&mirror, entity_id, cx);
            }
            assert_eq!(
                announced(&recorded, 1),
                0,
                "the host would be asked for a screen that cannot be taken yet"
            );
            assert!(pop_all(&mirror, 1, cx).is_empty());

            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            assert_eq!(
                announced(&recorded, 1),
                1,
                "announced once, when the snapshot can be taken"
            );
            assert!(pop_all(&mirror, 1, cx)[0].starts_with(b"\x1bc"));
        }

        /// A screen with more scrollback than fits in one frame, so its
        /// snapshot is several.
        const LONG_SCROLLBACK_SCRIPT: &str = "i=0; while [ $i -lt 1100 ]; do printf '%079d\\n' $i; i=$((i+1)); done; \
             printf 'LASTLINE\\n'; sleep 30";

        #[gpui::test]
        async fn a_snapshot_still_being_sent_survives_an_overflow_and_a_fresh_one_follows(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal(LONG_SCROLLBACK_SCRIPT, cx).await;
            wait_for(&terminal, "LASTLINE", cx).await;
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");

            let (_, first) = mirror
                .update(cx, |mirror, cx| mirror.pop_output(1, cx))
                .expect("the first frame of the snapshot");
            assert!(first.starts_with(b"\x1bc"));

            // The tap loses output while the device is still taking the
            // snapshot, which is what a flood does to a slow link.
            let entity_id = terminal.entity_id();
            mirror.update(cx, |mirror, cx| {
                let sequence = mirror
                    .terminals
                    .get(&entity_id)
                    .map_or(0, |mirrored| mirrored.last_sequence)
                    + 1;
                let chunk = RemoteOutputChunk {
                    sequence,
                    bytes: b"flood".to_vec(),
                };
                mirror.on_chunks(entity_id, vec![chunk], true, cx);
            });

            let rest = pop_all(&mirror, 1, cx);
            assert!(!rest.is_empty(), "the rest of the snapshot is still sent");
            let mut sent = first;
            sent.extend(rest.concat());
            let expected = terminal.read_with(cx, |terminal, _| terminal.remote_snapshot().bytes);
            assert!(
                expected.len() > MAX_INNER_PAYLOAD_LEN,
                "the screen has to need several frames for this to mean anything"
            );
            assert!(
                sent == expected,
                "the device got the whole screen, not the first part of it"
            );

            // The fresh snapshot is held back like any other, then arrives.
            assert!(pop_all(&mirror, 1, cx).is_empty());
            recorded.events.borrow_mut().clear();
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            assert_eq!(announced(&recorded, 1), 1);
            assert!(pop_all(&mirror, 1, cx)[0].starts_with(b"\x1bc"));
        }

        #[gpui::test]
        async fn a_second_device_attaching_does_not_hide_a_resize_from_the_first(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            pop_all(&mirror, 1, cx);

            // The pane is resized and the mirror has not heard of it yet.
            stop_following(&mirror, cx);
            resize_terminal(&terminal, 500., cx);
            recorded.events.borrow_mut().clear();

            let (columns, rows) = mirror
                .update(cx, |mirror, cx| mirror.attach(2, &id, 6, cx))
                .expect("a second device attaches");
            assert_eq!(
                (columns, rows),
                (100, 20),
                "the new device is told the size"
            );
            assert!(
                !recorded
                    .events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, TerminalMirrorEvent::Resized { .. })),
                "the news waits for the size to settle"
            );
            cx.executor().advance_clock(RESIZE_SETTLE);
            cx.run_until_parked();
            let events = recorded.events.borrow().clone();
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    TerminalMirrorEvent::Resized { columns: 100, rows: 20, sessions, .. }
                        if sessions == &vec![1, 2]
                )),
                "the first device is told along with the second: {events:?}"
            );

            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            assert!(pop_all(&mirror, 1, cx)[0].starts_with(b"\x1bc"));
        }

        #[gpui::test]
        async fn a_terminal_resized_before_anyone_heard_of_it_announces_it_before_the_screen(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            stop_following(&mirror, cx);
            resize_terminal(&terminal, 600., cx);
            recorded.events.borrow_mut().clear();

            assert_eq!(
                mirror.update(cx, |mirror, cx| mirror.pop_output(1, cx)),
                None,
                "a screen drawn at a size the device was not told of is not sent first"
            );
            cx.executor().advance_clock(RESIZE_SETTLE);
            cx.run_until_parked();
            let events = recorded.events.borrow().clone();
            let resized = events.iter().position(|event| {
                matches!(
                    event,
                    TerminalMirrorEvent::Resized { columns: 120, rows: 20, sessions, .. }
                        if sessions == &vec![1]
                )
            });
            let ready = events.iter().position(|event| {
                matches!(event, TerminalMirrorEvent::OutputReady { session_id: 1 })
            });
            assert!(
                matches!((resized, ready), (Some(resized), Some(ready)) if resized < ready),
                "the news comes first, then the invitation to ask again: {events:?}"
            );

            let frames = pop_all(&mirror, 1, cx);
            assert!(
                frames[0].starts_with(b"\x1bc"),
                "the screen follows at the new size, without waiting out the interval"
            );
        }

        #[gpui::test]
        async fn a_title_a_program_sets_does_not_list_the_terminals_again(cx: &mut TestAppContext) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            recorded.events.borrow_mut().clear();

            terminal.update(cx, |_, cx| cx.emit(terminal::Event::BreadcrumbsChanged));
            assert!(
                recorded.events.borrow().is_empty(),
                "no summary shows the breadcrumb, so there is nothing new to list"
            );
            terminal.update(cx, |_, cx| cx.emit(terminal::Event::TitleChanged));
            assert_eq!(
                *recorded.events.borrow(),
                vec![TerminalMirrorEvent::ListChanged],
                "a title that did change is listed"
            );
        }

        #[gpui::test]
        async fn attaching_again_does_not_reset_the_time_between_snapshots(
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
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);

            // On the same stream, then after leaving: neither earns a
            // snapshot sooner than staying would have.
            for leave_first in [false, true] {
                if leave_first {
                    mirror.update(cx, |mirror, cx| mirror.detach(1, &id, cx));
                }
                recorded.events.borrow_mut().clear();
                mirror
                    .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                    .expect("attaches again");
                assert!(
                    pop_all(&mirror, 1, cx).is_empty(),
                    "a device cannot force a snapshot per request (leave first: {leave_first})"
                );
                assert_eq!(announced(&recorded, 1), 0);
                cx.executor()
                    .advance_clock(MIN_SNAPSHOT_INTERVAL - Duration::from_millis(10));
                cx.run_until_parked();
                assert!(pop_all(&mirror, 1, cx).is_empty());

                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
                assert_eq!(announced(&recorded, 1), 1);
                assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);
            }

            // A device that was gone for longer than that is owed one at once.
            mirror.update(cx, |mirror, cx| mirror.detach(1, &id, cx));
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches after a while");
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);
            assert!(
                mirror.read_with(cx, |mirror, _| mirror
                    .terminals
                    .values()
                    .all(|mirrored| mirrored.recent_snapshots.is_empty())),
                "what is remembered of a device that left does not pile up"
            );
        }

        fn lose_output(
            mirror: &Entity<TerminalMirror>,
            entity_id: EntityId,
            cx: &mut TestAppContext,
        ) {
            mirror.update(cx, |mirror, cx| {
                let sequence = mirror
                    .terminals
                    .get(&entity_id)
                    .map_or(0, |mirrored| mirrored.last_sequence)
                    + 1;
                let chunk = RemoteOutputChunk {
                    sequence,
                    bytes: b"lost".to_vec(),
                };
                mirror.on_chunks(entity_id, vec![chunk], true, cx);
            });
        }

        fn repair_spacing(mirror: &Entity<TerminalMirror>, cx: &mut TestAppContext) -> Duration {
            mirror.read_with(cx, |mirror, _| {
                mirror
                    .terminals
                    .values()
                    .flat_map(|mirrored| mirrored.attachments.values())
                    .map(|attachment| attachment.repair_spacing)
                    .next()
                    .unwrap_or_default()
            })
        }

        #[gpui::test]
        async fn repairs_back_off_while_the_device_keeps_falling_behind_and_carry_no_scrollback(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal(LONG_SCROLLBACK_SCRIPT, cx).await;
            wait_for(&terminal, "LASTLINE", cx).await;
            let id = only_terminal_id(&mirror, cx);
            let entity_id = terminal.entity_id();
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");
            let full: usize = pop_all(&mirror, 1, cx).iter().map(Vec::len).sum();

            let tick = Duration::from_millis(10);
            for spacing in [
                MIN_SNAPSHOT_INTERVAL,
                MIN_SNAPSHOT_INTERVAL * 2,
                MIN_SNAPSHOT_INTERVAL * 4,
                MAX_REPAIR_SPACING,
                MAX_REPAIR_SPACING,
            ] {
                lose_output(&mirror, entity_id, cx);
                assert_eq!(repair_spacing(&mirror, cx), spacing);
                cx.executor().advance_clock(spacing - tick);
                cx.run_until_parked();
                assert!(
                    pop_all(&mirror, 1, cx).is_empty(),
                    "a repair waits {spacing:?} after the last snapshot"
                );
                cx.executor().advance_clock(tick * 2);
                cx.run_until_parked();
                let frames = pop_all(&mirror, 1, cx);
                assert_eq!(full_redraws(&frames), 1, "{spacing:?}");
                let repair: usize = frames.iter().map(Vec::len).sum();
                assert!(
                    repair * 10 < full,
                    "a repair is the screen alone: {repair} bytes against {full}"
                );
            }

            cx.executor().advance_clock(REPAIR_BACKOFF_RESET + tick);
            cx.run_until_parked();
            lose_output(&mirror, entity_id, cx);
            assert_eq!(
                repair_spacing(&mirror, cx),
                MIN_SNAPSHOT_INTERVAL,
                "a link that kept up for a while starts over"
            );
        }

        #[gpui::test]
        async fn a_burst_of_resizes_is_announced_once_with_one_snapshot(cx: &mut TestAppContext) {
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

            let step = RESIZE_SETTLE / 4;
            for width in [400., 450., 500., 550., 600.] {
                resize_terminal(&terminal, width, cx);
                cx.executor().advance_clock(step);
                cx.run_until_parked();
            }
            assert!(
                recorded.events.borrow().is_empty(),
                "nothing is announced while the size is still changing"
            );
            assert!(pop_all(&mirror, 1, cx).is_empty());

            cx.executor()
                .advance_clock(RESIZE_SETTLE + MIN_SNAPSHOT_INTERVAL);
            cx.run_until_parked();
            let resized: Vec<_> = recorded
                .events
                .borrow()
                .iter()
                .filter_map(|event| match event {
                    TerminalMirrorEvent::Resized { columns, .. } => Some(*columns),
                    _ => None,
                })
                .collect();
            assert_eq!(resized, vec![120]);
            assert_eq!(full_redraws(&pop_all(&mirror, 1, cx)), 1);
        }

        #[gpui::test]
        async fn what_is_remembered_of_devices_that_left_is_pruned_as_it_goes_stale(
            cx: &mut TestAppContext,
        ) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let _terminal = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = only_terminal_id(&mirror, cx);
            let remembered = |mirror: &Entity<TerminalMirror>, cx: &mut TestAppContext| {
                mirror.read_with(cx, |mirror, _| {
                    let mut sessions: Vec<u32> = mirror
                        .terminals
                        .values()
                        .flat_map(|mirrored| mirrored.recent_snapshots.keys().copied())
                        .collect();
                    sessions.sort_unstable();
                    sessions
                })
            };

            for session_id in [1, 2] {
                mirror
                    .update(cx, |mirror, cx| mirror.attach(session_id, &id, 5, cx))
                    .expect("attaches");
                pop_all(&mirror, session_id, cx);
                mirror.update(cx, |mirror, cx| mirror.detach(session_id, &id, cx));
            }
            assert_eq!(remembered(&mirror, cx), vec![1, 2]);

            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();
            mirror
                .update(cx, |mirror, cx| mirror.attach(3, &id, 5, cx))
                .expect("attaches");
            pop_all(&mirror, 3, cx);
            mirror.update(cx, |mirror, cx| mirror.detach(3, &id, cx));
            assert_eq!(
                remembered(&mirror, cx),
                vec![3],
                "the ones that are past the interval are forgotten"
            );
        }

        #[gpui::test]
        async fn dropping_the_mirror_releases_it(cx: &mut TestAppContext) {
            let (mirror, recorded) = mirror(cx);
            let weak = mirror.downgrade();
            drop(recorded);
            drop(mirror);
            cx.update(|_| {});
            assert!(
                weak.upgrade().is_none(),
                "something inside the mirror still holds it up"
            );
        }

        #[gpui::test]
        async fn leaving_the_alternate_screen_sends_the_primary_screen_again(
            cx: &mut TestAppContext,
        ) {
            let (mirror, recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal(
                "printf 'PRIMARYTEXT\\n\\033[?1049hALTERNATETEXT'; read line; \
                 printf '\\033[?1049lBACKTEXT\\n'; sleep 30",
                cx,
            )
            .await;
            wait_for(&terminal, "ALTERNATETEXT", cx).await;
            assert!(terminal.read_with(cx, |terminal, _| terminal.remote_in_alternate_screen()));
            let id = only_terminal_id(&mirror, cx);
            mirror
                .update(cx, |mirror, cx| mirror.attach(1, &id, 5, cx))
                .expect("attaches");

            let first = String::from_utf8_lossy(&pop_all(&mirror, 1, cx).concat()).into_owned();
            assert!(first.starts_with("\x1bc\x1b[?1049h"), "{first:?}");
            assert!(first.contains("ALTERNATETEXT"));
            assert!(
                !first.contains("PRIMARYTEXT"),
                "the snapshot of the alternate screen has no primary screen: {first:?}"
            );

            assert!(mirror.update(cx, |mirror, cx| mirror.input(1, &id, b"go\r".to_vec(), cx)));
            wait_for(&terminal, "BACKTEXT", cx).await;
            cx.run_until_parked();
            assert!(!terminal.read_with(cx, |terminal, _| terminal.remote_in_alternate_screen()));
            recorded.events.borrow_mut().clear();
            cx.executor()
                .advance_clock(MIN_SNAPSHOT_INTERVAL + Duration::from_millis(10));
            cx.run_until_parked();

            let frames = pop_all(&mirror, 1, cx);
            let after = String::from_utf8_lossy(&frames.concat()).into_owned();
            assert!(
                after.starts_with("\x1bc"),
                "a device left on a blank primary screen is redrawn: {after:?}"
            );
            assert!(
                after.contains("PRIMARYTEXT") && after.contains("BACKTEXT"),
                "{after:?}"
            );
            assert!(
                !after.contains("?1049h"),
                "it is the primary screen: {after:?}"
            );

            // The new snapshot is of the primary screen, so nothing is owed
            // when the program returns to it.
            recorded.events.borrow_mut().clear();
            cx.executor().advance_clock(Duration::from_secs(10));
            cx.run_until_parked();
            assert!(pop_all(&mirror, 1, cx).is_empty() && recorded.events.borrow().is_empty());
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
            // Nothing is printed, and nobody asks the mirror to look: the new
            // size alone is what makes it speak.
            resize_terminal(&terminal, 500., cx);
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
        async fn one_terminals_size_is_read_without_listing_them_all(cx: &mut TestAppContext) {
            let (mirror, _recorded) = mirror(cx);
            mirror.update(cx, |mirror, cx| mirror.activate(cx));
            let terminal = shell_terminal("sleep 30", cx).await;
            let _other = shell_terminal("sleep 30", cx).await;
            cx.run_until_parked();
            let id = terminal_id(terminal.entity_id());

            let before = mirror.read_with(cx, |mirror, cx| mirror.dimensions_of(&id, cx));
            let listed = mirror
                .read_with(cx, |mirror, cx| mirror.summaries(cx))
                .into_iter()
                .find(|summary| summary.id == id)
                .map(|summary| (summary.columns, summary.rows));
            assert!(before.is_some());
            assert_eq!(before, listed);

            resize_terminal(&terminal, 500., cx);
            assert_eq!(
                mirror.read_with(cx, |mirror, cx| mirror.dimensions_of(&id, cx)),
                Some((100, 20)),
                "it reads the terminal as it is now"
            );
            assert_eq!(
                mirror.read_with(cx, |mirror, cx| mirror.dimensions_of("terminal-0", cx)),
                None
            );
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
