//! When each reminder and the focus timer come due, as plain values over a
//! clock that is passed in, so every rule can be tested without a timer.

use std::time::{Duration, Instant};

/// A periodic reminder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReminderKind {
    Water,
    EyeBreak,
    StandUp,
}

impl ReminderKind {
    pub const ALL: [ReminderKind; 3] = [Self::Water, Self::EyeBreak, Self::StandUp];

    fn index(self) -> usize {
        match self {
            Self::Water => 0,
            Self::EyeBreak => 1,
            Self::StandUp => 2,
        }
    }
}

/// Which part of a focus cycle is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusPhase {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusLengths {
    pub focus: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
    pub rounds_before_long_break: u32,
}

impl FocusLengths {
    pub fn of(&self, phase: FocusPhase) -> Duration {
        match phase {
            FocusPhase::Focus => self.focus,
            FocusPhase::ShortBreak => self.short_break,
            FocusPhase::LongBreak => self.long_break,
        }
    }
}

/// What a tick found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleEvent {
    Due(ReminderKind),
    FocusPhaseChanged(FocusPhase),
}

/// The focus timer, when one has been started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusTimer {
    pub phase: FocusPhase,
    /// Completed focus rounds since the timer was started.
    pub rounds: u32,
    pub state: FocusState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusState {
    Running { ends_at: Instant },
    Paused { remaining: Duration },
}

impl FocusTimer {
    pub fn remaining(&self, now: Instant) -> Duration {
        match self.state {
            FocusState::Running { ends_at } => ends_at.saturating_duration_since(now),
            FocusState::Paused { remaining } => remaining,
        }
    }
}

/// A gap between two ticks longer than this means the clock jumped -- the
/// machine slept on a platform whose monotonic clock keeps counting through
/// sleep. Reminders that came due in the gap are rescheduled, not fired.
pub const CLOCK_JUMP: Duration = Duration::from_secs(120);

/// How often the clock is read while a seconds countdown is on screen.
pub const TICK: Duration = Duration::from_secs(1);

/// The longest wait between two ticks while anything runs. Kept under
/// [`CLOCK_JUMP`], so a gap that long still means the machine slept.
pub const MAX_WAIT: Duration = Duration::from_secs(60);

/// How often a reminder held for an absent user checks whether they are back.
pub const HELD_POLL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct Schedule {
    /// Interval per [`ReminderKind`], `None` when that reminder is off, and its
    /// next due time once started, `None` until then.
    periodic: [Option<(Duration, Option<Instant>)>; 3],
    focus: Option<FocusTimer>,
    lengths: FocusLengths,
    paused_until: Option<Instant>,
    last_tick: Instant,
}

impl Schedule {
    pub fn new(lengths: FocusLengths, now: Instant) -> Self {
        Self {
            periodic: [None; 3],
            focus: None,
            lengths,
            paused_until: None,
            last_tick: now,
        }
    }

    /// Applies each reminder's interval, `None` meaning off. A reminder just
    /// turned on waits for [`Self::start_reminder`]. One already running keeps
    /// its due time if its interval did not change, and starts a fresh interval
    /// from `now` if it did.
    pub fn set_intervals(&mut self, intervals: [Option<Duration>; 3], now: Instant) {
        for (slot, interval) in self.periodic.iter_mut().zip(intervals) {
            *slot = match (*slot, interval) {
                (_, None) => None,
                (Some((old, due)), Some(new)) if old == new => Some((old, due)),
                (Some((_, Some(_))), Some(new)) => Some((new, Some(now + new))),
                (_, Some(new)) => Some((new, None)),
            };
        }
    }

    /// Starts a reminder that is on: it first comes due one interval from `now`.
    pub fn start_reminder(&mut self, kind: ReminderKind, now: Instant) {
        if let Some((interval, due)) = &mut self.periodic[kind.index()] {
            *due = Some(now + *interval);
        }
    }

    /// Stops a reminder until it is started again; it stays on.
    pub fn stop_reminder(&mut self, kind: ReminderKind) {
        if let Some((_, due)) = &mut self.periodic[kind.index()] {
            *due = None;
        }
    }

    /// Whether `kind` is on and has been started.
    pub fn is_running(&self, kind: ReminderKind) -> bool {
        self.next_due(kind).is_some()
    }

    /// New lengths apply from the next phase; the running one keeps its end.
    pub fn set_focus_lengths(&mut self, lengths: FocusLengths) {
        self.lengths = lengths;
    }

    pub fn next_due(&self, kind: ReminderKind) -> Option<Instant> {
        self.periodic[kind.index()].and_then(|(_, due)| due)
    }

    pub fn focus(&self) -> Option<FocusTimer> {
        self.focus
    }

    pub fn paused_until(&self, now: Instant) -> Option<Instant> {
        self.paused_until.filter(|until| *until > now)
    }

    /// Holds every periodic reminder until `until`; the focus timer is not
    /// affected, it has its own pause.
    pub fn pause_reminders(&mut self, until: Option<Instant>) {
        self.paused_until = until;
    }

    pub fn snooze(&mut self, kind: ReminderKind, by: Duration, now: Instant) {
        if let Some((_, Some(due))) = &mut self.periodic[kind.index()] {
            *due = now + by;
        }
    }

    pub fn start_focus(&mut self, now: Instant) {
        self.focus = Some(FocusTimer {
            phase: FocusPhase::Focus,
            rounds: 0,
            state: FocusState::Running {
                ends_at: now + self.lengths.focus,
            },
        });
    }

    pub fn stop_focus(&mut self) {
        self.focus = None;
    }

    pub fn pause_focus(&mut self, now: Instant) {
        if let Some(timer) = &mut self.focus
            && let FocusState::Running { .. } = timer.state
        {
            timer.state = FocusState::Paused {
                remaining: timer.remaining(now),
            };
        }
    }

    pub fn resume_focus(&mut self, now: Instant) {
        if let Some(timer) = &mut self.focus
            && let FocusState::Paused { remaining } = timer.state
        {
            timer.state = FocusState::Running {
                ends_at: now + remaining,
            };
        }
    }

    /// Ends the current phase now and starts the next one, running.
    pub fn skip_focus_phase(&mut self, now: Instant) -> Option<FocusPhase> {
        let timer = self.focus.as_mut()?;
        let next = next_phase(timer, &self.lengths);
        timer.phase = next;
        timer.state = FocusState::Running {
            ends_at: now + self.lengths.of(next),
        };
        Some(next)
    }

    /// How long until the schedule next needs [`Self::tick`], or `None` when
    /// nothing runs and so nothing can come due.
    pub fn next_tick_in(&self, now: Instant) -> Option<Duration> {
        if let Some(FocusTimer {
            state: FocusState::Running { .. },
            ..
        }) = self.focus
        {
            return Some(TICK);
        }
        let paused_until = self.paused_until(now);
        let next = self
            .periodic
            .iter()
            .filter_map(|slot| slot.and_then(|(_, due)| due))
            .map(|due| {
                if due > now {
                    due
                } else {
                    paused_until.unwrap_or(now + HELD_POLL)
                }
            })
            .min()?;
        Some(next.saturating_duration_since(now).min(MAX_WAIT))
    }

    /// Marks `now` as the last tick, for a clock that starts after nothing
    /// was running: the quiet time before it is not a jump.
    pub fn resume_clock(&mut self, now: Instant) {
        self.last_tick = now;
    }

    /// Advances the schedule to `now`. `idle_for` is how long the user has
    /// been away; a reminder due while that is at least `idle_pause` waits for
    /// their return instead of firing.
    pub fn tick(
        &mut self,
        now: Instant,
        idle_for: Duration,
        idle_pause: Duration,
    ) -> Vec<ScheduleEvent> {
        let jumped = now.saturating_duration_since(self.last_tick) > CLOCK_JUMP;
        self.last_tick = now;
        let mut events = Vec::new();

        let held = jumped || self.paused_until(now).is_some() || idle_for >= idle_pause;
        for (index, slot) in self.periodic.iter_mut().enumerate() {
            let Some((interval, Some(due))) = slot else {
                continue;
            };
            if *due > now {
                continue;
            }
            if jumped {
                *due = now + *interval;
            } else if !held {
                *due = now + *interval;
                events.push(ScheduleEvent::Due(ReminderKind::ALL[index]));
            }
        }

        if let Some(timer) = &mut self.focus
            && let FocusState::Running { ends_at } = timer.state
            && ends_at <= now
        {
            let next = next_phase(timer, &self.lengths);
            timer.phase = next;
            timer.state = FocusState::Running {
                ends_at: now + self.lengths.of(next),
            };
            if !jumped {
                events.push(ScheduleEvent::FocusPhaseChanged(next));
            }
        }

        events
    }
}

/// The phase after `timer`'s current one, counting a finished focus round.
fn next_phase(timer: &mut FocusTimer, lengths: &FocusLengths) -> FocusPhase {
    match timer.phase {
        FocusPhase::Focus => {
            timer.rounds += 1;
            if timer
                .rounds
                .is_multiple_of(lengths.rounds_before_long_break.max(1))
            {
                FocusPhase::LongBreak
            } else {
                FocusPhase::ShortBreak
            }
        }
        FocusPhase::ShortBreak | FocusPhase::LongBreak => FocusPhase::Focus,
    }
}
