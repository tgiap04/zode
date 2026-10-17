//! Where a downloaded driver lives on disk.
//!
//! Pure path arithmetic and the filesystem calls that go with it. Nothing here
//! knows about HTTP or manifests -- it knows only the shape:
//!
//! ```text
//! <root>/<driver id>/<published version>/<executable>
//! ```
//!
//! The version in the path is the published driver set's, not the app's. One
//! directory per version means an install never overwrites an executable that
//! may be running: the new version is unpacked beside it, and the old one is
//! pruned afterwards. A driver of any version resolves, and a driver that has
//! outlived the protocol this build speaks is refused where that is actually
//! checked, `DriverClient::initialize`, rather than guessed at from a name.

use anyhow::Result;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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

/// Whether `name` has the shape of a published version: `MAJOR.MINOR.PATCH`,
/// optionally followed by `-` and dot-separated alphanumeric identifiers.
///
/// Mirrors the pattern the server accepts when a version is published. That is
/// what makes it safe to build a URL or a directory from a name that came from
/// the network.
///
/// It is also what keeps scratch space from resolving as an install. An unpack
/// stages into a directory that holds a complete-looking executable before the
/// rename that publishes it, so that directory must never read as a version.
/// Appending to the version cannot guarantee that: the server accepts any
/// alphanumeric pre-release identifier, so `0.1.5-rc.1.incoming` is itself a
/// version. [`hidden_sibling`] begins every scratch name with a dot instead,
/// and a version begins with a digit.
pub fn is_version(name: &str) -> bool {
    let (core, prerelease) = match name.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (name, None),
    };
    let numeric = core.split('.').count() == 3
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    numeric
        && prerelease.is_none_or(|identifiers| {
            identifiers.split('.').all(|identifier| {
                !identifier.is_empty()
                    && identifier.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
}

/// A path beside `path`, named `.<name>.<suffix>`: the scratch space for
/// something that will end up at `path`.
///
/// Beside it rather than in the system temp directory, so the move into place
/// is a rename within one filesystem rather than a copy. Leading with a dot is
/// what stops it being read as a version, however the real name ends; see
/// [`is_version`].
pub(crate) fn hidden_sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

/// Treats a path that was already gone as a successful removal.
///
/// For the removals whose point is that nothing should be there afterwards:
/// that it was never there is the ordinary case, not a failure to report.
pub(crate) fn ignore_not_found(removal: std::io::Result<()>) -> std::io::Result<()> {
    match removal {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The driver's executable, or `None` when it is not installed.
///
/// Whichever version it was installed under. When more than one is on disk --
/// only possible when a prune failed, because a driver was still running from
/// the old directory -- the most recently modified version directory wins, and
/// the greater name settles a tie. Neither the version order nor the name order
/// would do: a rollback can install a lower version than the one it replaced,
/// and the executable's own timestamp is the archive's build time rather than
/// the moment it was unpacked.
///
/// `None` rather than the path-it-would-have: handing back a path that is not
/// there is precisely the defect this module exists to end. A path that does
/// not exist gets spawned through a shell, the shell says `command not found`
/// on what the client reads as the driver's stderr, and the one fact worth
/// knowing -- it was never installed -- appears nowhere.
pub fn installed_path_in(root: &Path, id: &str) -> Option<PathBuf> {
    let executable = executable_name(id);
    std::fs::read_dir(root.join(id))
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = entry.file_name().into_string().ok()?;
            if !is_version(&version) {
                return None;
            }
            let directory = entry.path();
            let path = directory.join(&executable);
            if !path.is_file() {
                return None;
            }
            let modified = std::fs::metadata(&directory)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, version, path))
        })
        .max_by(
            |(modified, version, _), (other_modified, other_version, _)| {
                modified
                    .cmp(other_modified)
                    .then_with(|| version.cmp(other_version))
            },
        )
        .map(|(_, _, path)| path)
}

pub fn installed_path(id: &str) -> Option<PathBuf> {
    installed_path_in(root(), id)
}

/// Every directory under one driver's id, whatever it is called.
///
/// Unsorted and unfiltered: the only caller is the pruner, which compares
/// against one known-good version, and which is better off also clearing away a
/// `.<version>.incoming` staging directory an interrupted install left than
/// leaving it.
/// Which of several versions is current is `installed_path_in`'s question.
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
/// The whole `<root>/<id>` directory, every version: what is being asked for
/// is "this driver is gone", and a version left behind by an earlier install
/// is as much in the way as the newest one. The id is refused unless it is a
/// single plain name, so no id can reach outside the store.
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
    use crate::install::unpack;

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
        assert_eq!(installed_path_in(root.path(), "postgres"), None);
    }

    #[test]
    fn a_driver_is_found_whatever_version_it_was_installed_under() {
        let root = tempfile::tempdir().unwrap();
        let planted = plant(root.path(), "postgres", "0.1.0");
        assert_eq!(installed_path_in(root.path(), "postgres"), Some(planted));

        let other = tempfile::tempdir().unwrap();
        let planted = plant(other.path(), "postgres", "7.3.9-rc.2");
        assert_eq!(installed_path_in(other.path(), "postgres"), Some(planted));
    }

    /// A driver is looked up by its own id, so another driver's install never
    /// answers for it.
    #[test]
    fn another_drivers_install_does_not_answer() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "mysql", "0.1.1");
        assert_eq!(installed_path_in(root.path(), "postgres"), None);
    }

    /// A directory with no executable in it is not an install -- an unpack that
    /// died halfway must not read as success.
    #[test]
    fn an_empty_version_directory_is_not_an_install() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(version_dir_in(root.path(), "mysql", "0.1.1")).unwrap();
        assert_eq!(installed_path_in(root.path(), "mysql"), None);
    }

    /// Staging directories sit beside the real ones and hold a complete-looking
    /// executable before the rename that publishes it. Resolving by scanning
    /// must not mistake one for an install, nor a stray directory someone made.
    #[test]
    fn a_directory_that_is_not_a_version_never_resolves() {
        let root = tempfile::tempdir().unwrap();
        for name in [".0.2.0.incoming", "manual", ".hidden"] {
            plant(root.path(), "postgres", name);
        }
        assert_eq!(installed_path_in(root.path(), "postgres"), None);

        let planted = plant(root.path(), "postgres", "0.1.0");
        assert_eq!(
            installed_path_in(root.path(), "postgres"),
            Some(planted),
            "the one real version must win over the directories that only resemble one"
        );
    }

    /// Several versions exist only when a prune failed. A rollback can leave a
    /// lower version as the newest install, so neither name order nor version
    /// order means "the one installed last" -- the directory's own age does.
    #[cfg(unix)]
    #[test]
    fn the_most_recently_installed_version_wins() {
        let root = tempfile::tempdir().unwrap();
        let higher = plant(root.path(), "postgres", "0.2.0");
        let lower = plant(root.path(), "postgres", "0.1.0");
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        set_directory_modified(
            &version_dir_in(root.path(), "postgres", "0.2.0"),
            an_hour_ago,
        );

        assert_eq!(
            installed_path_in(root.path(), "postgres"),
            Some(lower),
            "the more recent install wins even though its version is lower"
        );

        let same_moment = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        for version in ["0.2.0", "0.1.0"] {
            set_directory_modified(
                &version_dir_in(root.path(), "postgres", version),
                same_moment,
            );
        }
        assert_eq!(
            installed_path_in(root.path(), "postgres"),
            Some(higher),
            "equal ages are settled by the greater name, so the answer is stable"
        );
    }

    #[cfg(unix)]
    fn set_directory_modified(directory: &Path, modified: std::time::SystemTime) {
        std::fs::File::open(directory)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    #[test]
    fn what_the_server_calls_a_version_is_one_and_nothing_else_is() {
        for accepted in ["0.1.4", "0.1.5-rc.1", "10.20.30-beta.2", "1.2.3-RC1"] {
            assert!(is_version(accepted), "{accepted:?} must be accepted");
        }
        for refused in [
            "",
            "latest",
            "0.1",
            "0.1.4+stable",
            "0.1.4.incoming",
            ".0.1.4.incoming",
            ".0.1.5-rc.1.incoming",
            ".0.1.5-rc.1.download",
            "../0.1.4",
            "0.1.4/x",
            "1.2.3-rc-1",
            "1.2.3-",
            "1.2.3-rc..1",
            "1..3",
            "v0.1.4",
            "0.1.4 ",
        ] {
            assert!(!is_version(refused), "{refused:?} must be refused");
        }
    }

    #[test]
    fn pruning_keeps_the_version_it_was_told_to() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "mysql", "0.1.0");
        let kept = plant(root.path(), "mysql", "0.1.1");
        plant(root.path(), "postgres", "0.1.0");

        prune_other_versions_in(root.path(), "mysql", "0.1.1").unwrap();

        assert_eq!(installed_versions_in(root.path(), "mysql"), ["0.1.1"]);
        assert_eq!(installed_path_in(root.path(), "mysql"), Some(kept));
        assert!(
            installed_path_in(root.path(), "postgres").is_some(),
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
            installed_path_in(root.path(), "postgres").is_some(),
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

    /// A file where a version directory should be is not an install, however
    /// its name reads.
    #[test]
    fn a_file_named_like_a_version_is_not_an_install() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("postgres")).unwrap();
        std::fs::write(
            root.path().join("postgres").join("0.3.0"),
            b"a file, not a directory",
        )
        .unwrap();
        assert_eq!(installed_path_in(root.path(), "postgres"), None);

        let real = plant(root.path(), "postgres", "0.1.0");
        assert_eq!(installed_path_in(root.path(), "postgres"), Some(real));
    }

    #[test]
    fn a_version_whose_executable_is_a_directory_is_not_an_install() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(
            version_dir_in(root.path(), "postgres", "0.3.0").join(executable_name("postgres")),
        )
        .unwrap();
        assert_eq!(installed_path_in(root.path(), "postgres"), None);
    }

    /// An interrupted install leaves its staging directory holding a
    /// complete-looking executable, and it is the newest thing on disk. It must
    /// not win over the install that is actually there.
    #[cfg(unix)]
    #[test]
    fn a_newer_staging_directory_does_not_outrank_the_real_install() {
        let root = tempfile::tempdir().unwrap();
        let installed = plant(root.path(), "postgres", "0.1.0");
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        set_directory_modified(
            &version_dir_in(root.path(), "postgres", "0.1.0"),
            an_hour_ago,
        );
        plant(root.path(), "postgres", ".0.2.0.incoming");

        assert_eq!(installed_path_in(root.path(), "postgres"), Some(installed));
    }

    /// The server accepts any alphanumeric pre-release identifier, so a staging
    /// name made by appending to the version would itself be a version -- and an
    /// unpacked one would resolve as an install. The staging directory is the one
    /// the unpack really uses, so a change to how it is named is tested here.
    #[test]
    fn a_staging_directory_for_a_prerelease_never_resolves() {
        let root = tempfile::tempdir().unwrap();
        let staging = unpack::staging_dir(&version_dir_in(root.path(), "postgres", "0.1.5-rc.1"));
        let name = staging.file_name().and_then(|name| name.to_str()).unwrap();
        plant(root.path(), "postgres", name);
        assert_eq!(installed_path_in(root.path(), "postgres"), None);
    }

    /// Whatever the version, the scratch name leads with a dot and sits in the
    /// same directory as the thing it will become, so the move stays a rename.
    #[test]
    fn scratch_space_sits_beside_its_destination_and_is_never_a_version() {
        let root = tempfile::tempdir().unwrap();
        for version in ["0.1.4", "0.1.5-rc.1", "10.20.30-beta.2", "1.2.3-RC1"] {
            let destination = version_dir_in(root.path(), "postgres", version);
            for suffix in ["incoming", "download", "1"] {
                let scratch = hidden_sibling(&destination, suffix);
                assert_eq!(scratch.parent(), destination.parent());
                let name = scratch.file_name().and_then(|name| name.to_str()).unwrap();
                assert_eq!(name, format!(".{version}.{suffix}"));
                assert!(!is_version(name), "{name:?} must not read as a version");
            }
        }
    }

    #[test]
    fn a_removal_of_something_already_gone_is_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("never-created");
        ignore_not_found(std::fs::remove_dir_all(&missing)).unwrap();
        ignore_not_found(std::fs::remove_file(&missing)).unwrap();

        let error = ignore_not_found(Err(std::io::ErrorKind::PermissionDenied.into()))
            .expect_err("any other failure is still a failure");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn pruning_after_an_install_clears_staging_leftovers_with_the_old_versions() {
        let root = tempfile::tempdir().unwrap();
        let kept = plant(root.path(), "postgres", "0.1.0");
        plant(root.path(), "postgres", "0.1.1");
        plant(root.path(), "postgres", ".0.2.0.incoming");
        plant(root.path(), "postgres", ".0.1.5-rc.1.incoming");
        plant(root.path(), "postgres", "manual");

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();

        assert_eq!(installed_versions_in(root.path(), "postgres"), ["0.1.0"]);
        assert_eq!(installed_path_in(root.path(), "postgres"), Some(kept));
    }
}
