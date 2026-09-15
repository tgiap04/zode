//! Where a project's logo lives, and the names it is allowed to answer to.
//!
//! The record on disk stores a bare file *name*, never a path. A hand-edited
//! or corrupted KVP record naming a full path would hand `Fs::remove_file` (in
//! `MultiWorkspace`) an arbitrary target the day the logo is cleared or the
//! project removed; a bare name resolved only under
//! `project_avatars_dir()` makes that class of bug unrepresentable rather than
//! merely unlikely. It also means moving the data directory
//! (`paths::set_custom_data_dir`) does not orphan every stored logo, since the
//! directory itself is never serialized.
//!
//! Pure and deterministic: no `Fs`, no `cx`. `project_logo_store` adds the
//! `MultiWorkspace` behaviour that reads and writes the actual bytes beside
//! these helpers -- split into its own module once that behaviour pushed
//! this one past its line budget.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// The largest logo Zode will keep a copy of.
///
/// Bounds both the disk a project's copy consumes and the RGBA buffer gpui
/// decodes to draw it at avatar size.
pub const MAX_LOGO_BYTES: u64 = 10 * 1024 * 1024;

/// The directory Zode's own copies of project logos live in.
pub fn project_avatars_dir() -> PathBuf {
    paths::data_dir().join("project_avatars")
}

/// The lowercased extension, if and only if `gpui::img()` can decode it.
///
/// Reads `gpui::Img::extensions()` rather than keeping its own list, so this
/// can never drift from what the element actually draws.
pub fn logo_extension(source: &Path) -> Option<String> {
    let extension = source.extension()?.to_str()?.to_lowercase();
    gpui::Img::extensions()
        .contains(&extension.as_str())
        .then_some(extension)
}

/// A fresh, collision-free file name for a copy stored under `extension`.
pub fn new_logo_file_name(extension: &str) -> String {
    format!("{}.{extension}", uuid::Uuid::new_v4())
}

/// Resolves a stored file name to the absolute path it names, refusing
/// anything that is not a single plain path component.
///
/// `Path::new(name).components()` yielding exactly one `Component::Normal`
/// rejects `..`, a leading `/` or `\`, `.`, and the empty string all at once,
/// without a hand-rolled blocklist that could miss one.
///
/// The colon is the one thing that check does not catch. `:` is not a path
/// separator on any platform, so `"a.png:evil"` arrives as a single `Normal`
/// component -- and on Windows the joined path names an NTFS alternate data
/// stream of `a.png` rather than a file called `a.png:evil`. It cannot leave
/// this directory, so it was never an escape; but this value is later handed
/// to `Fs::remove_file`, and a name that means something other than what it
/// reads as has no business getting that far. A name this module generated is
/// a uuid and an extension, so it never contains one.
pub fn logo_path_for(file_name: &str) -> Option<Arc<Path>> {
    if file_name.contains(':') {
        return None;
    }
    let path = Path::new(file_name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {
            Some(Arc::from(project_avatars_dir().join(file_name)))
        }
        _ => None,
    }
}

/// The stored name for a path previously returned by `logo_path_for`, or
/// `None` if it does not live under `project_avatars_dir()`.
pub fn logo_file_name(path: &Path) -> Option<String> {
    path.strip_prefix(project_avatars_dir())
        .ok()
        .and_then(|relative| relative.to_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_resolves_only_inside_the_avatars_dir() {
        let resolved = logo_path_for("a.png").expect("a plain name resolves");
        assert_eq!(resolved.as_ref(), project_avatars_dir().join("a.png"));

        for junk in [
            "",
            ".",
            "..",
            "../../etc/passwd",
            "/etc/passwd",
            "a/b.png",
            // Not a separator anywhere, so it survives `components()` as one
            // `Normal` -- and on Windows names an NTFS stream of `a.png`.
            "a.png:evil",
            "a.png:$DATA",
        ] {
            assert!(
                logo_path_for(junk).is_none(),
                "{junk:?} must be refused, not resolved"
            );
        }
    }

    #[test]
    fn a_logo_name_survives_the_trip_to_disk_and_back() {
        let name = new_logo_file_name("png");
        let path = logo_path_for(&name).expect("a generated name resolves");
        assert_eq!(logo_file_name(&path).as_deref(), Some(name.as_str()));
    }

    #[test]
    fn only_extensions_gpui_can_draw_are_accepted() {
        assert_eq!(logo_extension(Path::new("a.png")).as_deref(), Some("png"));
        assert_eq!(logo_extension(Path::new("a.JPG")).as_deref(), Some("jpg"));
        assert_eq!(logo_extension(Path::new("a.svg")).as_deref(), Some("svg"));
        assert_eq!(logo_extension(Path::new("a.pdf")), None);
        assert_eq!(logo_extension(Path::new("a.exe")), None);
        assert_eq!(logo_extension(Path::new("a")), None);

        for accepted in ["png", "jpg", "svg"] {
            assert!(gpui::Img::extensions().contains(&accepted));
        }
    }

    #[test]
    fn a_generated_name_carries_the_extension_and_is_unique() {
        let first = new_logo_file_name("png");
        let second = new_logo_file_name("png");
        assert_ne!(first, second);
        assert!(first.ends_with(".png"));
        assert!(second.ends_with(".png"));
    }
}
