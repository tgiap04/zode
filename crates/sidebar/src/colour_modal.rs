//! Choosing a project's colour, with the avatar itself as the preview.
//!
//! The picker is `ui::ColorPicker`; this is only the box around it and the
//! bookkeeping that lets Cancel really cancel. Nothing is written to the project
//! until Confirm, and the avatar shows the candidate colour the whole time —
//! people pick a colour to look at the avatar, not to look at a swatch.

use crate::Sidebar;
use gpui::{DismissEvent, Entity, EventEmitter, Focusable, Hsla, SharedString, WeakEntity};
use project::ProjectGroupKey;
use ui::{COLOR_PICKER_KEY_STEP, ColorChanged, ColorPicker, prelude::*};
use workspace::{ModalView, MultiWorkspace};

pub struct ColourModal {
    multi_workspace: WeakEntity<MultiWorkspace>,
    sidebar: WeakEntity<Sidebar>,
    key: ProjectGroupKey,
    label: SharedString,
    picker: Entity<ColorPicker>,
    focus_handle: gpui::FocusHandle,
    _subscription: gpui::Subscription,
}

impl EventEmitter<DismissEvent> for ColourModal {}

impl ModalView for ColourModal {
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> workspace::DismissDecision {
        // Dismissing by any route — Escape, a click outside, the modal layer
        // closing — is a cancel, so the preview has to come off here rather than
        // only in the Cancel handler.
        self.clear_preview(cx);
        workspace::DismissDecision::Dismiss(true)
    }
}

impl Focusable for ColourModal {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl ColourModal {
    pub fn new(
        multi_workspace: WeakEntity<MultiWorkspace>,
        sidebar: WeakEntity<Sidebar>,
        key: ProjectGroupKey,
        label: SharedString,
        current: Option<Hsla>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|_| ColorPicker::new(current));
        let subscription = cx.subscribe(&picker, |this, _picker, event: &ColorChanged, cx| {
            this.preview(event.0, cx);
        });

        Self {
            multi_workspace,
            sidebar,
            key,
            label,
            picker,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    fn preview(&mut self, colour: Hsla, cx: &mut Context<Self>) {
        let key = self.key.clone();
        self.sidebar
            .update(cx, |sidebar, cx| {
                sidebar.colour_preview = Some((key, colour));
                cx.notify();
            })
            .ok();
    }

    fn clear_preview(&mut self, cx: &mut Context<Self>) {
        self.sidebar
            .update(cx, |sidebar, cx| {
                sidebar.colour_preview = None;
                cx.notify();
            })
            .ok();
    }

    /// Writes the colour once, at the end. A preview that wrote on every frame
    /// of a drag would spend a disk write per pixel.
    ///
    /// If the project left this window while the picker was open — removed from
    /// another surface, or dragged out — the write finds no such project and
    /// does nothing. That is the right outcome (a colour for a project this
    /// window no longer shows would be a phantom), and it is silent: the modal
    /// closes as though it worked. Worth knowing rather than worth guarding,
    /// since the alternative is an error box for a project that is already gone.
    fn commit(&mut self, colour: Option<Hsla>, cx: &mut Context<Self>) {
        let key = self.key.clone();
        self.multi_workspace
            .update(cx, |multi_workspace, cx| {
                multi_workspace.set_project_colour(&key, colour, cx);
            })
            .ok();
        self.clear_preview(cx);
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &menu::Confirm, _window: &mut Window, cx: &mut Context<Self>) {
        let colour = self.picker.read(cx).colour();
        self.commit(Some(colour), cx);
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        self.clear_preview(cx);
        cx.emit(DismissEvent);
    }

    /// Arrow keys drive the picker.
    ///
    /// Handled here rather than inside the picker because key events travel the
    /// focus path, and this modal is what holds focus — the same handler on the
    /// picker's own root would never fire. A colour control that only answers a
    /// dragged mouse answers no one who does not use one.
    fn key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = COLOR_PICKER_KEY_STEP;
        let (dx, dy, hue) = match event.keystroke.key.as_str() {
            "left" => (-step, 0., 0.),
            "right" => (step, 0., 0.),
            "up" => (0., step, 0.),
            "down" => (0., -step, 0.),
            "[" => (0., 0., -step),
            "]" => (0., 0., step),
            _ => return,
        };
        self.picker.update(cx, |picker, cx| {
            if hue != 0. {
                picker.nudge_hue(hue, cx);
            } else {
                picker.nudge(dx, dy, cx);
            }
        });
        cx.stop_propagation();
    }
}

impl Render for ColourModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        v_flex()
            .key_context("ColourModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .on_key_down(cx.listener(Self::key_down))
            .elevation_3(cx)
            .w_80()
            .p_2()
            .gap_2()
            .child(
                Label::new(format!("Colour for {}", self.label))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(self.picker.clone())
            .child(
                h_flex()
                    .w_full()
                    .pt_1()
                    .gap_1()
                    .justify_between()
                    .border_t_1()
                    .border_color(colors.border_variant)
                    .child(
                        // Without a way back, a colour someone regrets is
                        // permanent by omission.
                        Button::new("colour-use-default", "Use Default")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| this.commit(None, cx))),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("colour-cancel", "Cancel")
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel(&menu::Cancel, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("colour-confirm", "Set Colour")
                                    .label_size(LabelSize::Small)
                                    .style(ButtonStyle::Filled)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm(&menu::Confirm, window, cx)
                                    })),
                            ),
                    ),
            )
    }
}
