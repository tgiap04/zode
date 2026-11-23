use github::{IssueStateFilter, Label as IssueLabel};
use gpui::{AppContext as _, Context, Entity, Hsla, IntoElement, Render, Subscription, Window, px};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time_format::TimestampFormat;
use ui::{
    ContextMenu, DropdownMenu, DropdownStyle, IconPosition, ToggleButtonGroup,
    ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip, prelude::*,
};
use ui_input::{ErasedEditorEvent, InputField};

use crate::issues::issue_filters::{MineFilter, label_color};
use crate::issues::issue_list::IssueList;
use crate::untrusted_text::reveal_hidden_characters;

pub(crate) const BAR_HEIGHT: f32 = 32.;

/// The search field, made on first draw since it needs a window.
pub(crate) struct SearchField {
    pub(crate) input: Entity<InputField>,
    _edited: Subscription,
}

/// `3 hours ago` for GitHub's ISO 8601 times, or the time as sent when it
/// does not parse.
pub fn updated_label(updated_at: &str, now: OffsetDateTime) -> String {
    match OffsetDateTime::parse(updated_at, &Rfc3339) {
        Ok(updated) => time_format::format_localized_timestamp(
            updated,
            now,
            time::UtcOffset::UTC,
            TimestampFormat::Relative,
        ),
        Err(_) => updated_at.to_string(),
    }
}

/// A menu row for a label: a check when chosen, the label's colour dot and
/// its name.
pub(crate) fn label_menu_entry(
    menu: ContextMenu,
    label: &IssueLabel,
    toggled: bool,
    handler: impl Fn(&mut Window, &mut App) + 'static,
) -> ContextMenu {
    let name = reveal_hidden_characters(&label.name).into_owned();
    let color = label.color.clone();
    menu.custom_entry(
        move |_window, cx| {
            let dot = label_color(&color).unwrap_or(cx.theme().colors().text_muted);
            h_flex()
                .w_full()
                .gap_2()
                .child(div().flex_none().w(px(14.)).when(toggled, |slot| {
                    slot.child(Icon::new(IconName::Check).size(IconSize::Small))
                }))
                .child(div().flex_none().size(px(10.)).rounded_full().bg(dot))
                .child(Label::new(name.clone()).single_line())
                .into_any_element()
        },
        handler,
    )
}

/// What a filter trigger reads: an icon, an optional colour dot and the
/// current choice, accented while the filter narrows the list.
pub(crate) fn filter_trigger(
    icon: IconName,
    dot: Option<Hsla>,
    text: String,
    active: bool,
) -> AnyElement {
    h_flex()
        .gap_1()
        .child(Icon::new(icon).size(IconSize::XSmall).color(if active {
            Color::Accent
        } else {
            Color::Muted
        }))
        .when_some(dot, |trigger, dot| {
            trigger.child(div().flex_none().size(px(8.)).rounded_full().bg(dot))
        })
        .child(
            Label::new(text)
                .size(LabelSize::Small)
                .color(if active { Color::Default } else { Color::Muted })
                .single_line(),
        )
        .into_any_element()
}

impl IssueList {
    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputField> {
        if let Some(field) = &self.search_field {
            return field.input.clone();
        }
        let input = cx.new(|cx| {
            InputField::new(window, cx, "Filter issues — Enter searches GitHub")
                .start_icon(IconName::MagnifyingGlass)
        });
        let this = cx.entity().downgrade();
        let editor = input.read(cx).editor().clone();
        let edited = editor.subscribe(
            Box::new({
                let input = input.clone();
                move |event, _window, cx| {
                    if event == ErasedEditorEvent::BufferEdited {
                        let text = input.read(cx).text(cx);
                        this.update(cx, |this, cx| this.set_query(text, cx)).ok();
                    }
                }
            }),
            window,
            cx,
        );
        self.search_field = Some(SearchField {
            input: input.clone(),
            _edited: edited,
        });
        input
    }

    /// Empties the search field and, after a GitHub search, goes back to
    /// the plain list.
    pub(crate) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.search_field {
            let input = field.input.clone();
            input.update(cx, |input, cx| input.clear(window, cx));
        }
        self.set_query(String::new(), cx);
        if self.searched().is_some() {
            self.search(cx);
        }
    }

    fn render_state_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.filters().state;
        let states = [
            (IssueStateFilter::Open, "Open"),
            (IssueStateFilter::Closed, "Closed"),
            (IssueStateFilter::All, "All"),
        ];
        let selected = states
            .iter()
            .position(|(state, _)| *state == current)
            .unwrap_or(0);
        let this = cx.entity().downgrade();
        let buttons = states.map(|(state, text)| {
            let this = this.clone();
            ToggleButtonSimple::new(text, move |_, _window, cx| {
                this.update(cx, |this, cx| this.set_state(state, cx)).ok();
            })
        });
        ToggleButtonGroup::single_row("github-issues-state", buttons)
            .style(ToggleButtonGroupStyle::Outlined)
            .selected_index(selected)
            .auto_width()
            .into_any_element()
    }

    fn render_mine_filter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let current = self.filters().mine;
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |menu, _, _| {
            let mut menu =
                menu.toggleable_entry("Anyone", current.is_none(), IconPosition::Start, None, {
                    let this = this.clone();
                    move |_, cx| {
                        this.update(cx, |this, cx| this.set_mine(None, cx)).ok();
                    }
                });
            for mine in MineFilter::ALL {
                let this = this.clone();
                menu = menu.toggleable_entry(
                    mine.label(),
                    current == Some(mine),
                    IconPosition::Start,
                    None,
                    move |_, cx| {
                        this.update(cx, |this, cx| this.set_mine(Some(mine), cx))
                            .ok();
                    },
                );
            }
            menu
        });
        let text = current.map_or("Anyone", MineFilter::label).to_string();
        DropdownMenu::new_with_element(
            "github-issues-mine",
            filter_trigger(IconName::Person, None, text, current.is_some()),
            menu,
        )
        .style(DropdownStyle::Ghost)
        .trigger_size(ButtonSize::Compact)
        .trigger_tooltip(Tooltip::text("Whose issues"))
        .into_any_element()
    }

    fn render_label_filter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let current = self.filters().label.clone();
        let labels = self.labels().to_vec();
        let dot = current.as_ref().and_then(|name| {
            labels
                .iter()
                .find(|label| &label.name == name)
                .and_then(|label| label_color(&label.color))
        });
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, {
            let current = current.clone();
            move |menu, _, _| {
                let mut menu = menu.toggleable_entry(
                    "Any label",
                    current.is_none(),
                    IconPosition::Start,
                    None,
                    {
                        let this = this.clone();
                        move |_, cx| {
                            this.update(cx, |this, cx| this.set_label(None, cx)).ok();
                        }
                    },
                );
                for label in &labels {
                    let this = this.clone();
                    let name = label.name.clone();
                    let toggled = current.as_ref() == Some(&label.name);
                    menu = label_menu_entry(menu, label, toggled, move |_, cx| {
                        let name = name.clone();
                        this.update(cx, |this, cx| this.set_label(Some(name), cx))
                            .ok();
                    });
                }
                menu
            }
        });
        let text = current.as_deref().map_or("Label".to_string(), |label| {
            reveal_hidden_characters(label).into_owned()
        });
        DropdownMenu::new_with_element(
            "github-issues-label",
            filter_trigger(IconName::Bookmark, dot, text, current.is_some()),
            menu,
        )
        .style(DropdownStyle::Ghost)
        .trigger_size(ButtonSize::Compact)
        .trigger_tooltip(Tooltip::text("Filter by label"))
        .into_any_element()
    }

    fn render_milestone_filter(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .filters()
            .milestone
            .as_ref()
            .map(|milestone| milestone.number);
        let milestones = self.milestones().to_vec();
        let this = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, {
            move |menu, _, _| {
                let mut menu = menu.toggleable_entry(
                    "Any milestone",
                    current.is_none(),
                    IconPosition::Start,
                    None,
                    {
                        let this = this.clone();
                        move |_, cx| {
                            this.update(cx, |this, cx| this.set_milestone(None, cx))
                                .ok();
                        }
                    },
                );
                for milestone in milestones {
                    let this = this.clone();
                    let shown = reveal_hidden_characters(&milestone.title).into_owned();
                    menu = menu.toggleable_entry(
                        shown,
                        current == Some(milestone.number),
                        IconPosition::Start,
                        None,
                        move |_, cx| {
                            let milestone = milestone.clone();
                            this.update(cx, |this, cx| this.set_milestone(Some(milestone), cx))
                                .ok();
                        },
                    );
                }
                menu
            }
        });
        let text = self
            .filters()
            .milestone
            .as_ref()
            .map_or("Milestone".to_string(), |milestone| {
                reveal_hidden_characters(&milestone.title).into_owned()
            });
        DropdownMenu::new_with_element(
            "github-issues-milestone",
            filter_trigger(IconName::Pin, None, text, current.is_some()),
            menu,
        )
        .style(DropdownStyle::Ghost)
        .trigger_size(ButtonSize::Compact)
        .trigger_tooltip(Tooltip::text("Filter by milestone"))
        .into_any_element()
    }

    /// Only when the repository's owner has issue types.
    fn render_type_filter(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.offers_type_filter() {
            return None;
        }
        let current = self.filters().issue_type.clone();
        let this = cx.entity().downgrade();
        let types = self.issue_types().to_vec();
        let menu = ContextMenu::build(window, cx, {
            let current = current.clone();
            move |menu, _, _| {
                let mut menu = menu.toggleable_entry(
                    "Any type",
                    current.is_none(),
                    IconPosition::Start,
                    None,
                    {
                        let this = this.clone();
                        move |_, cx| {
                            this.update(cx, |this, cx| this.set_issue_type_filter(None, cx))
                                .ok();
                        }
                    },
                );
                for issue_type in types {
                    let this = this.clone();
                    let shown = reveal_hidden_characters(&issue_type.name).into_owned();
                    let toggled = current.as_ref() == Some(&issue_type.name);
                    menu = menu.toggleable_entry(
                        shown,
                        toggled,
                        IconPosition::Start,
                        None,
                        move |_, cx| {
                            let name = issue_type.name.clone();
                            this.update(cx, |this, cx| this.set_issue_type_filter(Some(name), cx))
                                .ok();
                        },
                    );
                }
                menu
            }
        });
        let active = current.is_some();
        let text = current.map_or("Type".to_string(), |name| {
            reveal_hidden_characters(&name).into_owned()
        });
        Some(
            DropdownMenu::new_with_element(
                "github-issues-type",
                filter_trigger(IconName::SquareDot, None, text, active),
                menu,
            )
            .style(DropdownStyle::Ghost)
            .trigger_size(ButtonSize::Compact)
            .trigger_tooltip(Tooltip::text("Filter by issue type"))
            .into_any_element(),
        )
    }

    fn render_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input = self.search_input(window, cx);
        let has_text = !self.query().is_empty() || self.searched().is_some();
        let border = cx.theme().colors().border_variant;
        v_flex()
            .flex_none()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .h(px(BAR_HEIGHT))
                    .px_2()
                    .gap_2()
                    .child(self.render_state_toggle(cx))
                    .child(div().flex_1())
                    .child(self.render_new_issue_button(cx))
                    .child(
                        IconButton::new("github-issues-reload", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text("Reload issues"))
                            .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
                    ),
            )
            .child(
                h_flex()
                    .min_h(px(BAR_HEIGHT))
                    .px_2()
                    .pb_1()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(180.))
                            .gap_1()
                            .key_context("GithubIssueSearch")
                            .on_action(
                                cx.listener(|this, _: &menu::Confirm, _window, cx| this.search(cx)),
                            )
                            .child(div().flex_1().child(input))
                            .when(has_text, |field| {
                                field.child(
                                    IconButton::new("github-issues-clear-search", IconName::Close)
                                        .icon_size(IconSize::Small)
                                        .tooltip(Tooltip::text("Clear the search"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.clear_search(window, cx)
                                        })),
                                )
                            }),
                    )
                    .child(self.render_mine_filter(window, cx))
                    .child(self.render_label_filter(window, cx))
                    .child(self.render_milestone_filter(window, cx))
                    .children(self.render_type_filter(window, cx)),
            )
            .into_any_element()
    }
}

impl Render for IssueList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = OffsetDateTime::now_utc();
        let mut rows = Vec::new();
        for (index, row) in self.tree_rows().iter().enumerate() {
            rows.push(self.render_row(index, row, now, cx));
            rows.extend(self.render_children_status(row));
        }
        let placeholder = self.render_placeholder(cx);
        let toolbar = self.render_toolbar(window, cx);
        let selection_bar = self.render_selection_bar(window, cx);
        let messages = self.render_write_messages();
        let context_menu = self.render_context_menu();
        let search_caption = self.render_search_caption(cx);
        let load_more = self.render_load_more(cx);
        v_flex()
            .size_full()
            .child(toolbar)
            .when(!messages.is_empty(), |column| {
                column.child(v_flex().px_2().pt_2().gap_1().children(messages))
            })
            .children(selection_bar)
            .children(search_caption)
            .child(
                v_flex()
                    .id("github-issue-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(placeholder)
                    .children(rows)
                    .children(load_more),
            )
            .children(context_menu)
    }
}
