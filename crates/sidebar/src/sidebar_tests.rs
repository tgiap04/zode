use crate::Sidebar;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext};
use project::Project;
use serde_json::json;
use settings::Settings as _;
use workspace::MultiWorkspace;

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
