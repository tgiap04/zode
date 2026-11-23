//! What the issue list shows instead of rows: loading, empty, no match and
//! failure, plus the search caption and "Load more".

use gpui::{AnyElement, px};
use ui::{Callout, Tooltip, prelude::*};

use crate::issues::issue_list::{IssueList, ListLoad, ListPlaceholder};
use crate::issues::issue_row_render::ROW_HEIGHT;
use crate::untrusted_text::reveal_hidden_characters;

fn skeleton_row(index: usize, cx: &App) -> AnyElement {
    let colors = cx.theme().colors();
    let bar = |width: f32| {
        div()
            .h(px(10.))
            .w(px(width))
            .rounded_sm()
            .bg(colors.element_background)
    };
    h_flex()
        .h(px(ROW_HEIGHT))
        .px_2()
        .gap_2()
        .border_b_1()
        .border_color(colors.border_variant)
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(colors.element_background),
        )
        .child(bar([220., 160., 260.][index % 3]))
        .child(div().flex_1())
        .child(bar(48.))
        .into_any_element()
}

fn empty_state(
    icon: IconName,
    headline: String,
    hint: &'static str,
    action: Option<AnyElement>,
) -> AnyElement {
    v_flex()
        .w_full()
        .py_8()
        .px_4()
        .gap_2()
        .items_center()
        .child(Icon::new(icon).size(IconSize::Medium).color(Color::Muted))
        .child(Label::new(headline))
        .child(Label::new(hint).size(LabelSize::Small).color(Color::Muted))
        .children(action)
        .into_any_element()
}

impl IssueList {
    pub(crate) fn render_placeholder(
        &self,
        no_visible_rows: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        Some(match self.placeholder_for(no_visible_rows)? {
            ListPlaceholder::Loading => v_flex()
                .id("github-issues-loading")
                .tooltip(Tooltip::text("Loading issues…"))
                .children((0..3).map(|index| skeleton_row(index, cx)))
                .into_any_element(),
            ListPlaceholder::Failed(error) => div()
                .p_2()
                .child(
                    Callout::new()
                        .severity(Severity::Warning)
                        .title("Could not load the issues")
                        .description(error.to_string())
                        .actions_slot(
                            Button::new("github-issues-retry", "Retry")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
                        ),
                )
                .into_any_element(),
            ListPlaceholder::NoIssues => empty_state(
                IconName::TodoProgress,
                "No issues here".to_string(),
                "Change the filters, or open a new one.",
                Some(self.render_new_issue_button(cx)),
            ),
            ListPlaceholder::NoMatches(text) => empty_state(
                IconName::MagnifyingGlass,
                format!(
                    "No loaded issue matches “{}”",
                    reveal_hidden_characters(text.trim())
                ),
                "Press Enter to search all of the repository's issues on GitHub.",
                None,
            ),
            ListPlaceholder::NoSearchResults(text) => empty_state(
                IconName::MagnifyingGlass,
                format!(
                    "GitHub found no issues for “{}”",
                    reveal_hidden_characters(&text)
                ),
                "Try fewer or different words, or clear the search.",
                Some(
                    Button::new("github-issues-empty-clear-search", "Clear search")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx)))
                        .into_any_element(),
                ),
            ),
        })
    }

    /// Shown while the rows are GitHub's search results rather than the list.
    pub(crate) fn render_search_caption(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let searched = self.searched()?;
        Some(
            h_flex()
                .h(px(28.))
                .px_2()
                .gap_1()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    Icon::new(IconName::MagnifyingGlass)
                        .size(IconSize::XSmall)
                        .color(Color::Muted),
                )
                .child(
                    Label::new(format!(
                        "GitHub search results for “{}”",
                        reveal_hidden_characters(searched)
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .single_line()
                    .truncate(),
                )
                .child(div().flex_1())
                .child(
                    Button::new("github-issues-back-to-list", "Back to list")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn render_load_more(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.has_more() {
            return None;
        }
        let loading = *self.load_state() == ListLoad::LoadingMore;
        Some(
            h_flex()
                .w_full()
                .h(px(ROW_HEIGHT))
                .justify_center()
                .child(
                    Button::new(
                        "github-issues-load-more",
                        if loading { "Loading…" } else { "Load more" },
                    )
                    .label_size(LabelSize::Small)
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, _window, cx| this.load_more(cx))),
                )
                .into_any_element(),
        )
    }
}
