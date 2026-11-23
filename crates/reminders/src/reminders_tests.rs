use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{TestAppContext, UpdateGlobal as _};
use settings::{Settings as _, SettingsStore};

use crate::{
    FocusLengths, FocusPhase, ReminderEvent, ReminderKind, Reminders, RemindersSettings, Schedule,
    ScheduleEvent,
};

const MINUTE: Duration = Duration::from_secs(60);

fn lengths() -> FocusLengths {
    FocusLengths {
        focus: 25 * MINUTE,
        short_break: 5 * MINUTE,
        long_break: 15 * MINUTE,
        rounds_before_long_break: 2,
    }
}

/// Turns reminders on and starts every one that is on, at `now`.
fn set_and_start(schedule: &mut Schedule, intervals: [Option<Duration>; 3], now: Instant) {
    schedule.set_intervals(intervals, now);
    for kind in ReminderKind::ALL {
        schedule.start_reminder(kind, now);
    }
}

/// Ticks a schedule every second from `start` for `total`, user present.
fn run(schedule: &mut Schedule, start: Instant, total: Duration) -> Vec<(Duration, ScheduleEvent)> {
    let mut events = Vec::new();
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        elapsed += Duration::from_secs(1);
        for event in schedule.tick(start + elapsed, Duration::ZERO, 5 * MINUTE) {
            events.push((elapsed, event));
        }
    }
    events
}

#[test]
fn each_reminder_fires_at_its_own_interval() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(
        &mut schedule,
        [Some(10 * MINUTE), Some(3 * MINUTE), None],
        start,
    );

    let events = run(&mut schedule, start, 10 * MINUTE);
    let water: Vec<_> = events
        .iter()
        .filter(|(_, event)| *event == ScheduleEvent::Due(ReminderKind::Water))
        .map(|(at, _)| *at)
        .collect();
    let eyes: Vec<_> = events
        .iter()
        .filter(|(_, event)| *event == ScheduleEvent::Due(ReminderKind::EyeBreak))
        .map(|(at, _)| *at)
        .collect();
    assert_eq!(water, [10 * MINUTE]);
    assert_eq!(eyes, [3 * MINUTE, 6 * MINUTE, 9 * MINUTE]);
    assert!(
        !events
            .iter()
            .any(|(_, event)| *event == ScheduleEvent::Due(ReminderKind::StandUp)),
        "an interval of None is off"
    );
}

#[test]
fn changing_one_interval_restarts_only_that_reminder() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(
        &mut schedule,
        [Some(10 * MINUTE), Some(20 * MINUTE), None],
        start,
    );
    let later = start + 4 * MINUTE;
    schedule.set_intervals([Some(10 * MINUTE), Some(30 * MINUTE), None], later);

    assert_eq!(
        schedule.next_due(ReminderKind::Water),
        Some(start + 10 * MINUTE)
    );
    assert_eq!(
        schedule.next_due(ReminderKind::EyeBreak),
        Some(later + 30 * MINUTE)
    );
}

#[test]
fn a_reminder_due_while_away_waits_for_the_user() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(&mut schedule, [Some(MINUTE), None, None], start);

    let due = start + MINUTE;
    assert!(schedule.tick(due, 10 * MINUTE, 5 * MINUTE).is_empty());
    assert!(
        schedule
            .tick(due + Duration::from_secs(1), 10 * MINUTE, 5 * MINUTE)
            .is_empty()
    );
    assert_eq!(
        schedule.tick(due + Duration::from_secs(2), Duration::ZERO, 5 * MINUTE),
        [ScheduleEvent::Due(ReminderKind::Water)],
        "it fires once the user is back"
    );
}

#[test]
fn a_clock_jump_reschedules_instead_of_firing_a_backlog() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(
        &mut schedule,
        [Some(10 * MINUTE), Some(20 * MINUTE), None],
        start,
    );
    schedule.start_focus(start);

    let woke = start + 120 * MINUTE;
    assert!(schedule.tick(woke, Duration::ZERO, 5 * MINUTE).is_empty());
    assert_eq!(
        schedule.next_due(ReminderKind::Water),
        Some(woke + 10 * MINUTE)
    );
    assert_eq!(
        schedule.next_due(ReminderKind::EyeBreak),
        Some(woke + 20 * MINUTE)
    );
}

#[test]
fn paused_reminders_do_not_fire_until_the_pause_ends() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(&mut schedule, [Some(MINUTE), None, None], start);
    schedule.pause_reminders(Some(start + 3 * MINUTE));

    let events = run(&mut schedule, start, 4 * MINUTE);
    assert_eq!(
        events,
        [
            (3 * MINUTE, ScheduleEvent::Due(ReminderKind::Water)),
            (4 * MINUTE, ScheduleEvent::Due(ReminderKind::Water)),
        ],
        "held while paused, fired as the pause ends, then on interval again"
    );
}

#[test]
fn snooze_moves_the_due_time_by_the_snooze_length() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    set_and_start(&mut schedule, [Some(45 * MINUTE), None, None], start);
    let now = start + 45 * MINUTE;
    schedule.snooze(ReminderKind::Water, 5 * MINUTE, now);
    assert_eq!(
        schedule.next_due(ReminderKind::Water),
        Some(now + 5 * MINUTE)
    );
}

#[test]
fn focus_cycles_and_takes_a_long_break_after_the_set_rounds() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    schedule.start_focus(start);

    let phases: Vec<_> = run(&mut schedule, start, 110 * MINUTE)
        .into_iter()
        .map(|(at, event)| (at / 60, event))
        .collect();
    let minute = |minutes: u64| Duration::from_secs(minutes);
    assert_eq!(
        phases,
        [
            (
                minute(25),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::ShortBreak)
            ),
            (
                minute(30),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::Focus)
            ),
            (
                minute(55),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::LongBreak)
            ),
            (
                minute(70),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::Focus)
            ),
            (
                minute(95),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::ShortBreak)
            ),
            (
                minute(100),
                ScheduleEvent::FocusPhaseChanged(FocusPhase::Focus)
            ),
        ]
    );
}

#[test]
fn a_paused_focus_round_keeps_its_remaining_time() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    schedule.start_focus(start);
    schedule.pause_focus(start + 10 * MINUTE);

    let resumed = start + 60 * MINUTE;
    assert!(
        schedule
            .tick(resumed, Duration::ZERO, 5 * MINUTE)
            .is_empty()
    );
    schedule.resume_focus(resumed);
    assert_eq!(
        schedule.focus().map(|timer| timer.remaining(resumed)),
        Some(15 * MINUTE)
    );
}

#[test]
fn settings_outside_the_allowed_range_use_the_defaults() {
    let mut content = settings::SettingsContent::default();
    content.reminders = Some(settings::RemindersContent {
        enabled: Some(true),
        style: Some(settings::ReminderStyle::Bubble),
        idle_pause_minutes: Some(0),
        snooze_minutes: Some(5),
        water: Some(settings::ReminderContent {
            enabled: Some(true),
            interval_minutes: Some(10_000),
        }),
        eye_break: Some(settings::ReminderContent {
            enabled: Some(false),
            interval_minutes: Some(20),
        }),
        stand_up: Some(settings::ReminderContent {
            enabled: Some(true),
            interval_minutes: Some(90),
        }),
        focus: Some(settings::FocusTimerContent {
            focus_minutes: Some(50),
            short_break_minutes: Some(10),
            long_break_minutes: Some(30),
            rounds_before_long_break: Some(0),
        }),
    });
    let settings = RemindersSettings::from_settings(&content);
    assert_eq!(settings.idle_pause, 5 * MINUTE);
    assert_eq!(
        settings.intervals,
        [Some(45 * MINUTE), None, Some(90 * MINUTE)]
    );
    assert_eq!(settings.focus.focus, 50 * MINUTE);
    assert_eq!(settings.focus.rounds_before_long_break, 4);
}

fn init(cx: &mut TestAppContext) -> gpui::Entity<Reminders> {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        RemindersSettings::register(cx);
        crate::init(cx);
        Reminders::global(cx).unwrap()
    })
}

fn update_reminders(cx: &mut TestAppContext, f: impl FnOnce(&mut settings::RemindersContent)) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| f(content.reminders.get_or_insert_default()));
        });
    });
}

fn record(
    reminders: &gpui::Entity<Reminders>,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<Vec<ReminderEvent>>>, gpui::Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(reminders, move |_, event: &ReminderEvent, _| {
            if *event != ReminderEvent::Tick {
                events.borrow_mut().push(*event);
            }
        })
    });
    (events, subscription)
}

#[gpui::test]
async fn reminders_stay_silent_while_switched_off(cx: &mut TestAppContext) {
    let reminders = init(cx);
    let (events, _subscription) = record(&reminders, cx);
    cx.set_system_idle_time(Some(Duration::ZERO));

    cx.executor().advance_clock(120 * MINUTE);
    cx.run_until_parked();
    assert!(events.borrow().is_empty());
}

#[gpui::test]
async fn a_user_set_interval_drives_the_reminder(cx: &mut TestAppContext) {
    let reminders = init(cx);
    let (events, _subscription) = record(&reminders, cx);
    cx.set_system_idle_time(Some(Duration::ZERO));
    update_reminders(cx, |content| {
        content.enabled = Some(true);
        content.water.get_or_insert_default().interval_minutes = Some(2);
        content.eye_break.get_or_insert_default().enabled = Some(false);
        content.stand_up.get_or_insert_default().enabled = Some(false);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.start_reminder(ReminderKind::Water, cx)
    });

    for _ in 0..(4 * 60) {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
    assert_eq!(
        *events.borrow(),
        [
            ReminderEvent::Due(ReminderKind::Water),
            ReminderEvent::Due(ReminderKind::Water)
        ]
    );
}

#[gpui::test]
async fn system_idle_time_holds_a_due_reminder(cx: &mut TestAppContext) {
    let reminders = init(cx);
    let (events, _subscription) = record(&reminders, cx);
    cx.set_system_idle_time(Some(30 * MINUTE));
    update_reminders(cx, |content| {
        content.enabled = Some(true);
        content.water.get_or_insert_default().interval_minutes = Some(1);
        content.eye_break.get_or_insert_default().enabled = Some(false);
        content.stand_up.get_or_insert_default().enabled = Some(false);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.start_reminder(ReminderKind::Water, cx)
    });

    for _ in 0..(3 * 60) {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
    assert!(events.borrow().is_empty());

    // A held reminder looks for the user every `HELD_POLL`, not every second.
    cx.set_system_idle_time(Some(Duration::ZERO));
    cx.executor().advance_clock(crate::HELD_POLL);
    cx.run_until_parked();
    assert_eq!(*events.borrow(), [ReminderEvent::Due(ReminderKind::Water)]);
}

#[gpui::test]
async fn switching_off_stops_the_focus_timer(cx: &mut TestAppContext) {
    let reminders = init(cx);
    update_reminders(cx, |content| content.enabled = Some(true));
    reminders.update(cx, |reminders, cx| reminders.start_focus(cx));
    assert!(reminders.read_with(cx, |reminders, _| reminders.schedule().focus().is_some()));

    update_reminders(cx, |content| content.enabled = Some(false));
    assert!(reminders.read_with(cx, |reminders, _| reminders.schedule().focus().is_none()));
}

#[test]
fn a_reminder_that_is_on_waits_for_start() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    schedule.set_intervals([Some(MINUTE), None, None], start);

    assert!(run(&mut schedule, start, 10 * MINUTE).is_empty());
    assert_eq!(schedule.next_due(ReminderKind::Water), None);

    let started = start + 10 * MINUTE;
    schedule.start_reminder(ReminderKind::Water, started);
    assert_eq!(
        schedule.next_due(ReminderKind::Water),
        Some(started + MINUTE)
    );
    assert_eq!(
        run(&mut schedule, started, MINUTE),
        [(MINUTE, ScheduleEvent::Due(ReminderKind::Water))]
    );

    schedule.stop_reminder(ReminderKind::Water);
    assert_eq!(schedule.next_due(ReminderKind::Water), None);
    assert!(run(&mut schedule, started + MINUTE, 5 * MINUTE).is_empty());
}

fn count_ticks(
    reminders: &gpui::Entity<Reminders>,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<usize>>, gpui::Subscription) {
    let ticks = Rc::new(RefCell::new(0));
    let subscription = cx.update(|cx| {
        let ticks = ticks.clone();
        cx.subscribe(reminders, move |_, event: &ReminderEvent, _| {
            if *event == ReminderEvent::Tick {
                *ticks.borrow_mut() += 1;
            }
        })
    });
    (ticks, subscription)
}

fn advance_seconds(cx: &mut TestAppContext, seconds: u64) {
    for _ in 0..seconds {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
}

/// Switched on with nothing started, there is nothing to count down, so Zode
/// is not woken at all.
#[gpui::test]
async fn nothing_started_means_no_wakeups(cx: &mut TestAppContext) {
    let reminders = init(cx);
    cx.set_system_idle_time(Some(Duration::ZERO));
    update_reminders(cx, |content| content.enabled = Some(true));
    let (ticks, _subscription) = count_ticks(&reminders, cx);

    advance_seconds(cx, 30);
    assert_eq!(*ticks.borrow(), 0);
}

/// A running reminder needs the clock read only when it comes due; the wait is
/// capped below `CLOCK_JUMP` so a slept-through gap is still told apart.
#[gpui::test]
async fn a_running_reminder_wakes_zode_about_once_a_minute(cx: &mut TestAppContext) {
    let reminders = init(cx);
    cx.set_system_idle_time(Some(Duration::ZERO));
    let (events, _events) = record(&reminders, cx);
    update_reminders(cx, |content| {
        content.enabled = Some(true);
        content.water.get_or_insert_default().interval_minutes = Some(10);
        content.eye_break.get_or_insert_default().enabled = Some(false);
        content.stand_up.get_or_insert_default().enabled = Some(false);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.start_reminder(ReminderKind::Water, cx)
    });
    let (ticks, _ticks) = count_ticks(&reminders, cx);

    advance_seconds(cx, 10 * 60);
    assert!(
        *ticks.borrow() <= 11,
        "{} wakeups in ten minutes",
        ticks.borrow()
    );
    assert_eq!(*events.borrow(), [ReminderEvent::Due(ReminderKind::Water)]);
}

/// The focus countdown on the status bar moves every second, so a running
/// focus timer still ticks every second.
#[gpui::test]
async fn a_running_focus_timer_ticks_every_second(cx: &mut TestAppContext) {
    let reminders = init(cx);
    update_reminders(cx, |content| content.enabled = Some(true));
    reminders.update(cx, |reminders, cx| reminders.start_focus(cx));
    let (ticks, _subscription) = count_ticks(&reminders, cx);

    advance_seconds(cx, 10);
    assert_eq!(*ticks.borrow(), 10);

    reminders.update(cx, |reminders, cx| reminders.pause_focus(cx));
    advance_seconds(cx, 10);
    assert_eq!(*ticks.borrow(), 10, "a paused timer has nothing to count");
}

/// Where the system cannot say how long the user has been away, Zode cannot
/// either -- a user working in another application never presses a key in
/// Zode -- so nothing is held for being idle.
#[gpui::test]
async fn without_a_system_idle_time_nothing_is_held(cx: &mut TestAppContext) {
    let reminders = init(cx);
    cx.set_system_idle_time(None);
    let (events, _subscription) = record(&reminders, cx);
    update_reminders(cx, |content| {
        content.enabled = Some(true);
        content.water.get_or_insert_default().interval_minutes = Some(10);
        content.eye_break.get_or_insert_default().enabled = Some(false);
        content.stand_up.get_or_insert_default().enabled = Some(false);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.start_reminder(ReminderKind::Water, cx)
    });

    advance_seconds(cx, 10 * 60);
    assert_eq!(*events.borrow(), [ReminderEvent::Due(ReminderKind::Water)]);
}

#[test]
fn the_next_tick_waits_for_what_comes_due_first() {
    let start = Instant::now();
    let mut schedule = Schedule::new(lengths(), start);
    assert_eq!(schedule.next_tick_in(start), None, "nothing is on");

    schedule.set_intervals([Some(45 * MINUTE), Some(2 * MINUTE), None], start);
    assert_eq!(schedule.next_tick_in(start), None, "on, but not started");

    schedule.start_reminder(ReminderKind::EyeBreak, start);
    assert_eq!(schedule.next_tick_in(start), Some(crate::MAX_WAIT));
    let almost = start + 2 * MINUTE - Duration::from_secs(10);
    assert_eq!(schedule.next_tick_in(almost), Some(Duration::from_secs(10)));

    let overdue = start + 3 * MINUTE;
    assert_eq!(
        schedule.next_tick_in(overdue),
        Some(crate::HELD_POLL),
        "held for an absent user"
    );
    schedule.pause_reminders(Some(overdue + 20 * Duration::from_secs(1)));
    assert_eq!(
        schedule.next_tick_in(overdue),
        Some(Duration::from_secs(20)),
        "held until the pause ends"
    );

    schedule.start_focus(overdue);
    assert_eq!(schedule.next_tick_in(overdue), Some(crate::TICK));
    schedule.pause_focus(overdue);
    schedule.stop_reminder(ReminderKind::EyeBreak);
    assert_eq!(
        schedule.next_tick_in(overdue),
        None,
        "a paused focus timer counts nothing"
    );
}

/// Changes made less than a minute apart -- someone adjusting the panel --
/// must not keep pushing the next tick back: a gap that outgrows `CLOCK_JUMP`
/// would read as a slept-through machine and swallow the reminder due then.
#[gpui::test]
async fn frequent_changes_do_not_pass_for_a_clock_jump(cx: &mut TestAppContext) {
    let reminders = init(cx);
    cx.set_system_idle_time(Some(Duration::ZERO));
    let (events, _subscription) = record(&reminders, cx);
    update_reminders(cx, |content| {
        content.enabled = Some(true);
        content.water.get_or_insert_default().interval_minutes = Some(3);
        content.eye_break.get_or_insert_default().enabled = Some(false);
        content.stand_up.get_or_insert_default().enabled = Some(false);
    });
    reminders.update(cx, |reminders, cx| {
        reminders.start_reminder(ReminderKind::Water, cx)
    });

    for _ in 0..6 {
        advance_seconds(cx, 30);
        reminders.update(cx, |reminders, cx| reminders.pause_reminders(None, cx));
    }
    assert_eq!(*events.borrow(), [ReminderEvent::Due(ReminderKind::Water)]);
}
