use crate::Sidebar;
use crate::sidebar_tests::init_test;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext, px};
use project::{Project, ProjectActivity};
use serde_json::json;
use std::path::PathBuf;
use util::path_list::PathList;
use workspace::{MultiWorkspace, ProjectGroupKey, SerializedProjectGroupState};

/// FR3: typing into the filter editor must narrow `contents.entries` down to
/// the matching project, with byte-offset highlight positions into its
/// label.
#[gpui::test]
async fn test_filter_query_narrows_entries(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/zebra", json!({ "a.txt": "" })).await;
    fs.insert_tree("/apple", json!({ "b.txt": "" })).await;
    let project_zebra = Project::test(fs.clone(), ["/zebra".as_ref()], cx).await;
    let project_apple = Project::test(fs, ["/apple".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_zebra, window, cx));
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
        mw.test_add_workspace(project_apple, window, cx);
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.projects().count(),
            2,
            "no filter query yet"
        );
    });

    sidebar.update_in(cx, |sidebar, window, cx| {
        sidebar.filter_editor.update(cx, |editor, cx| {
            editor.set_text("zeb", window, cx);
        });
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        let matched: Vec<_> = sidebar.contents.projects().collect();
        assert_eq!(
            matched.len(),
            1,
            "\"zeb\" should match only the zebra project"
        );
        assert!(
            !matched[0].highlight_positions.is_empty(),
            "a matching entry should carry highlight positions for its match"
        );
        // The rail is the only project switcher visible while the panel is
        // closed, so a query typed into the panel must not be able to hide a
        // project from it.
        assert_eq!(
            sidebar.contents.rail_entries.len(),
            2,
            "the rail must keep listing every project regardless of the filter"
        );
    });
}

/// FR7: a project's `ProjectActivity` (Phase 2) must surface on its entry,
/// so the sidebar can show which project is asleep.
#[gpui::test]
async fn test_hibernated_project_reflected_in_entry_activity(cx: &mut TestAppContext) {
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
        mw.test_add_workspace(project_b.clone(), window, cx);
    });
    cx.run_until_parked();

    // `set_activity` refuses to jump straight from `Active` to
    // `Hibernated` (see its own doc comment) -- go through `Warm` first,
    // same as `MultiWorkspace`'s own idle-timer path would.
    project_b.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Warm, cx);
        project.set_activity(ProjectActivity::Hibernated, cx);
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        let hibernated_count = sidebar
            .contents
            .projects()
            .filter(|entry| entry.activity == Some(ProjectActivity::Hibernated))
            .count();
        assert_eq!(
            hibernated_count, 1,
            "exactly the hibernated project's entry should report Hibernated activity"
        );
    });
}

/// Step 7: `Sidebar` must actually serialize/restore its own state (width)
/// through the `workspace::Sidebar` trait's blob, and must not panic on a
/// blob saved by the pre-fork, thread-based sidebar (unknown fields should
/// just be ignored, per `serde`'s default behavior).
#[gpui::test]
async fn test_serialized_state_round_trips_width(cx: &mut TestAppContext) {
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

    sidebar.update(cx, |sidebar, _cx| {
        sidebar.width = px(420.0);
    });
    let serialized = sidebar
        .read_with(cx, |sidebar, _cx| sidebar.serialize_to_string())
        .expect("width should always serialize to a blob");

    sidebar.update(cx, |sidebar, cx| {
        sidebar.width = crate::DEFAULT_WIDTH;
        sidebar.apply_serialized_state(&serialized, cx);
    });
    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.width,
            px(420.0),
            "restoring a just-saved blob must recover the saved width"
        );
    });

    sidebar.update(cx, |sidebar, cx| {
        sidebar.apply_serialized_state(r#"{"active_view":"ThreadList"}"#, cx);
    });
}

/// A session restore replays the previous window's rail into `MultiWorkspace`
/// *after* the window (and this sidebar) already exist, so the projects it
/// brings back only reach the rail if that replay announces itself. It did not,
/// and a window closed on two projects reopened showing just the one
/// `derived_project_groups` synthesizes for the active workspace -- while the
/// persisted record still held both.
#[gpui::test]
async fn test_restored_project_groups_reach_the_rail(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "a.txt": "" })).await;
    let project_a = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.rail_entries.len(),
            1,
            "before the restore the rail shows only the window's own project"
        );
    });

    let restored = ["/root_a", "/root_b"]
        .into_iter()
        .map(|path| SerializedProjectGroupState {
            key: ProjectGroupKey::new(None, PathList::new(&[PathBuf::from(path)])),
            expanded: true,
            initials: None,
            colour: None,
        })
        .collect();
    multi_workspace.update(cx, |mw, cx| {
        mw.restore_project_groups(restored, cx);
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.rail_entries.len(),
            2,
            "every restored project must land on the rail, not just the active one"
        );
    });
}
