//! The reminders panel: the focus timer as a ring, each reminder with its own
//! interval, and how reminders are shown. Opens under the tray icon; it never
//! takes the keyboard, so every value is changed with a button.

use std::time::Duration;

use gpui::{Context, Entity, FontWeight, IntoElement, Render, Subscription, Task, Window};
use reminders::{FocusPhase, FocusState, ReminderEvent, ReminderKind, ReminderStyle, Reminders};
use settings::{RemindersContent, update_settings_file};
use ui::{
    Button, ButtonSize, ButtonStyle, CircularProgress, Headline, HeadlineSize, Icon, IconButton,
    IconName, IconSize, Label, LabelSize, Switch, TintColor, ToggleState, prelude::*,
};

use crate::status_item::countdown;

/// Minutes a −/+ press moves an interval by.
pub const STEP_MINUTES: u32 = 5;
const MIN_MINUTES: u32 = 1;
const MAX_MINUTES: u32 = 480;
/// How long "Pause" holds every reminder.
const PAUSE_FOR: Duration = Duration::from_secs(60 * 60);
const RING_SIZE: Pixels = px(120.);
const REMINDER_RING_SIZE: Pixels = px(36.);

/// `current` moved one step up or down, kept within 1..=480. Moving down from a
/// value that is not a multiple of the step lands on the multiple below it, so
/// the values a user sees stay round.
pub fn step_minutes(current: u32, up: bool) -> u32 {
    let next = if up {
        current / STEP_MINUTES * STEP_MINUTES + STEP_MINUTES
    } else if !current.is_multiple_of(STEP_MINUTES) {
        current / STEP_MINUTES * STEP_MINUTES
    } else {
        current.saturating_sub(STEP_MINUTES)
    };
    next.clamp(MIN_MINUTES, MAX_MINUTES)
}

/// How much of the current phase has passed, from 0 to 1.
pub fn phase_progress(remaining: Duration, length: Duration) -> f32 {
    if length.is_zero() {
        return 0.;
    }
    (1. - remaining.as_secs_f32() / length.as_secs_f32()).clamp(0., 1.)
}

fn minutes_of(duration: Duration) -> u32 {
    (duration.as_secs() / 60) as u32
}

fn write(cx: &App, update: impl FnOnce(&mut RemindersContent) + Send + 'static) {
    update_settings_file(<dyn fs::Fs>::global(cx), cx, move |content, _| {
        update(content.reminders.get_or_insert_default());
    });
}

/// One −/+ press on a reminder's interval, `shown` being the value on screen.
///
/// The step is taken from the file, inside the write: writes run in order, so
/// a second press made before the first was read back still steps from it.
pub(crate) fn step_interval(kind: ReminderKind, shown: u32, up: bool, cx: &App) {
    write(cx, move |content| {
        step_field(
            &mut reminder_content(content, kind).interval_minutes,
            shown,
            up,
        )
    });
}

/// `field` moved one step, starting from `shown` when the file has no value.
fn step_field(field: &mut Option<u32>, shown: u32, up: bool) {
    *field = Some(step_minutes(field.unwrap_or(shown), up));
}

fn reminder_name(kind: ReminderKind) -> &'static str {
    match kind {
        ReminderKind::Water => "Drink water",
        ReminderKind::EyeBreak => "Eye break",
        ReminderKind::StandUp => "Stand up",
    }
}

fn reminder_icon(kind: ReminderKind) -> IconName {
    match kind {
        ReminderKind::Water => IconName::Droplet,
        ReminderKind::EyeBreak => IconName::Eye,
        ReminderKind::StandUp => IconName::PersonStanding,
    }
}

fn reminder_id(kind: ReminderKind) -> &'static str {
    match kind {
        ReminderKind::Water => "water",
        ReminderKind::EyeBreak => "eye-break",
        ReminderKind::StandUp => "stand-up",
    }
}

fn reminder_content(
    content: &mut RemindersContent,
    kind: ReminderKind,
) -> &mut settings::ReminderContent {
    match kind {
        ReminderKind::Water => content.water.get_or_insert_default(),
        ReminderKind::EyeBreak => content.eye_break.get_or_insert_default(),
        ReminderKind::StandUp => content.stand_up.get_or_insert_default(),
    }
}

/// The ring's colour: the brand while focusing, the theme's green on a break,
/// so the phase reads at a glance.
fn phase_color(phase: Option<FocusPhase>, cx: &App) -> gpui::Hsla {
    match phase {
        Some(FocusPhase::ShortBreak | FocusPhase::LongBreak) => cx.theme().status().success,
        _ => ui::brand_accent(cx),
    }
}

/// Closes the panel from inside it and runs `action`.
///
/// The action is deferred until the panel's own update is over. Run at once, and
/// with no Zode window active (the panel used from the menu bar while another
/// application is in front) it runs right here, inside the panel's update: what
/// it builds -- a settings window, a workspace -- is recorded as built in the
/// panel's window, which is gone by the time those records are flushed.
pub fn leave_panel_and_dispatch(action: Box<dyn gpui::Action>, window: &mut Window, cx: &mut App) {
    crate::tray::forget_panel(cx);
    window.remove_window();
    cx.defer(move |cx| {
        // The panel never activates Zode, so whatever the action opens would
        // land behind the application the user was in.
        cx.activate(true);
        cx.dispatch_action(action.as_ref());
    });
}

pub struct RemindersPanel {
    reminders: Option<Entity<Reminders>>,
    _subscriptions: Vec<Subscription>,
    _refresh: Option<Task<()>>,
}

/// Whether anything on the panel counts down: a running focus timer or a
/// started reminder. Only then do its rings move with the seconds.
fn counts_down(reminders: &Reminders) -> bool {
    let schedule = reminders.schedule();
    let focus_running = schedule
        .focus()
        .is_some_and(|timer| matches!(timer.state, FocusState::Running { .. }));
    focus_running
        || ReminderKind::ALL
            .iter()
            .any(|kind| schedule.is_running(*kind))
}

impl RemindersPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let reminders = Reminders::global(cx);
        let mut subscriptions = Vec::new();
        if let Some(reminders) = &reminders {
            subscriptions.push(cx.observe(reminders, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe(reminders, |_, _, event: &ReminderEvent, cx| {
                if *event != ReminderEvent::Tick {
                    cx.notify();
                }
            }));
        }
        subscriptions.push(cx.observe_global::<crate::bubble::Pending>(|_, cx| cx.notify()));
        // The reminders' own clock wakes only when something comes due; the
        // rings need the seconds while the panel is on screen.
        let refresh = reminders.as_ref().map(|reminders| {
            let reminders = reminders.downgrade();
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(reminders::TICK).await;
                    let Ok(moving) = reminders.read_with(cx, |reminders, _| counts_down(reminders))
                    else {
                        break;
                    };
                    if moving && this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })
        });
        Self {
            reminders,
            _subscriptions: subscriptions,
            _refresh: refresh,
        }
    }

    /// Closes the panel once its window, having been active, is no longer.
    pub fn close_on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    crate::tray::note_closed_from_outside(cx);
                    crate::tray::forget_panel(cx);
                    window.remove_window();
                }
            }));
    }

    fn card(&self, cx: &App) -> Div {
        v_flex()
            .p_3()
            .gap_3()
            .rounded_xl()
            .border_1()
            .border_color(crate::card_border(cx))
            .bg(crate::card_fill(cx))
    }

    /// `− 25 min +`, the panel's only way to change a number.
    fn stepper(
        id: &'static str,
        value: u32,
        on_change: impl Fn(bool, &mut App) + Clone + 'static,
    ) -> impl IntoElement {
        let down = on_change.clone();
        h_flex()
            .gap_0p5()
            .child(
                IconButton::new(SharedString::from(format!("{id}-down")), IconName::Dash)
                    .icon_size(IconSize::XSmall)
                    .size(ButtonSize::Compact)
                    .disabled(value <= MIN_MINUTES)
                    .on_click(move |_, _, cx| down(false, cx)),
            )
            .child(
                div().min_w(px(44.)).flex().justify_center().child(
                    Label::new(format!("{value} min"))
                        .size(LabelSize::Small)
                        .weight(FontWeight::MEDIUM),
                ),
            )
            .child(
                IconButton::new(SharedString::from(format!("{id}-up")), IconName::Plus)
                    .icon_size(IconSize::XSmall)
                    .size(ButtonSize::Compact)
                    .disabled(value >= MAX_MINUTES)
                    .on_click(move |_, _, cx| on_change(true, cx)),
            )
    }

    fn header(&self, enabled: bool, cx: &App) -> impl IntoElement {
        let status: SharedString = if !enabled {
            "Off".into()
        } else if self.paused(cx) {
            "Reminders paused".into()
        } else {
            "On".into()
        };
        h_flex()
            .justify_between()
            .child(
                h_flex()
                    .gap_2p5()
                    .child(
                        div()
                            .size(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .bg(ui::brand_solid(cx))
                            .child(
                                Icon::new(IconName::CountdownTimer)
                                    .size(IconSize::Medium)
                                    .color(Color::Custom(ui::brand_on_solid(cx))),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(Headline::new("Reminders").size(HeadlineSize::XSmall))
                            .child(
                                Label::new(status)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    ),
            )
            .child(
                Switch::new("reminders-switch", ToggleState::from(enabled)).on_click(
                    |state, _, cx| {
                        let on = *state == ToggleState::Selected;
                        write(cx, move |content| content.enabled = Some(on));
                    },
                ),
            )
    }

    fn paused(&self, cx: &App) -> bool {
        let now = cx.background_executor().now();
        self.reminders
            .as_ref()
            .is_some_and(|reminders| reminders.read(cx).schedule().paused_until(now).is_some())
    }

    fn focus_card(&self, reminders: &Entity<Reminders>, cx: &App) -> impl IntoElement {
        let state = reminders.read(cx);
        let settings = state.settings().cloned();
        let now = cx.background_executor().now();
        let timer = state.schedule().focus();
        let phase = timer.map(|timer| timer.phase);
        let length = match (&settings, phase) {
            (Some(settings), Some(phase)) => settings.focus.of(phase),
            (Some(settings), None) => settings.focus.focus,
            (None, _) => Duration::ZERO,
        };
        let remaining = timer.map_or(length, |timer| timer.remaining(now));
        let progress = phase_progress(remaining, length);
        let paused = matches!(
            timer.map(|timer| timer.state),
            Some(FocusState::Paused { .. })
        );
        let phase_name = match phase {
            None => "Ready",
            Some(FocusPhase::Focus) if paused => "Paused",
            Some(FocusPhase::Focus) => "Focus",
            Some(FocusPhase::ShortBreak) => "Short break",
            Some(FocusPhase::LongBreak) => "Long break",
        };
        let rounds = timer.map(|timer| timer.rounds).unwrap_or(0);
        let rounds_total = settings
            .as_ref()
            .map_or(4, |settings| settings.focus.rounds_before_long_break);
        let ring_color = phase_color(phase, cx);

        let ring = div()
            .relative()
            .size(RING_SIZE)
            .flex_none()
            .child(
                CircularProgress::new(progress, 1., RING_SIZE, cx)
                    .stroke_width(px(8.))
                    .bg_color(crate::ring_track(cx))
                    .progress_color(ring_color),
            )
            .child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .child(Headline::new(countdown(remaining)).size(HeadlineSize::Medium))
                    .child(
                        Label::new(phase_name)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            );

        let entity = reminders.clone();
        let primary = IconButton::new(
            "focus-primary",
            if timer.is_some() && !paused {
                IconName::DebugPause
            } else {
                IconName::PlayFilled
            },
        )
        .style(ButtonStyle::Brand)
        .size(ButtonSize::Large)
        .icon_size(IconSize::Medium)
        .shape(ui::IconButtonShape::Square)
        .on_click({
            let entity = entity.clone();
            let running = timer.is_some();
            move |_, _, cx| {
                entity.update(cx, |reminders, cx| match (running, paused) {
                    (false, _) => reminders.start_focus(cx),
                    (true, true) => reminders.resume_focus(cx),
                    (true, false) => reminders.pause_focus(cx),
                })
            }
        });
        let skip = IconButton::new("focus-skip", IconName::FastForward)
            .size(ButtonSize::Large)
            .icon_size(IconSize::Small)
            .disabled(timer.is_none())
            .on_click({
                let entity = entity.clone();
                move |_, _, cx| entity.update(cx, |reminders, cx| reminders.skip_focus_phase(cx))
            });
        let stop = IconButton::new("focus-stop", IconName::Stop)
            .size(ButtonSize::Large)
            .icon_size(IconSize::Small)
            .disabled(timer.is_none())
            .on_click(move |_, _, cx| entity.update(cx, |reminders, cx| reminders.stop_focus(cx)));

        let side = v_flex()
            .flex_1()
            .gap_3()
            .child(
                v_flex()
                    .child(Label::new("Focus timer").weight(FontWeight::SEMIBOLD))
                    .child(
                        Label::new(format!(
                            "Round {} of {rounds_total}",
                            rounds % rounds_total.max(1) + 1
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    ),
            )
            .child(h_flex().gap_2().child(primary).child(skip).child(stop));

        let mut card = self
            .card(cx)
            .child(h_flex().gap_4().child(ring).child(side));
        if let Some(settings) = settings {
            card = card.child(
                h_flex()
                    .pt_1()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Label::new("Focus")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(Self::stepper(
                                "focus-length",
                                minutes_of(settings.focus.focus),
                                {
                                    let shown = minutes_of(settings.focus.focus);
                                    move |up, cx| {
                                        write(cx, move |content| {
                                            step_field(
                                                &mut content
                                                    .focus
                                                    .get_or_insert_default()
                                                    .focus_minutes,
                                                shown,
                                                up,
                                            )
                                        })
                                    }
                                },
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Label::new("Break")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(Self::stepper(
                                "break-length",
                                minutes_of(settings.focus.short_break),
                                {
                                    let shown = minutes_of(settings.focus.short_break);
                                    move |up, cx| {
                                        write(cx, move |content| {
                                            step_field(
                                                &mut content
                                                    .focus
                                                    .get_or_insert_default()
                                                    .short_break_minutes,
                                                shown,
                                                up,
                                            )
                                        })
                                    }
                                },
                            )),
                    ),
            );
        }
        card
    }

    fn reminder_row(
        &self,
        kind: ReminderKind,
        reminders: &Entity<Reminders>,
        cx: &App,
    ) -> impl IntoElement {
        let state = reminders.read(cx);
        let now = cx.background_executor().now();
        let index = ReminderKind::ALL
            .iter()
            .position(|each| *each == kind)
            .unwrap_or(0);
        let interval = state
            .settings()
            .and_then(|settings| settings.intervals[index]);
        let enabled = interval.is_some();
        let next_due = state.schedule().next_due(kind).filter(|_| enabled);
        let remaining = next_due.map(|due| due.saturating_duration_since(now));
        let detail: SharedString = match remaining {
            Some(remaining) => format!("Next in {} min", remaining.as_secs().div_ceil(60)).into(),
            None if enabled => "Press start".into(),
            None => "Off".into(),
        };
        let running = remaining.is_some();
        let entity = reminders.clone();
        let start_stop = IconButton::new(
            SharedString::from(format!("{}-start", reminder_id(kind))),
            if running {
                IconName::Stop
            } else {
                IconName::PlayFilled
            },
        )
        .icon_size(IconSize::XSmall)
        .size(ButtonSize::Compact)
        .when(!running, |button| button.style(ButtonStyle::Brand))
        .on_click(move |_, _, cx| {
            entity.update(cx, |reminders, cx| {
                if running {
                    reminders.stop_reminder(kind, cx)
                } else {
                    reminders.start_reminder(kind, cx)
                }
            })
        });
        let progress = match (remaining, interval) {
            (Some(remaining), Some(interval)) => phase_progress(remaining, interval),
            _ => 0.,
        };
        let id = reminder_id(kind);
        let ring = div()
            .relative()
            .size(REMINDER_RING_SIZE)
            .flex_none()
            .child(
                CircularProgress::new(progress, 1., REMINDER_RING_SIZE, cx)
                    .stroke_width(px(3.))
                    .bg_color(crate::ring_track(cx))
                    .progress_color(ui::brand_accent(cx)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(reminder_icon(kind)).size(IconSize::Small).color(
                        if enabled {
                            Color::Selected
                        } else {
                            Color::Muted
                        },
                    )),
            );
        h_flex()
            .gap_2p5()
            .child(ring)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(reminder_name(kind)).weight(FontWeight::MEDIUM))
                    .child(
                        Label::new(detail)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .when(enabled, |row| row.child(start_stop))
            .when_some(interval, |row, interval| {
                row.child(Self::stepper(id, minutes_of(interval), move |up, cx| {
                    step_interval(kind, minutes_of(interval), up, cx)
                }))
            })
            .child(
                Switch::new(
                    SharedString::from(format!("{id}-switch")),
                    ToggleState::from(enabled),
                )
                .on_click(move |state, _, cx| {
                    let on = *state == ToggleState::Selected;
                    write(cx, move |content| {
                        reminder_content(content, kind).enabled = Some(on)
                    });
                }),
            )
    }

    fn reminders_card(&self, reminders: &Entity<Reminders>, cx: &App) -> impl IntoElement {
        let paused = self.paused(cx);
        let entity = reminders.clone();
        let mut rows = v_flex().gap_3();
        for kind in ReminderKind::ALL {
            rows = rows.child(self.reminder_row(kind, reminders, cx));
        }
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .px_1()
                    .justify_between()
                    .child(
                        Label::new("REMINDERS")
                            .size(LabelSize::XSmall)
                            .weight(FontWeight::SEMIBOLD)
                            .color(Color::Muted),
                    )
                    .child(
                        Button::new(
                            "pause-reminders",
                            if paused { "Resume" } else { "Pause 1 h" },
                        )
                        .label_size(LabelSize::XSmall)
                        .size(ButtonSize::Compact)
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |reminders, cx| {
                                reminders.pause_reminders((!paused).then_some(PAUSE_FOR), cx)
                            })
                        }),
                    ),
            )
            .child(self.card(cx).child(rows))
    }

    fn style_row(&self, style: ReminderStyle, cx: &App) -> impl IntoElement {
        let option =
            |id: &'static str, label: &'static str, icon: IconName, value: ReminderStyle| {
                Button::new(id, label)
                    .start_icon(Icon::new(icon).size(IconSize::XSmall))
                    .size(ButtonSize::Medium)
                    .label_size(LabelSize::Small)
                    .full_width()
                    .toggle_state(style == value)
                    .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                    .on_click(move |_, _, cx| write(cx, move |content| content.style = Some(value)))
            };
        v_flex()
            .gap_2()
            .child(
                div().ml_1().child(
                    Label::new("SHOW REMINDERS AS")
                        .size(LabelSize::XSmall)
                        .weight(FontWeight::SEMIBOLD)
                        .color(Color::Muted),
                ),
            )
            .child(
                h_flex()
                    .p_1()
                    .gap_1()
                    .rounded_lg()
                    .bg(crate::card_fill(cx))
                    .border_1()
                    .border_color(crate::card_border(cx))
                    .child(div().flex_1().child(option(
                        "style-bubble",
                        "Bubble",
                        IconName::ToolNotification,
                        ReminderStyle::Bubble,
                    )))
                    .child(div().flex_1().child(option(
                        "style-system",
                        "Notification",
                        IconName::Envelope,
                        ReminderStyle::System,
                    ))),
            )
    }

    fn footer(&self, cx: &App) -> impl IntoElement {
        let action = |id: &'static str, label: &'static str, icon: IconName| {
            Button::new(id, label)
                .start_icon(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
                .label_size(LabelSize::Small)
                .size(ButtonSize::Medium)
        };
        h_flex()
            .pt_3()
            .border_t_1()
            .border_color(crate::card_border(cx))
            .justify_between()
            .child(
                action("open-settings", "Settings", IconName::Settings).on_click(
                    |_, window, cx| {
                        leave_panel_and_dispatch(
                            Box::new(zode_actions::OpenSettingsAt {
                                path: "reminders.enabled".into(),
                            }),
                            window,
                            cx,
                        )
                    },
                ),
            )
            .child(
                action("new-window", "New Window", IconName::OpenNewWindow).on_click(
                    |_, window, cx| {
                        leave_panel_and_dispatch(Box::new(workspace::NewWindow), window, cx)
                    },
                ),
            )
            .child(
                action("quit", "Quit", IconName::Power)
                    .on_click(|_, _, cx| cx.dispatch_action(&zode_actions::Quit)),
            )
    }
}

impl Render for RemindersPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let backdrop = crate::apply_window_effects(window, cx);
        let settings = self
            .reminders
            .as_ref()
            .and_then(|reminders| reminders.read(cx).settings().cloned());
        let enabled = settings.as_ref().is_some_and(|settings| settings.enabled);
        let style = settings
            .as_ref()
            .map(|settings| settings.style)
            .unwrap_or_default();

        let mut body = v_flex().gap_4();
        match (&self.reminders, enabled) {
            (Some(reminders), true) => {
                body = body
                    .child(self.focus_card(reminders, cx))
                    .child(self.reminders_card(reminders, cx))
                    .child(self.style_row(style, cx));
            }
            _ => {
                body = body.child(
                    self.card(cx).child(
                        Label::new(
                            "Turn on to get reminders to drink water, rest your eyes and stand \
                             up, and a focus timer.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
                );
            }
        }

        v_flex()
            .id("reminders-panel")
            .size_full()
            .gap_4()
            .p_4()
            .rounded_xl()
            .border_1()
            .border_color(crate::surface_border(cx))
            .bg(crate::surface_fill(cx))
            .text_color(cx.theme().colors().text)
            .overflow_y_scroll()
            .child(self.header(enabled, cx))
            .children(
                crate::pending(cx)
                    .map(|event| self.card(cx).child(crate::bubble::notice(event, cx))),
            )
            .child(body)
            .child(self.footer(cx))
            // Over the content, as in the editor's windows, where the platform
            // cannot draw the image behind it.
            .children(backdrop)
    }
}
