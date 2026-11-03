use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ids::ProjectId;

/// Which checkout stands for which cloud project: checkouts on this machine,
/// and checkouts on the remote hosts this machine opens projects on.
///
/// Kept on this machine, and it must stay here. It is the only structure in the feature that
/// describes how a user's disk is laid out, and it is worthless to synchronise
/// anyway — the same project lives at a different path on every machine.
///
/// Binding is a manual step: the user picks the project in the vault. That is
/// what removes the need to derive an identifier from a git remote or a path,
/// and it is why the identifiers can be plain random bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bindings {
    /// Checkouts on this machine. Kept where it always was, so a file written
    /// before remote hosts existed still reads as this machine's.
    #[serde(default)]
    worktrees: BTreeMap<String, ProjectId>,
    /// host identity -> checkout root on that host -> project.
    ///
    /// Separate from `worktrees` because `/home/me/acme` on a WSL distro and
    /// `/home/me/acme` on this machine are two directories, and sharing one
    /// binding would pull one's credentials into the other. An older build
    /// ignores the field; rewriting the file drops it, which fails closed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    remote_worktrees: BTreeMap<String, BTreeMap<String, ProjectId>>,
}

/// A checkout root, and the machine it is a directory on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Checkout {
    host: Option<String>,
    root: PathBuf,
}

impl Checkout {
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self {
            host: None,
            root: root.into(),
        }
    }

    /// `host` is the host's stable identity, the same spelling every other
    /// record keyed by host uses.
    pub fn remote(host: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            host: Some(host.into()),
            root: root.into(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    pub fn is_local(&self) -> bool {
        self.host.is_none()
    }

    /// Where `absolute` sits inside this checkout, slash-separated: the path
    /// the catalogue records. `None` for a path outside it, or the root
    /// itself.
    pub fn relative(&self, absolute: &Path) -> Option<String> {
        let rest = absolute.strip_prefix(&self.root).ok()?;
        if rest.as_os_str().is_empty() {
            return None;
        }
        Some(rest.to_string_lossy().replace('\\', "/"))
    }
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

    /// The project a checkout on this machine is bound to.
    pub fn project_for(&self, worktree_root: &Path) -> Option<ProjectId> {
        self.worktrees.get(&key(worktree_root)).copied()
    }

    pub fn project_for_checkout(&self, checkout: &Checkout) -> Option<ProjectId> {
        match checkout.host() {
            None => self.project_for(checkout.root()),
            Some(host) => self
                .remote_worktrees
                .get(host)?
                .get(&key(checkout.root()))
                .copied(),
        }
    }

    /// Every checkout on this machine bound to a project, so the vault can
    /// show where each one would land here. Remote checkouts are left out: their
    /// paths name directories on another host, and writing to them here would
    /// put a file somewhere nobody chose.
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

    pub fn bind_checkout(&mut self, checkout: &Checkout, project: ProjectId) {
        match checkout.host() {
            None => self.bind(checkout.root(), project),
            Some(host) => {
                self.remote_worktrees
                    .entry(host.to_string())
                    .or_default()
                    .insert(key(checkout.root()), project);
            }
        }
    }

    pub fn unbind(&mut self, worktree_root: &Path) {
        self.worktrees.remove(&key(worktree_root));
    }

    pub fn is_empty(&self) -> bool {
        self.worktrees.is_empty() && self.remote_worktrees.is_empty()
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
    fn a_remote_and_a_local_checkout_with_one_path_are_two_bindings() {
        let mut bindings = Bindings::default();
        let wsl = Checkout::remote(r#"["wsl","Ubuntu",null]"#, "/home/me/acme");
        bindings.bind_checkout(&wsl, project(0x11));

        assert_eq!(bindings.project_for_checkout(&wsl), Some(project(0x11)));
        assert_eq!(
            bindings.project_for(Path::new("/home/me/acme")),
            None,
            "the remote binding leaked onto this machine's directory"
        );
        assert!(
            bindings.worktrees_for(project(0x11)).is_empty(),
            "a remote root was offered as a place to write on this machine"
        );

        bindings.bind(Path::new("/home/me/acme"), project(0x22));
        assert_eq!(bindings.project_for_checkout(&wsl), Some(project(0x11)));
        assert_eq!(
            bindings.project_for_checkout(&Checkout::local("/home/me/acme")),
            Some(project(0x22))
        );
        assert_eq!(
            bindings.project_for_checkout(&Checkout::remote(
                r#"["ssh","box",null,null]"#,
                "/home/me/acme"
            )),
            None,
            "one host's binding answered for another"
        );
    }

    #[test]
    fn a_path_inside_a_checkout_is_recorded_relative_to_it() {
        let checkout = Checkout::remote(r#"["wsl","Ubuntu",null]"#, "/home/me/acme");

        assert_eq!(
            checkout
                .relative(Path::new("/home/me/acme/services/api/.env"))
                .as_deref(),
            Some("services/api/.env")
        );
        assert_eq!(
            Checkout::local("/work/acme")
                .relative(Path::new("/work/acme/.env"))
                .as_deref(),
            Some(".env")
        );
        for outside in [
            "/home/me/other/.env",
            "/home/me/acme-two/.env",
            "/home/me/acme",
        ] {
            assert_eq!(
                checkout.relative(Path::new(outside)),
                None,
                "{outside} is not a file inside the checkout"
            );
        }
    }

    #[test]
    fn remote_bindings_survive_the_file() {
        let path = temp("remote-roundtrip");
        let wsl = Checkout::remote(r#"["wsl","Ubuntu",null]"#, "/home/me/acme");
        let mut bindings = Bindings::default();
        bindings.bind_checkout(&wsl, project(0x11));
        bindings.save(&path).unwrap();

        let loaded = Bindings::load(&path);
        assert_eq!(loaded.project_for_checkout(&wsl), Some(project(0x11)));
        assert_eq!(loaded.project_for(Path::new("/home/me/acme")), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_file_written_before_remote_hosts_reads_and_writes_unchanged() {
        let path = temp("legacy");
        let legacy = format!(
            "{{\n  \"worktrees\": {{\n    \"/work/acme\": \"{}\"\n  }}\n}}",
            "11".repeat(16)
        );
        std::fs::write(&path, &legacy).unwrap();

        let loaded = Bindings::load(&path);
        assert_eq!(
            loaded.project_for(Path::new("/work/acme")),
            Some(project(0x11))
        );
        loaded.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            legacy,
            "an older build must be able to read what this one writes back"
        );
        std::fs::remove_file(&path).unwrap();
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
