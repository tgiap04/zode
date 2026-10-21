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
        assert_eq!(sidebar.contents.entries.len(), 2, "no filter query yet");
    });

    sidebar.update_in(cx, |sidebar, window, cx| {
        sidebar.filter_editor.update(cx, |editor, cx| {
            editor.set_text("zeb", window, cx);
        });
    });
    cx.run_until_parked();

    sidebar.read_with(cx, |sidebar, _cx| {
        assert_eq!(
            sidebar.contents.entries.len(),
            1,
            "\"zeb\" should match only the zebra project"
        );
        assert!(
            !sidebar.contents.entries[0].highlight_positions.is_empty(),
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
            .entries
            .iter()
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
            logo: None,
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

mod waiting_agents {
    use super::*;
    use crate::rail_test_support::{
        TwoProjects, add_waiting_claude_tab, new_claude_agent_view, two_projects, waiting,
    };
    use agent_ui::{AgentView, TurnEvent};
    use gpui::{Entity, Focusable as _, VisualTestContext};

    fn turn(view: &Entity<AgentView>, events: &[TurnEvent], cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| view.simulate_turn_events(events, cx));
        cx.run_until_parked();
    }

    #[gpui::test]
    async fn a_project_without_agent_tabs_has_none_waiting(cx: &mut TestAppContext) {
        let (TwoProjects { sidebar, .. }, cx) = two_projects(cx).await;
        assert_eq!(waiting(&sidebar, "root_a", cx), 0);
        assert_eq!(waiting(&sidebar, "root_b", cx), 0);
    }

    #[gpui::test]
    async fn a_finished_turn_in_a_background_project_reaches_its_rail_entry(
        cx: &mut TestAppContext,
    ) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 0, "nothing has ended yet");

        turn(&view, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 1);
        assert_eq!(
            waiting(&sidebar, "root_b", cx),
            0,
            "only the owning project"
        );

        turn(&view, &[TurnEvent::Started], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 0, "a new turn retracts it");
    }

    #[gpui::test]
    async fn an_interrupted_turn_never_counts(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        turn(&view, &[TurnEvent::Ended, TurnEvent::Interrupted], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 0);
    }

    #[gpui::test]
    async fn focusing_the_waiting_tab_clears_it(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                multi_workspace,
                sidebar,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        // Focus only lands in the workspace that is in front.
        let workspace_b = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let project_b = workspace_b.read_with(cx, |workspace, _| workspace.project().clone());
        let view = add_waiting_claude_tab(&workspace_b, &project_b, cx);
        turn(&view, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_b", cx), 1);

        view.update_in(cx, |view, window, cx| {
            let handle = view.focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.run_until_parked();
        assert_eq!(waiting(&sidebar, "root_b", cx), 0);
    }

    #[gpui::test]
    async fn an_approval_counts_even_on_the_focused_tab(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        view.update_in(cx, |view, window, cx| {
            let handle = view.focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.run_until_parked();

        view.update(cx, |view, cx| view.simulate_approval(true, cx));
        cx.run_until_parked();
        assert_eq!(waiting(&sidebar, "root_a", cx), 1);

        view.update(cx, |view, cx| view.simulate_approval(false, cx));
        cx.run_until_parked();
        assert_eq!(waiting(&sidebar, "root_a", cx), 0);
    }

    #[gpui::test]
    async fn the_agent_cli_ending_clears_the_count(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        turn(&view, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 1);

        view.update(cx, |view, cx| view.simulate_cli_ended(cx));
        cx.run_until_parked();
        assert_eq!(waiting(&sidebar, "root_a", cx), 0);
    }

    #[gpui::test]
    async fn closing_a_waiting_tab_clears_it_and_drops_its_subscription(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        turn(&view, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 1);
        let watched =
            sidebar.read_with(cx, |sidebar, _| sidebar.agent_attention_subscriptions.len());
        assert_eq!(watched, 1);

        workspace_a.update_in(cx, |workspace, window, cx| {
            workspace.active_pane().update(cx, |pane, cx| {
                pane.remove_item(view.entity_id(), false, false, window, cx);
            });
        });
        cx.run_until_parked();

        assert_eq!(waiting(&sidebar, "root_a", cx), 0);
        let watched =
            sidebar.read_with(cx, |sidebar, _| sidebar.agent_attention_subscriptions.len());
        assert_eq!(watched, 0, "a closed tab must not stay subscribed");
    }

    #[gpui::test]
    async fn a_hibernated_project_still_counts_its_waiting_tab(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        let view = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        turn(&view, &[TurnEvent::Ended], cx);

        project_a.update(cx, |project, cx| {
            project.set_activity(ProjectActivity::Warm, cx);
            project.set_activity(ProjectActivity::Hibernated, cx);
        });
        cx.run_until_parked();

        sidebar.read_with(cx, |sidebar, _| {
            let entry = sidebar
                .contents
                .rail_entries
                .iter()
                .find(|entry| entry.label.as_ref() == "root_a")
                .expect("on the rail");
            assert_eq!(entry.activity, Some(ProjectActivity::Hibernated));
            assert_eq!(entry.waiting_agents, 1);
        });
    }

    #[gpui::test]
    async fn waiting_tabs_across_a_groups_workspaces_are_summed(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                multi_workspace,
                sidebar,
                workspace_a,
                project_a,
            },
            cx,
        ) = two_projects(cx).await;
        let fs = project_a.read_with(cx, |project, _| project.fs().clone());
        let twin = Project::test(fs, ["/root_a".as_ref()], cx).await;
        let twin_workspace = multi_workspace.update_in(cx, |mw, window, cx| {
            mw.test_add_workspace(twin.clone(), window, cx)
        });
        cx.run_until_parked();

        let first = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        let second = add_waiting_claude_tab(&workspace_a, &project_a, cx);
        let third = add_waiting_claude_tab(&twin_workspace, &twin, cx);
        turn(&first, &[TurnEvent::Ended], cx);
        turn(&third, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 2);

        turn(&second, &[TurnEvent::Ended], cx);
        assert_eq!(waiting(&sidebar, "root_a", cx), 3);
    }

    /// A tab that already holds an approval when it lands emits no further
    /// `Attention`, so only the workspace's own `ItemAdded` can tell the rail.
    #[gpui::test]
    async fn a_tab_that_arrives_already_waiting_is_counted(cx: &mut TestAppContext) {
        let (
            TwoProjects {
                sidebar,
                workspace_a,
                project_a,
                ..
            },
            cx,
        ) = two_projects(cx).await;
        workspace_a.update_in(cx, |workspace, window, cx| {
            let view = new_claude_agent_view(workspace, &project_a, cx);
            view.update(cx, |view, cx| view.simulate_approval(true, cx));
            workspace.add_item_to_active_pane(Box::new(view), None, false, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(waiting(&sidebar, "root_a", cx), 1);
    }
}
