use container::{BackendKind, ContainerError, Resource, ResourceAction, ResourceKind, RunState};
use gpui::{Anchor, AnyElement, App, Context, Window};
use ui::{Banner, ContextMenu, Indicator, PopoverMenu, Severity, Tab, TabBar, Table, prelude::*};

use crate::container_panel::{ContainerPanel, ListState};
use crate::terminal::TerminalIntent;

impl Render for ContainerPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ContainerPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(self.render_header(cx))
            .when_some(self.last_error.clone(), |element, error| {
                element.child(self.render_error_banner(&error, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_body(window, cx)),
            )
            // The view has to know how wide it is before it can decide which
            // columns to show, and nothing tells it -- a tab is as wide as its
            // pane and a window as wide as it was dragged. A `canvas` is the only
            // place a real width can be read, so it is read there and used on the
            // next frame.
            .child({
                let this = cx.entity().downgrade();
                gpui::canvas(
                    move |bounds: gpui::Bounds<gpui::Pixels>, _window, cx: &mut App| {
                        let width = bounds.size.width;
                        // Deferred rather than applied here: this runs inside the
                        // prepaint of the very view it would update, and the width
                        // is wanted for the *next* frame anyway.
                        cx.defer(move |cx| {
                            this.update(cx, |this, cx| {
                                this.note_measured_width(width, cx);
                            })
                            .ok();
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
    }
}

impl ContainerPanel {
    /// One bar: which engine on the left, which kind in the middle, what can be
    /// done to the list on the right.
    ///
    /// A `TabBar` rather than three rows of look-alike buttons. The engine and
    /// the kind are different questions -- *which daemon* and *what am I looking
    /// at* -- and the previous header asked both with identical buttons sitting
    /// side by side, so neither read as a group. `TabBar` already draws the two
    /// dividers that separate its start and end slots from the tabs, which is
    /// exactly the separation those two questions wanted.
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        TabBar::new("container-header")
            .start_child(self.render_engine_picker(cx))
            .children(self.render_kind_tabs(cx))
            .end_child(self.render_tools(cx))
            .into_any_element()
    }

    /// The engine, as one control that names the engine it is on.
    ///
    /// A menu rather than three buttons in a row: only one engine is ever in
    /// use, two of the three are usually not even installed, and three equal
    /// buttons spend a third of the bar saying so. The chosen one is named and
    /// the rest are one click away.
    fn render_engine_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self
            .backend()
            .map(|backend| backend.kind())
            .unwrap_or(BackendKind::Docker);
        let choices: Vec<(usize, BackendKind)> = self
            .backends
            .iter()
            .enumerate()
            .map(|(index, backend)| (index, backend.kind()))
            .collect();
        let handle = cx.entity().downgrade();
        let chosen = self.active_backend;

        PopoverMenu::new("container-engine-picker")
            .trigger_with_tooltip(
                Button::new("container-engine-trigger", active.label())
                    .label_size(LabelSize::Small)
                    .start_icon(
                        Icon::new(engine_icon(active))
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .end_icon(
                        Icon::new(IconName::ChevronDown)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    ),
                ui::Tooltip::text("Choose an engine"),
            )
            .anchor(Anchor::TopLeft)
            .menu(move |window, cx| {
                let choices = choices.clone();
                let handle = handle.clone();
                Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                    for (index, kind) in choices.iter().copied() {
                        let handle = handle.clone();
                        menu = menu.toggleable_entry(
                            kind.label(),
                            index == chosen,
                            IconPosition::Start,
                            None,
                            move |_window, cx| {
                                handle
                                    .update(cx, |panel, cx| panel.choose_backend(index, cx))
                                    .ok();
                            },
                        );
                    }
                    menu
                }))
            })
            .into_any_element()
    }

    /// One tab per kind the engine actually lists.
    ///
    /// `TabPosition` is set so the bar draws its own separators; without it every
    /// tab claims to be first and the selected one loses its shoulders.
    fn render_kind_tabs(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let kinds = self.available_kinds();
        let last = kinds.len().saturating_sub(1);
        let selected = kinds.iter().position(|kind| *kind == self.active_kind);
        kinds
            .iter()
            .copied()
            .enumerate()
            .map(|(index, kind)| {
                let active = kind == self.active_kind;
                let position = if index == 0 {
                    ui::TabPosition::First
                } else if index == last {
                    ui::TabPosition::Last
                } else {
                    ui::TabPosition::Middle(
                        selected
                            .map_or(std::cmp::Ordering::Greater, |selected| index.cmp(&selected)),
                    )
                };
                Tab::new(("container-kind", kind as usize))
                    .position(position)
                    .toggle_state(active)
                    .start_slot(
                        Icon::new(kind_icon(kind))
                            .size(IconSize::XSmall)
                            .color(if active { Color::Default } else { Color::Muted }),
                    )
                    .child(
                        Label::new(kind.label())
                            .size(LabelSize::Small)
                            .color(if active { Color::Default } else { Color::Muted }),
                    )
                    .on_click(cx.listener(move |this, _, _window, cx| this.choose_kind(kind, cx)))
                    .into_any_element()
            })
            .collect()
    }

    /// How many rows there are, and what can be done to all of them.
    ///
    /// The count is here rather than nowhere: "Images" over an unbounded list
    /// tells you less than "Images · 42", and it is the one number that answers
    /// "did the refresh do anything" without reading the table.
    fn render_tools(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = match &self.state {
            ListState::Ready(resources) => Some(resources.len()),
            _ => None,
        };
        h_flex()
            .gap_0p5()
            .when_some(count, |element, count| {
                element.child(
                    div().mr_1().child(
                        Label::new(count.to_string())
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
                )
            })
            .child(
                IconButton::new("container-reload", IconName::RotateCw)
                    .icon_size(IconSize::Small)
                    .tooltip(|_window, cx| ui::Tooltip::simple("Refresh", cx))
                    .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
            )
            .when(self.prune_available(), |element| {
                element.child(
                    IconButton::new("container-prune", IconName::Trash)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Error)
                        .tooltip(|_window, cx| {
                            ui::Tooltip::simple("Remove everything unused\u{2026}", cx)
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.start_prune(window, cx))),
                )
            })
            // No "open in a tab" button: this *is* the tab. Only the window,
            // which is somewhere else to be.
            //
            // Dispatched rather than called directly, so the button and any
            // keybinding travel the one road and cannot drift apart.
            .child(
                IconButton::new("container-open-in-window", IconName::OpenNewWindow)
                    .icon_size(IconSize::Small)
                    .tooltip(|_window, cx| {
                        ui::Tooltip::for_action(
                            "Open in a floating window",
                            &zed_actions::container::OpenInFloatingWindow,
                            cx,
                        )
                    })
                    .on_click(|_event, window, cx| {
                        window.dispatch_action(
                            Box::new(zed_actions::container::OpenInFloatingWindow),
                            cx,
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // The open row wins over the list. Not beside it: a log line is long and
        // the table is wide, and a pane split between them gives each half of a
        // width neither can spare.
        if self.detail.is_some() {
            return self.render_detail(window, cx);
        }
        match &self.state {
            ListState::Loading => note(None, "Asking the engine\u{2026}"),
            ListState::Failed(error) => self.render_unusable(error, cx),
            // Empty is not a fault: the engine answered, and the answer was
            // none. Names the kind, because "nothing here" beside an Images tab
            // and beside a Containers tab mean different things.
            ListState::Ready(resources) if resources.is_empty() => note(
                Some(kind_icon(self.active_kind)),
                format!("No {}", self.active_kind.label().to_lowercase()),
            ),
            ListState::Ready(resources) => {
                let columns = columns_for(self.active_kind, self.shows_every_column());
                self.render_list(resources, columns, cx)
            }
        }
    }

    /// The engine cannot be used, and *why* decides the words.
    ///
    /// Never a dialog: on a machine without Docker this is the ordinary state of
    /// the panel, not an incident. Telling somebody to install what they already
    /// have -- which one error for both cases would do -- is the specific
    /// mistake these three branches exist to avoid.
    fn render_unusable(&self, error: &ContainerError, cx: &mut Context<Self>) -> AnyElement {
        let (headline, detail) = match error {
            ContainerError::EngineMissing { program } => (
                format!("{program} is not installed"),
                "Install it, then refresh.".to_string(),
            ),
            ContainerError::EngineNotRunning { program } => (
                format!("{program} is not running"),
                "Start it, then refresh.".to_string(),
            ),
            // Installed, reachable, and nothing chosen to look at. Its own
            // sentence because its cure is its own: with no kubeconfig context
            // `kubectl` reports a refused connection to localhost, and telling
            // somebody the cluster is down would send them after the wrong thing.
            ContainerError::NotConfigured { detail } => (
                "Nothing selected".to_string(),
                format!("{detail}. Choose a context, then refresh."),
            ),
            ContainerError::NotAuthorized { detail } => {
                ("Not authorized".to_string(), detail.clone())
            }
            // The engine's own words. Shown so a person can act on them, never
            // interpreted -- this text comes from outside.
            other => ("The engine could not answer".to_string(), other.to_string()),
        };

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_1()
            .p_4()
            .debug_selector(|| "container-unusable".into())
            .child(
                Icon::new(
                    self.backend()
                        .map_or(IconName::Box, |backend| engine_icon(backend.kind())),
                )
                .size(IconSize::XLarge)
                .color(Color::Muted),
            )
            .child(Label::new(headline).color(Color::Default))
            .child(
                Label::new(detail)
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                div().pt_1().child(
                    Button::new("container-refresh-after-error", "Refresh")
                        .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
                ),
            )
            .into_any_element()
    }
}

/// The centred "there is nothing to draw" state, with an optional icon above it.
///
/// One function for both empty and loading so the two never drift into different
/// vertical positions -- a message that jumps when the list arrives reads as a
/// flicker rather than an answer.
fn note(icon: Option<IconName>, message: impl Into<SharedString>) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .when_some(icon, |element, icon| {
            element.child(Icon::new(icon).size(IconSize::XLarge).color(Color::Muted))
        })
        .child(
            Label::new(message)
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .into_any_element()
}

/// The glyph for a kind, used on its tab and again on its empty state.
///
/// The same icon in both places on purpose: the empty state is answering a
/// question the tab asked, and repeating the glyph is what says so without a
/// sentence.
fn kind_icon(kind: ResourceKind) -> IconName {
    match kind {
        ResourceKind::Container => IconName::Box,
        // Layers, because that is what an image is.
        ResourceKind::Image => IconName::Blocks,
        ResourceKind::Volume => IconName::Database,
        ResourceKind::Network => IconName::Public,
        ResourceKind::ComposeProject => IconName::ListTree,
        ResourceKind::Pod => IconName::BoxOpen,
    }
}

/// The glyph for an engine.
///
/// Only Docker has a mark of its own here. Podman and Kubernetes fall back to
/// the generic box rather than borrowing Docker's whale, which would say the
/// wrong thing about which daemon is being talked to.
fn engine_icon(kind: BackendKind) -> IconName {
    match kind {
        BackendKind::Docker => IconName::Docker,
        BackendKind::Podman | BackendKind::Kubernetes => IconName::Box,
    }
}

/// Columns per kind and per width, rather than derived from `detail`.
///
/// Per kind, because the kinds do not share columns -- an image wants its size,
/// a volume its mount point -- and a table whose columns move as rows arrive is
/// unreadable. Per width, because the long columns only pay for themselves when
/// there is room.
///
/// Two *sets* rather than one that hides cells: a `Table` is built with a column
/// count, so dropping a column has to change that count or the header and the
/// rows stop agreeing about which cell is which. `""` is the run-state dot;
/// `"Name"` is the resource's own name; everything else is looked up in the
/// backend's `detail` pairs by that exact label, so a field no column asks for
/// is simply not shown.
fn columns_for(kind: ResourceKind, roomy: bool) -> &'static [&'static str] {
    match (kind, roomy) {
        (ResourceKind::Container, false) => &["", "Name", "Image"],
        (ResourceKind::Container, true) => &["", "Name", "Image", "Status", "Ports"],
        (ResourceKind::Image, false) => &["Name", "Size"],
        (ResourceKind::Image, true) => &["Name", "Size", "Created", "Dangling"],
        (ResourceKind::Volume, false) => &["Name", "Driver"],
        (ResourceKind::Volume, true) => &["Name", "Driver", "Mountpoint"],
        (ResourceKind::Network, false) => &["Name", "Driver"],
        (ResourceKind::Network, true) => &["Name", "Driver", "Scope"],
        (ResourceKind::ComposeProject, false) => &["", "Name", "Status"],
        (ResourceKind::ComposeProject, true) => &["", "Name", "Status", "Config"],
        (ResourceKind::Pod, false) => &["", "Name", "Namespace"],
        (ResourceKind::Pod, true) => &["", "Name", "Namespace", "Image", "Status", "Controller"],
    }
}

impl ContainerPanel {
    /// The table, with a trailing column of action buttons when the kind has any.
    ///
    /// The buttons come from `supported_actions`, never from a list written here:
    /// a container starts and stops, an image does neither, a podman pod starts
    /// and stops, a Kubernetes pod does nothing at all. Written out here, that
    /// knowledge would be wrong for three of the four.
    fn render_list(
        &self,
        resources: &[Resource],
        columns: &'static [&'static str],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let actions = self.available_actions();
        let logs = self.terminal_available(TerminalIntent::FollowLogs);
        let shell = self.terminal_available(TerminalIntent::Shell);
        // A trailing cell exists if there is anything at all to put in it.
        let removable = self.removable();
        let has_trailing = !actions.is_empty() || logs || shell || removable;
        // Handed to the closure as *data*, not as elements: `AnyElement` is not
        // `Clone`, and `uniform_list` asks for a range again on every frame.
        let resources = std::rc::Rc::new(resources.to_vec());
        let in_flight = std::rc::Rc::new(self.in_flight.clone());
        let count = resources.len();
        // One extra column for the buttons, and only when there are buttons: a
        // `Table` is built with a column count, so an unused trailing column
        // would be a blank stripe down every row.
        let column_count = columns.len() + usize::from(has_trailing);
        let mut header: Vec<SharedString> = columns.iter().map(|name| (*name).into()).collect();
        if has_trailing {
            header.push("".into());
        }
        let handle = cx.entity().downgrade();
        // Rows open onto their logs only where the engine has a logs command for
        // the kind. An image row therefore does not react to a click, because
        // there would be nothing behind it.
        let rows_open = self.rows_open_a_detail();
        let clickable = std::rc::Rc::new(resources.to_vec());
        let row_handle = handle.clone();

        div()
            .size_full()
            .child(
                Table::new(column_count)
                    .striped()
                    .when(rows_open, |table| {
                        table.map_row(move |(index, row), _window, _cx| {
                            let Some(resource) = clickable.get(index).cloned() else {
                                return row.into_any_element();
                            };
                            let handle = row_handle.clone();
                            row.cursor_pointer()
                                .on_click(move |_event, window, cx| {
                                    handle
                                        .update(cx, |panel, cx| {
                                            panel.open_detail(resource.clone(), window, cx)
                                        })
                                        .ok();
                                })
                                .into_any_element()
                        })
                    })
                    .header(header)
                    .uniform_list("container-list", count, move |range, _window, _cx| {
                        range
                            .filter_map(|index| resources.get(index))
                            .map(|resource| {
                                let mut cells = render_row(resource, columns);
                                if has_trailing {
                                    cells.push(render_trailing(
                                        resource,
                                        actions,
                                        in_flight.get(&resource.id).copied(),
                                        logs,
                                        shell,
                                        removable,
                                        handle.clone(),
                                    ));
                                }
                                cells
                            })
                            .collect()
                    }),
            )
            .into_any_element()
    }

    /// What the engine said when it refused, and a way to dismiss it.
    ///
    /// A banner rather than a dialog: an action that failed has not broken
    /// anything, and a modal over a list somebody is reading is a worse
    /// interruption than the failure was. `stderr` is shown verbatim -- it comes
    /// from outside, so it is text and never interpreted.
    ///
    /// The shared `Banner` rather than a hand-drawn bar, so its severity colour,
    /// its icon and its padding come from the same place as every other banner
    /// in the app and cannot drift from them.
    fn render_error_banner(&self, error: &ContainerError, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_shrink_0()
            .p_1p5()
            .child(
                Banner::new()
                    .severity(Severity::Error)
                    .child(
                        Label::new(error.to_string())
                            .size(LabelSize::Small)
                            .color(Color::Default),
                    )
                    .action_slot(
                        IconButton::new("container-dismiss-error", IconName::Close)
                            .icon_size(IconSize::XSmall)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Dismiss", cx))
                            .on_click(cx.listener(|this, _, _window, cx| this.dismiss_error(cx))),
                    ),
            )
            .into_any_element()
    }
}

pub(crate) fn render_row(resource: &Resource, columns: &[&str]) -> Vec<AnyElement> {
    let detail = |label: &str| {
        resource
            .detail
            .iter()
            .find(|(name, _)| *name == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    };
    columns
        .iter()
        .map(|column| match *column {
            "" => state_dot(resource.state).into_any_element(),
            "Name" => Label::new(resource.name.clone())
                .size(LabelSize::Small)
                .into_any_element(),
            other => Label::new(detail(other))
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element(),
        })
        .collect()
}

/// Green for up, amber for frozen, muted for down, and nothing at all for
/// unknown.
///
/// A container whose state word this crate does not recognise gets no dot rather
/// than a grey one: a wrong dot beside a running container is worse than none.
pub(crate) fn state_dot(state: RunState) -> impl IntoElement {
    let colour = match state {
        RunState::Running => Some(Color::Success),
        // Not green: a paused container answers nothing, and a green dot beside
        // one would send somebody looking for the fault elsewhere.
        RunState::Paused => Some(Color::Warning),
        RunState::Stopped => Some(Color::Muted),
        RunState::Unknown => None,
    };
    h_flex()
        .w_3()
        .justify_center()
        .when_some(colour, |element, colour| {
            element.child(Indicator::dot().color(colour))
        })
}

/// The buttons for one row, or what is being attempted instead.
///
/// While an action is running the row says so rather than showing buttons:
/// `docker stop` waits ten seconds before killing, and buttons that do nothing
/// for ten seconds read as broken.
/// Everything the row can be asked to do: lifecycle buttons, then the two
/// terminals.
///
/// Terminals last, and visually apart, because they are the two that open
/// something new rather than changing what is there.
pub(crate) fn render_trailing(
    resource: &Resource,
    actions: &'static [ResourceAction],
    running: Option<ResourceAction>,
    logs: bool,
    shell: bool,
    removable: bool,
    handle: gpui::WeakEntity<ContainerPanel>,
) -> AnyElement {
    h_flex()
        .gap_1()
        .child(render_actions(resource, actions, running, handle.clone()))
        .when(logs, |element| {
            let id = resource.id.clone();
            let name = resource.name.clone();
            let handle = handle.clone();
            element.child(
                IconButton::new(
                    SharedString::from(format!("logs-{}", resource.id)),
                    IconName::ListTree,
                )
                .icon_size(IconSize::XSmall)
                .tooltip(|_window, cx| ui::Tooltip::simple("Follow logs in a terminal", cx))
                .on_click(move |_event, window, cx| {
                    handle
                        .update(cx, |panel, cx| {
                            panel.open_terminal(
                                TerminalIntent::FollowLogs,
                                id.clone(),
                                name.clone(),
                                window,
                                cx,
                            )
                        })
                        .ok();
                }),
            )
        })
        .when(shell, |element| {
            // Only where a shell can actually be entered: a stopped container has
            // no process to attach to, and offering it would be a button that
            // always fails.
            let enabled = resource.state != RunState::Stopped;
            let id = resource.id.clone();
            let name = resource.name.clone();
            let handle = handle.clone();
            element.when(enabled, |element| {
                element.child(
                    IconButton::new(
                        SharedString::from(format!("shell-{}", resource.id)),
                        IconName::Terminal,
                    )
                    .icon_size(IconSize::XSmall)
                    .tooltip(|_window, cx| ui::Tooltip::simple("Open a shell in a terminal", cx))
                    .on_click(move |_event, window, cx| {
                        handle
                            .update(cx, |panel, cx| {
                                panel.open_terminal(
                                    TerminalIntent::Shell,
                                    id.clone(),
                                    name.clone(),
                                    window,
                                    cx,
                                )
                            })
                            .ok();
                    }),
                )
            })
        })
        .when(removable, |element| {
            let id = resource.id.clone();
            let handle = handle.clone();
            element.child(
                IconButton::new(
                    SharedString::from(format!("remove-{}", resource.id)),
                    IconName::Trash,
                )
                .icon_size(IconSize::XSmall)
                .icon_color(Color::Error)
                .tooltip(|_window, cx| ui::Tooltip::simple("Remove\u{2026}", cx))
                // Opens the confirmation and nothing else. The button cannot
                // remove anything; only a plan that got past the dialog can.
                .on_click(move |_event, window, cx| {
                    handle
                        .update(cx, |panel, cx| panel.start_removal(&id, window, cx))
                        .ok();
                }),
            )
        })
        .into_any_element()
}

fn render_actions(
    resource: &Resource,
    actions: &'static [ResourceAction],
    running: Option<ResourceAction>,
    handle: gpui::WeakEntity<ContainerPanel>,
) -> AnyElement {
    if let Some(running) = running {
        return Label::new(format!("{}...", running.label()))
            .size(LabelSize::Small)
            .color(Color::Muted)
            .into_any_element();
    }

    h_flex()
        .gap_1()
        .children(actions.iter().copied().filter_map(|action| {
            if !action_is_useful(action, resource.state) {
                return None;
            }
            let icon = match action {
                ResourceAction::Start => IconName::PlayFilled,
                // `Power` rather than a square: this crate's icon set has no
                // stop glyph, and a power symbol reads as "turn it off" without
                // pretending to be a media control.
                ResourceAction::Stop => IconName::Power,
                ResourceAction::Restart => IconName::RotateCw,
                ResourceAction::Pause => IconName::DebugPause,
                // The same glyph as Start, and deliberately: to a person both
                // buttons mean "make it run again". Which verb the engine wants
                // is the backend's business, and the tooltip says "Resume".
                ResourceAction::Unpause => IconName::PlayFilled,
            };
            let id = resource.id.clone();
            let handle = handle.clone();
            Some(
                IconButton::new(
                    SharedString::from(format!("act-{}-{}", action.label(), resource.id)),
                    icon,
                )
                .icon_size(IconSize::XSmall)
                .tooltip(move |_window, cx| ui::Tooltip::simple(action.label(), cx))
                .on_click(move |_event, _window, cx| {
                    handle
                        .update(cx, |panel, cx| panel.act(action, id.clone(), cx))
                        .ok();
                }),
            )
        }))
        .into_any_element()
}

/// Whether an action is worth offering on a resource in this state.
///
/// Not cosmetics -- it is the engine's refusals written down. `docker start` on
/// a paused container, `docker pause` on a stopped one and `docker unpause` on a
/// running one are all errors, so a button drawn in those states is a button
/// that can only ever fail.
///
/// `Unknown` keeps the three that are merely pointless when wrong (Start, Stop,
/// Restart) and loses the two that are certain to fail: with the state unread,
/// guessing which way to freeze it is guessing, and guessing the pause pair
/// wrong is guaranteed rather than possible.
pub(crate) fn action_is_useful(action: ResourceAction, state: RunState) -> bool {
    match (action, state) {
        (ResourceAction::Start, RunState::Running | RunState::Paused) => false,
        (ResourceAction::Stop, RunState::Stopped) => false,
        (ResourceAction::Restart, RunState::Stopped) => false,
        (ResourceAction::Pause, state) => state == RunState::Running,
        (ResourceAction::Unpause, state) => state == RunState::Paused,
        _ => true,
    }
}
