//! The list of things a project's avatar offers.
//!
//! Split from the item that draws it: the drawing and the offering change for
//! different reasons, and `rail_item.rs` was already carrying two features.

use crate::project_actions;
use gpui::{App, Entity, SharedString, WeakEntity, Window};
use project::ProjectGroupKey;
use ui::{ContextMenu, ContextMenuEntry, prelude::*};
use workspace::MultiWorkspace;

/// The project's own menu.
///
/// Five entries, every one of which does something today. The sixth — the
/// colour — arrives with the picker that makes it work; a disabled entry
/// promising "soon" would be the same dead label this feature keeps refusing.
///
/// `ContextMenu` rather than a view of our own: it brings its own background and
/// its own dismissal on an outside click.
pub(crate) fn build_project_menu(
    multi_workspace: WeakEntity<MultiWorkspace>,
    sidebar: WeakEntity<crate::Sidebar>,
    key: ProjectGroupKey,
    label: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    let can_move = multi_workspace
        .read_with(cx, |multi_workspace, _| {
            key.host().is_none() && multi_workspace.project_group_keys().len() >= 2
        })
        .unwrap_or(false);
    let path_exists = project_actions::project_path_exists(&key);

    ContextMenu::build(window, cx, move |mut menu, _window, _cx| {
        let entry = |menu: ContextMenu,
                     text: &'static str,
                     icon: IconName,
                     enabled: bool,
                     handler: Box<dyn Fn(&mut Window, &mut App)>| {
            menu.item(
                ContextMenuEntry::new(text)
                    .icon(icon)
                    .disabled(!enabled)
                    .handler(move |window, cx| handler(window, cx)),
            )
        };

        // Left out rather than drawn disabled, and the distinction is real: a
        // disabled entry says "not right now", but with one project open there
        // is no such action at all — the only window it could move to is this
        // one. Entries that exist and merely cannot act yet (`Reveal` on a path
        // that is gone) do get drawn disabled, just below.
        if can_move {
            menu = entry(menu, "Open Project in New Window", IconName::Plus, true, {
                let multi_workspace = multi_workspace.clone();
                let key = key.clone();
                let label = label.clone();
                Box::new(move |window, cx| {
                    project_actions::open_project_in_new_window(
                        &multi_workspace,
                        &key,
                        &label,
                        window,
                        cx,
                    );
                })
            });
            menu = menu.separator();
        }
        menu = entry(menu, "Reveal in Finder", IconName::Folder, path_exists, {
            let key = key.clone();
            Box::new(move |_window, cx| project_actions::reveal_project(&key, cx))
        });
        menu = entry(menu, "Copy Project Path", IconName::Copy, true, {
            let key = key.clone();
            Box::new(move |_window, cx| project_actions::copy_project_path(&key, cx))
        });
        menu = menu.separator();
        menu = entry(menu, "Change Initials\u{2026}", IconName::Pencil, true, {
            let multi_workspace = multi_workspace.clone();
            let key = key.clone();
            let label = label.clone();
            Box::new(move |window, cx| {
                project_actions::prompt_for_initials(&multi_workspace, &key, &label, window, cx);
            })
        });
        menu = entry(menu, "Change Colour\u{2026}", IconName::Palette, true, {
            let multi_workspace = multi_workspace.clone();
            let sidebar = sidebar.clone();
            let key = key.clone();
            let label = label.clone();
            Box::new(move |window, cx| {
                project_actions::prompt_for_colour(
                    &multi_workspace,
                    &sidebar,
                    &key,
                    &label,
                    window,
                    cx,
                );
            })
        });
        menu = menu.separator();
        entry(menu, "Remove Project", IconName::Trash, true, {
            let key = key.clone();
            Box::new(move |window, cx| {
                project_actions::remove_project(&multi_workspace, &key, &label, window, cx);
            })
        })
    })
}
