use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::envelope::Kind;

/// Where each artifact lives locally, and where its pre-overwrite copy goes.
///
/// `settings` and `keymap` already had backup paths in `paths` before sync
/// existed; reusing them means a user who has ever recovered a settings file
/// looks in the same place.
pub struct Artifact {
    pub kind: Kind,
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

impl Artifact {
    /// The real locations, for the running editor.
    ///
    /// Sync functions take an `Artifact` rather than calling this themselves,
    /// so a test can point them at a temporary directory. A hidden call to
    /// `paths::settings_file()` inside the sync logic would mean every test
    /// run rewrote the developer's own configuration.
    pub fn for_kind(kind: Kind) -> Self {
        match kind {
            Kind::Settings => Self {
                kind,
                path: paths::settings_file().clone(),
                backup_path: Some(paths::settings_backup_file().clone()),
            },
            Kind::Keymap => Self {
                kind,
                path: paths::keymap_file().clone(),
                backup_path: Some(paths::keymap_backup_file().clone()),
            },
            // Not a file the user edits — it is derived from what is installed,
            // and applying it is an install action rather than a write. See
            // phase-11.
            Kind::Extensions => Self {
                kind,
                path: PathBuf::new(),
                backup_path: None,
            },
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Artifact {
    /// An artifact rooted anywhere — for tests.
    pub fn rooted_at(kind: Kind, dir: &Path) -> Self {
        Self {
            kind,
            path: dir.join(format!("{kind}.json")),
            backup_path: Some(dir.join(format!("{kind}_backup.json"))),
        }
    }
}

/// Content hash, used to compare local against remote and against the last
/// sync without keeping either copy around.
pub fn hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// Reads an artifact, treating "not there" as empty rather than as an error.
///
/// A machine that has never written a `settings.json` is in a perfectly normal
/// state, and a first pull is exactly when that machine is most likely to be
/// syncing.
pub fn read(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes an artifact so that a crash cannot leave a half-written file.
///
/// Write to a sibling temporary file, then rename over the target. Rename
/// within one filesystem is atomic on all three platforms, so a reader either
/// sees the old file or the new one — never a truncated `settings.json`, which
/// would leave the editor unable to start with the user's own configuration.
pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "artifact path has no directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;

    // The temporary file must be a sibling: `std::fs::rename` across
    // filesystems is not atomic and on Windows fails outright.
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&temporary, content)?;

    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Do not leave debris behind for the next run to trip over.
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

/// Copies the current file aside before it is overwritten.
///
/// Invariant 8. Runs before every remote-wins write, and a failure here stops
/// the write: losing the user's configuration because the safety net could not
/// be laid is the exact outcome the safety net exists to prevent.
pub fn back_up(artifact: &Artifact) -> std::io::Result<()> {
    let Some(backup_path) = artifact.backup_path.as_ref() else {
        return Ok(());
    };
    let Some(current) = read(&artifact.path)? else {
        // Nothing to preserve.
        return Ok(());
    };
    write_atomic(backup_path, &current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_a_missing_file_is_not_an_error() {
        let missing = std::env::temp_dir().join("zode-sync-does-not-exist-9f3a.json");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(read(&missing).unwrap(), None);
    }

    #[test]
    fn writing_is_atomic_and_leaves_no_temporary_behind() {
        let dir = std::env::temp_dir().join("zode-sync-atomic-test");
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("settings.json");

        write_atomic(&target, "{ \"a\": 1 }").unwrap();
        assert_eq!(read(&target).unwrap().as_deref(), Some("{ \"a\": 1 }"));

        write_atomic(&target, "{ \"a\": 2 }").unwrap();
        assert_eq!(read(&target).unwrap().as_deref(), Some("{ \"a\": 2 }"));

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_hash_distinguishes_content() {
        assert_eq!(hash("same"), hash("same"));
        assert_ne!(hash("same"), hash("different"));
    }
}
