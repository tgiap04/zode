use std::io;
use std::path::{Path, PathBuf};

use crate::ids::EntryId;

/// How many superseded copies of one file are kept.
///
/// Bounded on purpose. A backup directory that only grows is a directory full
/// of old credentials, and the oldest of them are the ones most likely to have
/// been revoked and forgotten.
pub const BACKUPS_KEPT: usize = 10;

/// Writes a secret so that a crash cannot leave a half-written file, and so
/// that no moment exists in which it is readable by anyone else.
///
/// Two differences from `zode_sync::artifact::write_atomic`, and neither is
/// cosmetic:
///
/// - `std::fs::write` creates at `0644` minus the umask. For `settings.json`
///   that is correct. For a file holding a production database password it is
///   not.
/// - The permission is set **as the temporary file is created**, not after.
///   Creating at `0644` and calling `chmod` afterwards leaves a window — short,
///   but real, and on a shared machine that is the whole attack.
///
/// On Windows there is no mode to set; the file inherits the ACL of its parent
/// directory, which for a project checkout means the user's own profile. Said
/// plainly rather than papered over: the guarantee is weaker there.
pub fn write_secret_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the target path has no directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;

    // A sibling, because `rename` is only atomic within one filesystem and on
    // Windows fails outright across them. Unique, because a leftover from a
    // crashed run must not be silently reused or clobbered.
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("secret"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default(),
    ));

    let result = write_then_rename(&temporary, path, contents);
    if result.is_err() {
        // Do not leave a world-readable-by-default fragment of a secret behind
        // for the next run to trip over.
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn write_then_rename(temporary: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }

    let mut file = options.open(temporary)?;
    file.write_all(contents)?;
    // Durability before visibility: a rename that lands before the bytes do
    // leaves an empty `.env` where a working one used to be.
    file.sync_all()?;
    drop(file);

    std::fs::rename(temporary, path)
}

/// Copies a file aside before it is overwritten, outside any project.
///
/// `zode_sync::artifact::back_up` writes `settings_backup.json` beside the
/// file it protects. Doing the same here would put a `.env_backup` inside the
/// user's repository, where `.gitignore` has no rule for it — turning the
/// safety net into the leak it exists to prevent.
///
/// Returns the path written, or `None` when there was nothing to preserve.
pub fn back_up_secret(
    backups_root: &Path,
    entry: &EntryId,
    current: &[u8],
) -> io::Result<Option<PathBuf>> {
    let directory = backups_root.join(entry.as_hex());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default();

    // Zero-padded so the name sorts the same way the number does, and so a
    // directory listing reads in order.
    let target = directory.join(format!("{stamp:020}"));
    write_secret_atomic(&target, current)?;
    evict_old_backups(&directory)?;
    Ok(Some(target))
}

/// Keeps the newest [`BACKUPS_KEPT`] copies and removes the rest.
fn evict_old_backups(directory: &Path) -> io::Result<()> {
    let mut stamps: Vec<(u128, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let Some(stamp) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u128>().ok())
        else {
            // Not one of ours — a temporary file mid-write, or something a
            // person put there. Left alone rather than deleted.
            continue;
        };
        stamps.push((stamp, path));
    }

    if stamps.len() <= BACKUPS_KEPT {
        return Ok(());
    }

    stamps.sort_unstable_by(|left, right| right.0.cmp(&left.0));
    for (_, path) in stamps.into_iter().skip(BACKUPS_KEPT) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Resolves where an entry may be written, refusing anything that escapes.
///
/// The relative path arrives inside the encrypted manifest, which means it
/// arrives from whoever wrote that manifest — normally the user's own other
/// machine, but a compromised one would do just as well. `..` in that field is
/// an arbitrary file write with the editor's privileges, so it is refused at
/// the point of use rather than trusted at the point of parsing.
pub fn resolve_within(worktree_root: &Path, relative: &str) -> io::Result<PathBuf> {
    if !names_a_file_inside(relative) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{relative:?} does not name a file inside this project"),
        ));
    }
    Ok(worktree_root.join(Path::new(relative)))
}

/// Whether a recorded path names a file inside a project.
///
/// Split out of [`resolve_within`] so that anything which *produces* one of
/// these paths — renaming an entry, say — is held to the same rule as the code
/// that writes through it. Two copies of this test would eventually disagree,
/// and the disagreement would be a path that is accepted when typed and
/// refused when used.
pub fn names_a_file_inside(relative: &str) -> bool {
    let candidate = Path::new(relative);
    !relative.is_empty()
        && !candidate.is_absolute()
        && !relative.contains(':')
        && !relative.starts_with('\\')
        && candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("zode-env-secret-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the sandbox must be creatable");
            Self(dir)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("a valid fixture id")
    }

    #[test]
    fn a_written_secret_is_readable_only_by_its_owner() {
        let sandbox = Sandbox::new("mode");
        let target = sandbox.0.join(".env");
        write_secret_atomic(&target, b"DATABASE_URL=postgres://x").unwrap();

        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"DATABASE_URL=postgres://x"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "a .env must not be readable by anyone else"
            );
        }
        #[cfg(not(unix))]
        {
            // Windows has no mode; the file inherits the parent ACL. Asserted
            // as a note so the gap is visible rather than assumed away.
            eprintln!("skipped: no POSIX permission bits on this platform");
        }
    }

    #[test]
    fn writing_leaves_no_temporary_behind() {
        let sandbox = Sandbox::new("no-temp");
        let target = sandbox.0.join(".env");
        write_secret_atomic(&target, b"A=1").unwrap();
        write_secret_atomic(&target, b"A=2").unwrap();

        let leftovers: Vec<String> = std::fs::read_dir(&sandbox.0)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains("tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"A=2");
    }

    #[test]
    fn a_backup_never_lands_inside_the_project() {
        let project = Sandbox::new("project");
        let backups = Sandbox::new("backups");

        let written = back_up_secret(&backups.0, &entry(0xa1), b"OLD=1")
            .unwrap()
            .expect("a backup path");

        assert!(
            !written.starts_with(&project.0),
            "{} is inside the project",
            written.display()
        );
        assert!(written.starts_with(&backups.0));
        assert_eq!(std::fs::read(&written).unwrap(), b"OLD=1");
    }

    #[test]
    fn a_backup_is_readable_only_by_its_owner() {
        let backups = Sandbox::new("backup-mode");
        let written = back_up_secret(&backups.0, &entry(0xa1), b"OLD=1")
            .unwrap()
            .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&written).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        #[cfg(not(unix))]
        let _ = written;
    }

    #[test]
    fn backups_are_capped_and_the_newest_survive() {
        let backups = Sandbox::new("evict");
        let id = entry(0xa1);
        let directory = backups.0.join(id.as_hex());
        std::fs::create_dir_all(&directory).unwrap();

        // Written directly so the timestamps are controlled rather than raced.
        for stamp in 1u128..=12 {
            write_secret_atomic(
                &directory.join(format!("{stamp:020}")),
                format!("V={stamp}").as_bytes(),
            )
            .unwrap();
        }
        evict_old_backups(&directory).unwrap();

        let mut remaining: Vec<u128> = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u128>().ok())
            .collect();
        remaining.sort_unstable();

        assert_eq!(remaining.len(), BACKUPS_KEPT);
        assert_eq!(remaining, (3u128..=12).collect::<Vec<_>>());
    }

    #[test]
    fn an_escaping_path_cannot_be_resolved() {
        let root = Path::new("/projects/acme");
        for relative in [
            "../../../etc/passwd",
            "/etc/passwd",
            "a/../../b",
            "",
            "C:/Windows/system32",
            "\\\\server\\share",
            "./.env",
        ] {
            assert!(
                resolve_within(root, relative).is_err(),
                "{relative:?} was resolved"
            );
        }
    }

    #[test]
    fn an_ordinary_relative_path_resolves_under_the_project() {
        let root = Path::new("/projects/acme");
        assert_eq!(
            resolve_within(root, "services/api/.env.production").unwrap(),
            root.join("services/api/.env.production"),
        );
    }
}
