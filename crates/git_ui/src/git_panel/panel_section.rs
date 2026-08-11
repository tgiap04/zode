use std::sync::Arc;

use gpui::{AnyElement, ClickEvent, ElementId, SharedString};
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
            actions: Vec::new(),
            on_toggle: None,
            children: Vec::new(),
        }
    }

    pub(crate) fn badge(mut self, count: impl Into<Option<usize>>) -> Self {
        self.badge = count.into();
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
        let expanded = self.expanded;
        let on_toggle = self.on_toggle;

        v_flex()
            .w_full()
            .flex_none()
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
                                this.child(
                                    h_flex()
                                        .flex_none()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(cx.theme().colors().element_background)
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
                this.flex_1().min_h_0().child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .overflow_hidden()
                        .children(self.children),
                )
            })
    }
}
