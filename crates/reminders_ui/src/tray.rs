//! The tray icon (the macOS menu bar extra) and the panel it opens.

use gpui::{
    App, AppContext as _, Bounds, DisplayId, Global, Pixels, Size, TrayEdge, TrayIcon,
    TrayIconAnchor, WindowBounds, WindowHandle, WindowKind, WindowOptions, point, px,
};
use settings::{Settings as _, SettingsStore};

use crate::panel::RemindersPanel;

/// The white glyph drawn as a template, so macOS colors it for the menu bar.
#[cfg(target_os = "macos")]
const ICON: (&[u8], bool) = (include_bytes!("../resources/status-icon@2x.png"), true);
/// The app icon in color: trays elsewhere have light and dark panels alike, and
/// no template convention.
#[cfg(not(target_os = "macos"))]
const ICON: (&[u8], bool) = (include_bytes!("../resources/tray-icon.png"), false);

pub const PANEL_SIZE: Size<Pixels> = Size {
    width: px(360.),
    height: px(560.),
};

const CLOSES_ON_BLUR: bool = cfg!(not(target_os = "macos"));

/// A pop-up everywhere but Linux. An X11 pop-up is an override-redirect window,
/// which never takes focus, so it would never lose it either and a click
/// elsewhere could not close it; an ordinary window drawing its own frame can.
/// Wayland places windows itself whatever their kind.
const PANEL_KIND: WindowKind = if cfg!(any(target_os = "linux", target_os = "freebsd")) {
    WindowKind::Normal
} else {
    WindowKind::PopUp
};

/// Gap between the tray and the panel.
const GAP: Pixels = px(6.);

/// How soon after the panel closed on a click elsewhere a tray click still
/// counts as the same gesture. Clicking the icon of an open panel is a click
/// outside it first -- on macOS the menu bar belongs to another process, on the
/// other platforms the panel loses focus -- and only then a tray click.
const RECLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(400);

#[derive(Default)]
struct TrayState {
    shown: bool,
    panel: Option<WindowHandle<RemindersPanel>>,
    /// When the panel last closed because of a click outside it.
    closed_from_outside_at: Option<std::time::Instant>,
}
impl Global for TrayState {}

/// The `tray_icon` setting.
#[derive(Clone, Copy, Debug, PartialEq, settings::RegisterSetting)]
pub struct TrayIconSetting(pub bool);

impl settings::Settings for TrayIconSetting {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self(content.tray_icon.unwrap())
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(TrayState::default());
    cx.on_tray_icon_click(|anchor, cx| toggle_panel(anchor, cx));
    sync_icon(cx);
    cx.observe_global::<SettingsStore>(sync_icon).detach();
    // A window manager can close the panel or the bubble without going
    // through Zode; a record of a window that is gone would swallow the next
    // tray click and route reminders to nothing.
    cx.on_window_closed(|cx, window_id| {
        let panel_closed = cx.try_global::<TrayState>().is_some_and(|state| {
            state
                .panel
                .is_some_and(|handle| handle.window_id() == window_id)
        });
        if panel_closed {
            forget_panel(cx);
        }
        crate::bubble::forget_closed_bubble(window_id, cx);
    })
    .detach();
}

fn sync_icon(cx: &mut App) {
    let want = TrayIconSetting::try_get(cx).is_none_or(|setting| setting.0);
    if cx.global::<TrayState>().shown == want {
        return;
    }
    cx.global_mut::<TrayState>().shown = want;
    cx.set_tray_icon(want.then(|| TrayIcon {
        image: ICON.0,
        template: ICON.1,
        tooltip: "Zode".into(),
    }));
    if !want {
        close_panel(cx);
    }
}

/// Where a panel of `panel_size` opens for `anchor` on a display covering
/// `display`: centered on the icon, on the side away from the tray, kept inside
/// the display. `display` is in the space window bounds use on this platform,
/// the same space as the anchor.
pub fn panel_bounds(
    anchor: &TrayIconAnchor,
    panel_size: Size<Pixels>,
    display: Bounds<Pixels>,
) -> Bounds<Pixels> {
    let icon = anchor.bounds;
    let centered_x = icon.origin.x + icon.size.width / 2. - panel_size.width / 2.;
    let centered_y = icon.origin.y + icon.size.height / 2. - panel_size.height / 2.;
    let (x, y) = match anchor.edge {
        TrayEdge::Top => (centered_x, icon.origin.y + icon.size.height + GAP),
        TrayEdge::Bottom => (centered_x, icon.origin.y - panel_size.height - GAP),
        TrayEdge::Left => (icon.origin.x + icon.size.width + GAP, centered_y),
        TrayEdge::Right => (icon.origin.x - panel_size.width - GAP, centered_y),
    };
    let clamp = |value: Pixels, size: Pixels, start: Pixels, length: Pixels| {
        value.min(start + length - size - GAP).max(start + GAP)
    };
    Bounds::new(
        point(
            clamp(x, panel_size.width, display.origin.x, display.size.width),
            clamp(y, panel_size.height, display.origin.y, display.size.height),
        ),
        panel_size,
    )
}

pub fn panel_is_open(cx: &App) -> bool {
    cx.try_global::<TrayState>()
        .is_some_and(|state| state.panel.is_some())
}

pub fn toggle_panel(anchor: Option<TrayIconAnchor>, cx: &mut App) {
    if panel_is_open(cx) {
        close_panel(cx);
        return;
    }
    let now = cx.background_executor().now();
    let closed_just_now = cx
        .try_global::<TrayState>()
        .and_then(|state| state.closed_from_outside_at)
        .is_some_and(|closed| now.saturating_duration_since(closed) < RECLICK_WINDOW);
    if cx.has_global::<TrayState>() {
        cx.global_mut::<TrayState>().closed_from_outside_at = None;
    }
    if !closed_just_now {
        open_panel(anchor, cx);
    }
}

/// Records that the panel is closing because of a click outside it.
pub(crate) fn note_closed_from_outside(cx: &mut App) {
    let now = cx.background_executor().now();
    if cx.has_global::<TrayState>() {
        cx.global_mut::<TrayState>().closed_from_outside_at = Some(now);
    }
}

fn close_panel_from_outside(cx: &mut App) {
    note_closed_from_outside(cx);
    close_panel(cx);
}

fn open_panel(anchor: Option<TrayIconAnchor>, cx: &mut App) {
    let display_id: Option<DisplayId> = anchor.and_then(|anchor| anchor.display_id);
    let display = display_id
        .and_then(|id| cx.find_display(id))
        .or_else(|| cx.primary_display())
        .map(|display| display.bounds());
    let bounds = match (anchor, display) {
        (Some(anchor), Some(display)) => panel_bounds(&anchor, PANEL_SIZE, display),
        // No anchor (a tray that cannot say where it is): the middle of the screen.
        _ => Bounds::centered(None, PANEL_SIZE, cx),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        kind: PANEL_KIND,
        window_decorations: (PANEL_KIND == WindowKind::Normal)
            .then_some(gpui::WindowDecorations::Client),
        // macOS opens a non-activating panel and closes it on clicks elsewhere;
        // the other platforms have no such monitor, so the panel takes focus
        // there and closes when it loses it.
        focus: CLOSES_ON_BLUR,
        show: true,
        is_movable: false,
        is_resizable: false,
        window_background: crate::window_background(cx),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| {
            let mut panel = RemindersPanel::new(cx);
            if CLOSES_ON_BLUR {
                panel.close_on_blur(window, cx);
            }
            panel
        })
    }) {
        Ok(handle) => {
            cx.global_mut::<TrayState>().panel = Some(handle);
            cx.on_outside_mouse_down(Some(Box::new(close_panel_from_outside)));
        }
        Err(error) => log::error!("could not open the reminders panel: {error:#}"),
    }
}

/// Drops the record of the open panel without touching its window. For code
/// running inside the panel, which removes its own window: updating a window
/// from inside its own update fails.
pub fn forget_panel(cx: &mut App) -> Option<WindowHandle<RemindersPanel>> {
    cx.on_outside_mouse_down(None);
    cx.try_global::<TrayState>()?;
    cx.global_mut::<TrayState>().panel.take()
}

/// Closes the panel from outside it.
pub fn close_panel(cx: &mut App) {
    let Some(handle) = forget_panel(cx) else {
        return;
    };
    if let Err(error) = handle.update(cx, |_, window, _| window.remove_window()) {
        log::debug!("the reminders panel was already gone: {error:#}");
    }
}
