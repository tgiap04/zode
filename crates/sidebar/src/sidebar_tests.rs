use crate::Sidebar;
use fs::FakeFs;
use gpui::{AppContext as _, Focusable as _, TestAppContext};
use project::Project;
use serde_json::json;
use settings::Settings as _;
use workspace::{MultiWorkspace, ToggleWorkspaceSidebar};

pub(crate) fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
    });
}

/// Step 2 milestone (Phase 7): the crate exists, satisfies
/// `workspace::Sidebar`, and registering it makes
/// `MultiWorkspace::open_sidebar` (what `cmd-alt-j` /
/// `ToggleWorkspaceSidebar` drives — see `assets/keymaps/default-macos.json`)
/// actually have a sidebar to open, rather than a no-op with nothing
/// registered.
#[gpui::test]
async fn test_registered_sidebar_opens(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar, cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.sidebar().is_some(),
            "registering a Sidebar must make MultiWorkspace::sidebar() return it"
        );
    });

    multi_workspace.update(cx, |mw, cx| {
        mw.open_sidebar(cx);
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.sidebar_open(),
            "opening the sidebar with one registered must flip sidebar_open()"
        );
    });
}

/// The rail's panel-toggle button lives inside the sidebar, so its click
/// handler runs within `Sidebar::update`. `MultiWorkspace::toggle_sidebar`
/// reaches back through `SidebarHandle` (`prepare_for_focus`/`focus`), which
/// borrows that same entity — calling it
/// directly from there aborts with "cannot read Sidebar while it is already
/// being updated". Dispatching the action defers past the borrow; this locks
/// that the toggle still lands when driven from the sidebar's own context.
#[gpui::test]
async fn test_toggle_action_dispatched_from_sidebar_context(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });

    multi_workspace.update(cx, |mw, cx| mw.open_sidebar(cx));
    cx.run_until_parked();
    assert!(
        multi_workspace.read_with(cx, |mw, _cx| mw.sidebar_open()),
        "precondition: the sidebar starts open"
    );

    sidebar.update_in(cx, |_sidebar, window, cx| {
        window.dispatch_action(Box::new(ToggleWorkspaceSidebar), cx);
    });
    cx.run_until_parked();

    assert!(
        !multi_workspace.read_with(cx, |mw, _cx| mw.sidebar_open()),
        "toggling from inside a Sidebar update must close the sidebar, not re-enter it"
    );

    sidebar.update_in(cx, |_sidebar, window, cx| {
        window.dispatch_action(Box::new(ToggleWorkspaceSidebar), cx);
    });
    cx.run_until_parked();

    assert!(
        multi_workspace.read_with(cx, |mw, _cx| mw.sidebar_open()),
        "and toggling again must reopen it"
    );
}

/// FR2: the sidebar's entry list must come straight from
/// `MultiWorkspace::project_groups`, and the active project group must be
/// flagged as active.
#[gpui::test]
async fn test_rebuild_contents_reflects_open_projects(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.entries.len(),
            2,
            "both open projects should be listed"
        );
        let active_count = sidebar
            .contents
            .entries
            .iter()
            .filter(|entry| entry.is_active)
            .count();
        assert_eq!(
            active_count, 1,
            "exactly one entry should be flagged active"
        );
    });
}

/// A stand-in for the real title bar: `Workspace` renders whatever view is
/// mounted in that slot, and the sidebar column only reserves a row when one is.
struct TestTitleBar;

impl gpui::Render for TestTitleBar {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::Styled as _;
        gpui::div().h(ui::utils::platform_title_bar_height(window))
    }
}

/// The sidebar is a sibling of the whole `Workspace`, so left to itself it runs
/// the full window height and ends up alongside the title bar instead of
/// starting where the centre does. The column reserves that row up front — this
/// pins that the sidebar box actually begins below it.
#[gpui::test]
async fn test_sidebar_box_starts_below_the_title_bar(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar, cx);
    });
    cx.run_until_parked();

    // No title bar mounted yet: nothing to clear, so the box starts at the top
    // bar the surface margin. Without this half the assertion below would pass
    // on a sidebar that simply never moved.
    let title_bar_height = cx.update(|window, _| ui::utils::platform_title_bar_height(window));
    let before = cx
        .debug_bounds("sidebar-container")
        .expect("the sidebar container should be drawn");
    assert!(
        before.origin.y < title_bar_height,
        "with no title bar mounted the sidebar should start at the top, was {:?}",
        before.origin.y
    );

    multi_workspace.update_in(cx, |mw, window, cx| {
        let title_bar = cx.new(|_| TestTitleBar);
        mw.workspace().update(cx, |workspace, cx| {
            workspace.set_titlebar_item(title_bar.into(), window, cx);
        });
    });
    cx.run_until_parked();

    let after = cx
        .debug_bounds("sidebar-container")
        .expect("the sidebar container should still be drawn");
    assert!(
        after.origin.y >= title_bar_height,
        "the sidebar box must clear the {:?} title bar row, started at {:?}",
        title_bar_height,
        after.origin.y
    );
}

/// The rail is the outermost column on the left edge, the way VS Code's activity
/// bar sits beyond its sidebar rather than between the sidebar and the editor.
///
/// Measured rather than reasoned: the ordering lives in an element tree, and
/// nothing else in this crate would notice it flipping.
#[gpui::test]
async fn the_rail_stays_outside_the_panel_on_the_left_edge(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar, cx);
        mw.open_sidebar(cx);
    });
    cx.run_until_parked();

    let rail = cx
        .debug_bounds("project-rail")
        .expect("the rail is always drawn while the sidebar is registered");
    let panel = cx
        .debug_bounds("project-list-panel")
        .expect("the project list is drawn while the sidebar is open");

    // Rail first, then the panel. Both halves matter: the rail has to end before
    // the panel begins AND the panel must not start left of the rail, or a zero-
    // width rail would satisfy the first on its own.
    assert!(
        rail.right() <= panel.origin.x,
        "the rail stands outside the panel, got rail {rail:?} panel {panel:?}"
    );
    assert!(
        rail.origin.x < panel.origin.x,
        "the rail is the outer of the two, got rail {rail:?} panel {panel:?}"
    );
}

/// Characterization, written before the sidebar's side stopped being a value.
///
/// `Workspace::activate_pane_in_direction` branched on `sidebar_on_right` in nine
/// places and no test reached the function at all. These two cover the arms a
/// default keybinding actually walks. The tests live here rather than in
/// `workspace` because they need a real, drawn sidebar to hold focus -- a bare
/// `FocusHandle` that was never rendered cannot, so a test written over there
/// would have asserted nothing.
///
/// The remaining seven branches are checked against the plan's nine-site table by
/// diff review; that limit is stated in the plan rather than papered over.
#[gpui::test]
async fn moving_focus_left_from_the_centre_falls_through_to_the_rail(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        mw.open_sidebar(cx);
        sidebar
    });
    cx.run_until_parked();

    // Focus starts in the centre, and there is no left dock open to catch the
    // move -- so the rail beyond it is the only thing left to reach.
    workspace.update_in(cx, |workspace, window, cx| {
        let pane = workspace.active_pane().clone();
        window.focus(&pane.focus_handle(cx), cx);
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.set_open(false, window, cx));
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.activate_pane_in_direction(workspace::SplitDirection::Left, window, cx);
    });
    cx.run_until_parked();

    assert!(
        cx.update(|window, cx| sidebar.focus_handle(cx).contains_focused(window, cx)),
        "with no left dock open, going left from the centre must reach the rail"
    );
}

/// Going further left than the left dock reaches the rail standing beyond it.
#[gpui::test]
async fn moving_focus_from_the_left_dock_reaches_the_rail(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "a.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        mw.open_sidebar(cx);
        sidebar
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .left_dock()
            .update(cx, |dock, cx| dock.set_open(true, window, cx));
        window.focus(&workspace.left_dock().focus_handle(cx), cx);
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.activate_pane_in_direction(workspace::SplitDirection::Left, window, cx);
    });
    cx.run_until_parked();

    assert!(
        cx.update(|window, cx| sidebar.focus_handle(cx).contains_focused(window, cx)),
        "going left out of the left dock must reach the rail beyond it"
    );
}
