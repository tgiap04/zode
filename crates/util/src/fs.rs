use crate::ResultExt;
use anyhow::{Result, bail};
use async_fs as fs;
use futures_lite::StreamExt;
use std::path::{Path, PathBuf};

/// Removes all files and directories matching the given predicate
pub async fn remove_matching<F>(dir: &Path, predicate: F)
where
    F: Fn(&Path) -> bool,
{
    if let Some(mut entries) = fs::read_dir(dir).await.log_err() {
        while let Some(entry) = entries.next().await {
            if let Some(entry) = entry.log_err() {
                let entry_path = entry.path();
                if predicate(entry_path.as_path())
                    && let Ok(metadata) = fs::metadata(&entry_path).await
                {
                    if metadata.is_file() {
                        fs::remove_file(&entry_path).await.log_err();
                    } else {
                        fs::remove_dir_all(&entry_path).await.log_err();
                    }
                }
            }
        }
    }
}

pub async fn collect_matching<F>(dir: &Path, predicate: F) -> Vec<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    let mut matching = vec![];

    if let Some(mut entries) = fs::read_dir(dir).await.log_err() {
        while let Some(entry) = entries.next().await {
            if let Some(entry) = entry.log_err()
                && predicate(entry.path().as_path())
            {
                matching.push(entry.path());
            }
        }
    }

    matching
}

pub async fn find_file_name_in_dir<F>(dir: &Path, predicate: F) -> Option<PathBuf>
where
    F: Fn(&str) -> bool,
{
    if let Some(mut entries) = fs::read_dir(dir).await.log_err() {
        while let Some(entry) = entries.next().await {
            if let Some(entry) = entry.log_err() {
                let entry_path = entry.path();

                if let Some(file_name) = entry_path
                    .file_name()
                    .map(|file_name| file_name.to_string_lossy())
                    && predicate(&file_name)
                {
                    return Some(entry_path);
                }
            }
        }
    }

    None
}

pub async fn move_folder_files_to_folder<P: AsRef<Path>>(
    source_path: P,
    target_path: P,
) -> Result<()> {
    if !target_path.as_ref().is_dir() {
        bail!("Folder not found or is not a directory");
    }

    let mut entries = fs::read_dir(source_path.as_ref()).await?;
    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let old_path = entry.path();
        let new_path = target_path.as_ref().join(entry.file_name());

        fs::rename(&old_path, &new_path).await?;
    }

    fs::remove_dir(source_path).await?;

    Ok(())
}

#[cfg(unix)]
/// Set the permissions for the given path so that the file becomes executable.
/// This is a noop for non-unix platforms.
pub async fn make_file_executable(path: &Path) -> std::io::Result<()> {
    fs::set_permissions(
        path,
        <fs::Permissions as fs::unix::PermissionsExt>::from_mode(0o755),
    )
    .await
}

#[cfg(not(unix))]
#[allow(clippy::unused_async)]
/// Set the permissions for the given path so that the file becomes executable.
/// This is a noop for non-unix platforms.
pub async fn make_file_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Writes a secret so that a crash cannot leave a half-written file, and so
/// that no moment exists in which it is readable by anyone else.
///
/// Two differences from an ordinary atomic write, and neither is cosmetic:
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
pub fn write_secret_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the target path has no directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;

    // A sibling, because `rename` is only atomic within one filesystem and on
    // Windows fails outright across them. Unique, because a leftover from a
    // crashed run must not be silently reused or clobbered.
    let temporary = parent.join(temporary_name(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("secret"),
    ));
    write_secret_through(&temporary, path, contents)
}

fn write_secret_through(temporary: &Path, path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut created = false;
    let result = write_then_rename(temporary, path, contents, &mut created);
    // Do not leave a fragment of a secret behind for the next run to trip
    // over -- but only one this call made: a file already under that name
    // belongs to some other writer.
    if result.is_err()
        && created
        && let Err(error) = std::fs::remove_file(temporary)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("could not remove a partly written secret file: {error}");
    }
    result
}

fn temporary_name(name: &str) -> String {
    format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default(),
    )
}

fn write_then_rename(
    temporary: &Path,
    path: &Path,
    contents: &[u8],
    created: &mut bool,
) -> std::io::Result<()> {
    write_durably(temporary, contents, created)?;
    std::fs::rename(temporary, path)
}

/// Sets `created` once the temporary exists because of this call.
fn write_durably(temporary: &Path, contents: &[u8], created: &mut bool) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }

    let mut file = options.open(temporary)?;
    *created = true;
    file.write_all(contents)?;
    // Durability before visibility: a rename that lands before the bytes do
    // leaves an empty `.env` where a working one used to be.
    file.sync_all()
}

/// Why a file below a directory was not read or replaced.
#[derive(Debug)]
pub enum FileBelowError {
    /// A directory on the way to the file, or the file itself, is a link.
    Link,
    /// Something other than a regular file stands where the file should be,
    /// or other than a directory where a directory should be.
    NotAFile,
    /// The file is larger than the caller allows.
    TooLarge,
    Io(std::io::Error),
}

impl std::fmt::Display for FileBelowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link => write!(f, "it is reached through a symbolic link"),
            Self::NotAFile => write!(f, "it is not a regular file"),
            Self::TooLarge => write!(f, "it is larger than the limit"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FileBelowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FileBelowError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads the regular file that `components` name below `root`, following no
/// link anywhere below `root`. `None` when the file, or a directory on the way
/// to it, does not exist.
pub fn read_file_below(
    root: &Path,
    components: &[&str],
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, FileBelowError> {
    let (name, directories) = split_components(components)?;
    below::read(root, directories, name, max_bytes, &mut || {})
}

/// Replaces the file that `components` name below `root` the way
/// [`write_secret_atomic`] does, following no link anywhere below `root` and
/// creating the directories that are missing.
///
/// `still_expected` is shown what the file holds (`None` when absent) twice:
/// before anything is written, and again once the new bytes are durable,
/// immediately before the rename. Refusing either time leaves the file as it
/// was. A write landing between that second look and the rename is still
/// replaced; nothing short of a lock the other writer honours closes that.
///
/// On Unix every directory is opened relative to the one before it, refusing
/// links, so a directory swapped for a link after it was reached changes
/// nothing: the write goes where the opened directory is. Elsewhere each step
/// is checked by path and then used by path, which leaves a window in which a
/// swapped directory is followed.
///
/// The replacement is a new file: owned by this process's user, at `0600`
/// (where there are modes to set), and no longer sharing an inode with any
/// hard link the old one had.
pub fn replace_file_below<E: From<FileBelowError>>(
    root: &Path,
    components: &[&str],
    contents: &[u8],
    max_bytes: u64,
    still_expected: impl Fn(Option<&[u8]>) -> Result<(), E>,
) -> Result<(), E> {
    let (name, directories) = split_components(components)?;
    below::replace(
        root,
        directories,
        name,
        contents,
        max_bytes,
        &still_expected,
        &mut Seams {
            name_temporary: &temporary_name,
            after_directories: &mut || {},
            before_rename: &mut || {},
        },
    )
}

/// Where a test changes the filesystem in the middle of an operation, so that
/// a race is reproduced by order rather than by timing.
struct Seams<'a> {
    name_temporary: &'a dyn Fn(&str) -> String,
    after_directories: &'a mut dyn FnMut(),
    before_rename: &'a mut dyn FnMut(),
}

fn split_components<'a>(
    components: &'a [&'a str],
) -> Result<(&'a str, &'a [&'a str]), FileBelowError> {
    let invalid = || {
        FileBelowError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a path below the directory",
        ))
    };
    if components
        .iter()
        .any(|component| refuses_component(component, cfg!(windows)))
    {
        return Err(invalid());
    }
    let (name, directories) = components.split_last().ok_or_else(invalid)?;
    Ok((name, directories))
}

/// A Windows host reads a backslash as a separator and a colon as a drive or
/// a stream, so there either names some other file than the one checked.
fn refuses_component(component: &str, windows: bool) -> bool {
    component.is_empty()
        || component == "."
        || component == ".."
        || component.contains(['/', '\0'])
        || (windows && component.contains(['\\', ':']))
}

fn read_bounded(file: std::fs::File, max_bytes: u64) -> Result<Vec<u8>, FileBelowError> {
    use std::io::Read as _;

    if file.metadata()?.len() > max_bytes {
        return Err(FileBelowError::TooLarge);
    }
    // Bounded again while reading: the file can grow after the check.
    let mut content = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut content)?;
    if content.len() as u64 > max_bytes {
        return Err(FileBelowError::TooLarge);
    }
    Ok(content)
}

#[cfg(unix)]
use fd_below as below;
#[cfg(not(unix))]
use path_below as below;

/// Every step taken from the directory opened by the step before, so what is
/// checked is what is used.
#[cfg(unix)]
mod fd_below {
    use super::{FileBelowError, Seams, read_bounded};
    use nix::errno::Errno;
    use nix::fcntl::{AtFlags, OFlag};
    use nix::sys::stat::{Mode, SFlag};
    use std::fs::File;
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd};
    use std::path::Path;

    pub(super) fn read(
        root: &Path,
        directories: &[&str],
        name: &str,
        max_bytes: u64,
        after_directories: &mut dyn FnMut(),
    ) -> Result<Option<Vec<u8>>, FileBelowError> {
        let Some(directory) = open_directories(root, directories, false)? else {
            return Ok(None);
        };
        after_directories();
        current(&directory, name, max_bytes)
    }

    pub(super) fn replace<E: From<FileBelowError>>(
        root: &Path,
        directories: &[&str],
        name: &str,
        contents: &[u8],
        max_bytes: u64,
        still_expected: &dyn Fn(Option<&[u8]>) -> Result<(), E>,
        seams: &mut Seams,
    ) -> Result<(), E> {
        let directory = open_directories(root, directories, true)?
            .ok_or_else(|| FileBelowError::Io(std::io::ErrorKind::NotFound.into()))?;
        (seams.after_directories)();
        still_expected(current(&directory, name, max_bytes)?.as_deref())?;

        let temporary = (seams.name_temporary)(name);
        let mut created = false;
        let result = write_then_rename(
            &directory,
            &temporary,
            name,
            contents,
            max_bytes,
            still_expected,
            seams,
            &mut created,
        );
        // Only a temporary this call made: one already under that name
        // belongs to some other writer.
        if result.is_err() && created {
            match nix::unistd::unlinkat(
                Some(directory.as_raw_fd()),
                temporary.as_str(),
                nix::unistd::UnlinkatFlags::NoRemoveDir,
            ) {
                Ok(()) | Err(Errno::ENOENT) => {}
                Err(errno) => log::warn!("could not remove a partly written secret file: {errno}"),
            }
        }
        result
    }

    fn write_then_rename<E: From<FileBelowError>>(
        directory: &OwnedFd,
        temporary: &str,
        name: &str,
        contents: &[u8],
        max_bytes: u64,
        still_expected: &dyn Fn(Option<&[u8]>) -> Result<(), E>,
        seams: &mut Seams,
        created: &mut bool,
    ) -> Result<(), E> {
        use std::io::Write as _;

        let opened = nix::fcntl::openat(
            Some(directory.as_raw_fd()),
            temporary,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(io)?;
        *created = true;
        let mut file = File::from(owned(opened));
        file.write_all(contents).map_err(FileBelowError::from)?;
        file.sync_all().map_err(FileBelowError::from)?;
        drop(file);

        (seams.before_rename)();
        still_expected(current(directory, name, max_bytes)?.as_deref())?;
        nix::fcntl::renameat(
            Some(directory.as_raw_fd()),
            temporary,
            Some(directory.as_raw_fd()),
            name,
        )
        .map_err(io)?;
        // The new bytes are already durable and visible; what is not yet
        // durable is the rename itself, and failing here would report a
        // write that did happen as one that did not.
        if let Err(errno) = nix::unistd::fsync(directory.as_raw_fd()) {
            log::warn!("could not flush the directory holding a replaced secret file: {errno}");
        }
        Ok(())
    }

    fn open_directories(
        root: &Path,
        directories: &[&str],
        create: bool,
    ) -> Result<Option<OwnedFd>, FileBelowError> {
        // Resolved once, by path: the root is the project the user opened,
        // and only what lies below it is held to the no-link rule.
        let root = std::fs::canonicalize(root)?;
        let opened = nix::fcntl::open(
            &root,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io)?;
        let mut directory = owned(opened);
        for component in directories {
            let Some(next) = open_directory(&directory, component, create)? else {
                return Ok(None);
            };
            directory = next;
        }
        Ok(Some(directory))
    }

    fn open_directory(
        parent: &OwnedFd,
        name: &str,
        create: bool,
    ) -> Result<Option<OwnedFd>, FileBelowError> {
        let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
        match nix::fcntl::openat(Some(parent.as_raw_fd()), name, flags, Mode::empty()) {
            Ok(opened) => return Ok(Some(owned(opened))),
            Err(Errno::ENOENT) if create => {}
            Err(Errno::ENOENT) => return Ok(None),
            Err(errno) => return Err(explain(parent, name, errno)),
        }
        match nix::sys::stat::mkdirat(
            Some(parent.as_raw_fd()),
            name,
            Mode::from_bits_truncate(0o777),
        ) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(errno) => return Err(io(errno)),
        }
        // Opened again rather than assumed to be the one just made: a link
        // may have been put in its place.
        nix::fcntl::openat(Some(parent.as_raw_fd()), name, flags, Mode::empty())
            .map(|opened| Some(owned(opened)))
            .map_err(|errno| explain(parent, name, errno))
    }

    fn current(
        directory: &OwnedFd,
        name: &str,
        max_bytes: u64,
    ) -> Result<Option<Vec<u8>>, FileBelowError> {
        // Non-blocking, so a FIFO put where the file was cannot hang a read
        // that is only ever meant to see a regular file.
        let opened = match nix::fcntl::openat(
            Some(directory.as_raw_fd()),
            name,
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK,
            Mode::empty(),
        ) {
            Ok(opened) => opened,
            Err(Errno::ENOENT) => return Ok(None),
            Err(errno) => return Err(explain(directory, name, errno)),
        };
        let file = File::from(owned(opened));
        if !file.metadata()?.is_file() {
            return Err(FileBelowError::NotAFile);
        }
        read_bounded(file, max_bytes).map(Some)
    }

    /// Names a refused open by what is actually there, since `O_NOFOLLOW`
    /// reports a link as `ELOOP` on some systems and `EMLINK` on others.
    fn explain(parent: &OwnedFd, name: &str, errno: Errno) -> FileBelowError {
        match nix::sys::stat::fstatat(Some(parent.as_raw_fd()), name, AtFlags::AT_SYMLINK_NOFOLLOW)
        {
            Ok(stat)
                if SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT == SFlag::S_IFLNK =>
            {
                FileBelowError::Link
            }
            Ok(_) if matches!(errno, Errno::ENOTDIR | Errno::ELOOP) => FileBelowError::NotAFile,
            _ => io(errno),
        }
    }

    fn io(errno: Errno) -> FileBelowError {
        FileBelowError::Io(errno.into())
    }

    fn owned(descriptor: RawFd) -> OwnedFd {
        // SAFETY: `descriptor` was just returned by a successful `open` or
        // `openat`, so it is open and nothing else owns or will close it.
        unsafe { OwnedFd::from_raw_fd(descriptor) }
    }
}

/// Every step checked by path, then used by path.
#[cfg_attr(unix, allow(dead_code))]
mod path_below {
    use super::{FileBelowError, Seams, read_bounded, write_durably};
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

    pub(super) fn read(
        root: &Path,
        directories: &[&str],
        name: &str,
        max_bytes: u64,
        after_directories: &mut dyn FnMut(),
    ) -> Result<Option<Vec<u8>>, FileBelowError> {
        let Some(directory) = directory(root, directories, false)? else {
            return Ok(None);
        };
        after_directories();
        current(&directory.join(name), max_bytes)
    }

    pub(super) fn replace<E: From<FileBelowError>>(
        root: &Path,
        directories: &[&str],
        name: &str,
        contents: &[u8],
        max_bytes: u64,
        still_expected: &dyn Fn(Option<&[u8]>) -> Result<(), E>,
        seams: &mut Seams,
    ) -> Result<(), E> {
        let directory = directory(root, directories, true)?
            .ok_or_else(|| FileBelowError::Io(ErrorKind::NotFound.into()))?;
        (seams.after_directories)();
        let path = directory.join(name);
        still_expected(current(&path, max_bytes)?.as_deref())?;

        let temporary = directory.join((seams.name_temporary)(name));
        let mut created = false;
        let result = write_durably(&temporary, contents, &mut created)
            .map_err(FileBelowError::from)
            .map_err(E::from)
            .and_then(|()| {
                (seams.before_rename)();
                still_expected(current(&path, max_bytes)?.as_deref())?;
                std::fs::rename(&temporary, &path)
                    .map_err(FileBelowError::from)
                    .map_err(E::from)
            });
        if result.is_err()
            && created
            && let Err(error) = std::fs::remove_file(&temporary)
            && error.kind() != ErrorKind::NotFound
        {
            log::warn!("could not remove a partly written secret file: {error}");
        }
        result
    }

    fn directory(
        root: &Path,
        directories: &[&str],
        create: bool,
    ) -> Result<Option<PathBuf>, FileBelowError> {
        let root = std::fs::canonicalize(root)?;
        let mut directory = root.clone();
        for component in directories {
            directory.push(component);
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(FileBelowError::Link);
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(FileBelowError::NotAFile),
                Err(error) if error.kind() == ErrorKind::NotFound && create => {
                    match std::fs::create_dir(&directory) {
                        Ok(()) => {}
                        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            }
        }
        // A link the platform does not report as one, such as a junction,
        // still resolves somewhere else.
        if !std::fs::canonicalize(&directory)?.starts_with(&root) {
            return Err(FileBelowError::Link);
        }
        Ok(Some(directory))
    }

    fn current(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, FileBelowError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Err(FileBelowError::Link);
        }
        if !metadata.is_file() {
            return Err(FileBelowError::NotAFile);
        }
        read_bounded(std::fs::File::open(path)?, max_bytes).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("zode-util-secret-{name}-{}", std::process::id()));
            std::fs::remove_dir_all(&directory).ok();
            std::fs::create_dir_all(&directory).expect("the sandbox must be creatable");
            Self(directory)
        }

        fn leftovers(&self) -> Vec<String> {
            std::fs::read_dir(&self.0)
                .expect("the sandbox is readable")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".tmp"))
                .collect()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
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
    }

    #[test]
    fn writing_leaves_no_temporary_behind() {
        let sandbox = Sandbox::new("no-temp");
        let target = sandbox.0.join(".env");
        write_secret_atomic(&target, b"A=1").unwrap();
        write_secret_atomic(&target, b"A=2").unwrap();

        assert!(sandbox.leftovers().is_empty(), "{:?}", sandbox.leftovers());
        assert_eq!(std::fs::read(&target).unwrap(), b"A=2");
    }

    #[test]
    fn a_rename_that_fails_leaves_the_target_and_no_fragment() {
        // A non-empty directory where the file should land: the bytes reach
        // the temporary file, and only the final rename is refused.
        let sandbox = Sandbox::new("rename-fails");
        let target = sandbox.0.join(".env");
        std::fs::create_dir_all(target.join("occupied")).unwrap();

        let result = write_secret_atomic(&target, b"SECRET=1");

        assert!(result.is_err(), "a refused rename must be reported");
        assert!(target.join("occupied").is_dir(), "the target was disturbed");
        assert!(
            sandbox.leftovers().is_empty(),
            "a fragment of the secret was left behind: {:?}",
            sandbox.leftovers()
        );
    }

    /// A project directory and a directory beside it that nothing below the
    /// project may reach.
    struct Tree(Sandbox);

    impl Tree {
        fn new(name: &str) -> Self {
            let sandbox = Sandbox::new(name);
            std::fs::create_dir_all(sandbox.0.join("project")).unwrap();
            std::fs::create_dir_all(sandbox.0.join("outside")).unwrap();
            Self(sandbox)
        }

        fn project(&self) -> PathBuf {
            self.0.0.join("project")
        }

        #[cfg(unix)]
        fn outside(&self) -> PathBuf {
            self.0.0.join("outside")
        }
    }

    fn expect_absent(current: Option<&[u8]>) -> Result<(), FileBelowError> {
        match current {
            None => Ok(()),
            Some(_) => Err(FileBelowError::Io(std::io::Error::other("changed"))),
        }
    }

    fn expect_holding(
        expected: &'static [u8],
    ) -> impl Fn(Option<&[u8]>) -> Result<(), FileBelowError> {
        move |current| match current {
            Some(current) if current == expected => Ok(()),
            _ => Err(FileBelowError::Io(std::io::Error::other("changed"))),
        }
    }

    #[cfg(unix)]
    fn swap_for_a_link(directory: &Path, target: &Path) {
        let mut moved = directory.as_os_str().to_owned();
        moved.push(".moved");
        std::fs::rename(directory, &moved).unwrap();
        std::os::unix::fs::symlink(target, directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_swapped_for_a_link_once_reached_is_not_written_through() {
        let tree = Tree::new("swap-write");
        let config = tree.project().join("config");
        std::fs::create_dir_all(&config).unwrap();
        let outside = tree.outside();

        let result = below::replace(
            &tree.project(),
            &["config"],
            ".env",
            b"SECRET=1",
            1024,
            &expect_absent,
            &mut Seams {
                name_temporary: &temporary_name,
                after_directories: &mut || swap_for_a_link(&config, &outside),
                before_rename: &mut || {},
            },
        );

        assert!(
            !tree.outside().join(".env").exists(),
            "the write followed a link out of the project"
        );
        result.expect("the directory that was reached is still there to write into");
        assert_eq!(
            std::fs::read(tree.project().join("config.moved/.env")).unwrap(),
            b"SECRET=1"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_swapped_for_a_link_once_reached_is_not_read_through() {
        let tree = Tree::new("swap-read");
        let config = tree.project().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join(".env"), b"INSIDE=1").unwrap();
        std::fs::write(tree.outside().join(".env"), b"OUTSIDE=1").unwrap();
        let outside = tree.outside();

        let read = below::read(&tree.project(), &["config"], ".env", 1024, &mut || {
            swap_for_a_link(&config, &outside)
        })
        .expect("the directory that was reached is still there to read");

        assert_eq!(read.as_deref(), Some(&b"INSIDE=1"[..]));
    }

    #[test]
    fn a_file_changed_while_its_replacement_was_written_is_kept() {
        let tree = Tree::new("changed-before-rename");
        let target = tree.project().join(".env");
        std::fs::write(&target, b"OLD=1").unwrap();

        let result = below::replace(
            &tree.project(),
            &[],
            ".env",
            b"NEW=1",
            1024,
            &expect_holding(b"OLD=1"),
            &mut Seams {
                name_temporary: &temporary_name,
                after_directories: &mut || {},
                before_rename: &mut || std::fs::write(&target, b"EDITED=1").unwrap(),
            },
        );

        assert!(result.is_err(), "the edit made meanwhile was overwritten");
        assert_eq!(std::fs::read(&target).unwrap(), b"EDITED=1");
        let leftovers: Vec<_> = std::fs::read_dir(tree.project())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from(".env")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_wherever_it_stands() {
        let tree = Tree::new("links");
        std::fs::write(tree.outside().join(".env"), b"OUTSIDE=1").unwrap();
        std::fs::write(tree.project().join(".env.development"), b"DEV=1").unwrap();
        std::os::unix::fs::symlink(".env.development", tree.project().join(".env")).unwrap();
        std::os::unix::fs::symlink(tree.outside(), tree.project().join("config")).unwrap();

        for components in [&[".env"][..], &["config", ".env"][..]] {
            assert!(
                matches!(
                    read_file_below(&tree.project(), components, 1024),
                    Err(FileBelowError::Link)
                ),
                "{components:?} was read through a link"
            );
            assert!(
                matches!(
                    replace_file_below(&tree.project(), components, b"A=1", 1024, |_| Ok::<
                        (),
                        FileBelowError,
                    >(
                        ()
                    )),
                    Err(FileBelowError::Link)
                ),
                "{components:?} was written through a link"
            );
        }
        assert_eq!(
            std::fs::read(tree.outside().join(".env")).unwrap(),
            b"OUTSIDE=1"
        );
        assert_eq!(
            std::fs::read(tree.project().join(".env.development")).unwrap(),
            b"DEV=1"
        );
        assert!(
            std::fs::symlink_metadata(tree.project().join(".env"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_missing_file_is_created_owner_only_in_new_directories() {
        let tree = Tree::new("create-below");

        assert!(
            read_file_below(&tree.project(), &["deploy", ".env"], 1024)
                .unwrap()
                .is_none()
        );
        replace_file_below(
            &tree.project(),
            &["deploy", "eu", ".env"],
            b"A=1",
            1024,
            expect_absent,
        )
        .unwrap();

        let target = tree.project().join("deploy/eu/.env");
        assert_eq!(std::fs::read(&target).unwrap(), b"A=1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn something_other_than_a_file_or_a_directory_in_the_way_is_refused() {
        let tree = Tree::new("not-a-file");
        std::fs::create_dir_all(tree.project().join(".env")).unwrap();
        std::fs::write(tree.project().join("config"), b"a file").unwrap();

        assert!(matches!(
            read_file_below(&tree.project(), &[".env"], 1024),
            Err(FileBelowError::NotAFile)
        ));
        assert!(matches!(
            read_file_below(&tree.project(), &["config", ".env"], 1024),
            Err(FileBelowError::NotAFile)
        ));
        assert!(matches!(
            replace_file_below(
                &tree.project(),
                &["config", ".env"],
                b"A=1",
                1024,
                expect_absent
            ),
            Err(FileBelowError::NotAFile)
        ));
    }

    #[test]
    fn a_file_over_the_limit_is_not_read() {
        let tree = Tree::new("too-large");
        std::fs::write(tree.project().join(".env"), vec![b'A'; 17]).unwrap();

        assert!(matches!(
            read_file_below(&tree.project(), &[".env"], 16),
            Err(FileBelowError::TooLarge)
        ));
        assert!(
            read_file_below(&tree.project(), &[".env"], 17)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn components_that_would_climb_or_hide_a_separator_are_refused() {
        let tree = Tree::new("components");
        for components in [
            &[][..],
            &[""][..],
            &["."][..],
            &["..", ".env"][..],
            &["config/.env"][..],
        ] {
            assert!(
                read_file_below(&tree.project(), components, 1024).is_err(),
                "{components:?} must be refused"
            );
        }
    }

    #[test]
    fn a_component_a_windows_host_would_split_or_redirect_is_refused_there() {
        for component in [r"config\.env", "C:", ".env:stream", r"\"] {
            assert!(
                refuses_component(component, true),
                "{component:?} must be refused on Windows"
            );
        }
        // Legal in a Unix file name, and naming exactly one file there.
        for component in [r"config\.env", ".env:stream"] {
            assert!(
                !refuses_component(component, false),
                "{component:?} names one file on Unix"
            );
        }
        for component in [".env", ".env.production", "services"] {
            assert!(!refuses_component(component, true), "{component:?}");
            assert!(!refuses_component(component, false), "{component:?}");
        }
    }

    const TAKEN: &str = ".env.taken.tmp";

    fn taken_name(_name: &str) -> String {
        TAKEN.to_string()
    }

    #[test]
    fn a_temporary_this_call_did_not_create_is_left_alone() {
        // Another writer's file under the name this call picked: refusing to
        // reuse it is right, and removing it on the way out is not.
        let tree = Tree::new("temporary-taken");
        let theirs = tree.project().join(TAKEN);
        std::fs::write(&theirs, b"THEIRS=1").unwrap();
        let assert_theirs_kept = || {
            assert_eq!(
                std::fs::read(&theirs).ok().as_deref(),
                Some(&b"THEIRS=1"[..]),
                "another writer's temporary was removed"
            );
        };

        let result = below::replace(
            &tree.project(),
            &[],
            ".env",
            b"MINE=1",
            1024,
            &expect_absent,
            &mut Seams {
                name_temporary: &taken_name,
                after_directories: &mut || {},
                before_rename: &mut || {},
            },
        );
        assert!(
            result.is_err(),
            "a taken temporary must not be written into"
        );
        assert_theirs_kept();

        #[cfg(unix)]
        {
            let result = path_below::replace(
                &tree.project(),
                &[],
                ".env",
                b"MINE=1",
                1024,
                &expect_absent,
                &mut Seams {
                    name_temporary: &taken_name,
                    after_directories: &mut || {},
                    before_rename: &mut || {},
                },
            );
            assert!(
                result.is_err(),
                "a taken temporary must not be written into"
            );
            assert_theirs_kept();
        }

        let result = write_secret_through(&theirs, &tree.project().join(".env"), b"MINE=1");
        assert!(
            result.is_err(),
            "a taken temporary must not be written into"
        );
        assert_theirs_kept();
        assert!(!tree.project().join(".env").exists());
    }

    /// The fallback used where there is no `openat`, exercised here so it is
    /// at least run somewhere.
    #[cfg(unix)]
    #[test]
    fn the_path_checked_fallback_reads_replaces_and_refuses_links() {
        let tree = Tree::new("fallback");
        std::fs::write(tree.project().join(".env"), b"OLD=1").unwrap();
        std::os::unix::fs::symlink(tree.outside(), tree.project().join("config")).unwrap();
        let mut nothing = || {};

        assert_eq!(
            path_below::read(&tree.project(), &[], ".env", 1024, &mut nothing)
                .unwrap()
                .as_deref(),
            Some(&b"OLD=1"[..])
        );
        path_below::replace(
            &tree.project(),
            &[],
            ".env",
            b"NEW=1",
            1024,
            &expect_holding(b"OLD=1"),
            &mut Seams {
                name_temporary: &temporary_name,
                after_directories: &mut || {},
                before_rename: &mut || {},
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read(tree.project().join(".env")).unwrap(),
            b"NEW=1"
        );
        assert!(matches!(
            path_below::read(&tree.project(), &["config"], ".env", 1024, &mut nothing),
            Err(FileBelowError::Link)
        ));
    }
}
