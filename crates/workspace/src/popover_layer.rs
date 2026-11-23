use gpui::{AnyView, DismissEvent, Entity, EventEmitter, Subscription};
use ui::prelude::*;

/// Holds at most one small view that floats over the workspace at a window
/// point, such as the translation popover.
///
/// The view positions itself (usually with `deferred(anchored())`), so the
/// layer only decides which view is alive. Showing a new one drops the old
/// one, and with it any task the old one was running.
#[derive(Default)]
pub struct PopoverLayer {
    active_popover: Option<ActivePopover>,
}

struct ActivePopover {
    view: AnyView,
    _dismiss_subscription: Subscription,
}

impl PopoverLayer {
    pub fn show_popover<V>(&mut self, popover: Entity<V>, cx: &mut Context<Self>)
    where
        V: Render + EventEmitter<DismissEvent>,
    {
        let dismiss_subscription = cx.subscribe(&popover, |this, popover, _: &DismissEvent, cx| {
            let is_active = this
                .active_popover
                .as_ref()
                .is_some_and(|active| active.view.entity_id() == popover.entity_id());
            if is_active {
                this.hide_popover(cx);
            }
        });
        self.active_popover = Some(ActivePopover {
            view: popover.into(),
            _dismiss_subscription: dismiss_subscription,
        });
        cx.notify();
    }

    pub fn hide_popover(&mut self, cx: &mut Context<Self>) {
        if self.active_popover.take().is_some() {
            cx.notify();
        }
    }

    pub fn active_popover<V: 'static>(&self) -> Option<Entity<V>> {
        let active_popover = self.active_popover.as_ref()?;
        active_popover.view.clone().downcast::<V>().ok()
    }
}

impl Render for PopoverLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children(
            self.active_popover
                .as_ref()
                .map(|active_popover| active_popover.view.clone()),
        )
    }
}
