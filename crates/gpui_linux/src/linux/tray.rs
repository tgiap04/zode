//! The tray icon, through StatusNotifierItem (`tray_sni`).
//!
//! The item lives on the session bus from a background task; clicks hop to the
//! foreground, where the app's callback runs. A host that reports where the
//! click was (X11 panels) gives the panel somewhere to open; one that reports
//! 0, 0 (common on Wayland, where clients cannot be told positions) gives no
//! anchor, and the app opens its panel in the middle of the screen instead.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    BackgroundExecutor, ForegroundExecutor, Task, TrayEdge, TrayIcon, TrayIconAnchor, point, px,
    size,
};
use tray_sni::{SniTray, TrayEvent};

/// Sizes offered to the host; it picks the one closest to its panel height.
const PIXMAP_SIZES: [u32; 4] = [22, 32, 44, 64];

/// Below this, a click is taken to come from a panel along the top of the
/// screen, so the app's panel opens under it rather than above it.
const TOP_PANEL_LIMIT: i32 = 200;

#[derive(Default)]
struct Inner {
    tray: Option<SniTray>,
    click: Option<Box<dyn FnMut(Option<TrayIconAnchor>)>>,
    /// Owns the item and its click loop; dropping it removes the icon.
    task: Option<Task<()>>,
}

#[derive(Default, Clone)]
pub(crate) struct TrayState(Rc<RefCell<Inner>>);

/// The anchor for a click the host reported at `x`, `y` on screen.
pub(crate) fn anchor_for_click(x: i32, y: i32) -> Option<TrayIconAnchor> {
    if x == 0 && y == 0 {
        return None;
    }
    Some(TrayIconAnchor {
        bounds: gpui::Bounds::new(point(px(x as f32), px(y as f32)), size(px(1.), px(1.))),
        display_id: None,
        edge: if y < TOP_PANEL_LIMIT {
            TrayEdge::Top
        } else {
            TrayEdge::Bottom
        },
    })
}

impl TrayState {
    pub(crate) fn set_icon(
        &self,
        icon: Option<TrayIcon>,
        background: BackgroundExecutor,
        foreground: ForegroundExecutor,
    ) {
        {
            let mut inner = self.0.borrow_mut();
            inner.task = None;
            inner.tray = None;
        }
        let Some(icon) = icon else {
            return;
        };
        let state = self.clone();
        let task = foreground.spawn(async move {
            let image = icon.image;
            let pixmaps = match background
                .spawn(async move { tray_sni::png_to_pixmaps(image, &PIXMAP_SIZES) })
                .await
            {
                Ok(pixmaps) => pixmaps,
                Err(error) => {
                    log::error!("could not prepare the tray icon: {error:#}");
                    return;
                }
            };
            let (sender, receiver) = smol::channel::bounded(8);
            let title = icon.tooltip.to_string();
            let tray = match background
                .spawn(async move { SniTray::new("zode", &title, pixmaps, sender).await })
                .await
            {
                Ok(tray) => tray,
                Err(error) => {
                    log::info!("no tray icon: {error:#}");
                    return;
                }
            };
            state.0.borrow_mut().tray = Some(tray);
            while let Ok(TrayEvent::Activate { x, y }) = receiver.recv().await {
                let callback = state.0.borrow_mut().click.take();
                if let Some(mut callback) = callback {
                    callback(anchor_for_click(x, y));
                    state.0.borrow_mut().click.get_or_insert(callback);
                }
            }
        });
        self.0.borrow_mut().task = Some(task);
    }

    pub(crate) fn on_click(&self, callback: Box<dyn FnMut(Option<TrayIconAnchor>)>) {
        self.0.borrow_mut().click = Some(callback);
    }

    pub(crate) fn visible(&self) -> bool {
        self.0
            .borrow()
            .tray
            .as_ref()
            .is_some_and(SniTray::host_present)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_at_the_origin_gives_no_anchor() {
        assert_eq!(anchor_for_click(0, 0), None);
    }

    #[test]
    fn a_click_near_the_top_opens_below_and_lower_down_opens_above() {
        let top = anchor_for_click(1700, 12).unwrap();
        assert_eq!(top.edge, TrayEdge::Top);
        assert_eq!(top.bounds.origin, point(px(1700.), px(12.)));
        assert_eq!(anchor_for_click(1700, 1060).unwrap().edge, TrayEdge::Bottom);
    }
}
