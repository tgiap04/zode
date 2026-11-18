//! The History tab of an image: the layers it was built from, newest first.
//!
//! Read-only text the engine printed, so it is shown and never acted on. Each
//! layer's command is one line that can be copied whole.
//!
//! A layer's command is a build step, and a build step can carry a secret --
//! `ENV DB_PASSWORD=...`, `curl https://user:pw@host`. It is masked the way the
//! Inspect tab masks an environment value: a layer is revealed one at a time
//! with its eye, what is copied is what the screen shows, and a refresh, leaving
//! the tab or closing the row puts every mask back.

use std::collections::HashSet;

use container::{ImageLayer, mask_secrets_in_text};
use gpui::{AnyElement, ClipboardItem, Task, Window};
use ui::prelude::*;

use crate::container_panel::ContainerPanel;

#[derive(Default)]
pub(crate) struct HistoryState {
    /// `None` until the first answer arrives.
    pub(crate) layers: Option<Result<Vec<ImageLayer>, String>>,
    /// The request in flight. Held, so closing the row cancels it.
    pub(crate) load: Option<Task<()>>,
    /// The layers (by position) shown unmasked.
    pub(crate) revealed: HashSet<usize>,
}

/// What a layer's command shows: the real text only once its own eye was
/// pressed.
pub(crate) fn shown_command(layer: &ImageLayer, revealed: bool) -> String {
    if revealed {
        layer.created_by.clone()
    } else {
        mask_secrets_in_text(&layer.created_by)
    }
}

impl HistoryState {
    pub(crate) fn has_started(&self) -> bool {
        self.load.is_some() || self.layers.is_some()
    }
}

impl ContainerPanel {
    /// Asks the engine for the open image's layers. What is on screen stays
    /// while the new answer is awaited.
    pub(crate) fn load_history(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let id = detail.resource.id.clone();
        detail.history.load = Some(cx.spawn(async move |this, cx| {
            let layers = backend.history(&id).await;
            let applied = this.update(cx, |this, cx| {
                if let Some(detail) = this.detail.as_mut() {
                    detail.history.load = None;
                    detail.history.revealed.clear();
                    detail.history.layers = Some(layers.map_err(|error| error.to_string()));
                }
                cx.notify();
            });
            if let Err(error) = applied {
                log::debug!("panel closed before an image history arrived: {error}");
            }
        }));
        cx.notify();
    }

    pub(crate) fn toggle_layer_reveal(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if !detail.history.revealed.remove(&index) {
            detail.history.revealed.insert(index);
        }
        cx.notify();
    }

    pub(crate) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        let state = &detail.history;
        let body = match &state.layers {
            None => crate::render::note(None, "Asking the engine\u{2026}"),
            Some(Err(message)) => v_flex()
                .debug_selector(|| "container-history-error".into())
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .p_4()
                .child(
                    Icon::new(IconName::Warning)
                        .size(IconSize::XLarge)
                        .color(Color::Muted),
                )
                .child(Label::new("Could not read the history"))
                .child(
                    Label::new(message.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
            Some(Ok(layers)) if layers.is_empty() => {
                crate::render::note(None, "This image has no layers to show.")
            }
            Some(Ok(layers)) => v_flex()
                .id("container-history-layers")
                .size_full()
                .overflow_y_scroll()
                .children(layers.iter().enumerate().map(|(index, layer)| {
                    self.render_layer(index, layer, state.revealed.contains(&index), cx)
                }))
                .into_any_element(),
        };
        let count = match &state.layers {
            Some(Ok(layers)) => Some(layers.len()),
            _ => None,
        };
        v_flex()
            .debug_selector(|| "container-history".into())
            .size_full()
            .child(
                h_flex()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(
                        Label::new(match count {
                            Some(count) => format!("{count} layers"),
                            None => "Layers".to_string(),
                        })
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        IconButton::new("container-history-refresh", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .loading(state.load.is_some())
                            .tooltip(|_window, cx| ui::Tooltip::simple("Refresh", cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.load_history(window, cx)),
                            ),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }
}

impl ContainerPanel {
    fn render_layer(
        &self,
        index: usize,
        layer: &ImageLayer,
        revealed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = shown_command(layer, revealed);
        // Only a layer with something to hide gets an eye.
        let hides = shown_command(layer, false) != layer.created_by;
        let copied = shown.clone();
        h_flex()
            .debug_selector(move || format!("container-history-layer-{index}"))
            .px_2()
            .py_0p5()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(shown).size(LabelSize::Small).truncate()),
            )
            .child(
                div().w(px(72.)).flex_none().child(
                    Label::new(layer.size.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
            .child(
                div().w(px(120.)).flex_none().child(
                    Label::new(layer.created.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                ),
            )
            .when(hides, |element| {
                element.child(
                    div()
                        .debug_selector(move || format!("container-history-reveal-{index}"))
                        .child(
                            IconButton::new(
                                ("container-history-reveal", index),
                                if revealed {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                },
                            )
                            .icon_size(IconSize::XSmall)
                            .tooltip(move |_window, cx| {
                                ui::Tooltip::simple(if revealed { "Hide" } else { "Reveal" }, cx)
                            })
                            .on_click(cx.listener(
                                move |this, _, _window, cx| this.toggle_layer_reveal(index, cx),
                            )),
                        ),
                )
            })
            .child(
                div()
                    .debug_selector(move || format!("container-history-copy-{index}"))
                    .child(
                        IconButton::new(("container-history-copy", index), IconName::Copy)
                            .icon_size(IconSize::XSmall)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Copy command", cx))
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
                            }),
                    ),
            )
            .into_any_element()
    }
}
