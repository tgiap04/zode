use std::sync::Arc;

use gpui::{AnyElement, AnyView, ClickEvent, ElementId, SharedString};
use ui::{Disclosure, prelude::*};

/// A collapsible section inside the git panel: disclosure triangle, label, an
/// optional count badge, a row of actions revealed on hover, and content that
/// folds away when collapsed.
#[derive(IntoElement)]
pub(crate) struct PanelSection {
    /// Names the header row, the disclosure and the hover group. Kept separate from `label`,
    /// which is display text and free to be reworded.
    id: SharedString,
    label: SharedString,
    expanded: bool,
    badge: Option<usize>,
    /// Whether the expanded content claims the panel's leftover height. Off by default, so a
    /// section is only as tall as its rows and sections cannot end up dividing the panel between
    /// themselves.
    fills_height: bool,
    /// A fixed content height. Set for sections whose content is unbounded (a commit log, a
    /// graph): they scroll inside this height instead of growing the panel.
    height: Option<Pixels>,
    /// Rendered on the section's top edge, above the header, so it sits on the boundary with the
    /// section that gives up the space rather than against the bottom of the panel.
    resize_handle: Option<AnyElement>,
    badge_tooltip: Option<Box<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>>,
    on_badge_click: Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    actions: Vec<AnyElement>,
    on_toggle: Option<Arc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    children: Vec<AnyElement>,
}

impl PanelSection {
    pub(crate) fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        expanded: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded,
            badge: None,
            fills_height: false,
            height: None,
            resize_handle: None,
            badge_tooltip: None,
            on_badge_click: None,
            actions: Vec::new(),
            on_toggle: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn badge(mut self, count: impl Into<Option<usize>>) -> Self {
        self.badge = count.into();
        self
    }

    /// Let this section's content take whatever height the panel has left. At most one section
    /// should claim it.
    pub(crate) fn fills_height(mut self) -> Self {
        debug_assert!(
            self.height.is_none(),
            "a section cannot both fill the panel and hold a fixed height"
        );
        self.fills_height = true;
        self
    }

    /// Pin the content to `height` and hang `resize_handle` underneath it. The content is expected
    /// to manage its own scrolling; the box only clips. Mutually exclusive with
    /// [`Self::fills_height`].
    pub(crate) fn fixed_height(mut self, height: Pixels, resize_handle: impl IntoElement) -> Self {
        debug_assert!(
            !self.fills_height,
            "a section cannot both fill the panel and hold a fixed height"
        );
        self.height = Some(height);
        self.resize_handle = Some(resize_handle.into_any_element());
        self
    }

    /// Makes the count badge itself actionable. The handler stops the click from reaching the
    /// header row, which would otherwise fold the section instead.
    pub(crate) fn on_badge_click(
        mut self,
        tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.badge_tooltip = Some(Box::new(tooltip));
        self.on_badge_click = Some(Arc::new(handler));
        self
    }

    pub(crate) fn actions(mut self, actions: impl IntoIterator<Item = AnyElement>) -> Self {
        self.actions.extend(actions);
        self
    }

    pub(crate) fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Arc::new(handler));
        self
    }
}

impl ParentElement for PanelSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for PanelSection {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hover_group = SharedString::from(format!("{}-hover", self.id));
        let disclosure_id = ElementId::Name(format!("{}-disclosure", self.id).into());
        let content_id = ElementId::Name(format!("{}-content", self.id).into());
        let expanded = self.expanded;
        let on_toggle = self.on_toggle;
        // Taken before the closures below capture the rest of `self`.
        let resize_handle = self.resize_handle;

        v_flex()
            .w_full()
            .flex_none()
            // On the section's own top edge, which is the boundary it shares with the section above
            // — the section that yields the space. A handle under the content would put the last
            // section's handle against the bottom of the panel, and would grow a section downwards
            // while its top edge, the edge the pointer is on, stayed put.
            .when(expanded, |this| this.children(resize_handle))
            .child(
                h_flex()
                    .id(ElementId::Name(self.id))
                    .group(hover_group.clone())
                    .h(rems(1.75))
                    .w_full()
                    .flex_none()
                    .items_center()
                    .pl_1()
                    .pr_1()
                    .gap_1()
                    .when_some(on_toggle.clone(), |this, on_toggle| {
                        this.cursor_pointer()
                            .on_click(move |event, window, cx| on_toggle(event, window, cx))
                    })
                    .child(Disclosure::new(disclosure_id, expanded).on_toggle_expanded(on_toggle))
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_1()
                            .overflow_hidden()
                            .child(
                                Label::new(self.label)
                                    .size(LabelSize::Small)
                                    .line_height_style(LineHeightStyle::UiLabel)
                                    .single_line(),
                            )
                            .when_some(self.badge.filter(|count| *count > 0), |this, count| {
                                let on_badge_click = self.on_badge_click;
                                this.child(
                                    h_flex()
                                        .id("badge")
                                        .flex_none()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(cx.theme().colors().element_background)
                                        .when_some(on_badge_click, |this, on_badge_click| {
                                            this.cursor_pointer()
                                                .hover(|this| {
                                                    this.bg(cx.theme().colors().element_hover)
                                                })
                                                .on_click(move |event, window, cx| {
                                                    cx.stop_propagation();
                                                    on_badge_click(event, window, cx);
                                                })
                                        })
                                        .when_some(self.badge_tooltip, |this, tooltip| {
                                            this.tooltip(tooltip)
                                        })
                                        .child(
                                            Label::new(count.to_string())
                                                .size(LabelSize::XSmall)
                                                .color(Color::Muted)
                                                .line_height_style(LineHeightStyle::UiLabel),
                                        ),
                                )
                            }),
                    )
                    .when(!self.actions.is_empty(), |this| {
                        this.child(
                            h_flex()
                                .flex_none()
                                .gap_0p5()
                                .visible_on_hover(hover_group)
                                .children(self.actions),
                        )
                    }),
            )
            .when(expanded, |this| {
                let fills_height = self.fills_height;
                let height = self.height;
                this.when(fills_height, |this| this.flex_1().min_h_0())
                    .child(
                        v_flex()
                            .id(content_id)
                            .when(fills_height, |this| this.flex_1().min_h_0())
                            .w_full()
                            // Clip in both cases: a fixed-height section's content scrolls itself
                            // (a `uniform_list`), and a second scroll region stacked on top of that
                            // would fight it for the wheel.
                            .when_some(height, |this, height| this.h(height))
                            .overflow_hidden()
                            .children(self.children),
                    )
            })
    }
}
