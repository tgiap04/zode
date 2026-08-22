use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::item::test::TestItem;
use crate::multi_workspace::{MemoryPressureFuseToast, MemoryPressureReader};
use crate::notifications::NotificationId;
use client::proto;
use fs::{FakeFs, Fs};
use gpui::{TestAppContext, UpdateGlobal, VisualTestContext};
use project::{DisableAiSettings, ProjectActivity};
use serde_json::json;
use settings::{MultiProjectContent, SettingsStore};
use util::path;

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        DisableAiSettings::register(cx);
    });
}

/// Sets `workspace.multi_project.retain_background_projects` to `false`
/// explicitly, rather than relying on the phase's current default, so
/// tests pinning this behavior still hold once the default flips to
/// `true` at the end of Phase 3.
fn disable_background_project_retention(cx: &mut Context<MultiWorkspace>) {
    SettingsStore::update_global(cx, |settings, cx| {
        settings.update_user_settings(cx, |settings| {
            settings.workspace.multi_project = Some(MultiProjectContent {
                retain_background_projects: Some(false),
                // `None` is a no-op merge here — this helper only pins
                // retention, not hibernation, the memory fuse, or terminal
                // scroll history.
                hibernate_after_ms: None,
                memory_pressure_threshold_percent: None,
                background_scroll_history_lines: None,
            });
        });
    });
}

/// Sets both `retain_background_projects` and `hibernate_after_ms`
/// explicitly in one shot. Deliberately does not compose with the other
/// `Some(...)`/`None` helpers above by calling `update_user_settings`
/// twice: each call replaces the *entire* `multi_project` content for the
/// user layer, so a second call setting one field to `None` would silently
/// erase a field the first call had set — not a merge. Every hibernate
/// governor test below needs both fields pinned together, so this is the
/// only helper they use.
fn set_multi_project_settings(
    cx: &mut Context<MultiWorkspace>,
    retain_background_projects: bool,
    hibernate_after_ms: u64,
) {
    SettingsStore::update_global(cx, |settings, cx| {
        settings.update_user_settings(cx, |settings| {
            settings.workspace.multi_project = Some(MultiProjectContent {
                retain_background_projects: Some(retain_background_projects),
                hibernate_after_ms: Some(hibernate_after_ms),
                // `None` is a no-op merge here — this helper only pins
                // retention and hibernation, not the memory fuse or terminal
                // scroll history.
                memory_pressure_threshold_percent: None,
                background_scroll_history_lines: None,
            });
        });
    });
}

/// Sets `retain_background_projects`, `hibernate_after_ms`, and
/// `memory_pressure_threshold_percent` explicitly in one shot, for the
/// Phase 6 memory-fuse tests below. `hibernate_after_ms` is nearly always
/// `0` (idle-timer hibernation disabled) in those tests, so the fuse —
/// not the ordinary idle timer — is unambiguously what causes any
/// `Hibernated` transition being asserted on.
fn set_memory_fuse_settings(
    cx: &mut Context<MultiWorkspace>,
    retain_background_projects: bool,
    hibernate_after_ms: u64,
    memory_pressure_threshold_percent: f32,
) {
    SettingsStore::update_global(cx, |settings, cx| {
        settings.update_user_settings(cx, |settings| {
            settings.workspace.multi_project = Some(MultiProjectContent {
                retain_background_projects: Some(retain_background_projects),
                hibernate_after_ms: Some(hibernate_after_ms),
                memory_pressure_threshold_percent: Some(memory_pressure_threshold_percent),
                // `None` is a no-op merge here — the memory-fuse tests
                // don't exercise terminal scroll history.
                background_scroll_history_lines: None,
            });
        });
    });
}

/// Test double for `MemoryPressureReader` (Phase 6): reports whatever
/// `available_percent` currently holds, so a test can move memory
/// pressure up or down over time via the shared `Rc<Cell<_>>` without
/// re-injecting a new reader.
struct FakeMemoryPressureReader {
    available_percent: Rc<Cell<f32>>,
}

impl MemoryPressureReader for FakeMemoryPressureReader {
    fn available_memory_percent(&mut self) -> Option<f32> {
        Some(self.available_percent.get())
    }
}

#[gpui::test]
async fn test_sidebar_disabled_when_disable_ai_is_enabled(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    multi_workspace.read_with(cx, |mw, cx| {
        assert!(mw.multi_workspace_enabled(cx));
    });

    multi_workspace.update_in(cx, |mw, _window, cx| {
        mw.open_sidebar(cx);
        assert!(mw.sidebar_open());
    });

    cx.update(|_window, cx| {
        DisableAiSettings::override_global(DisableAiSettings { disable_ai: true }, cx);
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, cx| {
        assert!(
            !mw.sidebar_open(),
            "Sidebar should be closed when disable_ai is true"
        );
        assert!(
            !mw.multi_workspace_enabled(cx),
            "Multi-workspace should be disabled when disable_ai is true"
        );
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.toggle_sidebar(window, cx);
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.sidebar_open(),
            "Sidebar should remain closed when toggled with disable_ai true"
        );
    });

    cx.update(|_window, cx| {
        DisableAiSettings::override_global(DisableAiSettings { disable_ai: false }, cx);
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, cx| {
        assert!(
            mw.multi_workspace_enabled(cx),
            "Multi-workspace should be enabled after re-enabling AI"
        );
        assert!(
            !mw.sidebar_open(),
            "Sidebar should still be closed after re-enabling AI (not auto-opened)"
        );
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.toggle_sidebar(window, cx);
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.sidebar_open(),
            "Sidebar should open when toggled after re-enabling AI"
        );
    });
}

#[gpui::test]
async fn test_project_group_keys_initial(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let expected_key = project.read_with(cx, |project, cx| project.project_group_key(cx));

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys: Vec<ProjectGroupKey> = mw.project_group_keys();
        assert_eq!(keys.len(), 1, "should have exactly one key on creation");
        assert_eq!(keys[0], expected_key);
    });
}

#[gpui::test]
async fn test_project_group_keys_add_workspace(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;

    let key_a = project_a.read_with(cx, |p, cx| p.project_group_key(cx));
    let key_b = project_b.read_with(cx, |p, cx| p.project_group_key(cx));
    assert_ne!(
        key_a, key_b,
        "different roots should produce different keys"
    );

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(mw.project_group_keys().len(), 1);
    });

    // Adding a workspace with a different project root adds a new key.
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys: Vec<ProjectGroupKey> = mw.project_group_keys();
        assert_eq!(
            keys.len(),
            2,
            "should have two keys after adding a second workspace"
        );
        assert_eq!(keys[0], key_b);
        assert_eq!(keys[1], key_a);
    });
}

#[gpui::test]
async fn test_open_new_window_does_not_open_sidebar_on_existing_window(cx: &mut TestAppContext) {
    init_test(cx);

    let app_state = cx.update(AppState::test);
    let fs = app_state.fs.as_fake();
    fs.insert_tree(path!("/project_a"), json!({ "file.txt": "" }))
        .await;
    fs.insert_tree(path!("/project_b"), json!({ "file.txt": "" }))
        .await;

    let project = Project::test(app_state.fs.clone(), [path!("/project_a").as_ref()], cx).await;

    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project, window, cx));

    window
        .read_with(cx, |mw, _cx| {
            assert!(!mw.sidebar_open(), "sidebar should start closed",);
        })
        .unwrap();

    cx.update(|cx| {
        open_paths(
            &[PathBuf::from(path!("/project_b"))],
            app_state,
            OpenOptions {
                open_mode: OpenMode::NewWindow,
                ..OpenOptions::default()
            },
            cx,
        )
    })
    .await
    .unwrap();

    window
        .read_with(cx, |mw, _cx| {
            assert!(
                !mw.sidebar_open(),
                "opening a project in a new window must not open the sidebar on the original window",
            );
        })
        .unwrap();
}

#[gpui::test]
async fn test_open_directory_in_empty_workspace_does_not_open_sidebar(cx: &mut TestAppContext) {
    init_test(cx);

    let app_state = cx.update(AppState::test);
    let fs = app_state.fs.as_fake();
    fs.insert_tree(path!("/project"), json!({ "file.txt": "" }))
        .await;

    let project = Project::test(app_state.fs.clone(), [], cx).await;
    let window = cx.add_window(|window, cx| {
        let mw = MultiWorkspace::test_new(project, window, cx);
        // Simulate a blank project that has an untitled editor tab,
        // so that workspace_windows_for_location finds this window.
        mw.workspace().update(cx, |workspace, cx| {
            workspace.active_pane().update(cx, |pane, cx| {
                let item = cx.new(|cx| item::test::TestItem::new(cx));
                pane.add_item(Box::new(item), false, false, None, window, cx);
            });
        });
        mw
    });

    window
        .read_with(cx, |mw, _cx| {
            assert!(!mw.sidebar_open(), "sidebar should start closed");
        })
        .unwrap();

    // Simulate what open_workspace_for_paths does for an empty workspace:
    // it downgrades OpenMode::NewWindow to Activate and sets requesting_window.
    cx.update(|cx| {
        open_paths(
            &[PathBuf::from(path!("/project"))],
            app_state,
            OpenOptions {
                requesting_window: Some(window),
                open_mode: OpenMode::Activate,
                ..OpenOptions::default()
            },
            cx,
        )
    })
    .await
    .unwrap();

    window
        .read_with(cx, |mw, _cx| {
            assert!(
                !mw.sidebar_open(),
                "opening a directory in a blank project via the file picker must not open the sidebar",
            );
        })
        .unwrap();
}

#[gpui::test]
async fn test_project_group_keys_duplicate_not_added(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    // A second project entity pointing at the same path produces the same key.
    let project_a2 = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;

    let key_a = project_a.read_with(cx, |p, cx| p.project_group_key(cx));
    let key_a2 = project_a2.read_with(cx, |p, cx| p.project_group_key(cx));
    assert_eq!(key_a, key_a2, "same root path should produce the same key");

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_a2, window, cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys: Vec<ProjectGroupKey> = mw.project_group_keys();
        assert_eq!(
            keys.len(),
            1,
            "duplicate key should not be added when a workspace with the same root is inserted"
        );
    });
}

#[gpui::test]
async fn test_adding_worktree_updates_project_group_key(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "other.txt": "" })).await;
    let project = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;

    let initial_key = project.read_with(cx, |p, cx| p.project_group_key(cx));

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));

    // Enable retention so this workspace is retained and gets an initial
    // project group.
    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys = mw.project_group_keys();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], initial_key);
    });

    // Add a second worktree to the project. This triggers WorktreeAdded →
    // handle_workspace_key_change, which should update the group key.
    project
        .update(cx, |project, cx| {
            project.find_or_create_worktree("/root_b", true, cx)
        })
        .await
        .expect("adding worktree should succeed");
    cx.run_until_parked();

    let updated_key = project.read_with(cx, |p, cx| p.project_group_key(cx));
    assert_ne!(
        initial_key, updated_key,
        "adding a worktree should change the project group key"
    );

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys = mw.project_group_keys();
        assert!(
            keys.contains(&updated_key),
            "should contain the updated key; got {keys:?}"
        );
    });
}

#[gpui::test]
async fn test_find_or_create_local_workspace_reuses_active_workspace_when_sidebar_closed(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    let active_workspace = multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.retained_workspaces().is_empty(),
            "sidebar-closed setup should start with nothing retained"
        );
        mw.workspace().clone()
    });
    let active_workspace_id = active_workspace.entity_id();

    let workspace = multi_workspace
        .update_in(cx, |mw, window, cx| {
            mw.find_or_create_local_workspace(
                PathList::new(&[PathBuf::from("/root_a")]),
                None,
                &[],
                None,
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .await
        .expect("reopening the same local workspace should succeed");

    assert_eq!(
        workspace.entity_id(),
        active_workspace_id,
        "should reuse the current active workspace when the sidebar is closed"
    );

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            active_workspace_id,
            "active workspace should remain unchanged after reopening the same path"
        );
        assert_eq!(
            mw.workspaces().count(),
            1,
            "reusing the active workspace should not create a second open workspace"
        );
    });
}

#[gpui::test]
async fn test_find_or_create_workspace_uses_project_group_key_when_paths_are_missing(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/project",
        json!({
            ".git": {},
            "src": {},
        }),
    )
    .await;
    cx.update(|cx| <dyn Fs>::set_global(fs.clone(), cx));
    let project = Project::test(fs.clone(), ["/project".as_ref()], cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let project_group_key = project.read_with(cx, |project, cx| project.project_group_key(cx));

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    let main_workspace = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    let main_workspace_id = main_workspace.entity_id();

    let workspace = multi_workspace
        .update_in(cx, |mw, window, cx| {
            mw.find_or_create_workspace(
                PathList::new(&[PathBuf::from("/wt-feature-a")]),
                None,
                Some(project_group_key.clone()),
                |_options, _window, _cx| Task::ready(Ok(None)),
                &[],
                None,
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .await
        .expect("opening a missing linked-worktree path should fall back to the project group key workspace");

    assert_eq!(
        workspace.entity_id(),
        main_workspace_id,
        "missing linked-worktree paths should reuse the main worktree workspace from the project group key"
    );

    multi_workspace.read_with(cx, |mw, cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            main_workspace_id,
            "the active workspace should remain the main worktree workspace"
        );
        assert_eq!(
            PathList::new(&mw.workspace().read(cx).root_paths(cx)),
            project_group_key.path_list().clone(),
            "the activated workspace should use the project group key path list rather than the missing linked-worktree path"
        );
        assert_eq!(
            mw.workspaces().count(),
            1,
            "falling back to the project group key should not create a second workspace"
        );
    });
}

#[gpui::test]
async fn test_find_or_create_local_workspace_reuses_active_workspace_after_sidebar_open(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });
    cx.run_until_parked();

    let active_workspace = multi_workspace.read_with(cx, |mw, cx| {
        assert_eq!(
            mw.project_groups(cx).len(),
            1,
            "enabling retention should retain the active workspace in a project group"
        );
        mw.workspace().clone()
    });
    let active_workspace_id = active_workspace.entity_id();

    let workspace = multi_workspace
        .update_in(cx, |mw, window, cx| {
            mw.find_or_create_local_workspace(
                PathList::new(&[PathBuf::from("/root_a")]),
                None,
                &[],
                None,
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .await
        .expect("reopening the same retained local workspace should succeed");

    assert_eq!(
        workspace.entity_id(),
        active_workspace_id,
        "should reuse the retained active workspace after the sidebar is opened"
    );

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspaces().count(),
            1,
            "reopening the same retained workspace should not create another workspace"
        );
    });
}

#[gpui::test]
async fn test_close_workspace_prefers_already_loaded_neighboring_workspace(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "file_c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_b_key = project_b.read_with(cx, |project, cx| project.project_group_key(cx));
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;
    let project_c_key = project_c.read_with(cx, |project, cx| project.project_group_key(cx));

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    multi_workspace.update(cx, |multi_workspace, cx| {
        multi_workspace.test_enable_background_retention(cx);
    });
    cx.run_until_parked();

    let workspace_a = multi_workspace.read_with(cx, |multi_workspace, _cx| {
        multi_workspace.workspace().clone()
    });
    let workspace_b = multi_workspace.update_in(cx, |multi_workspace, window, cx| {
        multi_workspace.test_add_workspace(project_b, window, cx)
    });

    multi_workspace.update_in(cx, |multi_workspace, window, cx| {
        multi_workspace.activate(workspace_a.clone(), None, window, cx);
        multi_workspace.test_add_project_group(ProjectGroup {
            key: project_c_key.clone(),
            workspaces: Vec::new(),
            expanded: true,
        });
    });

    multi_workspace.read_with(cx, |multi_workspace, _cx| {
        let keys = multi_workspace.project_group_keys();
        assert_eq!(
            keys.len(),
            3,
            "expected three project groups in the test setup"
        );
        assert_eq!(keys[0], project_b_key);
        assert_eq!(
            keys[1],
            workspace_a.read_with(cx, |workspace, cx| { workspace.project_group_key(cx) })
        );
        assert_eq!(keys[2], project_c_key);
        assert_eq!(
            multi_workspace.workspace().entity_id(),
            workspace_a.entity_id(),
            "workspace A should be active before closing"
        );
    });

    let closed = multi_workspace
        .update_in(cx, |multi_workspace, window, cx| {
            multi_workspace.close_workspace(&workspace_a, window, cx)
        })
        .await
        .expect("closing the active workspace should succeed");

    assert!(
        closed,
        "close_workspace should report that it removed a workspace"
    );

    multi_workspace.read_with(cx, |multi_workspace, cx| {
        assert_eq!(
            multi_workspace.workspace().entity_id(),
            workspace_b.entity_id(),
            "closing workspace A should activate the already-loaded workspace B instead of opening group C"
        );
        assert_eq!(
            multi_workspace.workspaces().count(),
            1,
            "only workspace B should remain loaded after closing workspace A"
        );
        assert!(
            multi_workspace
                .workspaces_for_project_group(&project_c_key, cx)
                .unwrap_or_default()
                .is_empty(),
            "the unloaded neighboring group C should remain unopened"
        );
    });
}

#[gpui::test]
async fn test_switching_projects_with_sidebar_closed_detaches_old_active_workspace(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    // Pin the setting explicitly rather than relying on the phase's
    // current default, so this regression test still holds once the
    // default flips to `true` at the end of Phase 3.
    multi_workspace.update(cx, |_mw, cx| {
        disable_background_project_retention(cx);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.retained_workspaces().is_empty(),
            "sidebar-closed setup should start with nothing retained"
        );
        mw.workspace().clone()
    });
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_some()),
        "initial active workspace should start attached to the session"
    );

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx)
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_b.entity_id(),
            "the new workspace should become active"
        );
        assert_eq!(
            mw.workspaces().count(),
                        1,
                        "only the new active workspace should remain open after switching with the sidebar closed"
        );
    });

    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_none()),
        "the previous active workspace should be detached when switching away with the sidebar closed"
    );
}

#[gpui::test]
async fn test_remote_project_root_dir_changes_update_groups(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file.txt": "" })).await;
    fs.insert_tree("/local_b", json!({ "file.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/local_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
    });
    cx.run_until_parked();

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_b.clone(), window, cx));
        let key = workspace.read(cx).project_group_key(cx);
        mw.activate_provisional_workspace(workspace.clone(), key, window, cx);
        workspace
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_b.entity_id(),
            "registered workspace should become active"
        );
    });

    let initial_key = project_b.read_with(cx, |p, cx| p.project_group_key(cx));
    multi_workspace.read_with(cx, |mw, _cx| {
        let keys = mw.project_group_keys();
        assert!(
            keys.contains(&initial_key),
            "project groups should contain the initial key for the registered workspace"
        );
    });

    let remote_worktree = project_b.update(cx, |project, cx| {
        project.add_test_remote_worktree("/remote/project", cx)
    });
    cx.run_until_parked();

    let worktree_id = remote_worktree.read_with(cx, |wt, _| wt.id().to_proto());
    remote_worktree.update(cx, |worktree, _cx| {
        worktree
            .as_remote()
            .unwrap()
            .update_from_remote(proto::UpdateWorktree {
                project_id: 0,
                worktree_id,
                abs_path: "/remote/project".to_string(),
                root_name: "project".to_string(),
                updated_entries: vec![proto::Entry {
                    id: 1,
                    is_dir: true,
                    path: "".to_string(),
                    inode: 1,
                    mtime: Some(proto::Timestamp {
                        seconds: 0,
                        nanos: 0,
                    }),
                    is_ignored: false,
                    is_hidden: false,
                    is_external: false,
                    is_fifo: false,
                    size: None,
                    canonical_path: None,
                }],
                removed_entries: vec![],
                scan_id: 1,
                is_last_update: true,
                updated_repositories: vec![],
                removed_repositories: vec![],
                root_repo_common_dir: None,
            });
    });
    cx.run_until_parked();

    let updated_key = project_b.read_with(cx, |p, cx| p.project_group_key(cx));
    assert_ne!(
        initial_key, updated_key,
        "remote worktree update should change the project group key"
    );

    multi_workspace.read_with(cx, |mw, _cx| {
        let keys = mw.project_group_keys();
        assert!(
            keys.contains(&updated_key),
            "project groups should contain the updated key after remote change; got {keys:?}"
        );
        assert!(
            !keys.contains(&initial_key),
            "project groups should no longer contain the stale initial key; got {keys:?}"
        );
    });
}

#[gpui::test]
async fn test_open_project_closes_empty_workspace_but_not_non_empty_ones(cx: &mut TestAppContext) {
    init_test(cx);
    let app_state = cx.update(AppState::test);
    let fs = app_state.fs.as_fake();
    fs.insert_tree(path!("/project_a"), json!({ "file_a.txt": "" }))
        .await;
    fs.insert_tree(path!("/project_b"), json!({ "file_b.txt": "" }))
        .await;

    // Start with an empty (no-worktrees) workspace.
    let project = Project::test(app_state.fs.clone(), [], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project, window, cx));
    cx.run_until_parked();

    window
        .update(cx, |mw, _window, cx| {
            mw.test_enable_background_retention(cx)
        })
        .unwrap();
    cx.run_until_parked();

    let empty_workspace = window
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window.into(), cx);

    // Add a dirty untitled item to the empty workspace.
    let dirty_item = cx.new(|cx| TestItem::new(cx).with_dirty(true));
    empty_workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_item_to_active_pane(Box::new(dirty_item.clone()), None, true, window, cx);
    });

    // Opening a project while the lone empty workspace has unsaved
    // changes prompts the user.
    let open_task = window
        .update(cx, |mw, window, cx| {
            mw.open_project(
                vec![PathBuf::from(path!("/project_a"))],
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();

    // Cancelling keeps the empty workspace.
    assert!(cx.has_pending_prompt(),);
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(open_task.await.unwrap(), empty_workspace);
    window
        .read_with(cx, |mw, _cx| {
            assert_eq!(mw.workspaces().count(), 1);
            assert_eq!(mw.workspace(), &empty_workspace);
            assert_eq!(mw.project_group_keys(), vec![]);
        })
        .unwrap();

    // Discarding the unsaved changes closes the empty workspace
    // and opens the new project in its place.
    let open_task = window
        .update(cx, |mw, window, cx| {
            mw.open_project(
                vec![PathBuf::from(path!("/project_a"))],
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();

    assert!(cx.has_pending_prompt(),);
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();

    let workspace_a = open_task.await.unwrap();
    assert_ne!(workspace_a, empty_workspace);

    window
        .read_with(cx, |mw, _cx| {
            assert_eq!(mw.workspaces().count(), 1);
            assert_eq!(mw.workspace(), &workspace_a);
            assert_eq!(
                mw.project_group_keys(),
                vec![ProjectGroupKey::new(
                    None,
                    PathList::new(&[path!("/project_a")])
                )]
            );
        })
        .unwrap();
    assert!(
        empty_workspace.read_with(cx, |workspace, _cx| workspace.session_id().is_none()),
        "the detached empty workspace should no longer be attached to the session",
    );

    let dirty_item = cx.new(|cx| TestItem::new(cx).with_dirty(true));
    workspace_a.update_in(cx, |workspace, window, cx| {
        workspace.add_item_to_active_pane(Box::new(dirty_item.clone()), None, true, window, cx);
    });

    // Opening another project does not close the existing project or prompt.
    let workspace_b = window
        .update(cx, |mw, window, cx| {
            mw.open_project(
                vec![PathBuf::from(path!("/project_b"))],
                OpenMode::Activate,
                window,
                cx,
            )
        })
        .unwrap()
        .await
        .unwrap();
    cx.run_until_parked();

    assert!(!cx.has_pending_prompt());
    assert_ne!(workspace_b, workspace_a);
    window
        .read_with(cx, |mw, _cx| {
            assert_eq!(mw.workspaces().count(), 2);
            assert_eq!(mw.workspace(), &workspace_b);
            assert_eq!(
                mw.project_group_keys(),
                vec![
                    ProjectGroupKey::new(None, PathList::new(&[path!("/project_b")])),
                    ProjectGroupKey::new(None, PathList::new(&[path!("/project_a")]))
                ]
            );
        })
        .unwrap();
    assert!(workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_some()),);
}

#[gpui::test]
async fn test_cycle_project_reaches_workspace_added_via_open_mode_add(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    // Unit-level exercise of the `add()` + `cycle_project()` mechanism
    // directly: `add()` always retains without activating (this is what
    // deserialization uses, and what `add_or_activate()` falls back to
    // when `retain_background_projects` is `true`). The real
    // `cli_default_open_behavior: existing_window` CLI path does NOT call
    // `add()` — it goes through `workspace::open_paths` with
    // `OpenMode::Activate`, exercised end-to-end by
    // `test_open_paths_reusing_existing_window_respects_retain_background_projects_false`
    // below. What matters here: before this phase, a workspace retained via
    // `add()` (or `add_or_activate` with retention on) but never activated
    // was a dead end — no sidebar existed to cycle back to it.
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_b.clone(), window, cx));
        mw.add(workspace.clone(), &*window, cx);
        workspace
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_a.entity_id(),
            "OpenMode::Add must not change the active workspace"
        );
        assert_eq!(
            mw.workspaces().count(),
            2,
            "the added workspace must be reachable alongside the active one"
        );
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.cycle_project(true, window, cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_b.entity_id(),
            "NextProject must reach the workspace added via OpenMode::Add"
        );
    });
}

/// End-to-end regression test through the *real* CLI entry point
/// (`workspace::open_paths`), not a direct `add()`/`activate()` call. This
/// is the second `zode <dir>` scenario under the default settings
/// (`cli_default_open_behavior: existing_window`, `add_dirs_to_sidebar:
/// true`), which reuses the existing window rather than opening a new one.
///
/// A code review caught that `open_paths`'s "reuse an existing window"
/// fallback used to call `multi_workspace.open_sidebar(cx)` unconditionally,
/// which force-retained the window's active workspace via
/// `apply_open_sidebar`'s `retain_active_workspace` call regardless of
/// `retain_background_projects` — defeating the setting for exactly this
/// scenario. Every other test in this file enables retention by calling
/// `add()`/`activate()`/`activate_provisional_workspace()` directly, so none
/// of them exercised `open_paths` and none caught it.
#[gpui::test]
async fn test_open_paths_reusing_existing_window_respects_retain_background_projects_false(
    cx: &mut TestAppContext,
) {
    init_test(cx);

    let app_state = cx.update(AppState::test);
    let fs = app_state.fs.as_fake();
    fs.insert_tree(path!("/project_a"), json!({ "file_a.txt": "" }))
        .await;
    fs.insert_tree(path!("/project_b"), json!({ "file_b.txt": "" }))
        .await;

    let project_a = Project::test(app_state.fs.clone(), [path!("/project_a").as_ref()], cx).await;
    let window = cx.add_window(|window, cx| MultiWorkspace::test_new(project_a, window, cx));
    cx.run_until_parked();

    // Pin the setting explicitly rather than relying on the phase's
    // current default, so this regression test still holds once the
    // default flips to `true` at the end of Phase 3.
    window
        .update(cx, |_mw, _window, cx| {
            disable_background_project_retention(cx);
        })
        .unwrap();

    let workspace_a = window
        .read_with(cx, |mw, _cx| mw.workspace().clone())
        .unwrap();
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_some()),
        "workspace A should start attached to the session"
    );

    // No explicit `-e`/`-n` flag and no `requesting_window` — this is
    // exactly what a second `zode /project_b` invocation looks like from
    // the CLI's perspective. `open_paths` must discover the existing
    // window itself via `workspace_windows_for_location`.
    let open_result = cx
        .update(|cx| {
            open_paths(
                &[PathBuf::from(path!("/project_b"))],
                app_state,
                OpenOptions::default(),
                cx,
            )
        })
        .await
        .unwrap();
    cx.run_until_parked();

    assert_eq!(
        open_result.window, window,
        "the second invocation should reuse the existing window, not open a new one"
    );

    window
        .read_with(cx, |mw, _cx| {
            assert_eq!(
                mw.workspace().entity_id(),
                open_result.workspace.entity_id(),
                "the new directory should become the active workspace"
            );
            assert_eq!(
                mw.workspaces().count(),
                1,
                "retain_background_projects: false must keep at most one live workspace \
                 through the real open_paths CLI entry point, not just through add()/activate() \
                 called directly"
            );
        })
        .unwrap();
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_none()),
        "workspace A must be detached, not force-retained by open_paths's reuse-window path"
    );
}

/// Covers the phase's success criterion literally: three projects opened
/// into one window (the seed workspace plus two more added the way
/// `OpenMode::Add` does), `NextProject` visits all three and wraps back
/// to the first. This only holds with retention enabled — under the
/// `false` default, cycling away from a workspace that was never
/// explicitly retained detaches it by design (see
/// `test_retain_background_projects_false_detaches_on_switch`), so a
/// full round-trip that revisits the start requires
/// `retain_background_projects: true` (the state Phase 3 will make the
/// default once hibernation exists).
#[gpui::test]
async fn test_cycle_project_wraps_through_three_retained_workspaces(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "file_c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    let workspace_a = multi_workspace.update(cx, |mw, cx| {
        mw.test_enable_background_retention(cx);
        mw.workspace().clone()
    });

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_b.clone(), window, cx));
        mw.add(workspace.clone(), &*window, cx);
        workspace
    });
    let workspace_c = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_c.clone(), window, cx));
        mw.add(workspace.clone(), &*window, cx);
        workspace
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspaces().count(),
            3,
            "all three projects should be simultaneously live"
        );
    });

    let mut visited = Vec::new();
    for _ in 0..3 {
        visited.push(multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone()));
        multi_workspace.update_in(cx, |mw, window, cx| {
            mw.cycle_project(true, window, cx);
        });
    }

    assert_eq!(
        visited,
        vec![workspace_a.clone(), workspace_b, workspace_c],
        "NextProject should visit all three workspaces in retained order"
    );
    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace(),
            &workspace_a,
            "the fourth NextProject press should wrap back to the first workspace"
        );
    });
}

#[gpui::test]
async fn test_retain_background_projects_false_detaches_on_switch(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    // Pin the setting explicitly rather than relying on the phase's
    // current default, so this regression test still holds once the
    // default flips to `true` at the end of Phase 3.
    multi_workspace.update(cx, |_mw, cx| {
        disable_background_project_retention(cx);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_some()),
        "initial active workspace should start attached to the session"
    );

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b, window, cx)
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(mw.workspace().entity_id(), workspace_b.entity_id());
        assert_eq!(
            mw.workspaces().count(),
            1,
            "retain_background_projects: false must keep at most one live workspace"
        );
    });
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_none()),
        "switching away from workspace A with retention explicitly disabled should detach it"
    );
}

#[gpui::test]
async fn test_activate_provisional_workspace_honors_retain_background_projects_false(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    multi_workspace.update(cx, |_mw, cx| {
        disable_background_project_retention(cx);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_b.clone(), window, cx));
        let key = workspace.read(cx).project_group_key(cx);
        mw.activate_provisional_workspace(workspace.clone(), key, window, cx);
        workspace
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_b.entity_id(),
            "the provisional workspace should become active"
        );
        assert_eq!(
            mw.workspaces().count(),
            1,
            "retain_background_projects: false must not retain the provisional workspace's predecessor"
        );
    });
    assert!(
        workspace_a.read_with(cx, |workspace, _cx| workspace.session_id().is_none()),
        "the previous active workspace should be detached, matching activate()'s policy"
    );
}

/// `ToggleWorkspaceSidebar`/`FocusWorkspaceSidebar` are bound to real default
/// keybindings (Cmd+Alt+J / Cmd+Alt+; on macOS) that reach `open_sidebar()`
/// even though no sidebar renders anything yet (Phase 7 hasn't rebuilt it) —
/// found while auditing for other paths like the two a code review caught in
/// `open_paths()` and `add()`. `open_sidebar()` used to call
/// `apply_open_sidebar`, which unconditionally called
/// `retain_active_workspace()`, silently defeating
/// `retain_background_projects: false` for anyone who ever pressed that
/// keybinding.
#[gpui::test]
async fn test_open_sidebar_does_not_force_retain_when_retain_background_projects_false(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    let project_a = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a, window, cx));

    // Pin the setting explicitly rather than relying on the phase's
    // current default, so this regression test still holds once the
    // default flips to `true` at the end of Phase 3.
    multi_workspace.update(cx, |_mw, cx| {
        disable_background_project_retention(cx);
    });

    multi_workspace.update(cx, |mw, cx| {
        mw.open_sidebar(cx);
    });

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.sidebar_open(),
            "the sidebar-open UI flag itself should still flip"
        );
        assert_eq!(
            mw.retained_workspaces().len(),
            0,
            "opening the sidebar must not force-retain the active workspace when \
             retain_background_projects is false"
        );
    });
}

// Phase 2 (multi-project-window-switching) — `ProjectActivity` governor
// tests. Retention must be on for these: with it off, `activate()` detaches
// the outgoing workspace outright (Phase 1 behavior), which is a different
// code path than the Warm/Hibernated timer this phase adds.

#[gpui::test]
async fn test_reactivating_before_idle_threshold_never_hibernates(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |_mw, cx| {
        set_multi_project_settings(cx, true, 1000);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    // Activating B is what makes A lose focus; the returned handle to B
    // itself isn't needed for this test.
    let _workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "A should go Warm the moment B is activated"
        );
    });

    // Reactivate A well before its 1s idle timer would expire.
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.activate(workspace_a.clone(), None, window, cx);
    });
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "re-activating A should cancel its timer and return it to Active synchronously"
        );
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.has_pending_hibernate_timer(&workspace_a),
            "A's original hibernate timer must have been cancelled, not just not-yet-fired"
        );
    });

    // Advance well past what would have been A's original deadline. If the
    // cancellation were merely racing the timer instead of truly cancelling
    // it, this would flip A to Hibernated despite being Active again.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "A must never reach Hibernated once re-activated within the idle threshold"
        );
    });
}

#[gpui::test]
async fn test_hibernates_after_idle_threshold_elapses(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |_mw, cx| {
        set_multi_project_settings(cx, true, 1000);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "A should hibernate once idle past the configured threshold"
        );
    });
    project_b.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "B is the focused workspace and must stay Active throughout"
        );
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.has_pending_hibernate_timer(&workspace_a),
            "the timer should have removed its own map entry after firing"
        );
    });
}

#[gpui::test]
async fn test_hibernate_after_ms_zero_disables_hibernation(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |_mw, cx| {
        // `0` disables hibernation entirely (see `MultiProjectContent`'s
        // doc comment for why the sentinel is `0`, not `null`).
        set_multi_project_settings(cx, true, 0);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "losing focus still moves a project to Warm even with hibernation disabled"
        );
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.has_pending_hibernate_timer(&workspace_a),
            "no timer should ever be scheduled while hibernate_after_ms is 0"
        );
    });

    // However long we wait, A must never move past Warm.
    cx.executor().advance_clock(Duration::from_secs(600));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "no transition beyond Active/Warm may occur while hibernation is disabled"
        );
    });
}

#[gpui::test]
async fn test_disabling_hibernation_live_cancels_an_already_pending_timer(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |_mw, cx| {
        set_multi_project_settings(cx, true, 1000);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.has_pending_hibernate_timer(&workspace_a),
            "A should have a live pending timer before the setting changes"
        );
    });

    // Flip hibernation off *while A's timer is already pending* — this is
    // the settings-observer path (`MultiWorkspace::new`'s
    // `hibernate_settings_subscription`), distinct from the "never
    // scheduled one in the first place" case covered by
    // `test_hibernate_after_ms_zero_disables_hibernation` above.
    multi_workspace.update(cx, |_mw, cx| {
        set_multi_project_settings(cx, true, 0);
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.has_pending_hibernate_timer(&workspace_a),
            "the live settings change must cancel A's already-pending timer"
        );
    });

    // Advancing well past the original 1s deadline must not resurrect it.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "A must stay Warm — the cancelled timer must never fire late"
        );
    });
}

#[gpui::test]
async fn test_closing_workspace_with_pending_hibernate_timer_does_not_leak(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_multi_project_settings(cx, true, 1000);
        // `activate()` never retroactively retains the workspace it's
        // switching *away* from — only an explicit retain call does (see
        // `test_enable_background_retention`'s own doc comment). Without
        // this, A would still go Warm like the other tests observe, but
        // `close_workspace` below would find it was never in
        // `retained_workspaces` and report nothing removed.
        mw.retain_active_workspace(cx);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.has_pending_hibernate_timer(&workspace_a),
            "setup should leave A with a pending hibernate timer before it's closed"
        );
    });

    let closed = multi_workspace
        .update_in(cx, |mw, window, cx| {
            mw.close_workspace(&workspace_a, window, cx)
        })
        .await
        .expect("closing a background workspace with no unsaved changes should succeed");
    assert!(
        closed,
        "close_workspace should report the workspace removed"
    );
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            !mw.has_pending_hibernate_timer(&workspace_a),
            "closing the workspace must cancel (drop) its pending timer, not leak it"
        );
    });

    // Advancing the clock afterward must not panic even though the timer
    // that used to be keyed on A's EntityId is gone.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
}

/// A code review caught that `add()` — the entry point deserialization uses
/// (`open_workspace_by_id`) and that `add_or_activate()` falls through to
/// when `retain_background_projects` is `true` — never routed a
/// newly-added, non-active workspace through the hibernate governor at all.
/// `Project::local`/`Project::remote` initialize `Active` unconditionally,
/// and before this fix nothing on this path ever called
/// `schedule_hibernate`, so a workspace added here would sit at `Active`
/// forever and never become a hibernation candidate. Exercises `add()`
/// directly (not `test_add_workspace()`, which routes through `activate()`
/// and would not have caught this) while a different workspace is active.
#[gpui::test]
async fn test_add_routes_background_workspace_through_hibernate_governor(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |_mw, cx| {
        set_multi_project_settings(cx, true, 1000);
    });

    let workspace_a = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());

    let workspace_b = multi_workspace.update_in(cx, |mw, window, cx| {
        let workspace = cx.new(|cx| Workspace::test_new(project_b.clone(), window, cx));
        mw.add(workspace.clone(), &*window, cx);
        workspace
    });
    cx.run_until_parked();

    multi_workspace.read_with(cx, |mw, _cx| {
        assert_eq!(
            mw.workspace().entity_id(),
            workspace_a.entity_id(),
            "add() must not change the active workspace"
        );
    });
    project_a.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "the still-focused workspace must remain Active"
        );
    });
    project_b.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "a workspace added in the background via add() must be routed through the \
             hibernate governor, not left stuck at Active"
        );
    });
    multi_workspace.read_with(cx, |mw, _cx| {
        assert!(
            mw.has_pending_hibernate_timer(&workspace_b),
            "the newly added background workspace should have a pending hibernate timer"
        );
    });

    // And it actually hibernates once idle past the threshold, exactly like
    // a workspace that lost focus via activate() would.
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    project_b.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "the added workspace should hibernate once idle past the configured threshold"
        );
    });
}

// Phase 6 (multi-project-window-switching) — memory-pressure fuse tests.
// Every test here sets `hibernate_after_ms: 0` (idle-timer hibernation
// disabled) so the fuse, not the ordinary idle timer, is unambiguously
// what causes any `Warm -> Hibernated` transition being asserted on.

#[gpui::test]
async fn test_memory_fuse_disabled_never_triggers(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        // `0` is the disabling sentinel (see `MultiProjectContent::memory_pressure_threshold_percent`).
        set_memory_fuse_settings(cx, true, 0, 0.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();
    project_a.read_with(cx, |project, _| {
        assert_eq!(project.activity(), ProjectActivity::Warm);
    });

    // Comfortably past several poll cycles and the min-warm duration —
    // with the fuse disabled, none of it should matter.
    cx.executor().advance_clock(Duration::from_secs(150));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "memory_pressure_threshold_percent: 0 must disable the fuse entirely, \
             even under sustained severe simulated pressure"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_never_selects_active_project(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    let project_a = Project::test(fs, ["/root_a".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    // A single workspace that's never lost focus is never routed through
    // `schedule_hibernate`, so it's never `Warm` — but assert the outcome
    // (still `Active`) rather than the internal path, and cover several
    // poll cycles well past the min-warm duration.
    cx.executor().advance_clock(Duration::from_secs(150));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "FR4: the fuse must never touch the currently-focused (Active) project, \
             no matter how severe the simulated memory pressure"
        );
    });
    multi_workspace.read_with(cx, |mw, cx| {
        assert!(
            !mw.workspace()
                .read(cx)
                .notification_ids()
                .contains(&NotificationId::unique::<MemoryPressureFuseToast>()),
            "the fuse must not have triggered at all, so no toast should have shown"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_respects_min_warm_duration(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();

    // First poll (t=30s): A has been Warm for only ~30s, under the 60s
    // minimum — the fuse must leave it alone even though pressure is
    // already well under the threshold.
    cx.executor().advance_clock(Duration::from_secs(35));
    cx.run_until_parked();
    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "FR4b: a project must sit Warm at least 60s before the fuse may pick it, \
             even under pressure"
        );
    });

    // Second poll (t=60s): A has now been Warm long enough.
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "once past the 60s minimum, the fuse should pick A on the next poll"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_shows_toast_on_trigger(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs, ["/root_b".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(65));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _| {
        assert_eq!(project.activity(), ProjectActivity::Hibernated);
    });
    multi_workspace.read_with(cx, |mw, cx| {
        assert!(
            mw.workspace()
                .read(cx)
                .notification_ids()
                .contains(&NotificationId::unique::<MemoryPressureFuseToast>()),
            "FR6: the fuse must tell the user it acted, not hibernate silently"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_picks_least_recently_active_first(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "file_c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    // A becomes Warm now; B becomes Warm 10s later — deterministically
    // older `warm_since` for A.
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(10));
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_c.clone(), window, cx)
    });
    cx.run_until_parked();

    // By t ~= 75s (A warm ~75s, B warm ~65s), both are past the 60s
    // minimum and both are otherwise eligible — the fuse must pick the
    // older one (A) first, one victim per tick.
    cx.executor().advance_clock(Duration::from_secs(65));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "FR4: among equally-eligible Warm candidates, the one that's been \
             Warm the longest (least recently active) must be picked first"
        );
    });
    project_b.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "B is eligible too but must wait its turn behind the older candidate A"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_skips_project_that_would_defer_hibernation(cx: &mut TestAppContext) {
    init_test(cx);
    cx.update_global::<SettingsStore, _>(|store, cx| {
        store
            .set_user_settings(
                r#"{ "autosave": { "after_delay": { "milliseconds": 1000 } } }"#,
                cx,
            )
            .unwrap();
    });

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "file_c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;

    // A dirty buffer + autosave enabled makes A defer hibernation (Phase 3
    // FR8) — the fuse must respect the exact same barrier, not just the
    // ordinary idle timer.
    let buffer_a = project_a
        .update(cx, |project, cx| {
            project.open_local_buffer("/root_a/file_a.txt", cx)
        })
        .await
        .unwrap();
    buffer_a.update(cx, |buffer, cx| buffer.edit([(0..0, "x")], None, cx));

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_c.clone(), window, cx)
    });
    cx.run_until_parked();

    // A (blocked) is older and would otherwise be picked first; give both
    // A and B time to clear the 60s minimum and several poll cycles for
    // the fuse to reach B once it skips A.
    cx.executor().advance_clock(Duration::from_secs(95));
    cx.run_until_parked();

    project_a.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Warm,
            "FR4: a project with autosave racing a dirty buffer must never be picked \
             as a fuse victim — no shortcut through the same barrier Phase 3 defined"
        );
    });
    project_b.read_with(cx, |project, _| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "the fuse must skip the blocked candidate and pick the eligible one instead"
        );
    });
}

#[gpui::test]
async fn test_memory_fuse_hysteresis_delays_the_second_victim(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root_a", json!({ "file_a.txt": "" })).await;
    fs.insert_tree("/root_b", json!({ "file_b.txt": "" })).await;
    fs.insert_tree("/root_c", json!({ "file_c.txt": "" })).await;
    let project_a = Project::test(fs.clone(), ["/root_a".as_ref()], cx).await;
    let project_b = Project::test(fs.clone(), ["/root_b".as_ref()], cx).await;
    let project_c = Project::test(fs, ["/root_c".as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project_a.clone(), window, cx));
    multi_workspace.update(cx, |mw, cx| {
        set_memory_fuse_settings(cx, true, 0, 50.0);
        mw.set_memory_pressure_reader_for_test(Box::new(FakeMemoryPressureReader {
            available_percent: Rc::new(Cell::new(1.0)),
        }));
    });

    // A and B become Warm at the same simulated instant (no clock advance
    // between the two `test_add_workspace` calls) — both cross the 60s
    // minimum on the same poll cycle, so the outcome doesn't depend on
    // which of the two the fuse happens to pick first.
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_b.clone(), window, cx)
    });
    cx.run_until_parked();
    multi_workspace.update_in(cx, |mw, window, cx| {
        mw.test_add_workspace(project_c.clone(), window, cx)
    });
    cx.run_until_parked();

    let hibernated_count = |cx: &mut TestAppContext| {
        [&project_a, &project_b]
            .iter()
            .filter(|project| {
                project.read_with(cx, |project, _| {
                    project.activity() == ProjectActivity::Hibernated
                })
            })
            .count()
    };

    // t=60s poll: both A and B are eligible; exactly one is hibernated.
    cx.executor().advance_clock(Duration::from_secs(65));
    cx.run_until_parked();
    assert_eq!(
        hibernated_count(cx),
        1,
        "the first eligible poll must hibernate exactly one victim"
    );

    // t=90s poll: hysteresis (2 cycles = 60s since t=60) has not elapsed
    // yet — the second, still-eligible candidate must not be touched.
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    assert_eq!(
        hibernated_count(cx),
        1,
        "FR4b: hysteresis must block a second trigger within 2 poll cycles \
         of the first, even with an eligible victim still sitting there"
    );

    // t=120s poll: hysteresis has now cleared (120 - 60 = 60s >= 2 cycles)
    // — the remaining candidate is picked.
    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    assert_eq!(
        hibernated_count(cx),
        2,
        "once hysteresis clears, the fuse must pick up the remaining eligible victim"
    );
}

/// A window opened straight onto one folder has to appear in the project rail.
///
/// Nothing registers a group for that first workspace -- both writers are about
/// ADDING a project, and restore only replays an earlier session -- so the very
/// first run of a project drew an empty rail, which is not a state any test that
/// adds projects first would ever reach.
#[gpui::test]
async fn the_only_open_project_still_forms_a_group(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root_a"), json!({ "a.txt": "" }))
        .await;
    let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    multi_workspace.read_with(cx, |multi_workspace, cx| {
        let groups = multi_workspace.project_groups(cx);
        assert_eq!(
            groups.len(),
            1,
            "the window's own project must form a group without anything registering it"
        );
        assert!(
            !groups[0].key.path_list().paths().is_empty(),
            "the group must carry the project's path, or the rail has nothing to label"
        );
    });
}

/// The second run of the same project, with the sidebar left closed.
///
/// Group state comes back from disk while `retained_workspaces` starts empty --
/// the active workspace is only retained when the sidebar opens -- so the group
/// existed with nothing in it. The rail matches its active entry by workspace,
/// not by key (`sidebar::project_list`), so it listed the project the window was
/// built around and marked none of them current. Ensuring the KEY exists is not
/// enough; the workspace has to be attached to it.
#[gpui::test]
async fn a_restored_group_still_carries_the_windows_own_workspace(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/root_a"), json!({ "a.txt": "" }))
        .await;
    let project = Project::test(fs, [path!("/root_a").as_ref()], cx).await;

    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

    let active = multi_workspace.read_with(cx, |mw, _cx| mw.workspace().clone());
    let key =
        multi_workspace.read_with(cx, |mw, cx| mw.project_group_key_for_workspace(&active, cx));

    multi_workspace.update(cx, |mw, cx| {
        mw.restore_project_groups(
            vec![SerializedProjectGroupState {
                key: key.clone(),
                expanded: true,
            }],
            cx,
        );
        assert!(
            mw.retained_workspaces().is_empty(),
            "a restore with the sidebar closed retains nothing -- without that this \
             test would pass on the ordinary path instead of the restored one"
        );
    });

    multi_workspace.read_with(cx, |mw, cx| {
        let groups = mw.project_groups(cx);
        assert_eq!(
            groups.len(),
            1,
            "the restored group and the window's own project are the same group"
        );
        assert!(
            groups[0].workspaces.contains(&active),
            "the restored group must carry the active workspace, or the rail shows \
             the project without marking it current"
        );
    });
}
