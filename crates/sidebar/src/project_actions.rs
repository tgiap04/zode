//! Everything a project's menu can do, in one place.
//!
//! Two surfaces offer a project's actions — the avatar on the rail and the
//! ellipsis on the panel row — and they deliberately offer *different lists*.
//! What they must never do is behave differently: `Remove` has to mean one thing
//! whichever door it was opened from. So the lists live at the surfaces and the
//! behaviour lives here, and there is exactly one call to
//! `MultiWorkspace::remove_project_group` from this crate.
//!
//! Free functions rather than methods on `Sidebar`: these run from a
//! `ContextMenu` callback, long after the render that built it, and they have no
//! business reading sidebar state at that point. They take what they need.

use gpui::{App, SharedString, WeakEntity, Window};
use project::ProjectGroupKey;
use workspace::MultiWorkspace;

/// Takes the project off this window, after asking.
///
/// The question and the removal both live on `MultiWorkspace`: the window root's
/// drag-out drop target needs the same pair, and `workspace` cannot depend on
/// this crate. So this is a façade, not a second implementation.
pub(crate) fn remove_project(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    key: &ProjectGroupKey,
    label: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let key = key.clone();
    let label = label.clone();
    multi_workspace
        .update(cx, |multi_workspace, cx| {
            multi_workspace.confirm_and_remove_project_group(&key, &label, window, cx);
        })
        .ok();
}

/// Moves the project to a window of its own, after asking.
///
/// It asks for the same reason `Remove` does, and not out of extra caution:
/// `open_project_group_in_new_window` removes the group from this window before
/// opening the new one. One destructive step, one door.
pub(crate) fn open_project_in_new_window(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    key: &ProjectGroupKey,
    label: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let key = key.clone();
    let label = label.clone();
    multi_workspace
        .update(cx, |multi_workspace, cx| {
            multi_workspace.confirm_and_move_project_to_new_window(&key, &label, window, cx);
        })
        .ok();
}

/// Every path this project was opened with, one per line.
///
/// All of them, not the first: a project opened over three folders is three
/// folders, and handing back one of them would be a tidy answer to a question
/// nobody asked.
pub(crate) fn project_paths_text(key: &ProjectGroupKey) -> String {
    key.path_list()
        .ordered_paths()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether there is still something on disk to show.
pub(crate) fn project_path_exists(key: &ProjectGroupKey) -> bool {
    key.path_list()
        .ordered_paths()
        .next()
        .is_some_and(|path| path.exists())
}

pub(crate) fn reveal_project(key: &ProjectGroupKey, cx: &mut App) {
    if let Some(path) = key.path_list().ordered_paths().next() {
        cx.reveal_path(path);
    }
}

pub(crate) fn copy_project_path(key: &ProjectGroupKey, cx: &mut App) {
    let text = project_paths_text(key);
    if !text.is_empty() {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }
}

/// Opens the colour picker for a project.
pub(crate) fn prompt_for_colour(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    sidebar: &WeakEntity<crate::Sidebar>,
    key: &ProjectGroupKey,
    label: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(multi_workspace_entity) = multi_workspace.upgrade() else {
        return;
    };
    let current = multi_workspace_entity.read(cx).project_presentation(key).1;
    let handle = multi_workspace.clone();
    let sidebar = sidebar.clone();
    let key = key.clone();
    let label = label.clone();
    multi_workspace_entity.update(cx, |multi_workspace, cx| {
        let workspace = multi_workspace.workspace().clone();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                crate::colour_modal::ColourModal::new(
                    handle, sidebar, key, label, current, window, cx,
                )
            });
        });
    });
}

/// Opens the box that sets the two letters on the avatar.
pub(crate) fn prompt_for_initials(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    key: &ProjectGroupKey,
    label: &SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(multi_workspace) = multi_workspace.upgrade() else {
        return;
    };
    let current = multi_workspace.read(cx).project_presentation(key).0;
    let key = key.clone();
    let label = label.clone();
    let handle = multi_workspace.downgrade();
    multi_workspace.update(cx, |multi_workspace, cx| {
        let workspace = multi_workspace.workspace().clone();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                crate::initials_modal::InitialsModal::new(handle, key, label, current, window, cx)
            });
        });
    });
}

impl crate::Sidebar {
    /// Drops a project into the gap the pointer was last over.
    ///
    /// A gap index, not a row index: `0` is above the first row and `len` is
    /// below the last, which are exactly the two places a row index cannot name
    /// and exactly where people aim when they want a project first or last.
    /// Treating every miss as "append" — which is what this did — sent a drag to
    /// the top of the rail to the bottom, and made a drag to the bottom look
    /// like nothing at all when the project was already there.
    pub(crate) fn drop_project_at_gap(
        &mut self,
        key: &ProjectGroupKey,
        gap: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        self.drop_gap = None;
        let Some(from) = self
            .contents
            .rail_entries
            .iter()
            .position(|entry| entry.key == *key)
        else {
            return;
        };
        // Removing the row before inserting shifts everything after it down by
        // one, so a gap below the row it came from names a lower index.
        let to = if from < gap {
            gap.saturating_sub(1)
        } else {
            gap
        };
        let key = key.clone();
        self.multi_workspace
            .update(cx, |multi_workspace, cx| {
                multi_workspace.move_project_group(&key, to, cx);
            })
            .ok();
        cx.notify();
    }

    /// Records the gap a dragged project is currently over, or clears it.
    pub(crate) fn set_drop_gap(&mut self, gap: Option<usize>, cx: &mut gpui::Context<Self>) {
        if self.drop_gap != gap {
            self.drop_gap = gap;
            cx.notify();
        }
    }
}
