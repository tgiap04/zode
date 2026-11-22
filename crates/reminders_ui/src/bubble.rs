//! A reminder shown as a small bubble under the tray icon, and the notice the
//! bubble and the panel share while a reminder waits to be acknowledged.

use std::collections::VecDeque;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Global, IntoElement, Render, Size, Task, TrayIconAnchor, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, px,
};
use reminders::{ReminderEvent, Reminders};
use ui::{Button, Label, LabelSize, prelude::*};

use crate::tray::panel_bounds;

pub const BUBBLE_SIZE: Size<Pixels> = Size {
    width: px(320.),
    height: px(104.),
};

/// How long a bubble stays before it hides itself. The reminder stays pending,
/// and the status bar keeps showing it.
pub const BUBBLE_LIFETIME: Duration = Duration::from_secs(15);

/// Where a reminder that came due is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Inside the panel, which is already open.
    Panel,
    /// In a bubble under the tray icon.
    Bubble,
    /// As an OS notification.
    System,
}

/// The one rule for where a reminder goes. A bubble needs the icon to be seen
/// and placed: behind a full-screen app or with an auto-hidden menu bar, a
/// bubble would point at nothing, so the OS notification takes over.
pub fn delivery(
    style: reminders::ReminderStyle,
    panel_open: bool,
    tray_visible: bool,
    anchor_known: bool,
) -> Delivery {
    match style {
        reminders::ReminderStyle::System => Delivery::System,
        reminders::ReminderStyle::Bubble if panel_open => Delivery::Panel,
        reminders::ReminderStyle::Bubble if tray_visible && anchor_known => Delivery::Bubble,
        reminders::ReminderStyle::Bubble => Delivery::System,
    }
}

/// The reminders waiting to be acknowledged, oldest first. Several can come
/// due in one tick -- back from a pause or from being away -- and each is shown
/// in turn. One entry per kind, so it never holds more than four.
#[derive(Default)]
pub struct Pending {
    queue: VecDeque<ReminderEvent>,
    bubble: Option<WindowHandle<Bubble>>,
}
impl Global for Pending {}

fn same_kind(a: &ReminderEvent, b: &ReminderEvent) -> bool {
    match (a, b) {
        (ReminderEvent::FocusPhaseChanged(_), ReminderEvent::FocusPhaseChanged(_)) => true,
        _ => a == b,
    }
}

/// The reminder shown now: the oldest one not yet acknowledged.
pub fn pending(cx: &App) -> Option<ReminderEvent> {
    cx.try_global::<Pending>()
        .and_then(|pending| pending.queue.front().copied())
}

/// Queues `event`, replacing an older one of the same kind.
pub fn set_pending(event: ReminderEvent, cx: &mut App) {
    let queue = &mut cx.default_global::<Pending>().queue;
    queue.retain(|queued| !same_kind(queued, &event));
    queue.push_back(event);
}

/// Done: the reminder is dealt with, and the next one, if any, is shown.
pub fn acknowledge(cx: &mut App) {
    cx.default_global::<Pending>().queue.pop_front();
    close_bubble(cx);
    show_next(cx);
}

/// Done, pressed in `window`: when that is the bubble itself it removes its own
/// window, since a window cannot be updated from inside its own update.
pub fn acknowledge_from(window: &mut Window, cx: &mut App) {
    cx.default_global::<Pending>().queue.pop_front();
    if let Some(handle) = cx.default_global::<Pending>().bubble.take() {
        if handle.window_id() == window.window_handle().window_id() {
            window.remove_window();
        } else if let Err(error) = handle.update(cx, |_, window, _| window.remove_window()) {
            log::debug!("the reminder bubble was already gone: {error:#}");
        }
    }
    // Opening the next bubble waits for this window's update to end.
    cx.defer(show_next);
}

/// Gives the next queued reminder its bubble. The panel shows it by itself,
/// and one sent as an OS notification already had its notice.
fn show_next(cx: &mut App) {
    if pending(cx).is_none() || bubble_is_open(cx) {
        return;
    }
    let style = Reminders::global(cx)
        .and_then(|reminders| reminders.read(cx).settings().map(|settings| settings.style))
        .unwrap_or_default();
    let anchor = cx.tray_icon_anchor();
    let route = delivery(
        style,
        crate::panel_is_open(cx),
        cx.tray_icon_visible(),
        anchor.is_some(),
    );
    if let (Delivery::Bubble, Some(anchor)) = (route, anchor) {
        show_bubble(anchor, cx);
    }
}

/// Forgets the bubble whose window `window_id` was closed some other way than
/// by Zode.
pub(crate) fn forget_closed_bubble(window_id: gpui::WindowId, cx: &mut App) {
    if let Some(pending) = cx.try_global::<Pending>()
        && pending
            .bubble
            .is_some_and(|handle| handle.window_id() == window_id)
    {
        cx.global_mut::<Pending>().bubble = None;
    }
}

/// Snooze, pressed in `window`; see [`acknowledge_from`].
pub fn snooze_from(window: &mut Window, cx: &mut App) {
    if let Some(ReminderEvent::Due(kind)) = pending(cx)
        && let Some(reminders) = Reminders::global(cx)
    {
        reminders.update(cx, |reminders, cx| reminders.snooze(kind, cx));
    }
    acknowledge_from(window, cx);
}

/// Snooze: the reminder comes back after the snooze length. The focus timer
/// has no snooze; its notice is only acknowledged.
pub fn snooze(cx: &mut App) {
    if let Some(ReminderEvent::Due(kind)) = pending(cx)
        && let Some(reminders) = Reminders::global(cx)
    {
        reminders.update(cx, |reminders, cx| reminders.snooze(kind, cx));
    }
    acknowledge(cx);
}

pub fn show_bubble(anchor: TrayIconAnchor, cx: &mut App) {
    close_bubble(cx);
    let display = anchor
        .display_id
        .and_then(|id| cx.find_display(id))
        .or_else(|| cx.primary_display())
        .map(|display| display.bounds());
    let Some(display) = display else {
        return;
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(panel_bounds(
            &anchor,
            BUBBLE_SIZE,
            display,
        ))),
        display_id: anchor.display_id,
        titlebar: None,
        kind: WindowKind::PopUp,
        focus: false,
        show: true,
        is_movable: false,
        is_resizable: false,
        window_background: crate::window_background(cx),
        ..Default::default()
    };
    match cx.open_window(options, |_, cx| cx.new(Bubble::new)) {
        Ok(handle) => cx.default_global::<Pending>().bubble = Some(handle),
        Err(error) => log::error!("could not open the reminder bubble: {error:#}"),
    }
}

pub fn close_bubble(cx: &mut App) {
    let Some(handle) = cx.default_global::<Pending>().bubble.take() else {
        return;
    };
    if let Err(error) = handle.update(cx, |_, window, _| window.remove_window()) {
        log::debug!("the reminder bubble was already gone: {error:#}");
    }
}

pub fn bubble_is_open(cx: &App) -> bool {
    cx.try_global::<Pending>()
        .is_some_and(|pending| pending.bubble.is_some())
}

/// The notice for the pending reminder, with its buttons.
pub fn notice(event: ReminderEvent, cx: &App) -> impl IntoElement + use<> {
    let (title, body) = crate::event_text(event, cx);
    let snoozable = matches!(event, ReminderEvent::Due(_));
    v_flex()
        .gap_1()
        .child(Label::new(title))
        .child(Label::new(body).size(LabelSize::Small).color(Color::Muted))
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("reminder-done", "Done")
                        .on_click(|_, window, cx| acknowledge_from(window, cx)),
                )
                .when(snoozable, |row| {
                    row.child(
                        Button::new("reminder-snooze", "Snooze")
                            .on_click(|_, window, cx| snooze_from(window, cx)),
                    )
                }),
        )
}

pub struct Bubble {
    _hide: Task<()>,
}

impl Bubble {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            _hide: cx.spawn(async move |_, cx| {
                cx.background_executor().timer(BUBBLE_LIFETIME).await;
                cx.update(close_bubble);
            }),
        }
    }
}

impl Render for Bubble {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let backdrop = crate::apply_window_effects(window, cx);
        let colors = cx.theme().colors();
        v_flex()
            .size_full()
            .p_3()
            .rounded_xl()
            .border_1()
            .border_color(crate::surface_border(cx))
            .bg(crate::surface_fill(cx))
            .text_color(colors.text)
            .children(pending(cx).map(|event| notice(event, cx)))
            .children(backdrop)
    }
}
