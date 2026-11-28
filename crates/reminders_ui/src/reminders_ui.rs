//! Shows what `reminders` says is due: an OS notification, and a countdown on
//! the status bar.

mod bubble;
mod panel;
mod status_item;
mod tray;

use gpui::{App, Entity, Global, Notification, SharedString, Subscription};
use reminders::{FocusPhase, ReminderEvent, ReminderKind, Reminders, RemindersSettings};
use settings::Settings as _;

pub use bubble::{Delivery, acknowledge, bubble_is_open, delivery, pending, snooze};
pub use panel::RemindersPanel;
pub use status_item::RemindersStatusItem;
pub use tray::{TrayIconSetting, close_panel, panel_bounds, panel_is_open, toggle_panel};

/// The window background for the panel and the bubble: the app's own
/// `window_background` setting, as every Zode window has it, except that an
/// opaque one becomes transparent so the panel's rounded corners stay rounded --
/// the panel then paints the opaque surface itself (see [`surface_fill`]).
pub fn window_background(cx: &App) -> gpui::WindowBackgroundAppearance {
    use gpui::WindowBackgroundAppearance as Background;
    use ui::ActiveTheme as _;
    match cx.theme().window_background_appearance() {
        Background::Opaque => Background::Transparent,
        other => other,
    }
}

/// Gives the panel's or the bubble's `window` the app's window effects, the way
/// every editor window has them (`multi_workspace` does the same): the
/// background the setting asks for, the background image behind the content
/// where the platform can draw it there, and the whole window faded to the
/// app's surface opacity. Returns the image to lay over the content where the
/// platform cannot draw it behind. Cheap when nothing changed.
pub(crate) fn apply_window_effects(
    window: &mut gpui::Window,
    cx: &App,
) -> Option<gpui::AnyElement> {
    use settings::Settings as _;
    let wanted = window_background(cx);
    if window.background_appearance() != wanted {
        window.set_background_appearance(wanted);
    }
    let settings = theme_settings::ThemeSettings::get_global(cx);
    let background_opacity = settings.background_opacity();
    let image_is_set = settings.window_background_image.is_some();
    let content_opacity = if window.supports_vibrancy() {
        window.set_backdrop_image(workspace::window_backdrop::backdrop_image(cx));
        background_opacity
    } else if image_is_set {
        1.0
    } else {
        background_opacity
    };
    if window.content_opacity() != content_opacity {
        window.set_content_opacity(content_opacity);
    }
    if window.supports_vibrancy() {
        None
    } else {
        workspace::window_backdrop::render_window_backdrop(cx)
    }
}

/// The fill under the panel's and the bubble's content: the theme's surface,
/// solid. The window is faded as a whole (see [`apply_window_effects`]), as the
/// editor's windows are; a tint per surface would cover the material and the
/// background image instead of letting them through.
pub fn surface_fill(cx: &App) -> gpui::Hsla {
    use ui::ActiveTheme as _;
    cx.theme().colors().elevated_surface_background
}

/// The fill of a card on that surface: a faint lift of the material.
pub(crate) fn card_fill(cx: &App) -> gpui::Hsla {
    use ui::ActiveTheme as _;
    if cx.theme().appearance().is_light() {
        gpui::black().opacity(0.03)
    } else {
        gpui::white().opacity(0.045)
    }
}

/// A card's hairline edge, the one detail that keeps glass cards apart.
pub(crate) fn card_border(cx: &App) -> gpui::Hsla {
    use ui::ActiveTheme as _;
    if cx.theme().appearance().is_light() {
        gpui::black().opacity(0.07)
    } else {
        gpui::white().opacity(0.08)
    }
}

/// The unfilled part of the focus ring.
pub(crate) fn ring_track(cx: &App) -> gpui::Hsla {
    use ui::ActiveTheme as _;
    if cx.theme().appearance().is_light() {
        gpui::black().opacity(0.08)
    } else {
        gpui::white().opacity(0.10)
    }
}

/// A hairline border around the surface.
pub(crate) fn surface_border(cx: &App) -> gpui::Hsla {
    card_border(cx)
}

/// Title and body for a reminder that came due.
pub fn reminder_text(kind: ReminderKind) -> (&'static str, &'static str) {
    match kind {
        ReminderKind::Water => ("Time for some water", "Drink a glass of water."),
        ReminderKind::EyeBreak => (
            "Rest your eyes",
            "Look at something 6 m (20 ft) away for 20 seconds.",
        ),
        ReminderKind::StandUp => (
            "Time to stand up",
            "Stand up, stretch and move for a minute.",
        ),
    }
}

/// Title and body for the start of a focus phase.
pub fn focus_text(phase: FocusPhase, settings: &RemindersSettings) -> (String, String) {
    let minutes = |duration: std::time::Duration| duration.as_secs() / 60;
    match phase {
        FocusPhase::Focus => (
            "Back to focus".into(),
            format!("Focus for {} minutes.", minutes(settings.focus.focus)),
        ),
        FocusPhase::ShortBreak => (
            "Take a short break".into(),
            format!(
                "{} minutes. Step away from the screen.",
                minutes(settings.focus.short_break)
            ),
        ),
        FocusPhase::LongBreak => (
            "Take a long break".into(),
            format!(
                "{} minutes. You have earned it.",
                minutes(settings.focus.long_break)
            ),
        ),
    }
}

/// The notification id for a reminder or the focus timer: one per kind, so a
/// stale banner is replaced instead of stacked.
pub fn notification_id(event: ReminderEvent) -> Option<&'static str> {
    Some(match event {
        ReminderEvent::Due(ReminderKind::Water) => "reminders.water",
        ReminderEvent::Due(ReminderKind::EyeBreak) => "reminders.eye_break",
        ReminderEvent::Due(ReminderKind::StandUp) => "reminders.stand_up",
        ReminderEvent::FocusPhaseChanged(_) => "reminders.focus",
        ReminderEvent::Tick => return None,
    })
}

struct GlobalRemindersUi {
    _subscription: Subscription,
}
impl Global for GlobalRemindersUi {}

pub fn init(cx: &mut App) {
    let Some(reminders) = Reminders::global(cx) else {
        return;
    };
    let subscription = cx.subscribe(&reminders, |reminders, event: &ReminderEvent, cx| {
        deliver(&reminders, *event, cx);
    });
    cx.set_global(GlobalRemindersUi {
        _subscription: subscription,
    });
    tray::init(cx);
}

/// Title and body for any event that is shown to the user.
pub fn event_text(event: ReminderEvent, cx: &App) -> (SharedString, SharedString) {
    match event {
        ReminderEvent::Due(kind) => {
            let (title, body) = reminder_text(kind);
            (title.into(), body.into())
        }
        ReminderEvent::FocusPhaseChanged(phase) => {
            let settings = Reminders::global(cx)
                .and_then(|reminders| reminders.read(cx).settings().cloned())
                .unwrap_or_else(|| RemindersSettings::get_global(cx).clone());
            let (title, body) = focus_text(phase, &settings);
            (title.into(), body.into())
        }
        ReminderEvent::Tick => ("".into(), "".into()),
    }
}

fn deliver(reminders: &Entity<Reminders>, event: ReminderEvent, cx: &mut App) {
    let Some(id) = notification_id(event) else {
        return;
    };
    let style = reminders
        .read(cx)
        .settings()
        .map(|settings| settings.style)
        .unwrap_or_default();
    let anchor = cx.tray_icon_anchor();
    let route = delivery(
        style,
        panel_is_open(cx),
        cx.tray_icon_visible(),
        anchor.is_some(),
    );
    bubble::set_pending(event, cx);
    match (route, anchor) {
        (Delivery::Panel, _) => {}
        // An open bubble already shows the oldest pending reminder; this one
        // waits its turn behind it.
        (Delivery::Bubble, Some(_)) if bubble::bubble_is_open(cx) => {}
        (Delivery::Bubble, Some(anchor)) => bubble::show_bubble(anchor, cx),
        (Delivery::Bubble, None) | (Delivery::System, _) => {
            let (title, body) = event_text(event, cx);
            cx.post_notification(Notification {
                id: id.into(),
                title,
                body,
                awaits_user: true,
            });
        }
    }
}

#[cfg(test)]
mod reminders_ui_tests;
