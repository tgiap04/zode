//! Everything that changes how a project *looks* on the rail.
//!
//! Split from `project_actions.rs` for the reason that file's own doc gives for
//! keeping lists at the surfaces and behaviour behind them: these four change
//! for one reason -- a project's appearance -- and the lifecycle actions beside
//! them change for another. Keeping both in one file pushed it past the size
//! the rest of this crate holds to.
//!
//! Free functions, like their siblings: they run from a `ContextMenu` callback
//! long after the render that built it, and take what they need rather than
//! reading sidebar state at that point.

use gpui::{App, PathPromptOptions, SharedString, WeakEntity, Window};
use project::ProjectGroupKey;
use workspace::MultiWorkspace;

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
    let current = multi_workspace_entity
        .read(cx)
        .project_presentation(key)
        .colour;
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
    let current = multi_workspace.read(cx).project_presentation(key).initials;
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

/// Opens the native file picker and, if something was chosen, makes it this
/// project's logo.
///
/// Ask, then hand off. Every real decision — the extension gate, the size
/// gate, the copy, the toast on failure — lives in
/// `MultiWorkspace::set_project_logo`, which is also what every behaviour
/// test drives directly: `TestPlatform::prompt_for_paths` is
/// `unimplemented!()`, so this function must stay too thin to hold a bug a
/// test could otherwise catch.
pub(crate) fn prompt_for_logo(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    key: &ProjectGroupKey,
    _window: &mut Window,
    cx: &mut App,
) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose Logo".into()),
    });
    let multi_workspace = multi_workspace.clone();
    let key = key.clone();
    cx.spawn(async move |cx| {
        // A channel error, an `Err`, and a `None` all mean the same thing
        // here — the user did not choose a file — and none of them is worth
        // telling anyone about.
        let Ok(Ok(Some(mut paths))) = paths.await else {
            return;
        };
        let Some(source) = paths.pop() else {
            return;
        };
        multi_workspace
            .update(cx, |multi_workspace, cx| {
                multi_workspace.set_project_logo(&key, source, cx).detach();
            })
            .ok();
    })
    .detach();
}

/// Clears the project's logo.
pub(crate) fn remove_logo(
    multi_workspace: &WeakEntity<MultiWorkspace>,
    key: &ProjectGroupKey,
    cx: &mut App,
) {
    let key = key.clone();
    multi_workspace
        .update(cx, |multi_workspace, cx| {
            multi_workspace.clear_project_logo(&key, cx);
        })
        .ok();
}
