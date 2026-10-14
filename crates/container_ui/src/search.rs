//! Narrowing the list to what was typed.
//!
//! Over every column the engine filled in, not only the name: a pod is as often
//! looked for by its namespace and a container by its image as either is by its
//! own name, and what is matched should not depend on which columns the width
//! happened to leave on screen.

use container::{Resource, ResourceKind};
use gpui::{Context, Entity, Subscription, Window};
use ui::prelude::*;
use ui_input::{ErasedEditorEvent, InputField};

use crate::container_panel::{ContainerPanel, ListState};

/// Whether `resource` has every word of `query` somewhere in its name or its
/// details, ignoring case.
///
/// Every word rather than the whole phrase, so `nginx running` finds the
/// running nginx however the columns order those two facts.
pub(crate) fn matches(resource: &Resource, query: &str) -> bool {
    let haystack = std::iter::once(resource.name.as_str())
        .chain(resource.detail.iter().map(|(_, value)| value.as_str()))
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .all(|word| haystack.iter().any(|field| field.contains(&word)))
}

/// The field, once it exists, and the subscription that redraws the list as it
/// is typed into.
pub(crate) struct Search {
    input: Entity<InputField>,
    /// The kind the placeholder names, so it is rewritten only when that changes.
    placeholder_for: Option<ResourceKind>,
    _edited: Subscription,
}

impl ContainerPanel {
    /// What is typed in the search field, trimmed.
    pub(crate) fn query(&self, cx: &App) -> String {
        self.search
            .as_ref()
            .map(|search| search.input.read(cx).text(cx).trim().to_string())
            .unwrap_or_default()
    }

    /// The rows the list shows: all of them, or those the query matches.
    pub(crate) fn visible<'a>(&self, resources: &'a [Resource], cx: &App) -> Vec<&'a Resource> {
        let query = self.query(cx);
        resources
            .iter()
            .filter(|resource| query.is_empty() || matches(resource, &query))
            .collect()
    }

    /// The field, made on first draw: it needs a `Window`, and the standalone
    /// view is built where none can be had.
    pub(crate) fn search_field(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputField> {
        if let Some(search) = &self.search {
            return search.input.clone();
        }
        let input = cx
            .new(|cx| InputField::new(window, cx, "Search").start_icon(IconName::MagnifyingGlass));
        let this = cx.entity().downgrade();
        let editor = input.read(cx).editor().clone();
        let edited = editor.subscribe(
            Box::new(move |event, _window, cx| {
                if event == ErasedEditorEvent::BufferEdited {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            }),
            window,
            cx,
        );
        self.search = Some(Search {
            input: input.clone(),
            placeholder_for: None,
            _edited: edited,
        });
        input
    }

    /// Escape in the field empties it, so the whole list is one key away. With
    /// nothing typed it passes Escape on, so whatever above the panel answers
    /// Escape still does.
    pub(crate) fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(search) = &self.search else {
            cx.propagate();
            return;
        };
        if search.input.read(cx).text(cx).is_empty() {
            cx.propagate();
            return;
        }
        let editor = search.input.read(cx).editor().clone();
        editor.clear(window, cx);
        cx.notify();
    }

    /// Whether the field is drawn: over the list, and kept while a switch of
    /// kind or engine reloads it so a field being typed in does not vanish
    /// under the cursor. Not over a list the engine answered with nothing,
    /// unless something is typed -- a filter that matches nothing keeps its
    /// field so it can be changed.
    pub(crate) fn shows_search(&self, cx: &App) -> bool {
        if self.detail.is_some() {
            return false;
        }
        match &self.state {
            ListState::Failed(_) => false,
            ListState::Ready(resources) if resources.is_empty() => !self.query(cx).is_empty(),
            ListState::Ready(_) | ListState::Loading => true,
        }
    }

    pub(crate) fn render_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.search_field(window, cx);
        let kind = self.active_kind;
        if let Some(search) = self.search.as_mut()
            && search.placeholder_for != Some(kind)
        {
            search.placeholder_for = Some(kind);
            let editor = input.read(cx).editor().clone();
            editor.set_placeholder_text(
                &format!("Search {}\u{2026}", kind.label().to_lowercase()),
                window,
                cx,
            );
        }
        h_flex()
            .p_1p5()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .on_action(
                cx.listener(|this, _: &menu::Cancel, window, cx| this.clear_search(window, cx)),
            )
            .child(div().flex_1().child(input))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::matches;
    use container::{Resource, ResourceKind, RunState};

    fn pod(name: &str, namespace: &str, status: &str) -> Resource {
        Resource {
            kind: ResourceKind::Pod,
            id: name.into(),
            name: name.into(),
            state: RunState::Running,
            detail: vec![("Namespace", namespace.into()), ("Status", status.into())],
            parent: None,
        }
    }

    #[test]
    fn a_word_matches_the_name_or_any_detail_whatever_its_case() {
        let coredns = pod("coredns-5d78c9869d-x2kqp", "kube-system", "Running");
        assert!(matches(&coredns, "coredns"));
        assert!(matches(&coredns, "KUBE-SYSTEM"));
        assert!(matches(&coredns, "x2kq"));
        assert!(matches(&coredns, "running"));
        assert!(!matches(&coredns, "nginx"));
    }

    #[test]
    fn every_word_must_match_somewhere() {
        let coredns = pod("coredns", "kube-system", "Running");
        assert!(matches(&coredns, "kube-system   coredns"));
        assert!(matches(&coredns, "running coredns"));
        assert!(
            !matches(&coredns, "coredns pending"),
            "one word that matches nothing rules the row out"
        );
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert!(matches(&pod("a", "b", "c"), ""));
        assert!(matches(&pod("a", "b", "c"), "   "));
    }
}
