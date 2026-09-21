use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ids::ProjectId;

/// Which checkout on THIS machine stands for which cloud project.
///
/// Local, and it must stay local. It is the only structure in the feature that
/// describes how a user's disk is laid out, and it is worthless to synchronise
/// anyway — the same project lives at a different path on every machine.
///
/// Binding is a manual step: the user picks the project in the vault. That is
/// what removes the need to derive an identifier from a git remote or a path,
/// and it is why the identifiers can be plain random bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bindings {
    #[serde(default)]
    worktrees: BTreeMap<String, ProjectId>,
}

impl Bindings {
    /// Loads the file, treating anything unreadable as "nothing is bound".
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&raw) {
            Ok(bindings) => bindings,
            Err(error) => {
                log::warn!("env_bindings.json could not be read, starting fresh: {error}");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = serde_json::to_vec_pretty(self)?;
        crate::secret_file::write_secret_atomic(path, &raw)?;
        Ok(())
    }

    pub fn project_for(&self, worktree_root: &Path) -> Option<ProjectId> {
        self.worktrees.get(&key(worktree_root)).copied()
    }

    /// Every checkout bound to a project, so the vault can show where each one
    /// would land on this machine.
    pub fn worktrees_for(&self, project: ProjectId) -> Vec<PathBuf> {
        self.worktrees
            .iter()
            .filter(|(_, bound)| **bound == project)
            .map(|(path, _)| PathBuf::from(path))
            .collect()
    }

    pub fn bind(&mut self, worktree_root: &Path, project: ProjectId) {
        self.worktrees.insert(key(worktree_root), project);
    }

    pub fn unbind(&mut self, worktree_root: &Path) {
        self.worktrees.remove(&key(worktree_root));
    }

    pub fn is_empty(&self) -> bool {
        self.worktrees.is_empty()
    }
}

/// The map key for a checkout.
///
/// A trailing separator is stripped so `/work/acme` and `/work/acme/` are one
/// binding rather than two. Nothing further is normalised — resolving symlinks
/// or case would mean touching the filesystem from a pure data structure, and
/// guessing at it in string-space is how two spellings of one path end up
/// bound to different projects.
fn key(worktree_root: &Path) -> String {
    let rendered = worktree_root.to_string_lossy();
    let trimmed = rendered.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        rendered.into_owned()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zode-env-bindings-{name}-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn project(byte: u8) -> ProjectId {
        ProjectId::parse(&format!("{byte:02x}").repeat(16)).expect("a valid fixture id")
    }

    #[test]
    fn round_trips_through_the_file() {
        let path = temp("roundtrip");
        let mut bindings = Bindings::default();
        bindings.bind(Path::new("/work/acme"), project(0x11));
        bindings.save(&path).unwrap();

        let loaded = Bindings::load(&path);
        assert_eq!(
            loaded.project_for(Path::new("/work/acme")),
            Some(project(0x11))
        );
        assert_eq!(loaded.project_for(Path::new("/work/other")), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_trailing_separator_is_the_same_checkout() {
        let mut bindings = Bindings::default();
        bindings.bind(Path::new("/work/acme/"), project(0x11));
        assert_eq!(
            bindings.project_for(Path::new("/work/acme")),
            Some(project(0x11))
        );
    }

    #[test]
    fn several_checkouts_can_share_one_project() {
        // A worktree per branch is ordinary, and they all want the same `.env`.
        let mut bindings = Bindings::default();
        bindings.bind(Path::new("/work/acme"), project(0x11));
        bindings.bind(Path::new("/work/acme-hotfix"), project(0x11));
        bindings.bind(Path::new("/work/other"), project(0x22));

        let mut found = bindings.worktrees_for(project(0x11));
        found.sort();
        assert_eq!(
            found,
            vec![
                PathBuf::from("/work/acme"),
                PathBuf::from("/work/acme-hotfix")
            ]
        );
    }

    #[test]
    fn unbinding_removes_only_that_checkout() {
        let mut bindings = Bindings::default();
        bindings.bind(Path::new("/work/acme"), project(0x11));
        bindings.bind(Path::new("/work/other"), project(0x11));
        bindings.unbind(Path::new("/work/acme"));

        assert_eq!(bindings.project_for(Path::new("/work/acme")), None);
        assert_eq!(
            bindings.project_for(Path::new("/work/other")),
            Some(project(0x11))
        );
    }

    #[test]
    fn an_absent_file_means_nothing_is_bound() {
        assert!(Bindings::load(&temp("absent")).is_empty());
    }

    #[test]
    fn the_file_is_not_world_readable() {
        let path = temp("mode");
        Bindings::default().save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(&path).unwrap();
    }
}
