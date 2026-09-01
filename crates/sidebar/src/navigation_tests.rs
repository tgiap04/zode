use crate::Sidebar;
use crate::sidebar_tests::init_test;
use fs::FakeFs;
use gpui::{AppContext as _, TestAppContext};
use project::Project;
use serde_json::json;
use util::ResultExt as _;
use workspace::MultiWorkspace;

/// FR2: keyboard navigation must move `selection` through the entry list
/// and clamp at either end rather than wrapping or clearing -- unlike the
/// pre-hard-fork sidebar's thread list, a plain project list has no
/// "return focus to the search editor" escape hatch tied to the boundary.
#[gpui::test]
async fn test_keyboard_navigation_clamps_at_boundaries(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    let sidebar = multi_workspace.update_in(cx, |mw, window, cx| {
        let mw_entity = cx.entity();
        let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
        mw.register_sidebar(sidebar.clone(), cx);
        sidebar
    });
    multi_workspace.update(cx, |mw, cx| mw.test_enable_background_retention(cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });
    multi_workspace.update(cx, |mw, cx| mw.test_enable_background_retention(cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_c, window, cx);
    });
    cx.run_until_parked();
    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.projects().count(),
            3,
            "all three projects should be listed"
        );
        assert_eq!(
            sidebar.contents.entries.len(),
            6,
            "each project is open by default, so each brings its own worktree row"
        );
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.open_sidebar(cx);
        mw.focus_sidebar(window, cx);
    });
    assert_eq!(
        sidebar.read_with(cx, |s, _cx| s.selection),
        None,
        "focusing the sidebar must not itself set a selection"
    );

    cx.dispatch_action(menu::SelectNext);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(0));
    cx.dispatch_action(menu::SelectNext);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(1));
    cx.dispatch_action(menu::SelectNext);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(2));

    // Walking reaches worktree rows too -- they are rows of the list, not
    // decoration hung off the project above them.
    cx.dispatch_action(menu::SelectLast);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(5));
    cx.dispatch_action(menu::SelectNext);
    assert_eq!(
        sidebar.read_with(cx, |s, _cx| s.selection),
        Some(5),
        "SelectNext past the last entry must clamp, not wrap"
    );

    cx.dispatch_action(menu::SelectFirst);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(0));
    cx.dispatch_action(menu::SelectPrevious);
    assert_eq!(
        sidebar.read_with(cx, |s, _cx| s.selection),
        Some(0),
        "SelectPrevious before the first entry must clamp, not wrap or clear"
    );

    cx.dispatch_action(menu::SelectLast);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), Some(5));
}

/// Navigating an empty entry list (e.g. a filter query with no matches)
/// must be a no-op rather than panicking on an out-of-range index.
#[gpui::test]
async fn test_keyboard_navigation_on_empty_list_is_a_no_op(cx: &mut TestAppContext) {
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
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.open_sidebar(cx);
        mw.focus_sidebar(window, cx);
    });

    // A query that matches nothing empties `contents.entries` without
    // touching which projects are actually open.
    sidebar.update_in(cx, |sidebar, window, cx| {
        sidebar.filter_editor.update(cx, |editor, cx| {
            editor.set_text("no-such-project", window, cx);
        });
    });
    cx.run_until_parked();
    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.entries.len(),
            0,
            "the query should match nothing"
        );
    });

    cx.dispatch_action(menu::SelectNext);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), None);
    cx.dispatch_action(menu::SelectPrevious);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), None);
    cx.dispatch_action(menu::SelectFirst);
    assert_eq!(sidebar.read_with(cx, |s, _cx| s.selection), None);
    cx.dispatch_action(menu::SelectLast);
    assert_eq!(
        sidebar.read_with(cx, |s, _cx| s.selection),
        None,
        "navigating an empty list must never produce a selection"
    );
}

/// A `selection` pointing at a project that then closes must be pulled
/// back in bounds by the next `update_entries` rather than left stale
/// past the new end of `contents.entries`.
#[gpui::test]
async fn test_selection_clamps_after_project_removed(cx: &mut TestAppContext) {
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
    multi_workspace.update(cx, |mw, cx| mw.test_enable_background_retention(cx));
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });
    cx.run_until_parked();

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.open_sidebar(cx);
        mw.focus_sidebar(window, cx);
    });
    cx.dispatch_action(menu::SelectLast);
    let (selected_key, entry_count_before) = sidebar.read_with(cx, |sidebar, _cx| {
        (
            sidebar.contents.entries[sidebar.selection.unwrap()]
                .key()
                .clone(),
            sidebar.contents.entries.len(),
        )
    });
    assert_eq!(
        entry_count_before, 4,
        "two projects, each open and so each bringing one worktree row"
    );

    multi_workspace
        .update_in(cx, |mw, window, cx| {
            mw.remove_project_group(&selected_key, window, cx)
        })
        .await
        .log_err();
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        let entry_count = sidebar.contents.entries.len();
        assert_eq!(
            sidebar.contents.projects().count(),
            1,
            "the removed project's entry should be gone"
        );
        let selection = sidebar.selection;
        assert!(
            selection.is_none_or(|ix| ix < entry_count),
            "selection {selection:?} must stay within bounds (entries: {entry_count})"
        );
    });
}
