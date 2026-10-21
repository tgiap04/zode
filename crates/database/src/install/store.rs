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
//!
//! Two things sit in the store that are not drivers: a marker beside each
//! executable Zode installed itself, and one file at the root remembering the
//! newest driver set ever installed. See [`INSTALL_MARKER`] and
//! [`ensure_not_a_rollback_in`].

use anyhow::{Context as _, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Written beside an executable by the install that put it there.
///
/// What tells a driver Zode downloaded from one somebody copied into the store
/// by hand, which is exactly where the install error tells an offline user to
/// put it. The two are removed differently: a downloaded one is a click away
/// from coming back, so it is removed with a plain question, and one that may be
/// hand-placed is removed only after being told it may not come back.
///
/// Drivers downloaded before the marker existed have none, so they read as
/// hand-placed. That errs the safe way: they are still offered for removal, with
/// the warning.
pub(crate) const INSTALL_MARKER: &str = ".zode-installed";

/// The oldest driver set this build of Zode will install.
///
/// A signed manifest never expires, so a server that answers in the real one's
/// place can replay any set ever published. The record of the newest set
/// installed protects a machine that has installed one; this protects the rest --
/// a fresh install, and an upgrade from a version that kept no record -- by
/// refusing everything older than the first set that was ever signed. It is the
/// lowest prerelease of that version, so a release candidate of it is accepted.
///
/// A release can raise it, and should when an older signed set must stop being
/// accepted -- one that carries a flaw, say. A build protects only against what
/// it knew when it was built, which is why this is a constant and not something
/// the server could say.
pub const MINIMUM_DRIVER_SET_VERSION: &str = "0.1.5-0";

/// Remembers the newest driver set this machine has installed.
///
/// Dot-prefixed, so it cannot be mistaken for a driver's directory: an id is
/// never empty and `remove_in` refuses one that is not a plain name.
const NEWEST_INSTALLED_FILE: &str = ".newest-installed-version";

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

/// Scratch space that deletes itself when dropped.
///
/// An install is a future, and the person who started it can walk away from it:
/// dropping it stops it wherever it happens to be, with no code of its own left
/// to run. A partial download or a half-unpacked directory left there would sit
/// beside the install for good, so the cleanup belongs to a destructor and not
/// to the end of the function.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn at(path: PathBuf) -> Self {
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let removal = match std::fs::symlink_metadata(&self.0) {
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(&self.0),
            Ok(_) => std::fs::remove_file(&self.0),
            Err(error) => Err(error),
        };
        if let Err(error) = ignore_not_found(removal) {
            log::warn!(
                "could not remove the scratch file {}: {error}",
                self.0.display()
            );
        }
    }
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

/// Every version directory of a driver that holds its executable, with when it
/// was installed and under which version.
fn installs_in(root: &Path, id: &str) -> Vec<(SystemTime, String, PathBuf)> {
    let executable = executable_name(id);
    let Ok(entries) = std::fs::read_dir(root.join(id)) else {
        return Vec::new();
    };
    entries
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
        .collect()
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
    installs_in(root, id)
        .into_iter()
        .max_by(
            |(modified, version, _), (other_modified, other_version, _)| {
                modified
                    .cmp(other_modified)
                    .then_with(|| version.cmp(other_version))
            },
        )
        .map(|(_, _, path)| path)
}

/// Whether Zode itself installed the driver at `executable`, as opposed to
/// someone placing it there.
///
/// Read without following a link, so a marker that is a symlink to something
/// else does not count.
pub fn installed_by_zode(executable: &Path) -> bool {
    executable.parent().is_some_and(|directory| {
        std::fs::symlink_metadata(directory.join(INSTALL_MARKER))
            .is_ok_and(|metadata| metadata.file_type().is_file())
    })
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

/// How old a leftover has to be before the sweep that follows an install takes
/// it. An install's scratch files are rewritten as it goes, so anything this
/// old belongs to one that died; anything younger may be a live install's.
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Drops the versions of a driver other than the one just installed, and
/// whatever an install that was abandoned left beside them -- a half-unpacked
/// staging directory, a partial download.
///
/// Other versions go at once. Anything else goes only once it is [`STALE_AFTER`]
/// old: this runs while another install of the same driver may be midway, in
/// this process or another, and pulling its scratch files out from under it
/// would fail an install that was going to succeed.
///
/// Best-effort per entry: one that will not delete (a driver still running from
/// it, most likely) must not fail the install that has already succeeded. It is
/// stale disk, not a broken state. Links are removed, never followed.
pub fn prune_other_versions_in(root: &Path, id: &str, keep: &str) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(root.join(id)) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == keep {
            continue;
        }
        let is_a_version = name.to_str().is_some_and(is_version);
        if !is_a_version && !is_stale(&entry) {
            continue;
        }
        let path = entry.path();
        let removal = match entry.file_type() {
            Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
            _ => std::fs::remove_file(&path),
        };
        if let Err(error) = removal {
            log::warn!(
                "could not remove the old `{id}` driver entry at {}: {error}",
                path.display()
            );
        }
    }
    Ok(())
}

/// Left alone when its age cannot be told: deleting on a guess is worse than
/// leaving a file for the next sweep.
fn is_stale(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= STALE_AFTER)
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
///
/// Takes a driver whoever put it there. Whether that is Zode's to do is for the
/// caller to have asked about: this directory also holds drivers downloaded by
/// an earlier Zode, which have no marker, and one stray unmarked version must
/// not leave the rest impossible to remove.
pub fn remove_in(root: &Path, id: &str) -> Result<bool> {
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

/// Refuses a driver set older than `minimum`, or older than the newest one this
/// machine has installed.
///
/// A signature proves who published a manifest, not when. Every manifest ever
/// published stays signed, so without this anyone able to answer in the
/// server's place could hand back an older, vulnerable set that still verifies.
/// The newest version ever installed is therefore kept, and a set below it is
/// refused; `minimum` ([`MINIMUM_DRIVER_SET_VERSION`] in a real build) covers a
/// machine with no record yet. Equal is allowed: installing the same set again
/// is how a driver that was removed comes back.
///
/// There is deliberately no way to say "go back" to a machine that has already
/// moved on, short of publishing a newer set: a rollback the machine could be
/// talked into is the attack this exists to stop.
pub fn ensure_not_a_rollback_in(root: &Path, version: &str, minimum: &str) -> Result<()> {
    let offered = parse_version(version)?;
    let minimum = parse_version(minimum)?;
    anyhow::ensure!(
        offered >= minimum,
        "the published driver set is version {offered}, older than {minimum}, the oldest this \
         build of Zode accepts; wait for a newer set to be published."
    );
    let Some(newest) = newest_installed_in(root)? else {
        return Ok(());
    };
    anyhow::ensure!(
        offered >= newest,
        "the published driver set is version {offered}, but a newer one, {newest}, was installed \
         on this machine before. Zode does not go back to an older set; wait for a newer set to \
         be published."
    );
    Ok(())
}

/// Remembers `version` as installed, if nothing newer already is.
///
/// Written to a sibling, flushed to disk and then renamed, so a crash leaves
/// the old value or the new one and never half of either: a rename can reach
/// the disk before the data it renames, and an empty record would be worse than
/// none.
pub fn record_install_in(root: &Path, version: &str) -> Result<()> {
    use std::io::Write as _;

    let installed = parse_version(version)?;
    if newest_installed_in(root)?.is_some_and(|newest| newest >= installed) {
        return Ok(());
    }
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    let path = root.join(NEWEST_INSTALLED_FILE);
    let scratch = hidden_sibling(&path, &std::process::id().to_string());
    let mut file = std::fs::File::create(&scratch)
        .with_context(|| format!("creating {}", scratch.display()))?;
    file.write_all(version.as_bytes())
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", scratch.display()))?;
    drop(file);
    std::fs::rename(&scratch, &path).with_context(|| format!("replacing {}", path.display()))
}

/// What the record says, or `None` when it says nothing usable.
///
/// A record that is empty or not a version is treated as absent and logged, not
/// refused on: it is only a floor above the built-in one, a file that cannot be
/// read would otherwise wedge every install on the machine, and the next
/// install writes a good one over it.
fn newest_installed_in(root: &Path) -> Result<Option<semver::Version>> {
    let path = root.join(NEWEST_INSTALLED_FILE);
    let recorded = match std::fs::read_to_string(&path) {
        Ok(recorded) => recorded,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    match parse_version(recorded.trim()) {
        Ok(version) => Ok(Some(version)),
        Err(error) => {
            log::warn!(
                "ignoring {}, which does not hold a version: {error:#}",
                path.display()
            );
            Ok(None)
        }
    }
}

/// Ordered by semver, so `0.1.5-rc.1` is older than `0.1.5`.
///
/// A version the server would accept but semver cannot order -- a leading zero,
/// say -- is refused rather than guessed at: an order that is guessed is not a
/// guard.
fn parse_version(version: &str) -> Result<semver::Version> {
    semver::Version::parse(version)
        .with_context(|| format!("{version:?} is not a version that can be ordered"))
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

    /// What an install leaves: the driver, and the marker saying Zode put it
    /// there.
    fn plant_installed(root: &Path, id: &str, version: &str) -> PathBuf {
        let path = plant(root, id, version);
        std::fs::write(path.with_file_name(INSTALL_MARKER), b"installed by Zode\n").unwrap();
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
        plant_installed(root.path(), "mysql", "0.1.0");
        plant_installed(root.path(), "mysql", "0.1.1");
        plant_installed(root.path(), "postgres", "0.1.1");

        assert!(remove_in(root.path(), "mysql").unwrap());

        assert!(installed_versions_in(root.path(), "mysql").is_empty());
        assert!(
            installed_path_in(root.path(), "postgres").is_some(),
            "removing one driver must not touch another"
        );
    }

    /// Drivers downloaded by an earlier Zode have no marker, and read as placed
    /// by hand. The dialog asks before removing one; what is asked of the store
    /// is only that it does.
    #[test]
    fn a_driver_with_no_marker_can_be_removed() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "mysql", "0.1.1");

        assert!(remove_in(root.path(), "mysql").unwrap());

        assert_eq!(installed_path_in(root.path(), "mysql"), None);
        assert!(!root.path().join("mysql").exists());
    }

    /// One version Zode installed and one that has no marker: removing the
    /// driver takes the directory they share, and the stray one must not make it
    /// impossible.
    #[test]
    fn one_unmarked_version_beside_an_install_does_not_block_the_removal() {
        let root = tempfile::tempdir().unwrap();
        plant_installed(root.path(), "mysql", "0.1.1");
        plant(root.path(), "mysql", "0.1.0");

        assert!(remove_in(root.path(), "mysql").unwrap());

        assert!(!root.path().join("mysql").exists());
    }

    #[test]
    fn the_marker_says_who_installed_a_driver() {
        let root = tempfile::tempdir().unwrap();
        let ours = plant_installed(root.path(), "mysql", "0.1.1");
        let theirs = plant(root.path(), "postgres", "0.1.1");
        assert!(installed_by_zode(&ours));
        assert!(!installed_by_zode(&theirs));
        assert!(!installed_by_zode(&root.path().join("nowhere/zode-db-x")));
    }

    /// A marker that is a link says nothing: the thing it points at may be
    /// anywhere, and may have been put there by anyone.
    #[cfg(unix)]
    #[test]
    fn a_marker_that_is_a_link_does_not_count() {
        let root = tempfile::tempdir().unwrap();
        let theirs = plant(root.path(), "postgres", "0.1.1");
        let real = root.path().join("real-marker");
        std::fs::write(&real, b"x").unwrap();
        std::os::unix::fs::symlink(&real, theirs.with_file_name(INSTALL_MARKER)).unwrap();
        assert!(!installed_by_zode(&theirs));
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
        let staging = hidden_sibling(
            &version_dir_in(root.path(), "postgres", "0.1.5-rc.1"),
            "1234-0.incoming",
        );
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

    fn entries_of(root: &Path, id: &str) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(root.join(id))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    fn made_old(path: &Path) {
        std::fs::File::open(path)
            .unwrap()
            .set_modified(SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60))
            .unwrap();
    }

    /// A download that was cancelled, or that died, leaves a file and not a
    /// directory, which is the one thing the directory-only sweep never saw.
    #[cfg(unix)]
    #[test]
    fn pruning_clears_an_abandoned_download() {
        let root = tempfile::tempdir().unwrap();
        let kept = plant(root.path(), "postgres", "0.1.0");
        let download = root.path().join("postgres").join(".0.1.1.4242-0.download");
        std::fs::write(&download, vec![0u8; 1024]).unwrap();
        made_old(&download);

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();

        assert_eq!(entries_of(root.path(), "postgres"), ["0.1.0"]);
        assert_eq!(installed_path_in(root.path(), "postgres"), Some(kept));
    }

    #[cfg(unix)]
    #[test]
    fn pruning_removes_a_link_and_never_follows_it() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "postgres", "0.1.0");
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("postgres").join("0.0.9"))
            .unwrap();

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();

        assert_eq!(entries_of(root.path(), "postgres"), ["0.1.0"]);
        assert!(elsewhere.path().join("keep.txt").exists());
    }

    /// Another install of the same driver may be midway as this one finishes,
    /// and what it has on disk is not leftovers.
    #[test]
    fn pruning_leaves_the_fresh_scratch_files_of_an_install_that_is_still_going() {
        let root = tempfile::tempdir().unwrap();
        plant(root.path(), "postgres", "0.1.0");
        let driver_dir = root.path().join("postgres");
        std::fs::write(driver_dir.join(".0.1.1.4242-0.download"), b"half").unwrap();
        std::fs::create_dir_all(driver_dir.join(".0.1.1.4242-0.incoming")).unwrap();

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();

        assert_eq!(
            entries_of(root.path(), "postgres"),
            [".0.1.1.4242-0.download", ".0.1.1.4242-0.incoming", "0.1.0"]
        );
    }

    #[test]
    fn scratch_removes_what_it_guards_when_dropped() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("partial");
        std::fs::write(&file, b"half").unwrap();
        let directory = root.path().join("unpacking");
        std::fs::create_dir_all(directory.join("inside")).unwrap();
        std::fs::write(directory.join("inside").join("file"), b"x").unwrap();
        let never_made = root.path().join("never-made");

        drop(Scratch::at(file.clone()));
        drop(Scratch::at(directory.clone()));
        drop(Scratch::at(never_made));

        assert!(!file.exists());
        assert!(!directory.exists());
    }

    // ---- The newest set ever installed ----------------------------------

    /// A floor that lets everything through, for the tests that are about
    /// something else.
    const NO_FLOOR: &str = "0.0.0";

    #[test]
    fn a_machine_that_has_installed_nothing_accepts_any_version_above_the_floor() {
        let root = tempfile::tempdir().unwrap();
        ensure_not_a_rollback_in(root.path(), "0.0.1", NO_FLOOR).unwrap();
        ensure_not_a_rollback_in(root.path(), "9.9.9", NO_FLOOR).unwrap();
    }

    /// The replay a signature cannot stop: every manifest ever published is
    /// still correctly signed.
    #[test]
    fn a_set_older_than_one_already_installed_is_refused() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.2.0").unwrap();

        let error = ensure_not_a_rollback_in(root.path(), "0.1.9", NO_FLOOR)
            .expect_err("an older set must be refused");

        let message = error.to_string();
        assert!(
            message.contains("0.1.9") && message.contains("0.2.0"),
            "{message}"
        );
        assert!(
            message.contains("newer one") && message.contains("wait for a newer set"),
            "it must say what happened and what to wait for: {message}"
        );
        assert!(
            !message.contains(NEWEST_INSTALLED_FILE),
            "there is no way out by editing a file, and the message must not offer one: {message}"
        );
        ensure_not_a_rollback_in(root.path(), "0.2.0", NO_FLOOR).unwrap();
        ensure_not_a_rollback_in(root.path(), "0.2.1", NO_FLOOR).unwrap();
    }

    /// What protects a machine that has no record: a fresh install, and an
    /// upgrade from a Zode that kept none.
    #[test]
    fn a_set_older_than_the_built_in_minimum_is_refused_with_nothing_installed() {
        let root = tempfile::tempdir().unwrap();

        let error = ensure_not_a_rollback_in(root.path(), "0.1.4", MINIMUM_DRIVER_SET_VERSION)
            .expect_err("a set older than any ever signed must be refused");

        let message = error.to_string();
        assert!(message.contains("0.1.4"), "{message}");
        assert!(message.contains(MINIMUM_DRIVER_SET_VERSION), "{message}");
        assert!(message.contains("wait for a newer set"), "{message}");
        for accepted in ["0.1.5-0", "0.1.5-rc.1", "0.1.5", "0.1.6", "1.0.0"] {
            ensure_not_a_rollback_in(root.path(), accepted, MINIMUM_DRIVER_SET_VERSION)
                .unwrap_or_else(|error| panic!("{accepted} must be accepted: {error}"));
        }
    }

    /// The minimum bounds a machine that has a record too: whichever is higher
    /// wins.
    #[test]
    fn the_higher_of_the_minimum_and_the_record_is_the_floor() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.3.0").unwrap();

        ensure_not_a_rollback_in(root.path(), "0.2.9", "0.1.0")
            .expect_err("the record is higher than the minimum");
        ensure_not_a_rollback_in(root.path(), "0.2.9", "0.5.0")
            .expect_err("the minimum is higher than the record");
        ensure_not_a_rollback_in(root.path(), "0.5.0", "0.5.0").unwrap();
    }

    /// The constant is read as a version on every install, so one that cannot be
    /// would refuse them all.
    #[test]
    fn the_built_in_minimum_is_a_version_the_server_can_publish_and_zode_can_order() {
        assert!(is_version(MINIMUM_DRIVER_SET_VERSION));
        parse_version(MINIMUM_DRIVER_SET_VERSION).unwrap();
    }

    /// Ordered by semver, not by text: "0.1.10" sorts before "0.1.9" as a
    /// string, and a prerelease comes before its release.
    #[test]
    fn versions_are_ordered_as_versions() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.1.9").unwrap();
        ensure_not_a_rollback_in(root.path(), "0.1.10", NO_FLOOR).unwrap();
        record_install_in(root.path(), "0.1.10").unwrap();
        ensure_not_a_rollback_in(root.path(), "0.1.9", NO_FLOOR)
            .expect_err("0.1.9 is older than 0.1.10");

        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.1.5-rc.2").unwrap();
        ensure_not_a_rollback_in(root.path(), "0.1.5-rc.1", NO_FLOOR)
            .expect_err("rc.1 is older than rc.2");
        ensure_not_a_rollback_in(root.path(), "0.1.5", NO_FLOOR).unwrap();
        record_install_in(root.path(), "0.1.5").unwrap();
        ensure_not_a_rollback_in(root.path(), "0.1.5-rc.9", NO_FLOOR)
            .expect_err("a prerelease is older than its release");
    }

    /// Recording a lower version must not lower the bar.
    #[test]
    fn the_newest_version_is_kept_when_an_equal_or_older_one_is_recorded() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.3.0").unwrap();
        record_install_in(root.path(), "0.2.0").unwrap();
        record_install_in(root.path(), "0.3.0").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join(NEWEST_INSTALLED_FILE)).unwrap(),
            "0.3.0"
        );
        let leftovers: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            [NEWEST_INSTALLED_FILE],
            "no scratch file may stay"
        );
    }

    /// An empty record -- what a crash between rename and write leaves, on a
    /// filesystem that reorders them -- or a garbled one must not refuse every
    /// install on the machine. It is a floor above the built-in one and nothing
    /// more, and the next install writes a good one over it.
    #[test]
    fn a_record_that_is_empty_or_garbled_is_treated_as_absent_and_replaced() {
        for contents in ["", "  \n", "garbage", "0.1", "v0.2.0"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join(NEWEST_INSTALLED_FILE), contents).unwrap();

            ensure_not_a_rollback_in(root.path(), "0.1.0", NO_FLOOR)
                .unwrap_or_else(|error| panic!("{contents:?} must not refuse installs: {error}"));
            record_install_in(root.path(), "0.1.0").unwrap();

            assert_eq!(
                std::fs::read_to_string(root.path().join(NEWEST_INSTALLED_FILE)).unwrap(),
                "0.1.0",
                "{contents:?} must be written over"
            );
        }
    }

    /// The server accepts `0.01.0`; semver does not, so there is no order to
    /// compare it by.
    #[test]
    fn a_version_that_cannot_be_ordered_is_refused() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.1.0").unwrap();
        assert!(is_version("0.01.0"));
        assert!(ensure_not_a_rollback_in(root.path(), "0.01.0", NO_FLOOR).is_err());
    }

    /// The record sits beside the drivers' directories, and removing or pruning
    /// one driver must leave it where it is: removing a driver is not a way to
    /// forget how far the machine has already got.
    #[test]
    fn removing_or_pruning_a_driver_leaves_the_record_alone() {
        let root = tempfile::tempdir().unwrap();
        record_install_in(root.path(), "0.1.0").unwrap();
        plant_installed(root.path(), "postgres", "0.1.0");

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();
        assert!(remove_in(root.path(), "postgres").unwrap());

        assert!(root.path().join(NEWEST_INSTALLED_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn pruning_after_an_install_clears_stale_staging_leftovers_with_the_old_versions() {
        let root = tempfile::tempdir().unwrap();
        let kept = plant(root.path(), "postgres", "0.1.0");
        plant(root.path(), "postgres", "0.1.1");
        for leftover in [".0.2.0.incoming", ".0.1.5-rc.1.incoming", "manual"] {
            plant(root.path(), "postgres", leftover);
            made_old(&root.path().join("postgres").join(leftover));
        }

        prune_other_versions_in(root.path(), "postgres", "0.1.0").unwrap();

        assert_eq!(installed_versions_in(root.path(), "postgres"), ["0.1.0"]);
        assert_eq!(installed_path_in(root.path(), "postgres"), Some(kept));
    }
}
