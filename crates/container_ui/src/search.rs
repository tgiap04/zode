//! Narrowing the list to what was typed.
//!
//! Over every column the engine filled in, not only the name: a pod is as often
//! looked for by its namespace and a container by its image as either is by its
//! own name, and what is matched should not depend on which columns the width
//! happened to leave on screen.

use std::{
    cell::{OnceCell, RefCell},
    ops::Deref,
    rc::Rc,
};

use container::{Resource, ResourceKind, RunState};
use gpui::{Context, Entity, Subscription, Window};
use ui::prelude::*;
use ui_input::{ErasedEditorEvent, InputField};

use crate::container_panel::{ContainerPanel, ListState};

/// The rows an engine last answered with, and what narrowing them costs.
///
/// The panel narrows the list on every frame it draws, and a row that is busy
/// spins, which draws every frame. So what a frame cannot change is kept: the
/// text to look in is made once, the first time anything is typed, and the
/// answer to the last query is remembered. A new answer from the engine is a
/// new `Listing`, which is what ends both.
pub(crate) struct Listing {
    resources: Vec<Resource>,
    haystacks: OnceCell<Vec<String>>,
    last_answer: RefCell<Option<(String, Rc<[usize]>)>>,
}

impl Listing {
    pub(crate) fn new(resources: Vec<Resource>) -> Self {
        Self {
            resources,
            haystacks: OnceCell::new(),
            last_answer: RefCell::new(None),
        }
    }

    /// The positions of the rows that have every word of `query` somewhere in
    /// their name, details or state, ignoring case.
    ///
    /// Every word rather than the whole phrase, so `nginx running` finds the
    /// running nginx however the columns order those two facts.
    fn matching(&self, query: &str) -> Rc<[usize]> {
        if let Some((asked, answer)) = &*self.last_answer.borrow()
            && asked == query
        {
            return answer.clone();
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let haystacks = self
            .haystacks
            .get_or_init(|| self.resources.iter().map(haystack).collect());
        let answer: Rc<[usize]> = haystacks
            .iter()
            .enumerate()
            .filter(|(_, text)| words.iter().all(|word| text.contains(word.as_str())))
            .map(|(position, _)| position)
            .collect();
        *self.last_answer.borrow_mut() = Some((query.to_string(), answer.clone()));
        answer
    }
}

impl Deref for Listing {
    type Target = Vec<Resource>;

    fn deref(&self) -> &Vec<Resource> {
        &self.resources
    }
}

/// Everything about `resource` a query may name, lowercased, one field per
/// line. A query word has no whitespace in it, so it cannot match across two
/// fields.
fn haystack(resource: &Resource) -> String {
    let state = match resource.state {
        RunState::Running => Some("running"),
        RunState::Paused => Some("paused"),
        RunState::Stopped => Some("stopped"),
        RunState::Unknown => None,
    };
    std::iter::once(resource.name.as_str())
        .chain(resource.detail.iter().map(|(_, value)| value.as_str()))
        .chain(state)
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
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
    pub(crate) fn visible<'a>(&self, listing: &'a Listing, cx: &App) -> Vec<&'a Resource> {
        let query = self.query(cx);
        if query.is_empty() {
            return listing.iter().collect();
        }
        listing
            .matching(&query)
            .iter()
            .filter_map(|&position| listing.get(position))
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
    use super::Listing;
    use container::{Resource, ResourceKind, RunState};

    fn matches(resource: &Resource, query: &str) -> bool {
        !Listing::new(vec![resource.clone()])
            .matching(query)
            .is_empty()
    }

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

    fn container(name: &str, status: &str, state: RunState) -> Resource {
        Resource {
            kind: ResourceKind::Container,
            id: name.into(),
            name: name.into(),
            state,
            detail: vec![("Image", "nginx:latest".into()), ("Status", status.into())],
            parent: None,
        }
    }

    /// Docker's `Status` column says "Up 2 minutes", never "running", so the
    /// word the dot stands for has to be searchable on its own.
    #[test]
    fn a_container_is_found_by_its_state_whatever_its_status_text_says() {
        let up = container("web", "Up 2 minutes", RunState::Running);
        assert!(matches(&up, "running"));
        assert!(matches(&up, "web RUNNING"));
        assert!(!matches(&up, "stopped"));

        let paused = container("db", "Up 3 minutes (Paused)", RunState::Paused);
        assert!(matches(&paused, "paused"));
        assert!(!matches(&paused, "running"));

        let stopped = container("old", "Exited (0) 2 hours ago", RunState::Stopped);
        assert!(matches(&stopped, "stopped"));
        assert!(!matches(&stopped, "running"));
    }

    #[test]
    fn a_row_with_no_state_is_not_found_by_one() {
        let mut image = container("nginx", "", RunState::Unknown);
        image.kind = ResourceKind::Image;
        assert!(!matches(&image, "running"));
        assert!(matches(&image, "nginx"));
    }

    #[test]
    fn a_word_does_not_match_across_two_fields() {
        let split = pod("ab", "cd", "Pending");
        assert!(!matches(&split, "bc"));
        assert!(matches(&split, "ab cd"));
    }

    #[test]
    fn the_answer_to_the_same_query_is_worked_out_once() {
        let listing = Listing::new(vec![
            container("web", "Up", RunState::Running),
            container("db", "Up", RunState::Running),
        ]);
        let first = listing.matching("web");
        assert_eq!(&*first, &[0]);
        assert!(
            std::rc::Rc::ptr_eq(&first, &listing.matching("web")),
            "a second frame with the same query reuses the answer"
        );
        let other = listing.matching("db");
        assert_eq!(&*other, &[1], "a different query is answered afresh");
    }
}
