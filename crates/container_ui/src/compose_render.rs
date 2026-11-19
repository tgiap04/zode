//! The buttons at the end of a compose project row and of a service row.
//!
//! Drawn from the same [`ComposeEntry`] lists the menus use, so a button and
//! the entry beside it in the menu cannot disagree about whether they are
//! available, or why not.

use container::Resource;
use gpui::{AnyElement, WeakEntity};
use ui::prelude::*;

use crate::compose_actions::{ComposeCommand, ComposeContext, ComposeEntry, is_service_row};
use crate::container_panel::{Busy, ContainerPanel};
use crate::render::render_busy;

/// What a row of the Compose tab can be asked, as buttons -- or the spinner,
/// while something is being taken down on it.
///
/// `expandable` draws the chevron that shows a project's services; the open
/// project's header has no use for it.
pub(crate) fn render_compose_trailing(
    resource: &Resource,
    context: &ComposeContext,
    busy: Option<Busy>,
    expandable: bool,
    handle: WeakEntity<ContainerPanel>,
) -> AnyElement {
    if let Some(busy) = busy {
        return render_busy(&resource.id, busy);
    }
    let service = is_service_row(resource);
    let mut entries: Vec<ComposeEntry> = Vec::new();
    if !service && expandable {
        entries.push(ComposeEntry {
            command: ComposeCommand::ToggleServices {
                expanded: context.expanded,
            },
            blocked: None,
        });
    }
    entries.extend(if service {
        context.service_buttons()
    } else {
        context.project_buttons()
    });

    let id = resource.id.clone();
    h_flex()
        .debug_selector(move || format!("container-row-buttons:{id}"))
        .gap_0p5()
        .children(
            entries
                .into_iter()
                .map(|entry| compose_button(resource, entry, handle.clone())),
        )
        .into_any_element()
}

fn compose_button(
    resource: &Resource,
    entry: ComposeEntry,
    handle: WeakEntity<ContainerPanel>,
) -> AnyElement {
    let command = entry.command;
    let label = command.label();
    let row = resource.clone();
    let selector = format!("compose-button:{label}:{}", resource.id);
    let element_id = SharedString::from(format!("compose-{label}-{}", resource.id));
    let tooltip = match &entry.blocked {
        Some(reason) => format!("{label}: {reason}"),
        None => label.to_string(),
    };
    div()
        .debug_selector(move || selector)
        .child(
            IconButton::new(element_id, command.icon())
                .icon_size(IconSize::Small)
                .when(command == ComposeCommand::Down, |button| {
                    button.icon_color(Color::Error)
                })
                .disabled(entry.blocked.is_some())
                .tooltip(move |_window, cx| ui::Tooltip::simple(tooltip.clone(), cx))
                .on_click(move |_event, window, cx| {
                    handle
                        .update(cx, |panel, cx| {
                            panel.run_compose_command(command, row.clone(), window, cx)
                        })
                        .ok();
                }),
        )
        .into_any_element()
}
