//! The 32px square drawn on the rail: background, logo/initials, border, and
//! the hibernated-opacity treatment. Split out of `rail_item.rs` so that file
//! keeps to the row -- the menu wrapper, the drag handlers, the click, the
//! tooltip -- and doesn't carry the square's own decisions on top of them.

use gpui::{AnyElement, Context, Hsla, SharedString};
use std::path::Path;
use std::sync::Arc;
use ui::prelude::*;
use workspace::project_avatar::ProjectAvatar;

use crate::Sidebar;

pub(crate) const RAIL_SQUARE_SIZE: Pixels = px(32.0);

impl Sidebar {
    /// The square itself: background, then logo or initials (through
    /// `ProjectAvatar`, which owns that split), plus the selected border and
    /// the hibernated opacity. The active-project pill, the re-indexing dot,
    /// hover, tooltip, click, and drag all stay in `render_rail_item` -- they
    /// are positional to the row, not to the square.
    pub(crate) fn render_rail_avatar(
        &self,
        ix: usize,
        initials: SharedString,
        custom_colour: Option<Hsla>,
        square_bg: Hsla,
        logo: Option<Arc<Path>>,
        is_active: bool,
        is_hibernated: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors();
        let border_selected = colors.border_selected;
        let border_transparent = colors.border_transparent;

        div()
            .id(("project-rail-avatar", ix))
            .debug_selector(move || format!("project-rail-avatar:{ix}"))
            .size(RAIL_SQUARE_SIZE)
            .rounded_md()
            .border_1()
            .map(|el| {
                if is_active {
                    el.border_color(border_selected)
                } else {
                    el.border_color(border_transparent)
                }
            })
            .when(is_hibernated && !is_active, |el| el.opacity(0.6))
            .child(
                ProjectAvatar::new(initials, custom_colour, logo)
                    .size(RAIL_SQUARE_SIZE)
                    .background(square_bg)
                    // A project at rest reads quieter than the active one.
                    // Only the themed fallback dims: initials over a picked
                    // colour stay computed for contrast against it.
                    .muted_initials(!is_active),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use serde_json::json;
    use util::path;
    use workspace::MultiWorkspace;
    use workspace::project_avatar::{INITIALS_DEBUG_SELECTOR, LOGO_DEBUG_SELECTOR};

    /// One project, registered with a real `MultiWorkspace` and `Sidebar`,
    /// the same setup `rail_item.rs`'s own tests use.
    async fn one_project_on_a_rail(
        cx: &mut TestAppContext,
    ) -> (
        gpui::Entity<MultiWorkspace>,
        gpui::Entity<Sidebar>,
        &mut gpui::VisualTestContext,
    ) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/root_a"), json!({ "a.txt": "" }))
            .await;
        let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        (multi_workspace, sidebar, cx)
    }

    #[gpui::test]
    async fn a_project_without_a_logo_draws_its_initials(cx: &mut TestAppContext) {
        let (_multi_workspace, _sidebar, cx) = one_project_on_a_rail(cx).await;

        assert!(
            cx.debug_bounds(INITIALS_DEBUG_SELECTOR).is_some(),
            "with no logo set, the rail square must fall back to initials"
        );
        assert!(
            cx.debug_bounds(LOGO_DEBUG_SELECTOR).is_none(),
            "and must not draw a logo it doesn't have"
        );
    }

    /// Sets a real logo on the window's own project, through the same
    /// `set_project_logo` the "Add Logo…" menu entry calls, and waits for the
    /// copy + re-render to settle.
    async fn give_the_project_a_logo(
        multi_workspace: &gpui::Entity<MultiWorkspace>,
        cx: &mut gpui::VisualTestContext,
    ) {
        let key = multi_workspace.read_with(cx, |mw, cx| mw.project_groups(cx)[0].key.clone());
        multi_workspace
            .update(cx, |mw, cx| {
                mw.set_project_logo(
                    &key,
                    std::path::PathBuf::from(path!("/root_a/source_logo.png")),
                    cx,
                )
            })
            .await;
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_project_with_a_logo_draws_the_image_instead(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root_a"),
            json!({ "a.txt": "", "source_logo.png": "" }),
        )
        .await;
        let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        give_the_project_a_logo(&multi_workspace, cx).await;

        // `ImgResourceLoader` reads the real disk (`img.rs:613`), so this
        // never decodes under `FakeFs` -- only the element's presence is
        // asserted, never its pixels.
        assert!(
            cx.debug_bounds(LOGO_DEBUG_SELECTOR).is_some(),
            "with a logo set, the rail square must draw the image element"
        );
    }

    #[gpui::test]
    async fn the_rails_decorations_survive_the_extraction(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/root_a"),
            json!({ "a.txt": "", "source_logo.png": "" }),
        )
        .await;
        let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar.clone(), cx);
            sidebar
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        give_the_project_a_logo(&multi_workspace, cx).await;

        // The window's own project is the only one open, so it is already the
        // active project (the pill) -- forcing `is_reindexing` directly is the
        // one piece this test still has to fake, since that flag otherwise
        // only turns on behind real stale-diagnostics/LSP plumbing this test
        // has no business standing up.
        sidebar.update(cx, |sidebar, cx| {
            sidebar.contents.rail_entries[0].is_reindexing = true;
            cx.notify();
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();

        assert!(
            cx.debug_bounds(LOGO_DEBUG_SELECTOR).is_some(),
            "the logo must still draw once the square moves into its own module"
        );
        assert!(
            cx.debug_bounds("project-rail-item-pill:0").is_some(),
            "the active-project pill must survive the extraction"
        );
        assert!(
            cx.debug_bounds("project-rail-item-reindex-dot:0").is_some(),
            "the re-indexing dot must survive the extraction"
        );

        let avatar = cx
            .debug_bounds("project-rail-item:0")
            .expect("the row must still draw");
        let rail = cx
            .debug_bounds("project-rail")
            .expect("the rail must be drawn");
        assert_eq!(
            avatar.size.width, rail.size.width,
            "the row must still span the rail: {avatar:?} inside {rail:?}"
        );
        assert_eq!(
            avatar.origin.x, rail.origin.x,
            "and still start at its leading edge"
        );
    }
}
