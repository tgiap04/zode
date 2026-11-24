//! The worktree trust check in front of everything Run app reads or runs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use devices::{AppProject, HostSystem};
use gpui::{App, Context, SharedString};
use project::WorktreeId;
use project::trusted_worktrees::TrustedWorktrees;

use crate::devices_panel::DevicesPanel;
use crate::run_app::NOT_LOCAL;

#[derive(Default)]
pub(crate) struct AppRoots {
    pub trusted: Vec<PathBuf>,
    /// Never read, even when inside a trusted folder.
    pub untrusted: Vec<PathBuf>,
}

impl AppRoots {
    /// Every app in the trusted folders, once: a folder and a folder inside
    /// it may both be open.
    pub(crate) fn detect(&self, host: &dyn HostSystem) -> Vec<AppProject> {
        let mut apps: Vec<AppProject> = Vec::new();
        for root in &self.trusted {
            for app in devices::detect_app_projects_excluding(host, root, &self.untrusted) {
                if !apps.iter().any(|known| known.directory == app.directory) {
                    apps.push(app);
                }
            }
        }
        apps
    }
}

impl DevicesPanel {
    /// The open folders split by trust, for finding apps; with no trust store
    /// nothing is read. Only the store's restricted list is consulted:
    /// `can_trust` would restrict a folder not decided yet and bring up
    /// restricted mode just because the panel opened. Every local folder is
    /// decided as soon as it loads, so an undecided one is only ever read for
    /// its project files, which runs nothing; a Run click still goes through
    /// [`Self::ensure_trusted`].
    pub(crate) fn app_roots(&self, cx: &App) -> AppRoots {
        let mut roots = AppRoots::default();
        let Some(project) = self.project.upgrade() else {
            return roots;
        };
        let project = project.read(cx);
        if !project.is_local() {
            return roots;
        }
        let worktree_store = project.worktree_store();
        let folders: Vec<(WorktreeId, PathBuf)> = project
            .visible_worktrees(cx)
            .map(|worktree| {
                let worktree = worktree.read(cx);
                (worktree.id(), worktree.abs_path().to_path_buf())
            })
            .collect();
        let Some(store) = TrustedWorktrees::try_get_global(cx) else {
            roots.untrusted = folders.into_iter().map(|(_, root)| root).collect();
            return roots;
        };
        let restricted: HashSet<WorktreeId> = store
            .read(cx)
            .restricted_worktrees(&worktree_store, cx)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        for (id, root) in folders {
            if !restricted.contains(&id) {
                roots.trusted.push(root);
            } else {
                roots.untrusted.push(root);
            }
        }
        roots
    }

    /// Checks every open folder that holds `directory`, on each click, and
    /// returns the folder the app's files are read from.
    pub(crate) fn ensure_trusted(
        &self,
        directory: &Path,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf, SharedString> {
        // Without the store there is no way to tell, so nothing runs.
        let store = TrustedWorktrees::try_get_global(cx)
            .ok_or("Run app is off: Zode cannot check whether this folder is trusted.")?;
        let project = self.project.upgrade().ok_or(NOT_LOCAL)?;
        let project = project.read(cx);
        if !project.is_local() {
            return Err(NOT_LOCAL.into());
        }
        let worktree_store = project.worktree_store();
        // Nested open folders all hold the app; each of them must be trusted.
        let holders: Vec<(WorktreeId, PathBuf)> = project
            .visible_worktrees(cx)
            .filter_map(|worktree| {
                let worktree = worktree.read(cx);
                directory
                    .starts_with(worktree.abs_path())
                    .then(|| (worktree.id(), worktree.abs_path().to_path_buf()))
            })
            .collect();
        if holders.is_empty() {
            return Err("The app is not inside an open folder.".into());
        }
        for (id, _) in &holders {
            let trusted = store.update(cx, |store, cx| store.can_trust(&worktree_store, *id, cx));
            if !trusted {
                return Err("Trust this folder before running its app: a run runs the \
                            project's own scripts."
                    .into());
            }
        }
        holders
            .into_iter()
            .map(|(_, root)| root)
            .max_by_key(|root| root.components().count())
            .ok_or_else(|| "The app is not inside an open folder.".into())
    }
}
