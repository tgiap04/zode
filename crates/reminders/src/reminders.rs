//! Reminders to drink water, rest the eyes and stand up, and a focus timer.
//!
//! This crate keeps time and says when something is due; showing it is
//! `reminders_ui`'s job. Every interval comes from the `reminders` setting.

mod schedule;

use std::time::{Duration, Instant};

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription, Task};
use settings::{RegisterSetting, Settings, SettingsContent, SettingsStore};

pub use schedule::{
    CLOCK_JUMP, FocusLengths, FocusPhase, FocusState, FocusTimer, HELD_POLL, MAX_WAIT,
    ReminderKind, Schedule, ScheduleEvent, TICK,
};
pub use settings::ReminderStyle;

const MIN_MINUTES: u32 = 1;
const MAX_MINUTES: u32 = 480;

/// Minutes from the setting, or `default` when the value is outside 1..=480.
fn minutes(value: Option<u32>, default: u32) -> Duration {
    let minutes = match value {
        Some(minutes) if (MIN_MINUTES..=MAX_MINUTES).contains(&minutes) => minutes,
        Some(minutes) => {
            log::warn!(
                "reminder interval of {minutes} minutes is outside 1..=480; using {default}"
            );
            default
        }
        None => default,
    };
    Duration::from_secs(u64::from(minutes) * 60)
}

/// The `reminders` setting, resolved.
#[derive(Clone, Debug, PartialEq, RegisterSetting)]
pub struct RemindersSettings {
    pub enabled: bool,
    pub style: ReminderStyle,
    pub idle_pause: Duration,
    pub snooze: Duration,
    /// Interval per [`ReminderKind::ALL`]; `None` when that reminder is off.
    pub intervals: [Option<Duration>; 3],
    pub focus: FocusLengths,
}

impl Settings for RemindersSettings {
    fn from_settings(content: &SettingsContent) -> Self {
        // default.json carries every field, so a missing one is a broken
        // settings ritual and is meant to fail loudly, as in the other settings.
        let content = content.reminders.as_ref().unwrap();
        let interval = |reminder: &Option<settings::ReminderContent>, default: u32| {
            let reminder = reminder.as_ref().unwrap();
            reminder
                .enabled
                .unwrap()
                .then(|| minutes(reminder.interval_minutes, default))
        };
        let focus = content.focus.as_ref().unwrap();
        Self {
            enabled: content.enabled.unwrap(),
            style: content.style.unwrap(),
            idle_pause: minutes(content.idle_pause_minutes, 5),
            snooze: minutes(content.snooze_minutes, 5),
            intervals: [
                interval(&content.water, 45),
                interval(&content.eye_break, 20),
                interval(&content.stand_up, 60),
            ],
            focus: FocusLengths {
                focus: minutes(focus.focus_minutes, 25),
                short_break: minutes(focus.short_break_minutes, 5),
                long_break: minutes(focus.long_break_minutes, 15),
                rounds_before_long_break: focus
                    .rounds_before_long_break
                    .filter(|rounds| (1..=12).contains(rounds))
                    .unwrap_or(4),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReminderEvent {
    Due(ReminderKind),
    FocusPhaseChanged(FocusPhase),
    /// The clock was read: every second while the focus timer runs, otherwise
    /// only when something may have come due.
    Tick,
}

struct GlobalReminders(Entity<Reminders>);
impl Global for GlobalReminders {}

pub struct Reminders {
    schedule: Schedule,
    settings: Option<RemindersSettings>,
    /// The pending tick, and when it fires.
    ticking: Option<(Instant, Task<()>)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReminderEvent> for Reminders {}

pub fn init(cx: &mut App) {
    let reminders = cx.new(Reminders::new);
    cx.set_global(GlobalReminders(reminders));
}

impl Reminders {
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalReminders>()
            .map(|global| global.0.clone())
    }

    fn new(cx: &mut Context<Self>) -> Self {
        let now = cx.background_executor().now();
        let settings = RemindersSettings::get_global(cx).clone();
        let mut this = Self {
            schedule: Schedule::new(settings.focus, now),
            settings: None,
            ticking: None,
            _subscriptions: vec![
                cx.observe_global::<SettingsStore>(|this, cx| this.apply_settings(cx)),
            ],
        };
        this.apply_settings(cx);
        this
    }

    pub fn settings(&self) -> Option<&RemindersSettings> {
        self.settings.as_ref()
    }

    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let settings = RemindersSettings::get_global(cx).clone();
        if self.settings.as_ref() == Some(&settings) {
            return;
        }
        let now = cx.background_executor().now();
        if settings.enabled {
            self.schedule.set_intervals(settings.intervals, now);
        } else {
            self.schedule.set_intervals([None; 3], now);
            self.schedule.stop_focus();
            self.schedule.pause_reminders(None);
        }
        self.schedule.set_focus_lengths(settings.focus);
        self.settings = Some(settings);
        self.reschedule(cx);
        cx.notify();
    }

    /// Sleeps until the schedule next needs the clock, or stops when nothing
    /// runs. Every change to the schedule calls this, so the wait never
    /// outlasts a reminder that was just started or moved.
    fn reschedule(&mut self, cx: &mut Context<Self>) {
        let now = Self::now(cx);
        let Some(wait) = self.schedule.next_tick_in(now) else {
            self.ticking = None;
            return;
        };
        let at = now + wait;
        match &self.ticking {
            // A tick already due sooner stays: re-arming on every change would
            // let a run of changes push it past `CLOCK_JUMP`, and the gap would
            // read as a slept-through machine.
            Some((armed, _)) if *armed <= at => return,
            Some(_) => {}
            None => self.schedule.resume_clock(now),
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            if let Err(error) = this.update(cx, |this, cx| {
                this.ticking = None;
                this.tick(cx);
                this.reschedule(cx);
            }) {
                log::debug!("reminders are gone, and their clock with them: {error:#}");
            }
        });
        self.ticking = Some((at, task));
    }

    /// How long the user has been away. Without a system-wide figure Zode
    /// cannot tell an absent user from one working in another application, so
    /// it counts them as present and holds nothing.
    fn idle_for(&self, cx: &App) -> Duration {
        cx.system_idle_time().unwrap_or(Duration::ZERO)
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        let idle_pause = settings.idle_pause;
        let now = cx.background_executor().now();
        let idle_for = self.idle_for(cx);
        for event in self.schedule.tick(now, idle_for, idle_pause) {
            cx.emit(match event {
                ScheduleEvent::Due(kind) => ReminderEvent::Due(kind),
                ScheduleEvent::FocusPhaseChanged(phase) => ReminderEvent::FocusPhaseChanged(phase),
            });
        }
        cx.emit(ReminderEvent::Tick);
    }

    fn now(cx: &App) -> Instant {
        cx.background_executor().now()
    }

    /// Starts a focus round. Not announced: the user who started it knows.
    pub fn start_focus(&mut self, cx: &mut Context<Self>) {
        self.schedule.start_focus(Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    pub fn pause_focus(&mut self, cx: &mut Context<Self>) {
        self.schedule.pause_focus(Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    pub fn resume_focus(&mut self, cx: &mut Context<Self>) {
        self.schedule.resume_focus(Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    /// Ends the current phase now. Not announced, like `start_focus`; only a
    /// phase that runs out on its own is.
    pub fn skip_focus_phase(&mut self, cx: &mut Context<Self>) {
        self.schedule.skip_focus_phase(Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    pub fn stop_focus(&mut self, cx: &mut Context<Self>) {
        self.schedule.stop_focus();
        self.reschedule(cx);
        cx.notify();
    }

    /// Starts a reminder that is on; it first comes due one interval from now.
    pub fn start_reminder(&mut self, kind: ReminderKind, cx: &mut Context<Self>) {
        self.schedule.start_reminder(kind, Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    /// Stops a reminder until it is started again.
    pub fn stop_reminder(&mut self, kind: ReminderKind, cx: &mut Context<Self>) {
        self.schedule.stop_reminder(kind);
        self.reschedule(cx);
        cx.notify();
    }

    pub fn snooze(&mut self, kind: ReminderKind, cx: &mut Context<Self>) {
        let Some(snooze) = self.settings.as_ref().map(|settings| settings.snooze) else {
            return;
        };
        self.schedule.snooze(kind, snooze, Self::now(cx));
        self.reschedule(cx);
        cx.notify();
    }

    /// Holds every periodic reminder for `duration`, or lifts the hold.
    pub fn pause_reminders(&mut self, duration: Option<Duration>, cx: &mut Context<Self>) {
        let until = duration.map(|duration| Self::now(cx) + duration);
        self.schedule.pause_reminders(until);
        self.reschedule(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod reminders_tests;
