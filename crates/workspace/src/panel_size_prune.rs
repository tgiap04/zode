use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use db::kvp::KeyValueStore;
use fs::{Fs, Metadata};
use gpui::{App, AppContext as _, Task};

use crate::panel_size_key::{PROJECT_PANEL_SIZE_STATE_KEY, local_project_path_from_key};

/// Drops the dock sizes of local projects that are no longer on disk.
///
/// One row per project and panel accumulates, so the namespace grows with the
/// number of projects ever opened -- slowly, and by some eighty bytes a time,
/// but with no stated end. This is the stated end. A project whose directory
/// has been renamed, moved or deleted loses its widths, the same way it loses
/// its `workspaces` row, and re-seeds from the shared record if it ever returns.
///
/// Remote rows are never considered: `local_project_path_from_key` does not
/// answer for them, because nothing here can reach their host to ask.
pub fn prune_orphaned_panel_sizes(fs: Arc<dyn Fs>, cx: &App) -> Task<Result<()>> {
    let kvp = KeyValueStore::global(cx);
    cx.background_spawn(async move {
        let scope = kvp.scoped(PROJECT_PANEL_SIZE_STATE_KEY);
        let keys = scope.keys()?;

        let project_paths: HashSet<&str> = keys
            .iter()
            .filter_map(|key| local_project_path_from_key(key))
            .collect();

        let fs = &fs;
        let absent: HashSet<&str> =
            futures::future::join_all(project_paths.iter().map(|project_path| async move {
                let metadata = fs.metadata(Path::new(*project_path)).await;
                (*project_path, is_absent(metadata))
            }))
            .await
            .into_iter()
            .filter_map(|(project_path, absent)| absent.then_some(project_path))
            .collect();

        if absent.is_empty() {
            return Ok(());
        }

        let orphaned: Vec<String> = keys
            .iter()
            .filter(|key| {
                local_project_path_from_key(key).is_some_and(|path| absent.contains(path))
            })
            .cloned()
            .collect();

        log::debug!(
            "dropping {} dock size record(s) from {} project(s) no longer on disk",
            orphaned.len(),
            absent.len()
        );

        for key in orphaned {
            scope.delete(key).await?;
        }

        Ok(())
    })
}

/// Whether a project directory's absence has actually been established.
///
/// Only a clean "not there" counts. An unmounted volume or a disconnected share
/// answers with an error, which is not evidence of absence -- and the price of
/// reading it as one is a width the user still wanted, thrown away while they
/// were simply off the network.
fn is_absent(metadata: Result<Option<Metadata>>) -> bool {
    matches!(metadata, Ok(None))
}

#[cfg(test)]
mod tests {
    use fs::{FakeFs, MTime};
    use gpui::TestAppContext;
    use serde_json::json;

    use super::*;
    use crate::panel_size_key::panel_size_key;
    use remote::RemoteConnectionIdentity;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    async fn write_record(namespace: &'static str, key: String, cx: &mut TestAppContext) {
        cx.update(|cx| {
            let kvp = KeyValueStore::global(cx);
            cx.background_spawn(async move { kvp.scoped(namespace).write(key, "{}".into()).await })
        })
        .await
        .expect("writing a record should succeed");
    }

    fn keys_in(namespace: &'static str, cx: &mut TestAppContext) -> Vec<String> {
        cx.update(|cx| {
            let mut keys = KeyValueStore::global(cx)
                .scoped(namespace)
                .keys()
                .expect("listing keys should succeed");
            keys.sort();
            keys
        })
    }

    #[gpui::test]
    async fn a_project_that_is_gone_loses_its_rows_and_one_that_stays_keeps_them(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree("/present", json!({ "src": { "main.rs": "" } }))
            .await;

        let present = panel_size_key("ProjectPanel", None, Path::new("/present")).unwrap();
        let gone = panel_size_key("ProjectPanel", None, Path::new("/gone")).unwrap();
        write_record(PROJECT_PANEL_SIZE_STATE_KEY, present.clone(), cx).await;
        write_record(PROJECT_PANEL_SIZE_STATE_KEY, gone, cx).await;

        cx.update(|cx| prune_orphaned_panel_sizes(fs.clone(), cx))
            .await
            .expect("the prune should succeed");

        assert_eq!(keys_in(PROJECT_PANEL_SIZE_STATE_KEY, cx), vec![present]);
    }

    #[gpui::test]
    async fn the_older_namespace_is_never_touched(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());

        // The same shape of key, in the namespace that predates per-project
        // records. Nothing in that namespace names a path, so nothing there may
        // be judged by whether a path exists.
        let legacy = panel_size_key("ProjectPanel", None, Path::new("/gone")).unwrap();
        write_record(crate::dock::PANEL_SIZE_STATE_KEY, legacy.clone(), cx).await;
        write_record(PROJECT_PANEL_SIZE_STATE_KEY, legacy.clone(), cx).await;

        cx.update(|cx| prune_orphaned_panel_sizes(fs.clone(), cx))
            .await
            .expect("the prune should succeed");

        assert!(keys_in(PROJECT_PANEL_SIZE_STATE_KEY, cx).is_empty());
        assert_eq!(keys_in(crate::dock::PANEL_SIZE_STATE_KEY, cx), vec![legacy]);
    }

    #[gpui::test]
    async fn a_remote_row_survives_however_unreachable_its_host(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());

        let host = RemoteConnectionIdentity::Ssh {
            host: "example.com".to_string(),
            username: None,
            port: None,
        };
        let remote = panel_size_key("ProjectPanel", Some(&host), Path::new("/srv/app")).unwrap();
        write_record(PROJECT_PANEL_SIZE_STATE_KEY, remote.clone(), cx).await;

        cx.update(|cx| prune_orphaned_panel_sizes(fs.clone(), cx))
            .await
            .expect("the prune should succeed");

        assert_eq!(keys_in(PROJECT_PANEL_SIZE_STATE_KEY, cx), vec![remote]);
    }

    #[gpui::test]
    async fn a_key_that_does_not_parse_is_left_alone(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());

        write_record(PROJECT_PANEL_SIZE_STATE_KEY, "no-colon-at-all".into(), cx).await;

        cx.update(|cx| prune_orphaned_panel_sizes(fs.clone(), cx))
            .await
            .expect("the prune should neither panic nor fail on a malformed key");

        assert_eq!(
            keys_in(PROJECT_PANEL_SIZE_STATE_KEY, cx),
            vec!["no-colon-at-all".to_string()]
        );
    }

    #[test]
    fn only_a_clean_absence_prunes() {
        let metadata = Metadata {
            inode: 0,
            mtime: MTime::from_seconds_and_nanos(0, 0),
            is_symlink: false,
            is_dir: true,
            len: 0,
            is_fifo: false,
            is_executable: false,
        };

        assert!(is_absent(Ok(None)));
        assert!(!is_absent(Ok(Some(metadata))));
        assert!(
            !is_absent(Err(anyhow::anyhow!("the volume is not mounted"))),
            "an unreadable path keeps its record -- an error is not evidence \
             that the project is gone"
        );
    }
}
