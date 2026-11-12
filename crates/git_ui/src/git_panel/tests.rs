use git::{
    repository::repo_path,
    status::{StatusCode, UnmergedStatus, UnmergedStatusCode},
};
use gpui::{TestAppContext, UpdateGlobal, VisualTestContext, px};
use indoc::indoc;
use project::{FakeFs, WorktreeId};
use serde_json::json;
use settings::SettingsStore;
use theme::LoadThemes;
use util::path;
use util::rel_path::{RelPath, rel_path};

use workspace::MultiWorkspace;

use super::*;

fn init_test(cx: &mut gpui::TestAppContext) {
    zlog::init_test();

    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(LoadThemes::JustBase, cx);
        editor::init(cx);
        crate::init(cx);
    });
}

#[test]
fn test_format_git_error_toast_message_prefers_raw_rpc_message() {
    let rpc_error = RpcError::from_proto(
        &proto::Error {
            message: "Your local changes to the following files would be overwritten by merge\n"
                .to_string(),
            code: proto::ErrorCode::Internal as i32,
            tags: Default::default(),
        },
        "Pull",
    );

    let message = format_git_error_toast_message(&rpc_error);
    assert_eq!(
        message,
        "Your local changes to the following files would be overwritten by merge"
    );
}

#[test]
fn test_format_git_error_toast_message_prefers_raw_rpc_message_when_wrapped() {
    let rpc_error = RpcError::from_proto(
        &proto::Error {
            message: "Your local changes to the following files would be overwritten by merge\n"
                .to_string(),
            code: proto::ErrorCode::Internal as i32,
            tags: Default::default(),
        },
        "Pull",
    );
    let wrapped = rpc_error.context("sending pull request");

    let message = format_git_error_toast_message(&wrapped);
    assert_eq!(
        message,
        "Your local changes to the following files would be overwritten by merge"
    );
}

#[gpui::test]
async fn test_entry_worktree_paths(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/root",
        json!({
            "zed": {
                ".git": {},
                "crates": {
                    "gpui": {
                        "gpui.rs": "fn main() {}"
                    },
                    "util": {
                        "util.rs": "fn do_it() {}"
                    }
                }
            },
        }),
    )
    .await;

    fs.set_status_for_repo(
        Path::new(path!("/root/zed/.git")),
        &[
            ("crates/gpui/gpui.rs", StatusCode::Modified.worktree()),
            ("crates/util/util.rs", StatusCode::Modified.worktree()),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/root/zed/crates/gpui").as_ref()], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    pretty_assertions::assert_eq!(
        entries,
        [
            GitListEntry::Header(GitHeaderEntry {
                header: Section::Tracked
            }),
            GitListEntry::Status(GitStatusEntry {
                repo_path: repo_path("crates/gpui/gpui.rs"),
                status: StatusCode::Modified.worktree(),
                staging: StageStatus::Unstaged,
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 1,
                }),
            }),
            GitListEntry::Status(GitStatusEntry {
                repo_path: repo_path("crates/util/util.rs"),
                status: StatusCode::Modified.worktree(),
                staging: StageStatus::Unstaged,
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 1,
                }),
            },),
        ],
    );

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;
    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    pretty_assertions::assert_eq!(
        entries,
        [
            GitListEntry::Header(GitHeaderEntry {
                header: Section::Tracked
            }),
            GitListEntry::Status(GitStatusEntry {
                repo_path: repo_path("crates/gpui/gpui.rs"),
                status: StatusCode::Modified.worktree(),
                staging: StageStatus::Unstaged,
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 1,
                }),
            }),
            GitListEntry::Status(GitStatusEntry {
                repo_path: repo_path("crates/util/util.rs"),
                status: StatusCode::Modified.worktree(),
                staging: StageStatus::Unstaged,
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 1,
                }),
            },),
        ],
    );
}

#[gpui::test]
async fn test_bulk_staging(cx: &mut TestAppContext) {
    use GitListEntry::*;

    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/root",
        json!({
            "project": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}",
                    "lib.rs": "pub fn hello() {}",
                    "utils.rs": "pub fn util() {}"
                },
                "tests": {
                    "test.rs": "fn test() {}"
                },
                "new_file.txt": "new content",
                "another_new.rs": "// new file",
                "conflict.txt": "conflicted content"
            }
        }),
    )
    .await;

    fs.set_status_for_repo(
        Path::new(path!("/root/project/.git")),
        &[
            ("src/main.rs", StatusCode::Modified.worktree()),
            ("src/lib.rs", StatusCode::Modified.worktree()),
            ("tests/test.rs", StatusCode::Modified.worktree()),
            ("new_file.txt", FileStatus::Untracked),
            ("another_new.rs", FileStatus::Untracked),
            ("src/utils.rs", FileStatus::Untracked),
            (
                "conflict.txt",
                UnmergedStatus {
                    first_head: UnmergedStatusCode::Updated,
                    second_head: UnmergedStatusCode::Updated,
                }
                .into(),
            ),
        ],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/root/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Header(GitHeaderEntry { header: Section::Conflict }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::Tracked }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::New }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
        ],
    );

    let second_status_entry = entries[3].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&second_status_entry, window, cx);
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(7);
        panel.stage_range(&git::StageRange, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Header(GitHeaderEntry { header: Section::Conflict }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::Tracked }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Header(GitHeaderEntry { header: Section::New }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
        ],
    );

    let third_status_entry = entries[4].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&third_status_entry, window, cx);
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(9);
        panel.stage_range(&git::StageRange, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Header(GitHeaderEntry { header: Section::Conflict }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::Tracked }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Header(GitHeaderEntry { header: Section::New }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Staged, .. }),
        ],
    );
}

#[gpui::test]
async fn test_bulk_staging_with_sort_by_paths(cx: &mut TestAppContext) {
    use GitListEntry::*;

    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/root",
        json!({
            "project": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}",
                    "lib.rs": "pub fn hello() {}",
                    "utils.rs": "pub fn util() {}"
                },
                "tests": {
                    "test.rs": "fn test() {}"
                },
                "new_file.txt": "new content",
                "another_new.rs": "// new file",
                "conflict.txt": "conflicted content"
            }
        }),
    )
    .await;

    fs.set_status_for_repo(
        Path::new(path!("/root/project/.git")),
        &[
            ("src/main.rs", StatusCode::Modified.worktree()),
            ("src/lib.rs", StatusCode::Modified.worktree()),
            ("tests/test.rs", StatusCode::Modified.worktree()),
            ("new_file.txt", FileStatus::Untracked),
            ("another_new.rs", FileStatus::Untracked),
            ("src/utils.rs", FileStatus::Untracked),
            (
                "conflict.txt",
                UnmergedStatus {
                    first_head: UnmergedStatusCode::Updated,
                    second_head: UnmergedStatusCode::Updated,
                }
                .into(),
            ),
        ],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/root/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Header(GitHeaderEntry { header: Section::Conflict }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::Tracked }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Header(GitHeaderEntry { header: Section::New }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { staging: StageStatus::Unstaged, .. }),
        ],
    );

    assert_entry_paths(
        &entries,
        &[
            None,
            Some("conflict.txt"),
            None,
            Some("src/lib.rs"),
            Some("src/main.rs"),
            Some("tests/test.rs"),
            None,
            Some("another_new.rs"),
            Some("new_file.txt"),
            Some("src/utils.rs"),
        ],
    );

    let second_status_entry = entries[3].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&second_status_entry, window, cx);
    });

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.git_panel.get_or_insert_default().sort_by_path = Some(true);
            })
        });
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(7);
        panel.stage_range(&git::StageRange, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Unmerged(..), staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Unstaged, .. }),
        ],
    );

    assert_entry_paths(
        &entries,
        &[
            Some("another_new.rs"),
            Some("conflict.txt"),
            Some("new_file.txt"),
            Some("src/lib.rs"),
            Some("src/main.rs"),
            Some("src/utils.rs"),
            Some("tests/test.rs"),
        ],
    );

    let third_status_entry = entries[4].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&third_status_entry, window, cx);
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(9);
        panel.stage_range(&git::StageRange, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());
    #[rustfmt::skip]
    pretty_assertions::assert_matches!(
        entries.as_slice(),
        &[
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Unmerged(..), staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Staged, .. }),
            Status(GitStatusEntry { status: FileStatus::Untracked, staging: StageStatus::Unstaged, .. }),
            Status(GitStatusEntry { status: FileStatus::Tracked(..), staging: StageStatus::Unstaged, .. }),
        ],
    );

    assert_entry_paths(
        &entries,
        &[
            Some("another_new.rs"),
            Some("conflict.txt"),
            Some("new_file.txt"),
            Some("src/lib.rs"),
            Some("src/main.rs"),
            Some("src/utils.rs"),
            Some("tests/test.rs"),
        ],
    );
}

#[gpui::test]
async fn test_amend_commit_message_handling(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/root",
        json!({
            "project": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}"
                }
            }
        }),
    )
    .await;

    fs.set_status_for_repo(
        Path::new(path!("/root/project/.git")),
        &[("src/main.rs", StatusCode::Modified.worktree())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/root/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    let panel = workspace.update_in(cx, GitPanel::new);

    // Test: User has commit message, enables amend (saves message), then disables (restores message)
    panel.update(cx, |panel, cx| {
        panel.commit_message_buffer(cx).update(cx, |buffer, cx| {
            let start = buffer.anchor_before(0);
            let end = buffer.anchor_after(buffer.len());
            buffer.edit([(start..end, "Initial commit message")], None, cx);
        });

        panel.set_amend_pending(true, cx);
        assert!(panel.original_commit_message.is_some());

        panel.set_amend_pending(false, cx);
        let current_message = panel.commit_message_buffer(cx).read(cx).text();
        assert_eq!(current_message, "Initial commit message");
        assert!(panel.original_commit_message.is_none());
    });

    // Test: User has empty commit message, enables amend, then disables (clears message)
    panel.update(cx, |panel, cx| {
        panel.commit_message_buffer(cx).update(cx, |buffer, cx| {
            let start = buffer.anchor_before(0);
            let end = buffer.anchor_after(buffer.len());
            buffer.edit([(start..end, "")], None, cx);
        });

        panel.set_amend_pending(true, cx);
        assert!(panel.original_commit_message.is_none());

        panel.commit_message_buffer(cx).update(cx, |buffer, cx| {
            let start = buffer.anchor_before(0);
            let end = buffer.anchor_after(buffer.len());
            buffer.edit([(start..end, "Previous commit message")], None, cx);
        });

        panel.set_amend_pending(false, cx);
        let current_message = panel.commit_message_buffer(cx).read(cx).text();
        assert_eq!(current_message, "");
    });
}

#[gpui::test]
async fn test_amend(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/root",
        json!({
            "project": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}"
                }
            }
        }),
    )
    .await;

    fs.set_status_for_repo(
        Path::new(path!("/root/project/.git")),
        &[("src/main.rs", StatusCode::Modified.worktree())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/root/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    // Wait for the project scanning to finish so that `head_commit(cx)` is
    // actually set, otherwise no head commit would be available from which
    // to fetch the latest commit message from.
    cx.executor().run_until_parked();

    let panel = workspace.update_in(cx, GitPanel::new);
    panel.read_with(cx, |panel, cx| {
        assert!(panel.active_repository.is_some());
        assert!(panel.head_commit(cx).is_some());
    });

    panel.update_in(cx, |panel, window, cx| {
        // Update the commit editor's message to ensure that its contents
        // are later restored, after amending is finished.
        panel.commit_message_buffer(cx).update(cx, |buffer, cx| {
            buffer.set_text("refactor: update main.rs", cx);
        });

        // Start amending the previous commit.
        panel.focus_editor(&Default::default(), window, cx);
        panel.on_amend(&Amend, window, cx);
    });

    // Since `GitPanel.amend` attempts to fetch the latest commit message in
    // a background task, we need to wait for it to complete before being
    // able to assert that the commit message editor's state has been
    // updated.
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        assert_eq!(
            panel.commit_message_buffer(cx).read(cx).text(),
            "initial commit"
        );
        assert_eq!(
            panel.original_commit_message,
            Some("refactor: update main.rs".to_string())
        );

        // Finish amending the previous commit.
        panel.focus_editor(&Default::default(), window, cx);
        panel.on_amend(&Amend, window, cx);
    });

    // Since the actual commit logic is run in a background task, we need to
    // await its completion to actually ensure that the commit message
    // editor's contents are set to the original message and haven't been
    // cleared.
    cx.run_until_parked();

    panel.update_in(cx, |panel, _window, cx| {
        // After amending, the commit editor's message should be restored to
        // the original message.
        assert_eq!(
            panel.commit_message_buffer(cx).read(cx).text(),
            "refactor: update main.rs"
        );
        assert!(panel.original_commit_message.is_none());
    });
}

#[gpui::test]
async fn test_open_diff(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
            "untracked": "\n",
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    // Enable the `sort_by_path` setting and wait for entries to be updated,
    // as there should no longer be separators between Tracked and Untracked
    // files.
    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.git_panel.get_or_insert_default().sort_by_path = Some(true);
            })
        });
    });

    cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    })
    .await;

    // Confirm that `Open Diff` still works for the untracked file, updating
    // the Project Diff's active path.
    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(1);
        panel.open_diff(&menu::Confirm, window, cx);
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, _window, cx| {
        let active_path = workspace
            .item_of_type::<ProjectDiff>(cx)
            .expect("ProjectDiff should exist")
            .read(cx)
            .active_path(cx)
            .expect("active_path should exist");

        assert_eq!(active_path.path, rel_path("untracked").into_arc());
    });
}

#[gpui::test]
async fn test_tree_view_reveals_collapsed_parent_on_select_entry_by_path(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "src": {
                "a": {
                    "foo.rs": "fn foo() {}",
                },
                "b": {
                    "bar.rs": "fn bar() {}",
                },
            },
        }),
    )
    .await;

    fs.set_status_for_repo(
        path!("/project/.git").as_ref(),
        &[
            ("src/a/foo.rs", StatusCode::Modified.worktree()),
            ("src/b/bar.rs", StatusCode::Modified.worktree()),
        ],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.git_panel.get_or_insert_default().tree_view = Some(true);
            })
        });
    });

    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let src_key = panel.read_with(cx, |panel, _| {
        panel
            .entries
            .iter()
            .find_map(|entry| match entry {
                GitListEntry::Directory(dir) if dir.key.path == repo_path("src") => {
                    Some(dir.key.clone())
                }
                _ => None,
            })
            .expect("src directory should exist in tree view")
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_directory(&src_key, window, cx);
    });

    panel.read_with(cx, |panel, _| {
        let state = panel
            .view_mode
            .tree_state()
            .expect("tree view state should exist");
        assert_eq!(state.expanded_dirs.get(&src_key).copied(), Some(false));
    });

    let worktree_id = cx.read(|cx| project.read(cx).worktrees(cx).next().unwrap().read(cx).id());
    let project_path = ProjectPath {
        worktree_id,
        path: RelPath::unix("src/a/foo.rs").unwrap().into_arc(),
    };

    panel.update_in(cx, |panel, window, cx| {
        panel.select_entry_by_path(project_path, window, cx);
    });

    panel.read_with(cx, |panel, _| {
        let state = panel
            .view_mode
            .tree_state()
            .expect("tree view state should exist");
        assert_eq!(state.expanded_dirs.get(&src_key).copied(), Some(true));

        let selected_ix = panel.selected_entry.expect("selection should be set");
        assert!(state.logical_indices.contains(&selected_ix));

        let selected_entry = panel
            .entries
            .get(selected_ix)
            .and_then(|entry| entry.status_entry())
            .expect("selected entry should be a status entry");
        assert_eq!(selected_entry.repo_path, repo_path("src/a/foo.rs"));
    });
}

#[gpui::test]
async fn test_tree_view_select_next_at_last_visible_collapsed_directory(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "bar": {
                "bar1.py": "print('bar1')",
                "bar2.py": "print('bar2')",
            },
            "foo": {
                "foo1.py": "print('foo1')",
                "foo2.py": "print('foo2')",
            },
            "foobar.py": "print('foobar')",
        }),
    )
    .await;

    fs.set_status_for_repo(
        path!("/project/.git").as_ref(),
        &[
            ("bar/bar1.py", StatusCode::Modified.worktree()),
            ("bar/bar2.py", StatusCode::Modified.worktree()),
            ("foo/foo1.py", StatusCode::Modified.worktree()),
            ("foo/foo2.py", StatusCode::Modified.worktree()),
            ("foobar.py", FileStatus::Untracked),
        ],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();
    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.git_panel.get_or_insert_default().tree_view = Some(true);
            })
        });
    });

    let panel = workspace.update_in(cx, GitPanel::new);
    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });

    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let foo_key = panel.read_with(cx, |panel, _| {
        panel
            .entries
            .iter()
            .find_map(|entry| match entry {
                GitListEntry::Directory(dir) if dir.key.path == repo_path("foo") => {
                    Some(dir.key.clone())
                }
                _ => None,
            })
            .expect("foo directory should exist in tree view")
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_directory(&foo_key, window, cx);
    });

    let foo_idx = panel.read_with(cx, |panel, _| {
        let state = panel
            .view_mode
            .tree_state()
            .expect("tree view state should exist");
        assert_eq!(state.expanded_dirs.get(&foo_key).copied(), Some(false));

        let foo_idx = panel
            .entries
            .iter()
            .enumerate()
            .find_map(|(index, entry)| match entry {
                GitListEntry::Directory(dir) if dir.key.path == repo_path("foo") => Some(index),
                _ => None,
            })
            .expect("foo directory should exist in tree view");

        let foo_logical_idx = state
            .logical_indices
            .iter()
            .position(|&index| index == foo_idx)
            .expect("foo directory should be visible");
        let next_logical_idx = state.logical_indices[foo_logical_idx + 1];
        assert!(matches!(
            panel.entries.get(next_logical_idx),
            Some(GitListEntry::Header(GitHeaderEntry {
                header: Section::New
            }))
        ));

        foo_idx
    });

    panel.update_in(cx, |panel, window, cx| {
        panel.selected_entry = Some(foo_idx);
        panel.select_next(&menu::SelectNext, window, cx);
    });

    panel.read_with(cx, |panel, _| {
        let selected_idx = panel.selected_entry.expect("selection should be set");
        let selected_entry = panel
            .entries
            .get(selected_idx)
            .and_then(|entry| entry.status_entry())
            .expect("selected entry should be a status entry");
        assert_eq!(selected_entry.repo_path, repo_path("foobar.py"));
    });
}

fn assert_entry_paths(entries: &[GitListEntry], expected_paths: &[Option<&str>]) {
    assert_eq!(entries.len(), expected_paths.len());
    for (entry, expected_path) in entries.iter().zip(expected_paths) {
        assert_eq!(
            entry.status_entry().map(|status| status
                .repo_path
                .as_ref()
                .as_std_path()
                .to_string_lossy()
                .to_string()),
            expected_path.map(|s| s.to_string())
        );
    }
}

#[test]
fn test_compress_diff_no_truncation() {
    let diff = indoc! {"
        --- a/file.txt
        +++ b/file.txt
        @@ -1,2 +1,2 @@
        -old
        +new
    "};
    let result = GitPanel::compress_commit_diff(diff, 1000);
    assert_eq!(result, diff);
}

#[test]
fn test_compress_diff_truncate_long_lines() {
    let long_line = "🦀".repeat(300);
    let diff = indoc::formatdoc! {"
        --- a/file.txt
        +++ b/file.txt
        @@ -1,2 +1,3 @@
         context
        +{}
         more context
    ", long_line};
    let result = GitPanel::compress_commit_diff(&diff, 100);
    assert!(result.contains("...[truncated]"));
    assert!(result.len() < diff.len());
}

#[test]
fn test_compress_diff_truncate_hunks() {
    let diff = indoc! {"
        --- a/file.txt
        +++ b/file.txt
        @@ -1,2 +1,2 @@
         context
        -old1
        +new1
        @@ -5,2 +5,2 @@
         context 2
        -old2
        +new2
        @@ -10,2 +10,2 @@
         context 3
        -old3
        +new3
    "};
    let result = GitPanel::compress_commit_diff(diff, 100);
    let expected = indoc! {"
        --- a/file.txt
        +++ b/file.txt
        @@ -1,2 +1,2 @@
         context
        -old1
        +new1
        [...skipped 2 hunks...]
    "};
    assert_eq!(result, expected);
}

#[gpui::test]
async fn test_suggest_commit_message(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
            "untracked": "\n",
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let entries = panel.read_with(cx, |panel, _| panel.entries.clone());

    // GitPanel
    // - Tracked:
    // - [] tracked
    // - Untracked
    // - [] untracked
    //
    // The commit message should now read:
    // "Update tracked"
    let message = panel.update(cx, |panel, cx| panel.suggest_commit_message(cx));
    assert_eq!(message, Some("Update tracked".to_string()));

    let first_status_entry = entries[1].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&first_status_entry, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    // GitPanel
    // - Tracked:
    // - [x] tracked
    // - Untracked
    // - [] untracked
    //
    // The commit message should still read:
    // "Update tracked"
    let message = panel.update(cx, |panel, cx| panel.suggest_commit_message(cx));
    assert_eq!(message, Some("Update tracked".to_string()));

    let second_status_entry = entries[3].clone();
    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&second_status_entry, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    // GitPanel
    // - Tracked:
    // - [x] tracked
    // - Untracked
    // - [x] untracked
    //
    // The commit message should now read:
    // "Enter commit message"
    // (which means we should see None returned).
    let message = panel.update(cx, |panel, cx| panel.suggest_commit_message(cx));
    assert!(message.is_none());

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&first_status_entry, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    // GitPanel
    // - Tracked:
    // - [] tracked
    // - Untracked
    // - [x] untracked
    //
    // The commit message should now read:
    // "Update untracked"
    let message = panel.update(cx, |panel, cx| panel.suggest_commit_message(cx));
    assert_eq!(message, Some("Create untracked".to_string()));

    panel.update_in(cx, |panel, window, cx| {
        panel.toggle_staged_for_entry(&second_status_entry, window, cx);
    });

    cx.read(|cx| {
        project
            .read(cx)
            .worktrees(cx)
            .next()
            .unwrap()
            .read(cx)
            .as_local()
            .unwrap()
            .scan_complete()
    })
    .await;

    cx.executor().run_until_parked();

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    // GitPanel
    // - Tracked:
    // - [] tracked
    // - Untracked
    // - [] untracked
    //
    // The commit message should now read:
    // "Update tracked"
    let message = panel.update(cx, |panel, cx| panel.suggest_commit_message(cx));
    assert_eq!(message, Some("Update tracked".to_string()));
}

#[test]
fn test_git_output_handler_strips_ansi_codes() {
    use alacritty_terminal::vte::ansi;

    let cases = [
        ("no escape codes here\n", "no escape codes here\n"),
        ("\x1b[31mhello\x1b[0m", "hello"),
        ("\x1b[1;32mfoo\x1b[0m bar", "foo bar"),
        ("progress 10%\rprogress 100%\n", "progress 100%\n"),
    ];

    for (input, expected) in cases {
        let mut handler = GitOutputHandler::default();
        let mut processor = ansi::Processor::<ansi::StdSyncHandler>::default();
        processor.advance(&mut handler, input.as_bytes());
        assert_eq!(handler.output, expected);
    }
}

#[gpui::test]
async fn test_dispatch_context_with_focus_states(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    // Case 1: Focus the commit editor — should have "CommitEditor" but NOT "menu"/"ChangesList"
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_editor(&FocusEditor, window, cx);
        let editor_is_focused = panel.commit_editor.read(cx).is_focused(window);
        assert!(
            editor_is_focused,
            "commit editor should be focused after focus_editor action"
        );
        let context = panel.dispatch_context(window, cx);
        assert!(
            context.contains("GitPanel"),
            "should always have GitPanel context"
        );
        assert!(
            context.contains("CommitEditor"),
            "should have CommitEditor context when commit editor is focused"
        );
        assert!(
            !context.contains("menu"),
            "should not have menu context when commit editor is focused"
        );
        assert!(
            !context.contains("ChangesList"),
            "should not have ChangesList context when commit editor is focused"
        );
    });

    // Case 2: Focus the panel's focus handle directly — should have "menu" and "ChangesList".
    // We force a draw via simulate_resize to ensure the dispatch tree is populated,
    // since contains_focused() depends on the rendered dispatch tree.
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_handle.focus(window, cx);
    });
    cx.simulate_resize(gpui::size(px(800.), px(600.)));

    panel.update_in(cx, |panel, window, cx| {
        let context = panel.dispatch_context(window, cx);
        assert!(
            context.contains("GitPanel"),
            "should always have GitPanel context"
        );
        assert!(
            context.contains("menu"),
            "should have menu context when changes list is focused"
        );
        assert!(
            context.contains("ChangesList"),
            "should have ChangesList context when changes list is focused"
        );
        assert!(
            !context.contains("CommitEditor"),
            "should not have CommitEditor context when changes list is focused"
        );
    });

    // Case 3: Switch back to commit editor and verify context switches correctly
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_editor(&FocusEditor, window, cx);
    });

    panel.update_in(cx, |panel, window, cx| {
        let context = panel.dispatch_context(window, cx);
        assert!(
            context.contains("CommitEditor"),
            "should have CommitEditor after switching focus back to editor"
        );
        assert!(
            !context.contains("menu"),
            "should not have menu after switching focus back to editor"
        );
    });

    // Case 4: Re-focus changes list and verify it transitions back correctly
    panel.update_in(cx, |panel, window, cx| {
        panel.focus_handle.focus(window, cx);
    });
    cx.simulate_resize(gpui::size(px(800.), px(600.)));

    panel.update_in(cx, |panel, window, cx| {
        assert!(
            panel.focus_handle.contains_focused(window, cx),
            "panel focus handle should report contains_focused when directly focused"
        );
        let context = panel.dispatch_context(window, cx);
        assert!(
            context.contains("menu"),
            "should have menu context after re-focusing changes list"
        );
        assert!(
            context.contains("ChangesList"),
            "should have ChangesList context after re-focusing changes list"
        );
    });
}

#[test]
fn test_section_collapse_state_defaults() {
    let state = SectionCollapseState::default();

    assert!(state.is_expanded(PanelSectionKind::Repositories));
    assert!(state.is_expanded(PanelSectionKind::Changes));
    assert!(
        !state.is_expanded(PanelSectionKind::Graph),
        "graph must start collapsed so opening the panel never pays for a log fetch"
    );
    assert!(
        !state.is_expanded(PanelSectionKind::Commits),
        "commits shares that fetch, so it starts collapsed for the same reason"
    );
}

#[test]
fn test_deserialize_panel_written_before_collapsible_sections() {
    let serialized = serde_json::from_str::<SerializedGitPanel>(
        r#"{"amend_pending":false,"signoff_enabled":true}"#,
    )
    .expect("a blob written before collapsible sections must still deserialize");

    assert!(serialized.signoff_enabled);
    assert_eq!(serialized.section_collapse, None);
}

#[test]
fn test_deserialize_section_collapse_state_with_missing_fields() {
    let state = serde_json::from_str::<SectionCollapseState>(r#"{"changes":true}"#)
        .expect("a partially written collapse state must fall back to the defaults");

    assert!(!state.is_expanded(PanelSectionKind::Changes));
    assert!(!state.is_expanded(PanelSectionKind::Graph));
    assert!(!state.is_expanded(PanelSectionKind::Commits));
    assert!(state.is_expanded(PanelSectionKind::Repositories));
}

#[gpui::test]
async fn test_section_collapse_state_survives_serialization(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let serialization_key = workspace
        .read_with(cx, |workspace, _| GitPanel::serialization_key(workspace))
        .expect("test workspaces carry a session id, so a serialization key must exist");

    panel.update_in(cx, |panel, _, cx| {
        assert!(panel.section_expanded(PanelSectionKind::Changes));
        panel.toggle_section(PanelSectionKind::Changes, cx);
        assert!(!panel.section_expanded(PanelSectionKind::Changes));
    });

    cx.executor().advance_clock(2 * SERIALIZATION_THROTTLE_TIME);
    cx.run_until_parked();

    let written = cx
        .update(|_, cx| KeyValueStore::global(cx).read_kvp(&serialization_key))
        .expect("reading the serialized panel back must not fail")
        .expect("toggling a section must write the panel state");
    let serialized = serde_json::from_str::<SerializedGitPanel>(&written)
        .expect("the written blob must deserialize");

    let restored = workspace.update_in(cx, GitPanel::new);
    restored.update_in(cx, |restored, _, cx| {
        restored.apply_serialized_state(serialized, cx);
        assert!(
            !restored.section_expanded(PanelSectionKind::Changes),
            "a collapsed Changes section must come back collapsed"
        );
        assert!(
            !restored.section_expanded(PanelSectionKind::Graph),
            "graph must stay collapsed across a round trip"
        );
        assert!(!restored.section_expanded(PanelSectionKind::Commits));
    });
}

/// Draws the whole panel in both fold states. A real draw is what catches element-id
/// collisions — the overflow menu is now built three times per frame — and layout panics
/// from nesting the entry list inside a section.
#[gpui::test]
async fn test_panel_draws_with_changes_section_collapsed_and_expanded(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
            "untracked": "untracked\n",
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    panel.update_in(cx, |panel, _, _| {
        assert!(
            !panel.entries.is_empty(),
            "the entry list path must be the one under test"
        );
    });

    let space = gpui::size(px(360.), px(800.));
    cx.draw(gpui::point(px(0.), px(0.)), space, |_, _| {
        panel.clone().into_any_element()
    });

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Changes, cx);
    });

    cx.draw(gpui::point(px(0.), px(0.)), space, |_, _| {
        panel.clone().into_any_element()
    });
}

/// The title row's `⛶` and `✕` go through the dock: the panel emits `PanelEvent::ZoomIn` /
/// `ZoomOut` / `Close` and `Dock` calls back into `set_zoomed` or closes itself. Driving the
/// real dock is the only way to prove that round trip.
#[gpui::test]
async fn test_title_row_zoom_and_close(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/project"), json!({ ".git": {} }))
        .await;

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let dock_position = workspace.update_in(cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.open_panel::<GitPanel>(window, cx);
        panel.read(cx).position(window, cx)
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, _, cx| {
        assert!(
            workspace.is_dock_at_position_open(dock_position, cx),
            "the panel's dock must be open before we test closing it"
        );
    });

    panel.update_in(cx, |panel, window, cx| {
        assert!(!panel.is_zoomed(window, cx));
        panel.toggle_zoom(&ToggleZoom, window, cx);
    });
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        assert!(
            panel.is_zoomed(window, cx),
            "the dock must zoom the panel in response to ZoomIn"
        );
        panel.toggle_zoom(&ToggleZoom, window, cx);
    });
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        assert!(
            !panel.is_zoomed(window, cx),
            "toggling zoom a second time must restore the panel"
        );
        panel.close_panel(&Close, window, cx);
    });
    cx.run_until_parked();

    workspace.update_in(cx, |workspace, _, cx| {
        assert!(
            !workspace.is_dock_at_position_open(dock_position, cx),
            "closing the panel must close its dock"
        );
    });
}

#[gpui::test]
async fn test_commit_placeholder_names_the_target_branch(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );
    fs.set_branch_name(path!("/project/.git").as_ref(), Some("feature/parity"));

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        let placeholder = panel.commit_placeholder_text(window, cx);
        assert!(
            placeholder.contains("feature/parity"),
            "the placeholder must name the branch the commit lands on, got {placeholder:?}"
        );
        assert!(placeholder.starts_with("Message ("), "got {placeholder:?}");
    });
}

#[gpui::test]
async fn test_commit_placeholder_falls_back_to_settings_branch_name(cx: &mut TestAppContext) {
    init_test(cx);

    // No `.git` anywhere: no repository, so no branch to read.
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/project"), json!({ "tracked": "tracked\n" }))
        .await;

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        let fallback = GitPanelSettings::get_global(cx)
            .fallback_branch_name
            .clone();
        let placeholder = panel.commit_placeholder_text(window, cx);
        assert!(
            placeholder.contains(&fallback),
            "with no branch resolved the placeholder must name the settings fallback \
             {fallback:?}, got {placeholder:?}"
        );
    });
}

/// The commit box moved above the file list, so it now draws in every panel state: with
/// entries, with none, and with a message taller than the editor's line cap.
#[gpui::test]
async fn test_panel_draws_commit_box_above_the_list(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked": "tracked\n",
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);

    let handle = cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;

    let space = gpui::size(px(360.), px(800.));
    let draw = |cx: &mut VisualTestContext, panel: &Entity<GitPanel>| {
        let panel = panel.clone();
        cx.draw(gpui::point(px(0.), px(0.)), space, |_, _| {
            panel.into_any_element()
        });
    };

    panel.update_in(cx, |panel, _, _| {
        assert!(!panel.entries.is_empty(), "expected the entry-list path");
    });
    draw(cx, &panel);

    // A message well past MAX_PANEL_EDITOR_LINES must not blow the layout out of the panel.
    panel.update_in(cx, |panel, _, cx| {
        panel.commit_message_buffer(cx).update(cx, |buffer, cx| {
            buffer.set_text("line\n".repeat(12).as_str(), cx);
        });
    });
    cx.run_until_parked();
    draw(cx, &panel);

    // And with nothing to commit, the commit box is still there with the thin empty-state
    // line beneath it.
    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "tracked\n".into())],
    );
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();
    panel.update_in(cx, |panel, _, _| {
        assert!(
            panel.entries.is_empty(),
            "expected the empty-state path once the working tree is clean"
        );
    });
    draw(cx, &panel);
}

#[gpui::test]
async fn test_uncommit_and_branch_diff_menu_gating(cx: &mut TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({ ".git": {}, "tracked": "tracked\n" }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );
    fs.set_branch_name(path!("/project/.git").as_ref(), Some("main"));

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.run_until_parked();

    panel.update_in(cx, |panel, _, cx| {
        assert!(
            !panel.show_branch_diff(cx),
            "Branch Diff must stay hidden while HEAD is on main"
        );
    });

    fs.set_branch_name(path!("/project/.git").as_ref(), Some("feature/parity"));
    cx.run_until_parked();

    panel.update_in(cx, |panel, _, cx| {
        assert!(
            panel.show_branch_diff(cx),
            "Branch Diff must be offered once HEAD leaves main"
        );
    });
}

/// Two repositories whose alphabetical order is the reverse of the order they are added in, so
/// the assertion fails if the sort is ever dropped and iteration order leaks through.
async fn init_two_repo_panel(
    cx: &mut TestAppContext,
) -> (Entity<GitPanel>, Entity<Workspace>, VisualTestContext) {
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/z-repo"),
        json!({ ".git": {}, "tracked": "tracked\n" }),
    )
    .await;
    fs.insert_tree(
        path!("/a-repo"),
        json!({ ".git": {}, "tracked": "tracked\n" }),
    )
    .await;

    let project = Project::test(
        fs.clone(),
        [Path::new(path!("/z-repo")), Path::new(path!("/a-repo"))],
        cx,
    )
    .await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let mut visual_cx = VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(&mut visual_cx, GitPanel::new);
    visual_cx.run_until_parked();

    (panel, workspace, visual_cx)
}

fn repo_names(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.update_in(cx, |panel, _, cx| {
        let repositories = panel.project.read(cx).git_store().read(cx).repositories();
        panel
            .sorted_repo_ids
            .iter()
            .filter_map(|id| Some(repositories.get(id)?.read(cx).display_name().to_string()))
            .collect()
    })
}

#[gpui::test]
async fn test_repository_rows_are_ordered_by_name_not_hash_order(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _workspace, mut cx) = init_two_repo_panel(cx).await;
    let cx = &mut cx;

    let names = repo_names(&panel, cx);
    assert_eq!(
        names,
        vec!["a-repo".to_string(), "z-repo".to_string()],
        "rows must be ordered by the name they display, regardless of the order the \
         repositories were registered in"
    );

    // Rebuilding must be idempotent — the order is a function of the repositories alone.
    panel.update_in(cx, |panel, _, cx| panel.update_sorted_repo_ids(cx));
    assert_eq!(repo_names(&panel, cx), names);
}

#[gpui::test]
async fn test_switching_active_repository_keeps_row_order(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _workspace, mut cx) = init_two_repo_panel(cx).await;
    let cx = &mut cx;

    let before = repo_names(&panel, cx);
    let initial_active = panel.update_in(cx, |panel, _, cx| {
        panel
            .active_repository
            .as_ref()
            .map(|repo| repo.read(cx).display_name().to_string())
    });

    // Activate the repository that is not currently active, the way a row click does.
    let other = panel.update_in(cx, |panel, _, cx| {
        let git_store = panel.project.read(cx).git_store().read(cx);
        let active_id = panel
            .active_repository
            .as_ref()
            .map(|repo| repo.read(cx).id);
        panel
            .sorted_repo_ids
            .iter()
            .find(|id| Some(**id) != active_id)
            .and_then(|id| git_store.repositories().get(id).cloned())
            .expect("two repositories, so one of them is not the active one")
    });
    cx.update(|_, cx| {
        other.update(cx, |repo, cx| repo.set_as_active_repository(cx));
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    let now_active = panel.update_in(cx, |panel, _, cx| {
        panel
            .active_repository
            .as_ref()
            .map(|repo| repo.read(cx).display_name().to_string())
    });
    assert_ne!(
        now_active, initial_active,
        "activating the other repository must move the panel's active repository"
    );
    assert_eq!(
        repo_names(&panel, cx),
        before,
        "which repository is active must not reorder the rows"
    );
}

/// Draws the panel with two repository rows. Each row builds its own branch-selector popover and
/// button, so a shared element id across rows would surface here.
#[gpui::test]
async fn test_panel_draws_repositories_section(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _workspace, mut cx) = init_two_repo_panel(cx).await;
    let cx = &mut cx;

    let space = gpui::size(px(360.), px(800.));
    let draw = |cx: &mut VisualTestContext, panel: &Entity<GitPanel>| {
        let panel = panel.clone();
        cx.draw(gpui::point(px(0.), px(0.)), space, |_, _| {
            panel.into_any_element()
        });
    };

    panel.update_in(cx, |panel, _, _| {
        assert_eq!(panel.sorted_repo_ids.len(), 2);
        assert!(panel.section_expanded(PanelSectionKind::Repositories));
    });
    draw(cx, &panel);

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Repositories, cx);
    });
    draw(cx, &panel);
}

/// The height cap exists so that a workspace with many repositories cannot grow the section until
/// `Changes` — the only shrinkable child — is squeezed away. Drawn at a deliberately short panel
/// height, which is where that would show up.
#[gpui::test]
async fn test_panel_draws_with_many_repositories(cx: &mut TestAppContext) {
    init_test(cx);

    const REPO_COUNT: usize = 8;
    let fs = FakeFs::new(cx.background_executor.clone());
    let mut roots = Vec::with_capacity(REPO_COUNT);
    for index in 0..REPO_COUNT {
        let root = format!("{}-{index}", path!("/repo"));
        fs.insert_tree(&root, json!({ ".git": {}, "tracked": "tracked\n" }))
            .await;
        roots.push(root);
    }

    let project = Project::test(
        fs.clone(),
        roots.iter().map(Path::new).collect::<Vec<_>>(),
        cx,
    )
    .await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    panel.update_in(cx, |panel, _, _| {
        assert_eq!(
            panel.sorted_repo_ids.len(),
            REPO_COUNT,
            "every repository must get a row"
        );
    });

    for height in [px(300.), px(800.)] {
        let panel = panel.clone();
        cx.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(360.), height),
            |_, _| panel.into_any_element(),
        );
    }
}

#[gpui::test]
async fn test_panel_draws_with_no_repository(cx: &mut TestAppContext) {
    init_test(cx);

    // A worktree with no `.git` anywhere, so the Repositories section owns the empty state.
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/project"), json!({ "file": "contents\n" }))
        .await;

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    panel.update_in(cx, |panel, _, _| {
        assert!(panel.sorted_repo_ids.is_empty());
        assert!(panel.active_repository.is_none());
    });

    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );
}

fn upstream(tracking: UpstreamTracking) -> Upstream {
    Upstream {
        ref_name: "refs/remotes/origin/main".into(),
        tracking,
    }
}

#[test]
fn test_tracking_status_label_covers_every_case() {
    use crate::git_panel::commits_section::tracking_status_label;

    assert_eq!(tracking_status_label(None), "No upstream");
    assert_eq!(
        tracking_status_label(Some(&upstream(UpstreamTracking::Gone))),
        "Upstream gone"
    );
    assert_eq!(
        tracking_status_label(Some(&upstream(
            UpstreamTrackingStatus {
                ahead: 0,
                behind: 0
            }
            .into()
        ))),
        "Up to date with origin"
    );
    assert_eq!(
        tracking_status_label(Some(&upstream(
            UpstreamTrackingStatus {
                ahead: 2,
                behind: 0
            }
            .into()
        ))),
        "↑2 ahead of origin"
    );
    assert_eq!(
        tracking_status_label(Some(&upstream(
            UpstreamTrackingStatus {
                ahead: 0,
                behind: 3
            }
            .into()
        ))),
        "↓3 behind origin"
    );
    assert_eq!(
        tracking_status_label(Some(&upstream(
            UpstreamTrackingStatus {
                ahead: 2,
                behind: 3
            }
            .into()
        ))),
        "↑2 ↓3 — origin"
    );
}

async fn init_commits_panel(
    cx: &mut TestAppContext,
) -> (Entity<GitPanel>, Entity<Repository>, VisualTestContext) {
    init_commits_panel_with_log(cx, Vec::new()).await
}

/// A linear log, newest first: commit `i` has commit `i + 1` as its only parent. The tip carries the
/// two long refs a checked-out branch really carries, which is what pushes a commit row's subject
/// out of the way when the row lets them.
fn linear_graph_log(count: usize) -> Vec<Arc<git::repository::InitialGraphCommitData>> {
    let shas = (0..count)
        .map(|ix| git::Oid::from_bytes(&[ix as u8 + 1; 20]).expect("20 bytes is a valid oid"))
        .collect::<Vec<_>>();

    shas.iter()
        .enumerate()
        .map(|(ix, sha)| {
            Arc::new(git::repository::InitialGraphCommitData {
                sha: *sha,
                parents: shas.get(ix + 1).copied().into_iter().collect(),
                ref_names: if ix == 0 {
                    vec![
                        "HEAD -> feat/vscode-parity-git-panel".into(),
                        "origin/feat/vscode-parity-git-panel".into(),
                    ]
                } else {
                    Vec::new()
                },
            })
        })
        .collect()
}

async fn init_commits_panel_with_log(
    cx: &mut TestAppContext,
    graph_commits: Vec<Arc<git::repository::InitialGraphCommitData>>,
) -> (Entity<GitPanel>, Entity<Repository>, VisualTestContext) {
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({ ".git": {}, "tracked": "tracked\n" }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/project/.git").as_ref(),
        &[("tracked", "old tracked\n".into())],
    );
    fs.set_branch_name(path!("/project/.git").as_ref(), Some("main"));
    fs.set_graph_commits(path!("/project/.git").as_ref(), graph_commits);

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let mut visual_cx = VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(&mut visual_cx, GitPanel::new);
    visual_cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    visual_cx.run_until_parked();

    let repository = panel.update_in(&mut visual_cx, |panel, _, _| {
        panel
            .active_repository
            .clone()
            .expect("the fixture has a repository")
    });

    (panel, repository, visual_cx)
}

/// Whether the branch's log has been asked for at all. `get_graph_data` reads the cache without
/// creating an entry, so `None` means nothing ever called `graph_data`.
fn branch_log_was_requested(
    panel: &Entity<GitPanel>,
    repository: &Entity<Repository>,
    cx: &mut VisualTestContext,
) -> bool {
    panel.update_in(cx, |panel, _, cx| {
        let Some(branch) = panel.current_branch_ref(cx) else {
            return false;
        };
        repository
            .read(cx)
            .get_graph_data(LogSource::Branch(branch), LogOrder::default())
            .is_some()
    })
}

/// The precondition phase 05 rests on: a folded section costs nothing.
#[gpui::test]
async fn test_collapsed_commits_section_never_requests_the_log(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, _| {
        assert!(
            !panel.section_expanded(PanelSectionKind::Commits),
            "the commits section must default to folded, since the fetch it starts is uncapped"
        );
        assert!(
            panel.commits_section.is_none(),
            "a folded section must not even allocate its state"
        );
    });

    // Draw it folded — the render path must not be a back door to the fetch.
    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    assert!(
        !branch_log_was_requested(&panel, &repository, cx),
        "nothing may ask the repository for the branch log while the section is folded"
    );
    panel.update_in(cx, |panel, _, _| {
        assert!(panel.commits_section.is_none());
    });
}

#[gpui::test]
async fn test_expanding_commits_section_requests_the_log_once(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Commits, cx);
        assert!(panel.section_expanded(PanelSectionKind::Commits));
    });
    cx.run_until_parked();

    assert!(
        branch_log_was_requested(&panel, &repository, cx),
        "expanding must be what asks for the log"
    );

    let loaded_for = panel.update_in(cx, |panel, _, _| {
        panel
            .commits_section
            .as_ref()
            .and_then(|state| state.loaded_for_branch.clone())
    });
    assert_eq!(
        loaded_for
            .clone()
            .map(|branch| branch.to_string())
            .as_deref(),
        Some("refs/heads/main")
    );

    // Asking again is a no-op — the branch has not changed and nothing dropped the cache.
    panel.update_in(cx, |panel, _, cx| panel.ensure_commits_loaded(cx));
    cx.run_until_parked();
    let loaded_again = panel.update_in(cx, |panel, _, _| {
        panel
            .commits_section
            .as_ref()
            .and_then(|state| state.loaded_for_branch.clone())
    });
    assert_eq!(loaded_again, loaded_for);
}

/// `Repository` drops its graph cache on any HEAD change, including a commit on the branch that is
/// already checked out. Invalidation must therefore not skip when the branch *name* is unchanged —
/// doing so left the section stuck on "Loading…" forever.
#[gpui::test]
async fn test_invalidation_does_not_skip_when_the_branch_name_is_unchanged(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Commits, cx);
    });
    cx.run_until_parked();

    let branch_before = panel.update_in(cx, |panel, _, _| {
        panel
            .commits_section
            .as_ref()
            .and_then(|state| state.loaded_for_branch.clone())
    });
    assert_eq!(
        branch_before.map(|branch| branch.to_string()).as_deref(),
        Some("refs/heads/main")
    );

    // Expanded: the marker must be re-established, meaning the log was asked for again.
    panel.update_in(cx, |panel, _, cx| {
        panel.invalidate_commits(cx);
        assert_eq!(
            panel
                .commits_section
                .as_ref()
                .and_then(|state| state.loaded_for_branch.clone())
                .map(|branch| branch.to_string())
                .as_deref(),
            Some("refs/heads/main"),
            "invalidating an expanded section must reload it, not leave it empty"
        );
    });
    cx.run_until_parked();

    // Folded: the state is dropped outright, even though the branch name never changed.
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Commits, cx);
        assert!(
            panel.commits_section.is_some(),
            "folding alone must not discard what was already loaded"
        );
        panel.invalidate_commits(cx);
        assert!(
            panel.commits_section.is_none(),
            "an unchanged branch name must not stop invalidation"
        );
    });
}

#[gpui::test]
async fn test_commits_section_height_survives_serialization(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        assert_eq!(panel.commits_section_height, COMMITS_SECTION_DEFAULT_HEIGHT);
        panel.commits_section_height = px(320.);
        let serialized = panel.serialized_state();
        let round_tripped = serde_json::from_str::<SerializedGitPanel>(
            &serde_json::to_string(&serialized).expect("serializes"),
        )
        .expect("deserializes");
        panel.commits_section_height = px(0.);
        panel.apply_serialized_state(round_tripped, cx);
        assert_eq!(panel.commits_section_height, px(320.));
    });

    // A stored height from a build with different bounds is clamped back inside them.
    panel.update_in(cx, |panel, _, cx| {
        let out_of_range = SerializedGitPanel {
            amend_pending: false,
            signoff_enabled: false,
            section_collapse: None,
            commits_section_height: Some(5_000.),
            graph_section_height: None,
            repositories_section_height: None,
        };
        panel.apply_serialized_state(out_of_range, cx);
        assert_eq!(panel.commits_section_height, COMMITS_SECTION_MAX_HEIGHT);
    });
}

/// The Graph section rests on the same precondition as Commits: folded costs nothing. `graph_data`
/// is what starts the fetch, so a folded section must never allocate its state.
#[gpui::test]
async fn test_collapsed_graph_section_never_requests_the_log(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, _| {
        assert!(
            !panel.section_expanded(PanelSectionKind::Graph),
            "the graph section must default to folded"
        );
        assert!(panel.graph_section.is_none());
    });

    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    assert!(
        !branch_log_was_requested(&panel, &repository, cx),
        "nothing may ask for the branch log while the graph section is folded"
    );
    panel.update_in(cx, |panel, _, _| {
        assert!(panel.graph_section.is_none());
    });
}

#[gpui::test]
async fn test_expanding_graph_section_requests_the_log(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
        assert!(panel.section_expanded(PanelSectionKind::Graph));
    });
    cx.run_until_parked();

    assert!(
        branch_log_was_requested(&panel, &repository, cx),
        "expanding the graph must be what asks for the log"
    );
    panel.update_in(cx, |panel, _, _| {
        assert!(
            panel.graph_section.is_some(),
            "the expanded section must hold its state"
        );
    });

    // Drawing it expanded must not panic — this is the lane canvas plus the row list together.
    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );
}

fn graph_status(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Option<SharedString> {
    panel.update_in(cx, |panel, _, cx| panel.graph_status_message(cx))
}

/// A failed log used to look exactly like a slow one: the section said "Loading graph…" for as long
/// as it had no rows, so the reason nothing arrived was nowhere on screen.
#[gpui::test]
async fn test_graph_section_says_why_the_log_failed(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel
        .read_with(cx, |panel, _| panel.fs.clone())
        .as_fake()
        .set_graph_error(path!("/project/.git").as_ref(), Some("boom".to_string()));
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();

    let message = graph_status(&panel, cx).expect("a failed log has to say so");
    assert!(
        message.contains("boom"),
        "the failure's reason must reach the section, got {message:?}"
    );
}

/// An empty history and one still being read both have no rows; only the first is final.
#[gpui::test]
async fn test_graph_section_tells_an_empty_log_from_a_loading_one(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    assert_eq!(
        graph_status(&panel, cx),
        Some(SharedString::from("Loading graph…")),
        "before the fetch has ended, no rows means still loading"
    );

    cx.run_until_parked();
    assert_eq!(
        graph_status(&panel, cx),
        Some(SharedString::from("No commits")),
        "a fetch that ended with nothing is an empty history, not a slow one"
    );
}

#[gpui::test]
async fn test_graph_section_has_no_status_once_rows_arrive(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(3)).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        graph_status(&panel, cx),
        None,
        "rows on screen need no message above them"
    );
}

fn graph_fetch_count(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> usize {
    panel
        .read_with(cx, |panel, _| panel.fs.clone())
        .as_fake()
        .graph_fetch_count(path!("/project/.git").as_ref())
}

/// The panel re-runs `ensure_graph_loaded` on every status update. A failed log has no rows, so
/// taking "no rows" for "not loaded" fetched it again on each update, and the section flickered
/// between the error and "Loading graph…" for as long as files kept changing.
#[gpui::test]
async fn test_status_updates_do_not_refetch_a_failed_graph(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;
    let fs = panel.read_with(cx, |panel, _| panel.fs.clone());

    fs.as_fake()
        .set_graph_error(path!("/project/.git").as_ref(), Some("boom".to_string()));
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();
    assert_eq!(graph_fetch_count(&panel, cx), 1);

    for round in 0..3 {
        // A new untracked file changes the statuses, which is what schedules a panel update.
        fs.as_fake()
            .insert_file(
                Path::new(path!("/project")).join(format!("untracked_{round}")),
                b"new\n".to_vec(),
            )
            .await;
        cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(
            panel.read_with(cx, |panel, _| panel
                .entries
                .iter()
                .filter(|entry| entry.status_entry().is_some())
                .count()),
            round + 2,
            "status update {round} should have reached the panel"
        );

        assert_eq!(
            graph_fetch_count(&panel, cx),
            1,
            "status update {round} fetched the failed graph again"
        );
        let message = graph_status(&panel, cx).expect("a failed log has to say so");
        assert!(
            message.contains("boom"),
            "status update {round} replaced the error with {message:?}"
        );
    }
}

/// The reload button is how a failed graph is asked for again.
#[gpui::test]
async fn test_reloading_the_graph_retries_a_failed_log(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;
    let fs = panel.read_with(cx, |panel, _| panel.fs.clone());
    let dot_git: &Path = path!("/project/.git").as_ref();

    fs.as_fake()
        .set_graph_error(dot_git, Some("boom".to_string()));
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();
    assert!(graph_status(&panel, cx).is_some_and(|message| message.contains("boom")));

    fs.as_fake().set_graph_error(dot_git, None);
    fs.as_fake().set_graph_commits(dot_git, linear_graph_log(3));
    panel.update_in(cx, |panel, _, cx| panel.invalidate_graph(cx));
    cx.run_until_parked();

    assert_eq!(
        graph_fetch_count(&panel, cx),
        2,
        "reloading must fetch again"
    );
    assert_eq!(graph_status(&panel, cx), None);
    assert_eq!(
        panel.update_in(cx, |panel, _, _| panel
            .graph_section
            .as_ref()
            .map_or(0, |state| state.graph_data.commits.len())),
        3
    );

    panel.update_in(cx, |panel, _, cx| panel.invalidate_graph(cx));
    cx.run_until_parked();
    assert_eq!(
        graph_fetch_count(&panel, cx),
        2,
        "a graph that loaded is served from the cache, not fetched again"
    );
}

async fn detach_head(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    panel
        .read_with(cx, |panel, _| panel.fs.clone())
        .as_fake()
        .set_branch_name(path!("/project/.git").as_ref(), None::<&str>);
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();
    assert_eq!(
        panel.update_in(cx, |panel, _, cx| panel.current_branch_ref(cx)),
        None,
        "the fixture should now have no branch checked out"
    );
}

/// "Auto" follows the checked-out branch, so with HEAD detached it has nothing to follow, and it
/// has to say so rather than wait on a log nobody asked for.
#[gpui::test]
async fn test_graph_section_on_auto_says_no_branch_is_checked_out(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(3)).await;
    let cx = &mut cx;
    detach_head(&panel, cx).await;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();

    assert!(panel.update_in(cx, |panel, _, _| panel.graph_log_source_is_auto()));
    assert_eq!(
        graph_status(&panel, cx),
        Some(SharedString::from("No branch checked out"))
    );
    assert_eq!(graph_fetch_count(&panel, cx), 0);
}

/// "All" shows every ref, which needs no branch, so a detached HEAD must not leave it loading.
#[gpui::test]
async fn test_graph_section_on_all_loads_with_a_detached_head(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(3)).await;
    let cx = &mut cx;
    detach_head(&panel, cx).await;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();
    panel.update_in(cx, |panel, _, cx| panel.toggle_graph_log_source(cx));
    cx.run_until_parked();

    assert!(
        !panel.update_in(cx, |panel, _, _| panel.graph_log_source_is_auto()),
        "the graph should have moved onto all refs"
    );
    assert_eq!(graph_status(&panel, cx), None);
    assert_eq!(
        panel.update_in(cx, |panel, _, _| panel
            .graph_section
            .as_ref()
            .map_or(0, |state| state.graph_data.commits.len())),
        3
    );
}

/// The handle sits on the section's top edge, the boundary it shares with the section that yields
/// the space, so dragging it up has to make the section taller. Under the content the opposite held,
/// and the last section's handle landed against the bottom of the panel.
#[test]
fn test_a_section_grows_when_its_top_handle_is_dragged_up() {
    assert_eq!(
        height_dragged_from_top_edge(px(200.), px(500.), px(460.)),
        px(240.),
        "dragging the top edge 40px up must add 40px to the section"
    );
    assert_eq!(
        height_dragged_from_top_edge(px(200.), px(500.), px(540.)),
        px(160.),
        "dragging the top edge down must take height away"
    );
    assert_eq!(
        height_dragged_from_top_edge(px(100.), px(500.), px(900.)),
        COMMITS_SECTION_MIN_HEIGHT,
        "a drag past the floor stops at it"
    );
    assert_eq!(
        height_dragged_from_top_edge(px(590.), px(500.), px(0.)),
        COMMITS_SECTION_MAX_HEIGHT,
        "a drag past the ceiling stops at it"
    );
}

/// The tip of a branch carries two refs -- `HEAD -> <branch>` and `origin/<branch>` -- whose names
/// together run wider than the panel. Chips that will not shrink take the subject's width first and
/// the timestamp's next, and the row that should read as a commit reads as two blue labels and a
/// clipped word. The subject is the row's point, so it has to keep a usable share of the width.
///
/// Measured from a docked panel: the window draws that itself and publishes the frame, whereas
/// `cx.draw` paints into the pending frame and never swaps it, leaving `debug_bounds` empty.
#[gpui::test]
async fn test_commit_row_keeps_its_subject_beside_two_long_refs(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(4)).await;
    let cx = &mut cx;

    // Deliberately not the test store's default of 14, where 1.75rem works out to 24.5px and the
    // old hardcoded graph row of 24px would land within a rounded pixel of it and pass.
    cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.theme.ui_font_size = Some(17.0.into());
            });
        })
    });

    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
    workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.add_panel(panel.clone(), window, cx);
            workspace.open_panel::<GitPanel>(window, cx);
        })
        .expect("the fixture's workspace is alive");

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Commits, cx);
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();
    // Commit details land after the rows are first drawn; repaint the panel itself so the frame
    // measured below is one the panel drew rather than one that reused its cached view.
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();

    let row = cx
        .debug_bounds("commit-row-with-refs")
        .expect("the commits section must draw the row carrying the branch's refs");
    let subject = cx
        .debug_bounds("commit-row-with-refs-subject")
        .expect("that row must draw a subject");

    // The row follows `ui_font_size` like the `Changes` list above it, rather than a pixel constant
    // that leaves this section at one density while its neighbour grows with the font.
    let rem_size = cx.update(|window, _| window.rem_size());
    assert!(
        (row.size.height - rem_size * 1.75).abs() <= px(1.),
        "a commit row is {:?} tall at a rem of {:?}, so it no longer tracks the UI font size \
         (1.75rem, rounded to whole pixels)",
        row.size.height,
        rem_size
    );

    assert!(
        subject.size.width > px(0.),
        "the refs took the whole {:?}-wide row and left the subject nothing",
        row.size.width
    );
    assert!(
        subject.size.width >= row.size.width * 0.2,
        "the subject got {:?} of a {:?}-wide row, too little to read a commit message in",
        subject.size.width,
        row.size.width
    );
}

/// A tip merging `branches` root commits: one row with every branch open beside it.
fn fan_out_graph_log(branches: usize) -> Vec<Arc<git::repository::InitialGraphCommitData>> {
    let shas = (0..=branches)
        .map(|ix| git::Oid::from_bytes(&[ix as u8 + 1; 20]).expect("20 bytes is a valid oid"))
        .collect::<Vec<_>>();
    shas.iter()
        .enumerate()
        .map(|(ix, sha)| {
            Arc::new(git::repository::InitialGraphCommitData {
                sha: *sha,
                parents: if ix == 0 {
                    shas[1..].to_vec().into()
                } else {
                    Default::default()
                },
                ref_names: Vec::new(),
            })
        })
        .collect()
}

/// The graph row list's laid-out box and its content, after drawing the panel `width` wide.
async fn graph_list_size(
    log: Vec<Arc<git::repository::InitialGraphCommitData>>,
    width: Pixels,
    cx: &mut TestAppContext,
) -> gpui::ItemSize {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, log).await;
    let cx = &mut cx;
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(width, px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );
    panel
        .update_in(cx, |panel, _, _| {
            panel
                .graph_section
                .as_ref()
                .and_then(|state| state.scroll_handle.0.borrow().last_item_size)
        })
        .expect("the row list must reach layout")
}

/// Lanes too many for the panel's width push the rows past it, so the list scrolls sideways
/// rather than drawing the lanes underneath the subjects.
#[gpui::test]
async fn test_a_graph_wider_than_the_panel_scrolls_sideways(cx: &mut TestAppContext) {
    let size = graph_list_size(fan_out_graph_log(24), px(360.), cx).await;
    assert!(
        size.contents.width > size.item.width,
        "24 lanes cannot fit a 360px panel beside a subject, so the rows must be wider than the \
         list ({:?} of content in a {:?} list)",
        size.contents.width,
        size.item.width
    );
}

/// A graph that fits leaves the rows the panel's width: no scrollbar for nothing to scroll to.
#[gpui::test]
async fn test_a_graph_that_fits_does_not_scroll_sideways(cx: &mut TestAppContext) {
    let size = graph_list_size(linear_graph_log(8), px(360.), cx).await;
    assert_eq!(size.contents.width, size.item.width);
}

/// Drawing without panicking was all phase 05 asserted, and it let a blank section ship: the lane
/// canvas takes its height from an explicit `h_full` so it painted, while the row list took
/// `flex_1` inside a row-direction `div`, where that grows the width and leaves the height alone.
/// A `uniform_list` contributes no height of its own (`ListSizingBehavior::Auto`), so the list was
/// laid out zero pixels tall and rendered no rows — lanes on screen, not one subject beside them.
/// `last_item_size.item` is the box the list actually got.
#[gpui::test]
async fn test_expanded_graph_section_lays_out_its_row_list(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(8)).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    assert_ne!(
        panel.update_in(cx, |panel, _, _| panel
            .graph_section
            .as_ref()
            .map_or(0, |state| state.graph_data.commits.len())),
        0,
        "the fixture's log must have arrived, or this test cannot see the row list at all"
    );

    let panel_to_draw = panel.clone();
    cx.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(360.), px(800.)),
        |_, _| panel_to_draw.into_any_element(),
    );

    let list_box = panel
        .update_in(cx, |panel, _, _| {
            panel
                .graph_section
                .as_ref()
                .and_then(|state| state.scroll_handle.0.borrow().last_item_size)
        })
        .expect("the row list must reach layout, not be skipped entirely");

    let section_height = panel.update_in(cx, |panel, _, _| panel.graph_section_height);
    assert!(
        list_box.item.height >= section_height - px(1.),
        "the graph row list was laid out {:?} tall inside a {:?} section, so it shows its lanes \
         against an empty column instead of the commits beside them",
        list_box.item.height,
        section_height
    );
    assert!(
        list_box.item.width > px(0.),
        "the graph row list was laid out with no width"
    );
}

/// A log that died partway has rows and an error at once. The error sits above the rows, outside
/// the lane canvas's container, and the row list still has to get the rest of the section's height.
/// Measured from a docked panel, since only a frame the window drew itself has `debug_bounds`.
#[gpui::test]
async fn test_graph_section_lays_out_its_rows_under_an_error(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel_with_log(cx, linear_graph_log(8)).await;
    let cx = &mut cx;
    panel
        .read_with(cx, |panel, _| panel.fs.clone())
        .as_fake()
        .set_graph_error(path!("/project/.git").as_ref(), Some("boom".to_string()));

    let workspace = panel.read_with(cx, |panel, _| panel.workspace.clone());
    workspace
        .update_in(cx, |workspace, window, cx| {
            workspace.add_panel(panel.clone(), window, cx);
            workspace.open_panel::<GitPanel>(window, cx);
        })
        .expect("the fixture's workspace is alive");
    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();

    assert_eq!(
        panel.update_in(cx, |panel, _, _| panel
            .graph_section
            .as_ref()
            .map_or(0, |state| state.graph_data.commits.len())),
        8,
        "the commits before the failure must still be shown"
    );
    assert!(
        graph_status(&panel, cx).is_some_and(|message| message.contains("boom")),
        "the failure must be reported beside the rows"
    );

    let status = cx
        .debug_bounds("graph-section-status")
        .expect("the status row must be drawn above the rows");
    assert!(status.size.height > px(0.));

    let list_box = panel
        .update_in(cx, |panel, _, _| {
            panel
                .graph_section
                .as_ref()
                .and_then(|state| state.scroll_handle.0.borrow().last_item_size)
        })
        .expect("the row list must reach layout under the status row");
    let section_height = panel.update_in(cx, |panel, _, _| panel.graph_section_height);
    assert!(
        list_box.item.height >= section_height - status.size.height - px(1.),
        "the row list was laid out {:?} tall in a {:?} section under a {:?} status row",
        list_box.item.height,
        section_height,
        status.size.height
    );
}

#[gpui::test]
async fn test_graph_log_source_toggles_between_branch_and_all(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        panel.toggle_section(PanelSectionKind::Graph, cx);
    });
    cx.run_until_parked();

    assert!(
        panel.update_in(cx, |panel, _, _| panel.graph_log_source_is_auto()),
        "the graph starts on Auto, following the checked-out branch"
    );

    panel.update_in(cx, |panel, _, cx| panel.toggle_graph_log_source(cx));
    cx.run_until_parked();
    assert!(
        !panel.update_in(cx, |panel, _, _| panel.graph_log_source_is_auto()),
        "toggling must move the graph off the current branch onto all refs"
    );

    panel.update_in(cx, |panel, _, cx| panel.toggle_graph_log_source(cx));
    cx.run_until_parked();
    assert!(panel.update_in(cx, |panel, _, _| panel.graph_log_source_is_auto()));
}

/// The project rail draws one button per left-docked panel in `Dock::panels()`
/// order — which is `activation_priority` order — stacked top to bottom, so the
/// lower priority is the one that sits higher. Branches belongs above the git
/// panel, and the only thing holding that is two integers in two different files.
///
/// Asserting those integers would just restate them. Asserting the order through
/// the very iterator the rail renders from is what survives a renumber, and what
/// catches a half-applied swap — the two priorities have to move together or the
/// dock's own uniqueness check panics.
#[gpui::test]
async fn the_branch_panel_sits_above_the_git_panel_in_the_rail(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root/zed"),
        json!({
            ".git": {},
            "src": { "main.rs": "fn main() {}" },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root/zed").as_ref()], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    cx.executor().run_until_parked();

    // Added git-first, so a passing assertion cannot be insertion order wearing
    // the costume of a priority.
    workspace.update_in(cx, |workspace, window, cx| {
        let git_panel = GitPanel::new(workspace, window, cx);
        workspace.add_panel(git_panel, window, cx);
        let branch_panel = crate::branch_panel::BranchPanel::new(workspace, window, cx);
        workspace.add_panel(branch_panel, window, cx);
    });

    let order = workspace.read_with(cx, |workspace, cx| {
        workspace
            .left_dock()
            .read(cx)
            .panels()
            .map(|panel| panel.persistent_name())
            .collect::<Vec<_>>()
    });

    let branch = order.iter().position(|name| *name == "BranchPanel");
    let git = order.iter().position(|name| *name == "GitPanel");
    assert!(
        matches!((branch, git), (Some(branch), Some(git)) if branch < git),
        "the rail draws the left dock in this order, and Branches must come before Git Panel: {order:?}"
    );
}

/// The left dock shares one width across every panel in it, taken from
/// `Dock::size_governing_entry` — which is `panel_entries[0]`, the lowest
/// `activation_priority` on that edge. So the priority swap above did not only
/// move an icon: it handed the column's width to the branch panel.
///
/// Worth pinning because the obvious reading is wrong. `ProjectPanel` has the
/// lowest priority of all (1), which invites the conclusion that it governs the
/// left dock and that reordering git and branch cannot touch the width — but
/// `ProjectPanel::position` is hardcoded to `DockPosition::Right`, so it is never
/// on this edge at all. That mistake was made during this change and caught only
/// downstream; this test is what would have caught it.
#[gpui::test]
async fn the_left_dock_width_follows_the_branch_panel(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root/zed"),
        json!({
            ".git": {},
            "src": { "main.rs": "fn main() {}" },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root/zed").as_ref()], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    cx.executor().run_until_parked();

    workspace.update_in(cx, |workspace, window, cx| {
        let git_panel = GitPanel::new(workspace, window, cx);
        workspace.add_panel(git_panel, window, cx);
        let branch_panel = crate::branch_panel::BranchPanel::new(workspace, window, cx);
        workspace.add_panel(branch_panel, window, cx);
        workspace.left_dock().update(cx, |dock, cx| {
            dock.set_open(true, window, cx);
        });
    });

    let (governing, width) = workspace.update_in(cx, |workspace, window, cx| {
        let dock = workspace.left_dock().read(cx);
        let governing = dock
            .size_governing_panel()
            .map(|panel| panel.persistent_name());
        (governing, dock.stored_active_panel_size(window, cx))
    });

    assert_eq!(
        governing,
        Some("BranchPanel"),
        "the branch panel holds the lowest priority on the left edge, so it governs the column"
    );
    // Both panels declare 360, so the shared column is the same width it was
    // before the swap moved which of them decides it. Drift either apart and the
    // column silently resizes.
    assert_eq!(
        width,
        Some(px(360.)),
        "the left column must stay 360 wide after the swap handed it to the branch panel"
    );
}

struct DiscardAllFixture {
    fs: Arc<FakeFs>,
    panel: Entity<GitPanel>,
}

async fn init_discard_all_panel(
    cx: &mut TestAppContext,
    with_modified_file: bool,
) -> (DiscardAllFixture, VisualTestContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {},
            "tracked.txt": "edited\n",
            "new.txt": "brand new\n",
        }),
    )
    .await;
    let dot_git = path!("/project/.git");
    fs.set_head_and_index_for_repo(dot_git.as_ref(), &[("tracked.txt", "committed\n".into())]);
    if !with_modified_file {
        fs.insert_file(path!("/project/tracked.txt"), b"committed\n".to_vec())
            .await;
    }

    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let mut visual_cx = VisualTestContext::from_window(window_handle.into(), cx);
    visual_cx.run_until_parked();
    let panel = workspace.update_in(&mut visual_cx, GitPanel::new);
    let handle = visual_cx.update_window_entity(&panel, |panel, _, _| {
        std::mem::replace(&mut panel.update_visible_entries_task, Task::ready(()))
    });
    visual_cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    handle.await;
    (DiscardAllFixture { fs, panel }, visual_cx)
}

fn dispatch_discard_all(fixture: &DiscardAllFixture, cx: &mut VisualTestContext) {
    fixture.panel.update_in(cx, |panel, window, cx| {
        panel.discard_all_changes(&DiscardAllChanges, window, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
async fn test_discard_all_changes_confirmed_restores_tracked_and_trashes_untracked(
    cx: &mut TestAppContext,
) {
    let (fixture, mut cx) = init_discard_all_panel(cx, true).await;

    dispatch_discard_all(&fixture, &mut cx);
    assert!(
        cx.has_pending_prompt(),
        "discarding everything must ask first"
    );
    cx.simulate_prompt_answer("Discard All Changes");
    cx.run_until_parked();
    cx.executor().advance_clock(2 * UPDATE_DEBOUNCE);
    cx.run_until_parked();

    assert!(
        !fixture.fs.is_file(path!("/project/new.txt").as_ref()).await,
        "the untracked file must be trashed"
    );
    assert_eq!(
        fixture
            .fs
            .load(path!("/project/tracked.txt").as_ref())
            .await
            .unwrap(),
        "committed\n",
        "the modified tracked file must have its HEAD content back"
    );
    fixture.panel.update(&mut cx, |panel, _| {
        assert!(
            panel
                .entries
                .iter()
                .filter_map(|entry| entry.status_entry())
                .next()
                .is_none(),
            "neither the restored nor the trashed file may stay in the status list"
        );
    });
}

#[gpui::test]
async fn test_discard_all_changes_cancel_leaves_everything_untouched(cx: &mut TestAppContext) {
    let (fixture, mut cx) = init_discard_all_panel(cx, true).await;

    dispatch_discard_all(&fixture, &mut cx);
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();

    assert_eq!(
        fixture
            .fs
            .load(path!("/project/tracked.txt").as_ref())
            .await
            .unwrap(),
        "edited\n"
    );
    assert!(fixture.fs.is_file(path!("/project/new.txt").as_ref()).await);
}

#[gpui::test]
async fn test_discard_all_changes_prompts_for_a_single_changed_file(cx: &mut TestAppContext) {
    let (fixture, mut cx) = init_discard_all_panel(cx, false).await;

    dispatch_discard_all(&fixture, &mut cx);
    assert!(
        cx.has_pending_prompt(),
        "a lone untracked file is still destructive and must be confirmed"
    );
    assert!(
        fixture.fs.is_file(path!("/project/new.txt").as_ref()).await,
        "nothing may be deleted before the user answers"
    );
}

#[gpui::test]
async fn test_discard_all_changes_does_nothing_without_changes(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/project"), json!({ ".git": {}, "a.txt": "a\n" }))
        .await;
    fs.set_head_and_index_for_repo(path!("/project/.git").as_ref(), &[("a.txt", "a\n".into())]);
    let project = Project::test(fs.clone(), [Path::new(path!("/project"))], cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let cx = &mut VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(cx, GitPanel::new);
    cx.run_until_parked();

    panel.update_in(cx, |panel, window, cx| {
        assert!(!panel.has_discardable_changes());
        panel.discard_all_changes(&DiscardAllChanges, window, cx);
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
async fn test_discard_all_menu_gating_follows_the_changes(cx: &mut TestAppContext) {
    let (fixture, mut cx) = init_discard_all_panel(cx, true).await;
    fixture.panel.update(&mut cx, |panel, _| {
        assert!(panel.has_discardable_changes());
    });
}

async fn init_reload_repositories_panel(
    fs: &Arc<FakeFs>,
    roots: &[&Path],
    cx: &mut TestAppContext,
) -> (
    Entity<Project>,
    Entity<Workspace>,
    Entity<GitPanel>,
    VisualTestContext,
) {
    let project = Project::test(fs.clone(), roots.iter().copied(), cx).await;
    let window_handle =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = window_handle
        .read_with(cx, |mw, _| mw.workspace().clone())
        .unwrap();
    let mut cx = VisualTestContext::from_window(window_handle.into(), cx);
    let panel = workspace.update_in(&mut cx, GitPanel::new);
    cx.run_until_parked();
    (project, workspace, panel, cx)
}

/// Creates repositories the way a watcher across a WSL or SSH boundary can miss them.
async fn add_repositories_without_a_watcher_event(fs: &FakeFs, directories: &[&str]) {
    fs.pause_events();
    for directory in directories {
        fs.insert_tree(directory, json!({ ".git": {}, "file.txt": "" }))
            .await;
    }
    fs.clear_buffered_events();
    fs.unpause_events_and_flush();
}

#[gpui::test]
async fn test_reload_repositories_rescans_every_visible_worktree(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root1"), json!({ ".git": {} })).await;
    fs.insert_tree(path!("/root2"), json!({ ".git": {} })).await;
    let (_project, _workspace, panel, mut cx) = init_reload_repositories_panel(
        &fs,
        &[Path::new(path!("/root1")), Path::new(path!("/root2"))],
        cx,
    )
    .await;
    let cx = &mut cx;
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.sorted_repo_ids.len()),
        2
    );

    add_repositories_without_a_watcher_event(
        &fs,
        &[path!("/root1/nested1"), path!("/root2/nested2")],
    )
    .await;
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.sorted_repo_ids.len()),
        2,
        "the watcher missed both repositories"
    );

    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    cx.executor().advance_clock(MIN_RELOAD_SPINNER);
    cx.run_until_parked();

    assert_eq!(
        panel.read_with(cx, |panel, _| panel.sorted_repo_ids.len()),
        4,
        "a repository in each worktree is found, not only in the active one"
    );
}

#[gpui::test]
async fn test_reload_repositories_is_single_flight(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root"), json!({ ".git": {} })).await;
    let (_project, _workspace, panel, mut cx) =
        init_reload_repositories_panel(&fs, &[Path::new(path!("/root"))], cx).await;
    let cx = &mut cx;
    let reloading =
        |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.reloading_repositories);

    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    assert!(reloading(cx), "the spinner starts at once");

    // The scan itself is long done by now; only the spinner is still up.
    cx.executor().advance_clock(MIN_RELOAD_SPINNER / 2);
    cx.run_until_parked();
    let read_dir_calls_before = fs.read_dir_call_count();
    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    cx.run_until_parked();
    assert_eq!(
        fs.read_dir_call_count(),
        read_dir_calls_before,
        "a press while the spinner is up must not start another batch"
    );

    // The second press must not restart the minimum spinner time either.
    cx.executor().advance_clock(MIN_RELOAD_SPINNER / 2);
    cx.run_until_parked();
    assert!(
        !reloading(cx),
        "the flag clears once the reload has run its course"
    );
}

#[gpui::test]
async fn test_reload_repositories_failure_shows_notification(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root1"), json!({ ".git": {} })).await;
    fs.insert_tree(path!("/root2"), json!({ ".git": {} })).await;
    let (project, workspace, panel, mut cx) = init_reload_repositories_panel(
        &fs,
        &[Path::new(path!("/root1")), Path::new(path!("/root2"))],
        cx,
    )
    .await;
    let cx = &mut cx;
    project.update(cx, |project, _| project.mark_as_collab_for_testing());

    let read_dir_calls_before = fs.read_dir_call_count();
    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    cx.executor().advance_clock(MIN_RELOAD_SPINNER);
    cx.run_until_parked();

    assert_eq!(fs.read_dir_call_count(), read_dir_calls_before);
    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.notification_ids().len(),
            1,
            "two refusals are reported as one notification"
        );
    });
    assert!(!panel.read_with(cx, |panel, _| panel.reloading_repositories));
}

fn visible_worktree_ids(project: &Entity<Project>, cx: &mut VisualTestContext) -> Vec<WorktreeId> {
    project.read_with(cx, |project, cx| {
        project
            .visible_worktrees(cx)
            .map(|worktree| worktree.read(cx).id())
            .collect()
    })
}

#[gpui::test]
async fn test_reload_repositories_of_removed_worktrees_stays_silent(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root1"), json!({ ".git": {} })).await;
    fs.insert_tree(path!("/root2"), json!({ ".git": {} })).await;
    let (project, workspace, panel, mut cx) = init_reload_repositories_panel(
        &fs,
        &[Path::new(path!("/root1")), Path::new(path!("/root2"))],
        cx,
    )
    .await;
    let cx = &mut cx;
    let worktree_ids = visible_worktree_ids(&project, cx);
    // A guest refusal is the one failure a local project can produce; what is under test is that
    // a failure is not reported once its worktree is gone, whatever the failure was.
    project.update(cx, |project, _| project.mark_as_collab_for_testing());

    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    for worktree_id in worktree_ids {
        project.update(cx, |project, cx| project.remove_worktree(worktree_id, cx));
    }
    cx.executor().advance_clock(MIN_RELOAD_SPINNER);
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert!(
            workspace.notification_ids().is_empty(),
            "a reload that failed because its folders were removed has nothing left to tell the user"
        );
    });
    assert!(!panel.read_with(cx, |panel, _| panel.reloading_repositories));
}

#[gpui::test]
async fn test_reload_repositories_still_reports_a_worktree_that_remains(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root1"), json!({ ".git": {} })).await;
    fs.insert_tree(path!("/root2"), json!({ ".git": {} })).await;
    let (project, workspace, panel, mut cx) = init_reload_repositories_panel(
        &fs,
        &[Path::new(path!("/root1")), Path::new(path!("/root2"))],
        cx,
    )
    .await;
    let cx = &mut cx;
    let worktree_ids = visible_worktree_ids(&project, cx);
    project.update(cx, |project, _| project.mark_as_collab_for_testing());

    panel.update(cx, |panel, cx| panel.reload_repositories(cx));
    project.update(cx, |project, cx| {
        project.remove_worktree(worktree_ids[0], cx)
    });
    cx.executor().advance_clock(MIN_RELOAD_SPINNER);
    cx.run_until_parked();

    workspace.update(cx, |workspace, _| {
        assert_eq!(
            workspace.notification_ids().len(),
            1,
            "the folder that is still open failed to reload, and that is still news"
        );
    });
}

/// The reload button is drawn only while the pointer is over the header it sits in, and nothing
/// but the button itself carries a selector. So the pointer walks down the panel until the button
/// has appeared.
fn hover_the_repositories_header(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let panel_bounds = cx
        .debug_bounds("dock-panel")
        .expect("the docked panel is drawn");
    let mut y = panel_bounds.origin.y;
    while y < panel_bounds.bottom() {
        cx.simulate_mouse_move(
            point(panel_bounds.origin.x + px(40.), y),
            None,
            gpui::Modifiers::default(),
        );
        if let Some(button) = cx.debug_bounds("reload-repositories") {
            return button;
        }
        y += px(4.);
    }
    panic!("no height of the panel showed the reload button on hover");
}

/// A panel in an open dock, which is what it takes for the window to draw it.
async fn init_reload_repositories_panel_in_open_dock(
    fs: &Arc<FakeFs>,
    root: &Path,
    cx: &mut TestAppContext,
) -> (
    Entity<Project>,
    Entity<Workspace>,
    Entity<GitPanel>,
    VisualTestContext,
) {
    let (project, workspace, panel, mut cx) = init_reload_repositories_panel(fs, &[root], cx).await;
    workspace.update_in(&mut cx, |workspace, window, cx| {
        workspace.add_panel(panel.clone(), window, cx);
        workspace.open_panel::<GitPanel>(window, cx);
    });
    cx.run_until_parked();
    (project, workspace, panel, cx)
}

fn repositories_section_is_expanded(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> bool {
    panel.read_with(cx, |panel, _| {
        panel.section_expanded(PanelSectionKind::Repositories)
    })
}

#[gpui::test]
async fn test_pressing_the_disabled_reload_button_leaves_the_section_expanded(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root"), json!({ ".git": {} })).await;
    let (project, _workspace, panel, mut cx) =
        init_reload_repositories_panel_in_open_dock(&fs, Path::new(path!("/root")), cx).await;
    let cx = &mut cx;
    project.update(cx, |project, _| project.mark_as_collab_for_testing());
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(repositories_section_is_expanded(&panel, cx));

    let button = hover_the_repositories_header(cx);
    cx.simulate_click(button.center(), gpui::Modifiers::default());

    assert!(
        repositories_section_is_expanded(&panel, cx),
        "a press on the greyed-out button must not fold the section it sits in"
    );
}

#[gpui::test]
async fn test_pressing_the_reload_button_reloads_and_leaves_the_section_expanded(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root"), json!({ ".git": {} })).await;
    let (_project, _workspace, panel, mut cx) =
        init_reload_repositories_panel_in_open_dock(&fs, Path::new(path!("/root")), cx).await;
    let cx = &mut cx;

    let button = hover_the_repositories_header(cx);
    cx.simulate_click(button.center(), gpui::Modifiers::default());

    assert!(
        panel.read_with(cx, |panel, _| panel.reloading_repositories),
        "the press starts a reload"
    );
    assert!(
        repositories_section_is_expanded(&panel, cx),
        "and does not fold the section it sits in"
    );
}

fn repository_rows_height(cx: &mut VisualTestContext) -> Pixels {
    cx.debug_bounds("repository-rows")
        .expect("the repository rows are drawn")
        .size
        .height
}

/// Presses the boundary between `Repositories` and `Changes` and moves it by `delta`.
fn drag_repositories_boundary(cx: &mut VisualTestContext, delta: Pixels) {
    let handle = cx
        .debug_bounds("repositories-section-resize-handle")
        .expect("an expanded Repositories section has a handle on its boundary with Changes");
    let start = handle.center();
    let modifiers = gpui::Modifiers::default();
    // Hit-testing reads the hover state the last mouse move left behind.
    cx.simulate_mouse_move(start, None, modifiers);
    cx.simulate_mouse_down(start, MouseButton::Left, modifiers);
    // The move past GPUI's 2px threshold only starts the drag; the first drag-move after it is what
    // records where the drag began. Only moves after that one change the height.
    let past_threshold = point(start.x, start.y + px(3.));
    cx.simulate_mouse_move(past_threshold, MouseButton::Left, modifiers);
    let origin = point(start.x, past_threshold.y + px(1.));
    cx.simulate_mouse_move(origin, MouseButton::Left, modifiers);
    let end = point(start.x, origin.y + delta);
    cx.simulate_mouse_move(end, MouseButton::Left, modifiers);
    cx.simulate_mouse_up(end, MouseButton::Left, modifiers);
    cx.run_until_parked();
}

#[gpui::test]
async fn test_dragging_the_repositories_boundary_resizes_the_section(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/root"), json!({ ".git": {} })).await;
    let (_project, _workspace, panel, mut cx) =
        init_reload_repositories_panel_in_open_dock(&fs, Path::new(path!("/root")), cx).await;
    let cx = &mut cx;

    let natural_height = repository_rows_height(cx);
    assert_eq!(
        natural_height,
        px(36.),
        "before any drag the section is as tall as its one row"
    );

    drag_repositories_boundary(cx, px(100.));
    assert_eq!(
        repository_rows_height(cx),
        natural_height + px(100.),
        "dragging the boundary down grows Repositories by the same distance"
    );

    drag_repositories_boundary(cx, px(-40.));
    assert_eq!(
        repository_rows_height(cx),
        natural_height + px(60.),
        "and dragging it back up shrinks it again"
    );

    drag_repositories_boundary(cx, px(-1_000.));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.repositories_section_height),
        Some(REPOSITORY_ROW_HEIGHT),
        "the section never shrinks below one row"
    );
}

#[gpui::test]
async fn test_repositories_section_height_survives_serialization(cx: &mut TestAppContext) {
    init_test(cx);
    let (panel, _repository, mut cx) = init_commits_panel(cx).await;
    let cx = &mut cx;

    panel.update_in(cx, |panel, _, cx| {
        assert_eq!(
            panel.repositories_section_height, None,
            "a panel never resized keeps the natural height"
        );
        panel.repositories_section_height = Some(px(250.));
        let round_tripped = serde_json::from_str::<SerializedGitPanel>(
            &serde_json::to_string(&panel.serialized_state()).expect("serializes"),
        )
        .expect("deserializes");
        panel.repositories_section_height = None;
        panel.apply_serialized_state(round_tripped, cx);
        assert_eq!(panel.repositories_section_height, Some(px(250.)));
    });

    panel.update_in(cx, |panel, _, cx| {
        let from_an_older_build = serde_json::from_str::<SerializedGitPanel>("{}")
            .expect("a blob without the field still deserializes");
        panel.apply_serialized_state(from_an_older_build, cx);
        assert_eq!(panel.repositories_section_height, None);

        let out_of_range = serde_json::from_str::<SerializedGitPanel>(
            r#"{ "repositories_section_height": 5000.0 }"#,
        )
        .expect("deserializes");
        panel.apply_serialized_state(out_of_range, cx);
        assert_eq!(
            panel.repositories_section_height,
            Some(COMMITS_SECTION_MAX_HEIGHT)
        );
    });
}
