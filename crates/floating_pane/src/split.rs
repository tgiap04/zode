//! Pane events (split, remove/collapse, focus, zoom) and pane navigation.

use gpui::{Context, Entity, Focusable, Window};
use workspace::{Pane, SplitDirection, SplitMode, pane};

use crate::host::FloatingPane;

impl FloatingPane {
    /// One subscription per pane, keyed by id so a removed pane's is dropped with it.
    pub(crate) fn subscribe_to(
        &mut self,
        pane: &Entity<Pane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_subscriptions.insert(
            pane.entity_id(),
            cx.subscribe_in(pane, window, Self::handle_pane_event),
        );
    }

    /// The one path every split goes through, so creation/subscription/focus never drift apart.
    pub(crate) fn split_off(
        &mut self,
        from: &Entity<Pane>,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Pane>> {
        let this = cx.weak_entity();
        let new_pane = Self::build_pane(&self.workspace, self.project.clone(), this, window, cx);
        self.subscribe_to(&new_pane, window, cx);
        self.center.split(from, &new_pane, direction, cx);
        self.active_pane = new_pane.clone();
        self.focus_pane(window, cx);
        cx.notify();
        Some(new_pane)
    }

    pub(crate) fn handle_pane_event(
        &mut self,
        pane: &Entity<Pane>,
        event: &pane::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            &pane::Event::Split { direction, mode } => {
                self.handle_split(pane, direction, mode, window, cx);
            }
            pane::Event::Remove { focus_on_pane } => {
                self.handle_remove(pane, focus_on_pane.clone(), window, cx);
            }
            // A closed last tab must still redraw the window into its menu.
            pane::Event::RemovedItem { .. } => cx.notify(),
            pane::Event::Focus => {
                self.active_pane = pane.clone();
            }
            pane::Event::ZoomIn => {
                self.zoomed = Some(pane.downgrade().into());
                for other in self.center.panes() {
                    let zoom = other == pane;
                    other.update(cx, |other, cx| other.set_zoomed(zoom, cx));
                }
                cx.notify();
            }
            pane::Event::ZoomOut => {
                self.zoomed = None;
                for other in self.center.panes() {
                    other.update(cx, |other, cx| other.set_zoomed(false, cx));
                }
                cx.notify();
            }
            pane::Event::AddItem { item } => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        item.added_to_pane(workspace, pane.clone(), window, cx)
                    });
                }
            }
            _ => {}
        }
    }

    /// `MovePane` needs a tab to spare, or the split would collapse straight
    /// back out; that case and `ClonePane` both fall back to a new terminal.
    fn handle_split(
        &mut self,
        pane: &Entity<Pane>,
        direction: SplitDirection,
        mode: SplitMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if mode == SplitMode::MovePane && pane.read(cx).items_len() > 1 {
            let Some(item) = pane.update(cx, |pane, cx| pane.take_active_item(window, cx)) else {
                return;
            };
            let Some(new_pane) = self.split_off(pane, direction, window, cx) else {
                return;
            };
            new_pane.update(cx, |new_pane, cx| {
                new_pane.add_item(item, true, true, None, window, cx);
            });
            return;
        }

        if self.split_off(pane, direction, window, cx).is_some() {
            self.open_terminal(window, cx);
        }
    }

    /// Moves focus to the pane in `direction` from the active one.
    ///
    /// Deliberately no fallback into the workspace at a group edge (unlike
    /// `terminal_panel::activate_pane_in_direction`): moving focus into the
    /// editor behind this window on a keystroke would be invisible to the user.
    pub(crate) fn activate_in(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open || self.is_empty(cx) {
            return;
        }
        if let Some(pane) = self
            .center
            .find_pane_in_direction(&self.active_pane, direction, cx)
        {
            window.focus(&pane.focus_handle(cx), cx);
        }
    }

    pub(crate) fn swap_in(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        if !self.open || self.is_empty(cx) {
            return;
        }
        if let Some(to) = self
            .center
            .find_pane_in_direction(&self.active_pane, direction, cx)
            .cloned()
        {
            self.center.swap(&self.active_pane, &to, cx);
            cx.notify();
        }
    }

    /// `Ok(false)` covers "already there" and "only one pane", neither an
    /// error; only `Err` (active pane missing from `center`) gets logged.
    pub(crate) fn move_active_to_border(
        &mut self,
        direction: SplitDirection,
        cx: &mut Context<Self>,
    ) {
        if !self.open || self.is_empty(cx) {
            return;
        }
        match self.center.move_to_border(&self.active_pane, direction, cx) {
            Ok(true) => cx.notify(),
            Ok(false) => {}
            Err(error) => {
                log::error!(
                    "could not move the floating window's pane to the {direction} border: {error}"
                );
            }
        }
    }

    /// The last pane is never removed -- `is_empty` takes over the body instead.
    fn handle_remove(
        &mut self,
        pane: &Entity<Pane>,
        focus_on_pane: Option<Entity<Pane>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.center.panes().len() == 1 {
            pane.update(cx, |pane, cx| pane.set_zoomed(false, cx));
            self.zoomed = None;
            cx.notify();
            return;
        }

        match self.center.remove(pane, cx) {
            Ok(_) => {
                self.pane_subscriptions.remove(&pane.entity_id());
                if self.zoomed.as_ref() == Some(&pane.downgrade().into()) {
                    self.zoomed = None;
                }
                self.active_pane = focus_on_pane.unwrap_or_else(|| self.center.last_pane());
                self.focus_pane(window, cx);
                cx.notify();
            }
            Err(error) => {
                log::error!("could not collapse a pane out of the floating window: {error}");
            }
        }
    }
}
