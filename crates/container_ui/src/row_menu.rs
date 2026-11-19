//! The right-click menus: on a row, on several selected rows, on the empty list.
//!
//! What a menu offers is decided by [`row_menu_items`] and [`bulk_menu_items`],
//! plain functions of the rows and of what the backend said it can do. "An image
//! has no Start" and "a stopped container has no shell" are therefore facts a
//! test states without taking a menu apart, and nothing here asks which engine it
//! is holding -- the backend's answers arrive as [`RowCapabilities`].

use container::resource::row_key;
use container::{Op, Resource, ResourceAction, ResourceKind, RunState};
use gpui::{
    App, ClipboardItem, Context, DismissEvent, Focusable as _, Modifiers, Pixels, Point,
    WeakEntity, Window,
};
use ui::{ContextMenu, ContextMenuEntry, DocumentationSide, prelude::*};

use crate::compose_actions::{ComposeEntry, is_service_row};
use crate::container_panel::ContainerPanel;
use crate::detail_tabs::DetailTab;
use crate::network_membership::Membership;
use crate::published_ports::{address_of, copyable_addresses, port_url, published_ports};
use crate::render::{action_icon, action_is_useful};
use crate::terminal::TerminalIntent;

/// What the backend allows for the kind on screen, gathered once per menu.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowCapabilities {
    pub(crate) actions: &'static [ResourceAction],
    pub(crate) logs: bool,
    pub(crate) shell: bool,
    pub(crate) detail: bool,
    pub(crate) removable: bool,
    pub(crate) writes: Writes,
}

/// The write-side entries the backend offers, each asked of it by name.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Writes {
    pub(crate) run: bool,
    pub(crate) tag: bool,
    pub(crate) history: bool,
    pub(crate) connect: bool,
}

/// The actions a selection of rows can be given together. Pause and resume are
/// left to a row of their own: freezing a handful of containers at once is rare
/// and one wrong click on it is not obvious.
const BULK_ACTIONS: [ResourceAction; 3] = [
    ResourceAction::Start,
    ResourceAction::Stop,
    ResourceAction::Restart,
];

/// A published port, ready to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortLink {
    pub(crate) label: String,
    pub(crate) url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuItem {
    Act(ResourceAction),
    FollowLogs,
    OpenShell,
    OpenDetail,
    OpenPorts(Vec<PortLink>),
    /// The published ports as text to paste: `host:port` where the host's
    /// address is known, the port alone where it is not.
    CopyAddress(Vec<String>),
    CopyId,
    CopyName,
    CopyImage,
    CopyReference,
    Remove,
    /// An image: start a container from it, give it another name, read its layers.
    RunImage,
    TagImage,
    ImageHistory,
    /// A network: put a container on it, take one off.
    ConnectContainer,
    DisconnectContainer,
    /// A compose entry, possibly unavailable: shown with its reason rather than
    /// left out, so nobody has to guess why Up is missing.
    Compose(ComposeEntry),
    Separator,
}

impl MenuItem {
    fn label(&self, count: usize) -> String {
        match self {
            MenuItem::Act(action) => action.label().to_string(),
            MenuItem::FollowLogs => "Follow Logs".to_string(),
            MenuItem::OpenShell => "Open Shell".to_string(),
            MenuItem::OpenDetail => "Open Details".to_string(),
            MenuItem::OpenPorts(_) => "Open Port".to_string(),
            MenuItem::CopyAddress(_) => "Copy Address".to_string(),
            MenuItem::CopyId => "Copy ID".to_string(),
            MenuItem::CopyName if count > 1 => "Copy Names".to_string(),
            MenuItem::CopyName => "Copy Name".to_string(),
            MenuItem::CopyImage => "Copy Image".to_string(),
            MenuItem::CopyReference => "Copy Reference".to_string(),
            MenuItem::Remove if count > 1 => format!("Remove {count}\u{2026}"),
            MenuItem::Remove => "Remove\u{2026}".to_string(),
            MenuItem::RunImage => "Run Container\u{2026}".to_string(),
            MenuItem::TagImage => "Tag\u{2026}".to_string(),
            MenuItem::ImageHistory => "History".to_string(),
            MenuItem::ConnectContainer => "Connect Container\u{2026}".to_string(),
            MenuItem::DisconnectContainer => "Disconnect Container\u{2026}".to_string(),
            MenuItem::Compose(entry) => entry.command.label().to_string(),
            MenuItem::Separator => String::new(),
        }
    }

    fn icon(&self) -> IconName {
        match self {
            MenuItem::Act(action) => action_icon(*action),
            MenuItem::FollowLogs => IconName::ListTree,
            MenuItem::OpenShell => IconName::Terminal,
            MenuItem::OpenDetail => IconName::Info,
            MenuItem::OpenPorts(_) => IconName::Public,
            MenuItem::CopyId => IconName::Hash,
            MenuItem::CopyName | MenuItem::CopyImage | MenuItem::CopyAddress(_) => IconName::Copy,
            MenuItem::CopyReference => IconName::BookCopy,
            MenuItem::Remove => IconName::Trash,
            MenuItem::RunImage => IconName::PlayFilled,
            MenuItem::TagImage => IconName::Bookmark,
            MenuItem::ImageHistory => IconName::Clock,
            MenuItem::ConnectContainer => IconName::Link,
            MenuItem::DisconnectContainer => IconName::Disconnected,
            MenuItem::Compose(entry) => entry.command.icon(),
            MenuItem::Separator => IconName::Blocks,
        }
    }
}

/// Joins the groups that have anything in them with a separator between each.
pub(crate) fn grouped(groups: Vec<Vec<MenuItem>>) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !items.is_empty() {
            items.push(MenuItem::Separator);
        }
        items.extend(group);
    }
    items
}

/// Whether a person calls `image` by a name worth copying: a dangling image has
/// only the placeholder Docker prints, which is no reference to anything.
fn has_reference(resource: &Resource) -> bool {
    !resource
        .detail
        .iter()
        .any(|(label, value)| *label == "Dangling" && value == "yes")
}

fn image_of(resource: &Resource) -> Option<&str> {
    resource
        .detail
        .iter()
        .find(|(label, _)| *label == "Image")
        .map(|(_, value)| value.as_str())
        .filter(|image| !image.is_empty())
}

/// What one row's menu offers, in order.
///
/// `address` is where a browser on this machine reaches what the engine's host
/// publishes, `None` when that is not known: it decides where an Open Port entry
/// points, and whether there is an honest place to point at all.
pub(crate) fn row_menu_items(
    resource: &Resource,
    capabilities: RowCapabilities,
    address: Option<&str>,
) -> Vec<MenuItem> {
    let lifecycle = capabilities
        .actions
        .iter()
        .copied()
        .filter(|action| action_is_useful(*action, resource.state))
        .map(MenuItem::Act)
        .collect();

    let mut open = Vec::new();
    if capabilities.logs {
        open.push(MenuItem::FollowLogs);
    }
    // A stopped container has no process to attach to, so a shell offered there
    // is an entry that can only fail -- the row's own button is hidden for the
    // same reason.
    if capabilities.shell && resource.state != RunState::Stopped {
        open.push(MenuItem::OpenShell);
    }
    if capabilities.detail {
        open.push(MenuItem::OpenDetail);
    }
    let ports = published_ports(&resource.detail);
    let links: Vec<PortLink> = ports
        .iter()
        .filter_map(|port| {
            let url = port_url(address, *port)?;
            Some(PortLink {
                label: url.trim_start_matches("http://").to_string(),
                url,
            })
        })
        .collect();
    if !links.is_empty() {
        open.push(MenuItem::OpenPorts(links));
    }

    let mut copy = Vec::new();
    // A volume's id is its name, and offering the same text twice is noise.
    if resource.id != resource.name {
        copy.push(MenuItem::CopyId);
    }
    copy.push(MenuItem::CopyName);
    if resource.kind == ResourceKind::Container && image_of(resource).is_some() {
        copy.push(MenuItem::CopyImage);
    }
    if resource.kind == ResourceKind::Image && has_reference(resource) {
        copy.push(MenuItem::CopyReference);
    }
    if !ports.is_empty() {
        copy.push(MenuItem::CopyAddress(copyable_addresses(address, &ports)));
    }

    let writes = capabilities.writes;
    let manage = match resource.kind {
        ResourceKind::Image => [
            (writes.run, MenuItem::RunImage),
            (writes.tag, MenuItem::TagImage),
            (writes.history, MenuItem::ImageHistory),
        ]
        .into_iter()
        .filter_map(|(offered, item)| offered.then_some(item))
        .collect(),
        ResourceKind::Network if writes.connect => {
            vec![MenuItem::ConnectContainer, MenuItem::DisconnectContainer]
        }
        _ => Vec::new(),
    };

    let remove = if capabilities.removable {
        vec![MenuItem::Remove]
    } else {
        Vec::new()
    };
    grouped(vec![lifecycle, open, manage, copy, remove])
}

/// What the menu on several selected rows offers.
pub(crate) fn bulk_menu_items(rows: &[Resource], capabilities: RowCapabilities) -> Vec<MenuItem> {
    let lifecycle = BULK_ACTIONS
        .into_iter()
        .filter(|action| capabilities.actions.contains(action))
        .filter(|action| rows.iter().any(|row| action_is_useful(*action, row.state)))
        .map(MenuItem::Act)
        .collect();
    let remove = if capabilities.removable {
        vec![MenuItem::Remove]
    } else {
        Vec::new()
    };
    grouped(vec![lifecycle, vec![MenuItem::CopyName], remove])
}

/// An entry with its icon at the start, muted, the way every database menu draws
/// one.
fn icon_entry(
    menu: ContextMenu,
    label: impl Into<SharedString>,
    icon: IconName,
    handler: impl Fn(&mut Window, &mut App) + 'static,
) -> ContextMenu {
    menu.item(
        ContextMenuEntry::new(label)
            .icon(icon)
            .icon_position(IconPosition::Start)
            .icon_color(Color::Muted)
            .handler(handler),
    )
}

pub(crate) fn render_items(
    mut menu: ContextMenu,
    items: &[MenuItem],
    targets: &[Resource],
    panel: &WeakEntity<ContainerPanel>,
) -> ContextMenu {
    for item in items {
        match item {
            MenuItem::Separator => menu = menu.separator(),
            // One port is one entry: a submenu with a single row is a click
            // spent on nothing.
            MenuItem::OpenPorts(links) if links.len() == 1 => {
                let url = links[0].url.clone();
                menu = icon_entry(
                    menu,
                    format!("Open {}", links[0].label),
                    item.icon(),
                    move |_window, cx| cx.open_url(&url),
                );
            }
            MenuItem::OpenPorts(links) => {
                let links = links.clone();
                menu = menu.submenu_with_icon(
                    item.label(targets.len()),
                    item.icon(),
                    move |mut submenu, _window, _cx| {
                        for link in &links {
                            let url = link.url.clone();
                            submenu = icon_entry(
                                submenu,
                                link.label.clone(),
                                IconName::ArrowUpRight,
                                move |_window, cx| cx.open_url(&url),
                            );
                        }
                        submenu
                    },
                );
            }
            MenuItem::Compose(entry) if entry.blocked.is_some() => {
                let reason = entry.blocked.clone().unwrap_or_default();
                menu = menu.item(
                    ContextMenuEntry::new(item.label(targets.len()))
                        .icon(item.icon())
                        .icon_position(IconPosition::Start)
                        .icon_color(Color::Muted)
                        .disabled(true)
                        .documentation_aside(DocumentationSide::Right, move |_| {
                            Label::new(reason.clone()).into_any_element()
                        }),
                );
            }
            other => {
                let panel = panel.clone();
                let targets = targets.to_vec();
                let item = other.clone();
                menu = icon_entry(
                    menu,
                    other.label(targets.len()),
                    other.icon(),
                    move |window, cx| {
                        panel
                            .update(cx, |panel, cx| {
                                panel.run_menu_item(&item, &targets, window, cx)
                            })
                            .ok();
                    },
                );
            }
        }
    }
    menu
}

impl ContainerPanel {
    /// Where a browser on this machine reaches the ports the engines publish,
    /// when the host can say.
    pub(crate) fn port_address(&self) -> Option<String> {
        address_of(self.command_host.as_deref()?)
    }

    pub(crate) fn row_capabilities(&self) -> RowCapabilities {
        RowCapabilities {
            actions: self.available_actions(),
            logs: self.terminal_available(TerminalIntent::FollowLogs),
            shell: self.terminal_available(TerminalIntent::Shell),
            detail: self.rows_open_a_detail(),
            removable: self.removable(),
            writes: self
                .backend()
                .map_or_else(Writes::default, |backend| Writes {
                    run: backend.offers(Op::ImageRun),
                    tag: backend.offers(Op::ImageTag),
                    history: backend.offers(Op::ImageHistory),
                    connect: backend.offers(Op::NetworkConnect),
                }),
        }
    }

    /// Puts a menu on the panel at `position` and clears it when it closes.
    ///
    /// Held by the panel rather than by a row: rows are recycled by the
    /// virtualised list, and a menu owned by one would vanish on scroll.
    pub(crate) fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(ContextMenu, &mut Window, &mut Context<ContextMenu>) -> ContextMenu,
    ) {
        let menu = ContextMenu::build(window, cx, build);
        cx.focus_view(&menu, window);
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                if this
                    .context_menu
                    .as_ref()
                    .is_some_and(|(menu, _, _)| menu.focus_handle(cx).contains_focused(window, cx))
                {
                    window.focus(&this.results_focus, cx);
                }
                this.context_menu.take();
                cx.notify();
            });
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// What a right-click on `resource` offers, and to which rows.
    ///
    /// On a row that is part of a selection of several, the menu is for the
    /// whole selection: acting on one row while five are highlighted is the
    /// surprise, not the other way round. Anywhere else it is for that row
    /// alone and leaves the selection as it was. `None` for a busy row: its
    /// buttons are replaced by a spinner for the same reason, whatever the menu
    /// could ask is already being done.
    pub(crate) fn row_menu_plan(
        &self,
        resource: &Resource,
        cx: &App,
    ) -> Option<(Vec<Resource>, Vec<MenuItem>)> {
        let key = row_key(resource);
        if self.busy_for(&key).is_some() {
            return None;
        }
        let capabilities = self.row_capabilities();
        let selected = if self.selection.contains(&key) {
            self.selected_resources(cx)
        } else {
            Vec::new()
        };
        if selected.len() > 1 {
            let items = bulk_menu_items(&selected, capabilities);
            return Some((selected, items));
        }
        // A compose project or one of its services: its own menu, because what
        // they offer is compose's and none of the container verbs above.
        if resource.kind == ResourceKind::ComposeProject {
            let context = self.compose_context(resource, Some(cx));
            let items = if is_service_row(resource) {
                context.service_menu_items()
            } else {
                context.project_menu_items()
            };
            return Some((vec![resource.clone()], items));
        }
        let address = self.port_address();
        let items = row_menu_items(resource, capabilities, address.as_deref());
        Some((vec![resource.clone()], items))
    }

    pub(crate) fn deploy_row_menu(
        &mut self,
        resource: Resource,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((targets, items)) = self.row_menu_plan(&resource, cx) else {
            return;
        };
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            render_items(menu, &items, &targets, &panel)
        });
    }

    /// The menu for a right-click on empty space.
    pub(crate) fn deploy_panel_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = !self.visible_keys(cx).is_empty();
        let creation = self.creation().map(|(_, label)| label);
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let refresh = panel.clone();
            let mut menu = icon_entry(menu, "Refresh", IconName::RotateCw, move |_window, cx| {
                refresh.update(cx, |panel, cx| panel.reload(cx)).ok();
            });
            if let Some(label) = creation {
                let create = panel.clone();
                menu = icon_entry(menu, label, IconName::Plus, move |window, cx| {
                    create
                        .update(cx, |panel, cx| panel.start_creation(window, cx))
                        .ok();
                });
            }
            if !rows {
                return menu;
            }
            icon_entry(
                menu,
                "Select All",
                IconName::ListTree,
                move |_window, cx| {
                    panel.update(cx, |panel, cx| panel.select_all_rows(cx)).ok();
                },
            )
        });
    }

    /// Does what a menu entry says to `targets`.
    pub(crate) fn run_menu_item(
        &mut self,
        item: &MenuItem,
        targets: &[Resource],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(first) = targets.first() else {
            return;
        };
        match item {
            MenuItem::Act(action) => self.act_many(*action, targets, cx),
            MenuItem::FollowLogs => self.open_terminal(
                TerminalIntent::FollowLogs,
                first.id.clone(),
                first.name.clone(),
                window,
                cx,
            ),
            MenuItem::OpenShell => self.open_terminal(
                TerminalIntent::Shell,
                first.id.clone(),
                first.name.clone(),
                window,
                cx,
            ),
            MenuItem::OpenDetail => self.open_detail(first.clone(), window, cx),
            MenuItem::CopyId => copy_lines(targets.iter().map(|row| row.id.as_str()), cx),
            MenuItem::CopyName | MenuItem::CopyReference => {
                copy_lines(targets.iter().map(|row| row.name.as_str()), cx)
            }
            MenuItem::CopyImage => copy_lines(targets.iter().filter_map(image_of), cx),
            MenuItem::CopyAddress(lines) => copy_lines(lines.iter().map(String::as_str), cx),
            MenuItem::Remove => {
                let keys = targets.iter().map(row_key).collect();
                self.start_removal_of(keys, window, cx);
            }
            MenuItem::Compose(entry) => {
                self.run_compose_command(entry.command, first.clone(), window, cx)
            }
            MenuItem::RunImage => self.open_run_dialog(first.clone(), window, cx),
            MenuItem::TagImage => self.open_tag_dialog(first.clone(), window, cx),
            MenuItem::ImageHistory => {
                self.open_detail_on(first.clone(), Some(DetailTab::History), window, cx)
            }
            MenuItem::ConnectContainer => {
                self.open_membership(Membership::Connect, first.clone(), window, cx)
            }
            MenuItem::DisconnectContainer => {
                self.open_membership(Membership::Disconnect, first.clone(), window, cx)
            }
            // Ports are entries of their own submenu, and a separator is not an
            // entry.
            MenuItem::OpenPorts(_) | MenuItem::Separator => {}
        }
    }

    /// A click on a row, with whatever modifier it carried.
    ///
    /// Plain, it opens the row where the kind has a detail view -- as it always
    /// did -- and otherwise selects it. With shift or the platform's command key
    /// it only changes the selection, so building a range never opens and closes
    /// a log view along the way.
    pub(crate) fn click_row(
        &mut self,
        resource: &Resource,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A service is not a selectable row: it is a view of containers the
        // selection and the bulk actions know nothing of.
        if is_service_row(resource) {
            window.focus(&self.results_focus, cx);
            return;
        }
        let key = row_key(resource);
        let order = self.visible_keys(cx);
        let order: Vec<&str> = order.iter().map(String::as_str).collect();
        if modifiers.shift {
            self.selection.extend_to(&key, &order);
        } else if modifiers.secondary() {
            self.selection.toggle(&key);
        } else if self.rows_open_a_detail() {
            self.selection.clear();
            self.open_detail(resource.clone(), window, cx);
            cx.notify();
            // Not focused: the list is about to be replaced by the log view, and
            // a focus left on something that is no longer drawn takes the
            // keyboard with it.
            return;
        } else {
            self.selection.select_only(&key);
        }
        // A click does not move focus on its own, and the keys are bound for
        // whatever holds the list's focus.
        window.focus(&self.results_focus, cx);
        cx.notify();
    }
}

fn copy_lines<'a>(lines: impl Iterator<Item = &'a str>, cx: &mut App) {
    let text = lines.collect::<Vec<_>>().join("\n");
    if text.is_empty() {
        return;
    }
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(kind: ResourceKind) -> RowCapabilities {
        // What `FakeBackend` and `DockerBackend` answer, written out.
        match kind {
            ResourceKind::Container => RowCapabilities {
                actions: &[
                    ResourceAction::Start,
                    ResourceAction::Stop,
                    ResourceAction::Restart,
                    ResourceAction::Pause,
                    ResourceAction::Unpause,
                ],
                logs: true,
                shell: true,
                detail: true,
                removable: true,
                writes: Writes::default(),
            },
            ResourceKind::ComposeProject => RowCapabilities {
                actions: &[ResourceAction::Start, ResourceAction::Stop],
                logs: false,
                shell: false,
                detail: false,
                removable: false,
                writes: Writes::default(),
            },
            _ => RowCapabilities {
                actions: &[],
                logs: false,
                shell: false,
                detail: false,
                removable: true,
                writes: Writes {
                    run: true,
                    tag: true,
                    history: true,
                    connect: true,
                },
            },
        }
    }

    fn row(kind: ResourceKind, name: &str, state: RunState) -> Resource {
        Resource {
            kind,
            id: format!("id-{name}"),
            name: name.into(),
            state,
            detail: Vec::new(),
            parent: None,
            compose: None,
        }
    }

    fn with(mut resource: Resource, label: &'static str, value: &str) -> Resource {
        resource.detail.push((label, value.into()));
        resource
    }

    fn has(items: &[MenuItem], wanted: &MenuItem) -> bool {
        items.contains(wanted)
    }

    #[test]
    fn a_running_container_offers_stop_restart_pause_but_not_start_or_resume() {
        let web = row(ResourceKind::Container, "web", RunState::Running);
        let items = row_menu_items(&web, caps(ResourceKind::Container), None);
        for wanted in [
            ResourceAction::Stop,
            ResourceAction::Restart,
            ResourceAction::Pause,
        ] {
            assert!(has(&items, &MenuItem::Act(wanted)), "{wanted:?}");
        }
        for unwanted in [ResourceAction::Start, ResourceAction::Unpause] {
            assert!(!has(&items, &MenuItem::Act(unwanted)), "{unwanted:?}");
        }
        assert!(has(&items, &MenuItem::OpenShell));
        assert!(has(&items, &MenuItem::FollowLogs));
        assert!(has(&items, &MenuItem::Remove));
    }

    #[test]
    fn a_stopped_container_starts_but_has_no_shell_to_open() {
        let web = row(ResourceKind::Container, "web", RunState::Stopped);
        let items = row_menu_items(&web, caps(ResourceKind::Container), None);
        assert!(has(&items, &MenuItem::Act(ResourceAction::Start)));
        assert!(!has(&items, &MenuItem::Act(ResourceAction::Stop)));
        assert!(!has(&items, &MenuItem::Act(ResourceAction::Restart)));
        assert!(!has(&items, &MenuItem::OpenShell), "nothing to attach to");
        assert!(
            has(&items, &MenuItem::FollowLogs),
            "a stopped one still has a log"
        );
    }

    #[test]
    fn a_paused_container_resumes_rather_than_starts() {
        let web = row(ResourceKind::Container, "web", RunState::Paused);
        let items = row_menu_items(&web, caps(ResourceKind::Container), None);
        assert!(has(&items, &MenuItem::Act(ResourceAction::Unpause)));
        assert!(!has(&items, &MenuItem::Act(ResourceAction::Start)));
        assert!(!has(&items, &MenuItem::Act(ResourceAction::Pause)));
    }

    #[test]
    fn a_container_can_copy_its_image_and_an_image_its_reference() {
        let web = with(
            row(ResourceKind::Container, "web", RunState::Running),
            "Image",
            "nginx:1",
        );
        let items = row_menu_items(&web, caps(ResourceKind::Container), None);
        assert!(has(&items, &MenuItem::CopyImage));
        assert!(has(&items, &MenuItem::CopyId));
        assert!(!has(&items, &MenuItem::CopyReference));

        let image = row(ResourceKind::Image, "nginx:1", RunState::Unknown);
        let items = row_menu_items(&image, caps(ResourceKind::Image), None);
        assert!(has(&items, &MenuItem::CopyReference));
        assert!(!has(&items, &MenuItem::CopyImage));
        assert!(!has(&items, &MenuItem::FollowLogs));
        assert!(!items.iter().any(|item| matches!(item, MenuItem::Act(_))));
    }

    #[test]
    fn a_dangling_image_has_no_reference_to_copy() {
        let dangling = with(
            row(ResourceKind::Image, "<none> (abc)", RunState::Unknown),
            "Dangling",
            "yes",
        );
        let items = row_menu_items(&dangling, caps(ResourceKind::Image), None);
        assert!(!has(&items, &MenuItem::CopyReference));
        assert!(
            has(&items, &MenuItem::CopyId),
            "its id is all that names it"
        );
    }

    #[test]
    fn a_volume_copies_its_name_once() {
        let mut volume = row(ResourceKind::Volume, "pgdata", RunState::Unknown);
        volume.id = "pgdata".into();
        let items = row_menu_items(&volume, caps(ResourceKind::Volume), None);
        assert!(has(&items, &MenuItem::CopyName));
        assert!(!has(&items, &MenuItem::CopyId), "the id is the name");
        assert!(has(&items, &MenuItem::Remove));
    }

    #[test]
    fn a_compose_project_cannot_be_removed_from_its_menu() {
        let project = row(ResourceKind::ComposeProject, "shop", RunState::Running);
        let items = row_menu_items(&project, caps(ResourceKind::ComposeProject), None);
        assert!(!has(&items, &MenuItem::Remove));
        assert!(has(&items, &MenuItem::Act(ResourceAction::Stop)));
    }

    #[test]
    fn published_ports_become_links_at_the_address_the_host_gave() {
        let web = with(
            row(ResourceKind::Container, "web", RunState::Running),
            "Ports",
            "0.0.0.0:8080->80/tcp, :::8080->80/tcp, 0.0.0.0:9090->9090/tcp",
        );
        let items = |address| row_menu_items(&web, caps(ResourceKind::Container), address);
        let links = |address| {
            items(address).into_iter().find_map(|item| match item {
                MenuItem::OpenPorts(links) => Some(links),
                _ => None,
            })
        };
        let local = links(Some("localhost")).expect("a known address opens ports");
        assert_eq!(
            local
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            ["http://localhost:8080", "http://localhost:9090"]
        );
        assert_eq!(local[0].label, "localhost:8080");

        let remote = links(Some("10.0.0.7")).expect("a remote one points at its address");
        assert_eq!(remote[0].url, "http://10.0.0.7:8080");
        assert!(
            has(
                &items(Some("10.0.0.7")),
                &MenuItem::CopyAddress(vec!["10.0.0.7:8080".into(), "10.0.0.7:9090".into()])
            ),
            "and what is pasted names the same place"
        );

        assert!(
            links(None).is_none(),
            "no known address offers nothing rather than localhost"
        );
        assert!(
            has(
                &items(None),
                &MenuItem::CopyAddress(vec!["8080".into(), "9090".into()])
            ),
            "the ports can still be copied, without a host to put in front"
        );
    }

    #[test]
    fn a_container_with_nothing_published_has_no_port_entry() {
        let web = with(
            row(ResourceKind::Container, "web", RunState::Running),
            "Ports",
            "5432/tcp",
        );
        let items = row_menu_items(&web, caps(ResourceKind::Container), None);
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, MenuItem::OpenPorts(_) | MenuItem::CopyAddress(_)))
        );
    }

    #[test]
    fn groups_are_separated_and_never_doubled_or_dangling() {
        for state in [
            RunState::Running,
            RunState::Stopped,
            RunState::Paused,
            RunState::Unknown,
        ] {
            let items = row_menu_items(
                &row(ResourceKind::Container, "web", state),
                caps(ResourceKind::Container),
                None,
            );
            assert_ne!(items.first(), Some(&MenuItem::Separator));
            assert_ne!(items.last(), Some(&MenuItem::Separator));
            assert!(
                !items
                    .windows(2)
                    .any(|pair| pair[0] == MenuItem::Separator && pair[1] == MenuItem::Separator),
                "{state:?}"
            );
        }
        let image = row(ResourceKind::Image, "nginx:1", RunState::Unknown);
        let items = row_menu_items(&image, caps(ResourceKind::Image), None);
        assert_ne!(items.first(), Some(&MenuItem::Separator));
    }

    #[test]
    fn the_bulk_menu_offers_what_at_least_one_row_can_take() {
        let rows = [
            row(ResourceKind::Container, "a", RunState::Running),
            row(ResourceKind::Container, "b", RunState::Running),
        ];
        let items = bulk_menu_items(&rows, caps(ResourceKind::Container));
        assert!(has(&items, &MenuItem::Act(ResourceAction::Stop)));
        assert!(has(&items, &MenuItem::Act(ResourceAction::Restart)));
        assert!(
            !has(&items, &MenuItem::Act(ResourceAction::Start)),
            "nothing selected is stopped"
        );
        assert!(
            !has(&items, &MenuItem::Act(ResourceAction::Pause)),
            "pause is a row-at-a-time action"
        );
        assert!(has(&items, &MenuItem::Remove));
        assert!(has(&items, &MenuItem::CopyName));
        assert!(!has(&items, &MenuItem::OpenShell));
        assert_eq!(MenuItem::Remove.label(2), "Remove 2\u{2026}");
        assert_eq!(MenuItem::Remove.label(1), "Remove\u{2026}");
    }

    #[test]
    fn the_bulk_menu_of_a_kind_that_cannot_be_removed_has_no_remove() {
        let rows = [
            row(ResourceKind::ComposeProject, "a", RunState::Running),
            row(ResourceKind::ComposeProject, "b", RunState::Stopped),
        ];
        let items = bulk_menu_items(&rows, caps(ResourceKind::ComposeProject));
        assert!(!has(&items, &MenuItem::Remove));
        assert!(has(&items, &MenuItem::Act(ResourceAction::Start)));
        assert!(has(&items, &MenuItem::Act(ResourceAction::Stop)));
    }
}
