//! The one-click filters beside the search field: running only, one compose
//! project, dangling images.
//!
//! Kept per kind on the panel, so narrowing the containers does not leak onto
//! the images tab and survives a trip to another tab and back. Not saved to
//! settings: there is no settings key for this panel, and a filter left on from
//! last week reads as missing rows.

use std::collections::BTreeSet;

use container::{Resource, ResourceKind, RunState};
use gpui::{Anchor, AnyElement, Context};
use ui::{ContextMenu, PopoverMenu, prelude::*};

use crate::container_panel::{ContainerPanel, ListState};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct QuickFilter {
    pub(crate) running_only: bool,
    pub(crate) dangling_only: bool,
    pub(crate) compose_project: Option<String>,
}

/// Which filters make sense for a kind. A data question rather than an engine
/// one: a Kubernetes pod and a container are both "running or not", and neither
/// has a dangling state to speak of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offered {
    pub(crate) running: bool,
    pub(crate) compose: bool,
    pub(crate) dangling: bool,
}

impl Offered {
    pub(crate) fn for_kind(kind: ResourceKind) -> Self {
        match kind {
            ResourceKind::Container => Self {
                running: true,
                compose: true,
                dangling: false,
            },
            ResourceKind::Pod => Self {
                running: true,
                ..Self::default()
            },
            ResourceKind::Image => Self {
                dangling: true,
                ..Self::default()
            },
            ResourceKind::Volume | ResourceKind::Network | ResourceKind::ComposeProject => {
                Self::default()
            }
        }
    }

    fn any(self) -> bool {
        self.running || self.compose || self.dangling
    }
}

impl QuickFilter {
    pub(crate) fn is_active(&self) -> bool {
        self.running_only || self.dangling_only || self.compose_project.is_some()
    }

    pub(crate) fn admits(&self, resource: &Resource) -> bool {
        if self.running_only && resource.state != RunState::Running {
            return false;
        }
        if self.dangling_only
            && !resource
                .detail
                .iter()
                .any(|(label, value)| *label == "Dangling" && value == "yes")
        {
            return false;
        }
        match &self.compose_project {
            Some(project) => resource
                .compose
                .as_ref()
                .is_some_and(|labels| &labels.project == project),
            None => true,
        }
    }
}

/// Every compose project some row belongs to, sorted, for the dropdown.
pub(crate) fn compose_projects(resources: &[Resource]) -> Vec<String> {
    resources
        .iter()
        .filter_map(|resource| resource.compose.as_ref())
        .map(|labels| labels.project.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl ContainerPanel {
    /// The filter of the kind on screen.
    pub(crate) fn quick_filter(&self) -> QuickFilter {
        self.filters
            .get(&self.active_kind)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn quick_filter_active(&self) -> bool {
        self.filters
            .get(&self.active_kind)
            .is_some_and(QuickFilter::is_active)
    }

    fn change_quick_filter(
        &mut self,
        change: impl FnOnce(&mut QuickFilter),
        cx: &mut Context<Self>,
    ) {
        let filter = self.filters.entry(self.active_kind).or_default();
        change(filter);
        if !filter.is_active() {
            self.filters.remove(&self.active_kind);
        }
        cx.notify();
    }

    pub(crate) fn toggle_running_only(&mut self, cx: &mut Context<Self>) {
        self.change_quick_filter(|filter| filter.running_only = !filter.running_only, cx);
    }

    pub(crate) fn toggle_dangling_only(&mut self, cx: &mut Context<Self>) {
        self.change_quick_filter(|filter| filter.dangling_only = !filter.dangling_only, cx);
    }

    pub(crate) fn choose_compose_project(
        &mut self,
        project: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.change_quick_filter(|filter| filter.compose_project = project, cx);
    }

    /// The toggles for the kind on screen, or `None` when it has none.
    pub(crate) fn render_quick_filters(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let offered = Offered::for_kind(self.active_kind);
        let ListState::Ready(resources) = &self.state else {
            return None;
        };
        if !offered.any() {
            return None;
        }
        let filter = self.quick_filter();
        let projects = if offered.compose {
            compose_projects(resources)
        } else {
            Vec::new()
        };
        // A project chosen and then gone from the list is still offered, so the
        // filter that is hiding every row can be seen and cleared.
        let show_projects = !projects.is_empty() || filter.compose_project.is_some();

        Some(
            h_flex()
                .gap_1()
                .when(offered.running, |bar| {
                    bar.child(
                        Button::new("container-filter-running", "Running")
                            .label_size(LabelSize::Small)
                            .toggle_state(filter.running_only)
                            .start_icon(
                                Icon::new(IconName::PlayFilled)
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .on_click(
                                cx.listener(|this, _, _window, cx| this.toggle_running_only(cx)),
                            ),
                    )
                })
                .when(offered.dangling, |bar| {
                    bar.child(
                        Button::new("container-filter-dangling", "Dangling")
                            .label_size(LabelSize::Small)
                            .toggle_state(filter.dangling_only)
                            .start_icon(
                                Icon::new(IconName::Blocks)
                                    .size(IconSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .on_click(
                                cx.listener(|this, _, _window, cx| this.toggle_dangling_only(cx)),
                            ),
                    )
                })
                .when(show_projects, |bar| {
                    bar.child(self.render_project_picker(projects, filter, cx))
                })
                .into_any_element(),
        )
    }

    fn render_project_picker(
        &self,
        projects: Vec<String>,
        filter: QuickFilter,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handle = cx.entity().downgrade();
        let chosen = filter.compose_project;
        let label = chosen.clone().unwrap_or_else(|| "All projects".to_string());
        let mut projects = projects;
        if let Some(chosen) = &chosen
            && !projects.contains(chosen)
        {
            projects.push(chosen.clone());
        }
        PopoverMenu::new("container-project-picker")
            .trigger_with_tooltip(
                Button::new("container-project-trigger", label)
                    .label_size(LabelSize::Small)
                    .toggle_state(chosen.is_some())
                    .start_icon(
                        Icon::new(IconName::ListTree)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .end_icon(
                        Icon::new(IconName::ChevronDown)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    ),
                ui::Tooltip::text("Only one compose project"),
            )
            .anchor(Anchor::TopRight)
            .menu(move |window, cx| {
                let projects = projects.clone();
                let chosen = chosen.clone();
                let handle = handle.clone();
                Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                    let all = handle.clone();
                    menu = menu.toggleable_entry(
                        "All projects",
                        chosen.is_none(),
                        IconPosition::Start,
                        None,
                        move |_window, cx| {
                            all.update(cx, |panel, cx| panel.choose_compose_project(None, cx))
                                .ok();
                        },
                    );
                    for project in projects.iter() {
                        let handle = handle.clone();
                        let picked = project.clone();
                        menu = menu.toggleable_entry(
                            project.clone(),
                            chosen.as_deref() == Some(project.as_str()),
                            IconPosition::Start,
                            None,
                            move |_window, cx| {
                                let project = picked.clone();
                                handle
                                    .update(cx, |panel, cx| {
                                        panel.choose_compose_project(Some(project), cx)
                                    })
                                    .ok();
                            },
                        );
                    }
                    menu
                }))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use container::resource::ComposeLabels;

    fn container(name: &str, state: RunState, project: Option<&str>) -> Resource {
        Resource {
            kind: ResourceKind::Container,
            id: name.into(),
            name: name.into(),
            state,
            detail: Vec::new(),
            parent: None,
            compose: project.map(|project| ComposeLabels {
                project: project.into(),
                service: "svc".into(),
                working_dir: None,
                config_files: Vec::new(),
            }),
        }
    }

    fn image(name: &str, dangling: bool) -> Resource {
        Resource {
            kind: ResourceKind::Image,
            id: name.into(),
            name: name.into(),
            state: RunState::Unknown,
            detail: if dangling {
                vec![("Dangling", "yes".into())]
            } else {
                Vec::new()
            },
            parent: None,
            compose: None,
        }
    }

    #[test]
    fn no_filter_admits_everything() {
        let filter = QuickFilter::default();
        assert!(!filter.is_active());
        assert!(filter.admits(&container("a", RunState::Stopped, None)));
        assert!(filter.admits(&image("i", false)));
    }

    #[test]
    fn running_only_keeps_exactly_the_running() {
        let filter = QuickFilter {
            running_only: true,
            ..QuickFilter::default()
        };
        assert!(filter.admits(&container("a", RunState::Running, None)));
        for state in [RunState::Stopped, RunState::Paused, RunState::Unknown] {
            assert!(!filter.admits(&container("a", state, None)), "{state:?}");
        }
    }

    #[test]
    fn a_compose_project_keeps_only_its_members() {
        let filter = QuickFilter {
            compose_project: Some("shop".into()),
            ..QuickFilter::default()
        };
        assert!(filter.admits(&container("web", RunState::Running, Some("shop"))));
        assert!(!filter.admits(&container("web", RunState::Running, Some("blog"))));
        assert!(!filter.admits(&container("loose", RunState::Running, None)));
    }

    #[test]
    fn dangling_only_keeps_the_untagged_images() {
        let filter = QuickFilter {
            dangling_only: true,
            ..QuickFilter::default()
        };
        assert!(filter.admits(&image("<none>", true)));
        assert!(!filter.admits(&image("nginx:latest", false)));
    }

    #[test]
    fn filters_combine_as_and() {
        let filter = QuickFilter {
            running_only: true,
            compose_project: Some("shop".into()),
            ..QuickFilter::default()
        };
        assert!(filter.admits(&container("a", RunState::Running, Some("shop"))));
        assert!(!filter.admits(&container("b", RunState::Stopped, Some("shop"))));
        assert!(!filter.admits(&container("c", RunState::Running, Some("blog"))));
    }

    #[test]
    fn projects_are_listed_once_and_sorted() {
        let rows = [
            container("a", RunState::Running, Some("shop")),
            container("b", RunState::Running, Some("blog")),
            container("c", RunState::Running, Some("shop")),
            container("d", RunState::Running, None),
        ];
        assert_eq!(compose_projects(&rows), ["blog", "shop"]);
    }

    #[test]
    fn only_the_kinds_with_something_to_narrow_offer_a_filter() {
        assert!(Offered::for_kind(ResourceKind::Container).running);
        assert!(Offered::for_kind(ResourceKind::Container).compose);
        assert!(Offered::for_kind(ResourceKind::Pod).running);
        assert!(!Offered::for_kind(ResourceKind::Pod).compose);
        assert!(Offered::for_kind(ResourceKind::Image).dangling);
        for kind in [
            ResourceKind::Volume,
            ResourceKind::Network,
            ResourceKind::ComposeProject,
        ] {
            assert!(!Offered::for_kind(kind).any(), "{kind:?}");
        }
    }
}
