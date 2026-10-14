//! Where a downloaded driver lives on disk.
//!
//! Pure path arithmetic and the filesystem calls that go with it. Nothing here
//! knows about HTTP, manifests, or versions being *correct* -- it knows only
//! the shape:
//!
//! ```text
//! <root>/<driver id>/<app version>/<executable>
//! ```
//!
//! A directory per version rather than one binary per driver, because the
//! protocol a driver speaks is pinned to the app that shipped it. An app that
//! has been updated must not keep running the driver that happened to be on
//! disk already -- so the version is part of the path, and a miss is a miss.

use anyhow::Result;
use std::path::{Path, PathBuf};

/// The directory holding every downloaded driver.
pub fn root() -> &'static Path {
    paths::database_drivers_dir().as_path()
}

/// What the driver's binary is called.
///
/// The same name the bundle scripts build and the release publishes, so one
/// spelling serves the beside-the-executable lookup and the store alike.
pub fn executable_name(id: &str) -> String {
    if cfg!(windows) {
        format!("zode-db-{id}.exe")
    } else {
        format!("zode-db-{id}")
    }
}

/// Where a given version of a driver belongs, whether or not it is there.
pub fn version_dir_in(root: &Path, id: &str, version: &str) -> PathBuf {
    root.join(id).join(version)
}

pub fn version_dir(id: &str, version: &str) -> PathBuf {
    version_dir_in(root(), id, version)
}

/// The driver's executable, or `None` when it is not installed.
///
/// `None` rather than the path-it-would-have: handing back a path that is not
/// there is precisely the defect this module exists to end. A path that does
/// not exist gets spawned through a shell, the shell says `command not found`
/// on what the client reads as the driver's stderr, and the one fact worth
/// knowing -- it was never installed -- appears nowhere.
pub fn installed_path_in(root: &Path, id: &str, version: &str) -> Option<PathBuf> {
    let path = version_dir_in(root, id, version).join(executable_name(id));
    path.is_file().then_some(path)
}

pub fn installed_path(id: &str, version: &str) -> Option<PathBuf> {
    installed_path_in(root(), id, version)
}

/// Every version of one driver currently on disk, newest-unaware.
///
/// Unsorted and unparsed: the only caller is the pruner, which compares against
/// one known-good version rather than deciding which of two is newer. Ordering
/// version strings is a job with no correct answer here -- the app's own
/// version is the only one that matters.
pub fn installed_versions_in(root: &Path, id: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(id)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

/// Drops every version of a driver except the one just installed.
///
/// Best-effort per directory: a version that will not delete (a driver still
/// running from it, most likely) must not fail the install that has already
/// succeeded. It is stale disk, not a broken state.
pub fn prune_other_versions_in(root: &Path, id: &str, keep: &str) -> Result<()> {
    for version in installed_versions_in(root, id) {
        if version == keep {
            continue;
        }
        let dir = version_dir_in(root, id, &version);
        if let Err(error) = std::fs::remove_dir_all(&dir) {
            log::warn!(
                "could not remove the old `{id}` driver at {}: {error}",
                dir.display()
            );
        }
    }
    Ok(())
}

pub fn prune_other_versions(id: &str, keep: &str) -> Result<()> {
    prune_other_versions_in(root(), id, keep)
}

/// Deletes every downloaded version of a driver, for someone who wants the
/// disk back. Returns whether there was anything to delete.
///
/// The whole `<root>/<id>` directory, not only the running app's version:
/// what is being asked for is "this driver is gone", and a version an older
/// app left behind is as much in the way as the current one. The id is refused
/// unless it is a single plain name, so no id can reach outside the store.
pub fn remove_in(root: &Path, id: &str) -> Result<bool> {
    use anyhow::Context as _;
    use std::path::Component;
    // Exactly one ordinary component, judged by the platform's own parser: on
    // Windows `C:` is a prefix, not a name, and joining it would leave `root`.
    // A backslash is refused everywhere, so an id means the same on every
    // platform rather than being a name on one and a path on another.
    let mut components = Path::new(id).components();
    let plain_name = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(name)), None) if name == std::ffi::OsStr::new(id)
    ) && !id.contains('\\');
    anyhow::ensure!(plain_name, "{id:?} is not a driver id");
    let dir = root.join(id);
    // `symlink_metadata` rather than `exists`: a dangling link here is still
    // something to remove, and an unreadable entry is an error, not "absent".
    match std::fs::symlink_metadata(&dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", dir.display()));
        }
        Ok(_) => {}
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
    Ok(true)
}

/// Makes a freshly unpacked driver executable.
///
/// A no-op on Windows, where being executable is a property of the name.
pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        // Imported here rather than at the top of the file: it is reached only
        // on unix, and an import that is unused on Windows fails a clippy run
        // with warnings denied.
        use anyhow::Context as _;
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(path)
            .with_context(|| format!("reading permissions of {}", path.display()))?
            .permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        std::fs::set_permissions(path, permissions)
            .with_context(|| format!("making {} executable", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(root: &Path, id: &str, version: &str) -> PathBuf {
        let dir = version_dir_in(root, id, version);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(executable_name(id));
        std::fs::write(&path, b"not really a driver").unwrap();
        path
    }

    /// The whole contract: a driver that is not there reports nothing, rather
    /// than a path that will fail somewhere less obvious.
    #[test]
    fn an_absent_driver_has_no_path() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(installed_path_in(root.path(), "postgres", "0.1.1"), None);
    }

    #[test]
    fn a_planted_driver_is_found_at_its_version() {
        let root = tempfile::tempdir().unwrap();
        let planted = plant(root.path(), "postgres", "0.1.1");
        assert_eq!(
            installed_path_in(root.path(), "postgres", "0.1.1"),
            Some(planted)
        );
    }

    /// The reason the version is in the path at all: an app that has been
    /// updated must not silently run the driver the previous version left.
    #[test]
    fn a_driver_installed_for_another_version_does_not_answer() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "postgres", "0.1.0");
        assert_eq!(installed_path_in(root.path(), "postgres", "0.1.1"), None);
    }

    /// A directory with no executable in it is not an install -- an unpack that
    /// died halfway must not read as success.
    #[test]
    fn an_empty_version_directory_is_not_an_install() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(version_dir_in(root.path(), "mysql", "0.1.1")).unwrap();
        assert_eq!(installed_path_in(root.path(), "mysql", "0.1.1"), None);
    }

    #[test]
    fn pruning_keeps_the_version_it_was_told_to() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "mysql", "0.1.0");
        plant(root.path(), "mysql", "0.1.1");
        plant(root.path(), "postgres", "0.1.0");

        prune_other_versions_in(root.path(), "mysql", "0.1.1").unwrap();

        assert!(installed_path_in(root.path(), "mysql", "0.1.1").is_some());
        assert_eq!(installed_path_in(root.path(), "mysql", "0.1.0"), None);
        assert!(
            installed_path_in(root.path(), "postgres", "0.1.0").is_some(),
            "pruning one driver must not touch another"
        );
    }

    #[test]
    fn removing_a_driver_takes_every_version_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "mysql", "0.1.0");
        plant(root.path(), "mysql", "0.1.1");
        plant(root.path(), "postgres", "0.1.1");

        assert!(remove_in(root.path(), "mysql").unwrap());

        assert!(installed_versions_in(root.path(), "mysql").is_empty());
        assert!(
            installed_path_in(root.path(), "postgres", "0.1.1").is_some(),
            "removing one driver must not touch another"
        );
    }

    #[test]
    fn removing_a_driver_that_is_not_there_says_so() {
        let root = tempfile::tempdir().unwrap();
        assert!(!remove_in(root.path(), "mongodb").unwrap());
    }

    /// The id becomes a path under the store, so one that is not a plain name
    /// could name something that is not a driver at all.
    #[test]
    fn an_id_that_is_not_a_plain_name_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("store");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(root.path().join("keep.txt"), b"keep").unwrap();
        for id in ["", ".", "..", "../keep.txt", "a/b", "a\\b"] {
            assert!(remove_in(&inner, id).is_err(), "{id:?} must be refused");
        }
        assert!(root.path().join("keep.txt").exists());
    }

    /// A link left where a driver was is removed, not reported as absent and
    /// left behind -- and only the link goes, never what it points at.
    #[cfg(unix)]
    #[test]
    fn a_dangling_or_live_link_in_the_store_is_removed_without_following_it() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("keep.txt"), b"keep").unwrap();

        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("postgres")).unwrap();
        std::os::unix::fs::symlink(root.path().join("gone"), root.path().join("mysql")).unwrap();

        assert!(remove_in(root.path(), "postgres").unwrap());
        assert!(remove_in(root.path(), "mysql").unwrap());
        assert!(std::fs::symlink_metadata(root.path().join("postgres")).is_err());
        assert!(std::fs::symlink_metadata(root.path().join("mysql")).is_err());
        assert!(
            elsewhere.path().join("keep.txt").exists(),
            "the link's target is not the store's to delete"
        );
    }

    /// On Windows a drive prefix is not a name, and joining one replaces the
    /// store root entirely.
    #[cfg(windows)]
    #[test]
    fn a_drive_prefix_is_not_a_driver_id() {
        let root = tempfile::tempdir().unwrap();
        for id in ["C:", "C:x", "\\\\server\\share"] {
            assert!(
                remove_in(root.path(), id).is_err(),
                "{id:?} must be refused"
            );
        }
    }

    /// Called on every install, including the first.
    #[test]
    fn pruning_a_driver_with_nothing_installed_is_not_an_error() {
        let root = tempfile::tempdir().unwrap();
        prune_other_versions_in(root.path(), "mongodb", "0.1.1").unwrap();
    }
}
