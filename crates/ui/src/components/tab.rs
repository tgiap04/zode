use std::cmp::Ordering;

use gpui::{AnyElement, IntoElement, Stateful};
use smallvec::SmallVec;

use crate::prelude::*;

const START_TAB_SLOT_SIZE: Pixels = px(12.);
const END_TAB_SLOT_SIZE: Pixels = px(14.);

/// The position of a [`Tab`] within a list of tabs.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TabPosition {
    /// The tab is first in the list.
    First,

    /// The tab is in the middle of the list (i.e., it is not the first or last tab).
    ///
    /// The [`Ordering`] is where this tab is positioned with respect to the selected tab.
    Middle(Ordering),

    /// The tab is last in the list.
    Last,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TabCloseSide {
    Start,
    End,
}

#[derive(IntoElement, RegisterComponent)]
pub struct Tab {
    div: Stateful<Div>,
    selected: bool,
    position: TabPosition,
    close_side: TabCloseSide,
    start_slot: Option<AnyElement>,
    end_slot: Option<AnyElement>,
    children: SmallVec<[AnyElement; 2]>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            div: div()
                .id(id.clone())
                .debug_selector(|| format!("TAB-{}", id)),
            selected: false,
            position: TabPosition::First,
            close_side: TabCloseSide::End,
            start_slot: None,
            end_slot: None,
            children: SmallVec::new(),
        }
    }

    pub fn position(mut self, position: TabPosition) -> Self {
        self.position = position;
        self
    }

    pub fn close_side(mut self, close_side: TabCloseSide) -> Self {
        self.close_side = close_side;
        self
    }

    pub fn start_slot<E: IntoElement>(mut self, element: impl Into<Option<E>>) -> Self {
        self.start_slot = element.into().map(IntoElement::into_any_element);
        self
    }

    pub fn end_slot<E: IntoElement>(mut self, element: impl Into<Option<E>>) -> Self {
        self.end_slot = element.into().map(IntoElement::into_any_element);
        self
    }

    pub fn content_height(cx: &App) -> Pixels {
        DynamicSpacing::Base32.px(cx) - px(1.)
    }

    pub fn container_height(cx: &App) -> Pixels {
        DynamicSpacing::Base32.px(cx)
    }
}

impl InteractiveElement for Tab {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.div.interactivity()
    }
}

impl StatefulInteractiveElement for Tab {}

impl Toggleable for Tab {
    fn toggle_state(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl ParentElement for Tab {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for Tab {
    #[allow(refining_impl_trait)]
    fn render(self, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let colors = cx.theme().colors();
        let text_color = if self.selected {
            crate::brand_on_solid(cx)
        } else {
            colors.text_muted
        };

        let (start_slot, end_slot) = {
            let start_slot = h_flex()
                .size(START_TAB_SLOT_SIZE)
                .justify_center()
                .children(self.start_slot);

            let end_slot = h_flex()
                .size(END_TAB_SLOT_SIZE)
                .justify_center()
                .children(self.end_slot);

            match self.close_side {
                TabCloseSide::End => (start_slot, end_slot),
                TabCloseSide::Start => (end_slot, start_slot),
            }
        };

        // No borders between tabs: each tab's surface is inset so neighbours
        // never touch. The selected tab is a solid pill in the app's orange
        // with its text in the colour that reads on it -- one mark, plain at a
        // glance whether the pane is focused or not, where an unfocused pane
        // mutes every label alike.
        let inset = DynamicSpacing::Base02.px(cx);
        self.div
            .h(Tab::container_height(cx))
            .relative()
            .flex_none()
            .px(inset)
            .when(self.position == TabPosition::First, |this| {
                this.pl(inset * 2.)
            })
            .cursor_pointer()
            .child(
                h_flex()
                    .group("")
                    .relative()
                    .h(Tab::content_height(cx) - inset * 2.)
                    .my(inset)
                    .px(DynamicSpacing::Base06.px(cx))
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .rounded_md()
                    .text_color(text_color)
                    .when(self.selected, |content| {
                        content
                            .debug_selector(|| "TAB_ACTIVE_FILL".into())
                            .bg(crate::brand_solid(cx))
                    })
                    .when(!self.selected, |content| {
                        content.hover(|content| {
                            content
                                .bg(colors.ghost_element_hover)
                                .text_color(colors.text)
                        })
                    })
                    .child(start_slot)
                    .children(self.children)
                    .child(end_slot),
            )
    }
}

impl Component for Tab {
    fn scope() -> ComponentScope {
        ComponentScope::Navigation
    }

    fn description() -> Option<&'static str> {
        Some(
            "A tab component that can be used in a tabbed interface, supporting different positions and states.",
        )
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        Some(
            v_flex()
                .gap_6()
                .children(vec![example_group_with_title(
                    "Variations",
                    vec![
                        single_example(
                            "Default",
                            Tab::new("default").child("Default Tab").into_any_element(),
                        ),
                        single_example(
                            "Selected",
                            Tab::new("selected")
                                .toggle_state(true)
                                .child("Selected Tab")
                                .into_any_element(),
                        ),
                        single_example(
                            "First",
                            Tab::new("first")
                                .position(TabPosition::First)
                                .child("First Tab")
                                .into_any_element(),
                        ),
                        single_example(
                            "Middle",
                            Tab::new("middle")
                                .position(TabPosition::Middle(Ordering::Equal))
                                .child("Middle Tab")
                                .into_any_element(),
                        ),
                        single_example(
                            "Last",
                            Tab::new("last")
                                .position(TabPosition::Last)
                                .child("Last Tab")
                                .into_any_element(),
                        ),
                    ],
                )])
                .into_any_element(),
        )
    }
}
