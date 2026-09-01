//! The modal that stands between a dirty working tree and a checkout.
//!
//! Losing uncommitted work is the worst thing this panel could do, so the
//! choice is never made silently. Three options, each saying plainly what it
//! will do, and Cancel is the one that has focus.

use gpui::{
    DismissEvent, EventEmitter, FocusHandle, Focusable, FontWeight, Render, SharedString, Window,
    prelude::*,
};
use ui::{Divider, prelude::*};
use workspace::ModalView;

/// What the user chose to do about their uncommitted changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirtyChoice {
    /// Stash everything, then switch. The changes are recoverable from the
    /// stash list afterwards.
    Stash,
    /// Leave this checkout alone and open the target in a new worktree. The
    /// only option that risks nothing at all.
    Worktree,
}

pub(crate) struct DirtyPrompt {
    target: SharedString,
    focus_handle: FocusHandle,
    chosen: Option<DirtyChoice>,
}

impl DirtyPrompt {
    pub(crate) fn new(target: SharedString, cx: &mut Context<Self>) -> Self {
        Self {
            target,
            focus_handle: cx.focus_handle(),
            chosen: None,
        }
    }

    /// What the user picked, or `None` if they cancelled. Read by the caller
    /// once the modal has dismissed.
    pub(crate) fn choice(&self) -> Option<DirtyChoice> {
        self.chosen
    }

    fn choose(&mut self, choice: DirtyChoice, cx: &mut Context<Self>) {
        self.chosen = Some(choice);
        cx.emit(DismissEvent);
    }
}

impl Focusable for DirtyPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for DirtyPrompt {}
impl ModalView for DirtyPrompt {}

impl Render for DirtyPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DirtyPrompt")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(28.))
            .p_3()
            .gap_2()
            .child(
                Label::new(format!("Switch to {}?", self.target))
                    .size(LabelSize::Default)
                    .weight(FontWeight::MEDIUM),
            )
            .child(
                Label::new("This checkout has uncommitted changes.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(Divider::horizontal())
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Button::new("stash-and-switch", "Stash changes and switch")
                            .full_width()
                            .on_click(
                                cx.listener(|this, _, _, cx| this.choose(DirtyChoice::Stash, cx)),
                            ),
                    )
                    .child(
                        Button::new("open-worktree", "Open in a new worktree instead")
                            .full_width()
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.choose(DirtyChoice::Worktree, cx)
                                }),
                            ),
                    )
                    .child(
                        Button::new("cancel", "Cancel")
                            .full_width()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                    ),
            )
    }
}
