//! The status bar entry: the focus countdown while a focus timer runs, and an
//! icon to reach the reminders otherwise. Draws nothing while reminders are off.

use std::time::Duration;

use gpui::{Context, Entity, IntoElement, Render, SharedString, Subscription, Window, div};
use reminders::{FocusPhase, FocusState, ReminderEvent, Reminders};
use ui::{Button, Icon, IconName, IconSize, LabelSize, Tooltip, prelude::*};
use workspace::{ItemHandle, StatusItemView};

pub struct RemindersStatusItem {
    reminders: Option<Entity<Reminders>>,
    /// What the label last said, so a tick that changes nothing visible does
    /// not re-render the bar.
    label: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// `mm:ss`, or `h:mm:ss` from an hour up.
pub fn countdown(remaining: Duration) -> String {
    let seconds = remaining.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

impl RemindersStatusItem {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let reminders = Reminders::global(cx);
        let mut subscriptions = Vec::new();
        if let Some(reminders) = &reminders {
            subscriptions.push(cx.observe(reminders, |this, _, cx| this.refresh(cx)));
            subscriptions.push(
                cx.subscribe(reminders, |this, _, event: &ReminderEvent, cx| {
                    if matches!(
                        event,
                        ReminderEvent::Tick | ReminderEvent::FocusPhaseChanged(_)
                    ) {
                        this.refresh(cx);
                    }
                }),
            );
        }
        subscriptions.push(cx.observe_global::<crate::bubble::Pending>(|_, cx| cx.notify()));
        let mut this = Self {
            reminders,
            label: None,
            _subscriptions: subscriptions,
        };
        this.label = this.compute_label(cx);
        this
    }

    fn enabled(&self, cx: &App) -> bool {
        self.reminders.as_ref().is_some_and(|reminders| {
            reminders
                .read(cx)
                .settings()
                .is_some_and(|settings| settings.enabled)
        })
    }

    /// The countdown, with the phase it counts, while a focus timer exists.
    pub fn compute_label(&self, cx: &App) -> Option<SharedString> {
        let reminders = self.reminders.as_ref()?.read(cx);
        let timer = reminders.schedule().focus()?;
        let now = cx.background_executor().now();
        let phase = match timer.phase {
            FocusPhase::Focus => "Focus",
            FocusPhase::ShortBreak | FocusPhase::LongBreak => "Break",
        };
        let paused = matches!(timer.state, FocusState::Paused { .. });
        let mut label = format!("{phase} {}", countdown(timer.remaining(now)));
        if paused {
            label.push_str(" (paused)");
        }
        Some(label.into())
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let label = self.compute_label(cx);
        if label != self.label {
            self.label = label;
            cx.notify();
        }
    }
}

impl Render for RemindersStatusItem {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.enabled(cx) {
            return div().into_any_element();
        }
        let label = self.label.clone().unwrap_or_default();
        // A reminder waiting to be acknowledged colors the icon, so one whose
        // bubble hid itself is still in sight.
        let waiting = crate::pending(cx).is_some();
        let button = Button::new("reminders-status", label)
            .start_icon(
                Icon::new(IconName::CountdownTimer)
                    .size(IconSize::Small)
                    .color(if waiting {
                        Color::Accent
                    } else {
                        Color::Default
                    }),
            )
            .on_click(|_, _, cx| crate::toggle_panel(cx.tray_icon_anchor(), cx));
        div()
            .child(
                button
                    .label_size(LabelSize::Small)
                    .tooltip(Tooltip::text("Reminders")),
            )
            .into_any_element()
    }
}

impl StatusItemView for RemindersStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}
