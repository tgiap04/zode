use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::EnvCryptoError;
use crate::ids::{EntryId, ProjectId};

/// Manifest format version, independent of the envelope version.
pub const MANIFEST_VERSION: u32 = 1;

/// The catalogue: which projects exist, which files each holds, and where each
/// file goes on disk.
///
/// Encrypted like everything else, which is what makes the server's view of a
/// user's projects a list of random hex strings and nothing more.
///
/// `BTreeMap` rather than `HashMap` so the serialised form is stable — two
/// machines writing the same catalogue produce the same bytes, and a diff of
/// two manifests is readable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub v: u32,
    #[serde(default)]
    pub projects: BTreeMap<ProjectId, ManifestProject>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestProject {
    /// What the user called it. Never leaves the ciphertext.
    pub name: String,
    #[serde(default)]
    pub entries: BTreeMap<EntryId, ManifestEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Where the file goes, **relative to the bound worktree root**.
    ///
    /// Never absolute. An absolute path is the one field that would tell the
    /// server how a machine's disk is laid out, and it is meaningless across
    /// machines anyway.
    pub path: String,
    /// The last version this catalogue knows about.
    pub seq: u64,
}

impl Manifest {
    /// A path no other entry in this project is already using.
    ///
    /// Collisions are ordinary rather than exotic: a file chosen from outside
    /// any bound checkout keeps only its bare name, and so does the next one.
    /// Two rows reading `.env` name nothing — and the path is also where a
    /// pull writes, so the two would fight over one file on disk.
    ///
    /// Counting from 2, the way every file manager does, so the first
    /// duplicate reads as the second copy.
    pub fn unique_path(&self, project: ProjectId, wanted: &str, except: Option<EntryId>) -> String {
        let Some(holder) = self.projects.get(&project) else {
            return wanted.to_string();
        };
        let taken = |candidate: &str| {
            holder
                .entries
                .iter()
                .any(|(id, entry)| Some(*id) != except && entry.path == candidate)
        };

        if !taken(wanted) {
            return wanted.to_string();
        }
        // Terminates: only finitely many names are taken, so some candidate is
        // free. The fallback exists to satisfy the type, not the logic.
        (2..)
            .map(|index| format!("{wanted} ({index})"))
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| wanted.to_string())
    }

    pub fn new() -> Self {
        Self {
            v: MANIFEST_VERSION,
            projects: BTreeMap::new(),
        }
    }

    pub fn from_json(raw: &[u8]) -> Result<Self, EnvCryptoError> {
        let manifest: Self = serde_json::from_slice(raw)
            .map_err(|error| EnvCryptoError::Malformed(error.to_string()))?;
        if manifest.v != MANIFEST_VERSION {
            return Err(EnvCryptoError::UnsupportedVersion(manifest.v));
        }
        Ok(manifest)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, EnvCryptoError> {
        serde_json::to_vec(self).map_err(|error| EnvCryptoError::Malformed(error.to_string()))
    }

    /// Finds which project owns an entry.
    pub fn project_of(&self, entry: &EntryId) -> Option<(&ProjectId, &ManifestProject)> {
        self.projects
            .iter()
            .find(|(_, project)| project.entries.contains_key(entry))
    }

    /// Every entry, flattened, for reconciling against the server's listing.
    pub fn entry_ids(&self) -> impl Iterator<Item = &EntryId> {
        self.projects
            .values()
            .flat_map(|project| project.entries.keys())
    }
}

impl ManifestEntry {
    /// Whether this entry names somewhere it is allowed to write.
    ///
    /// Checked here because the manifest is where a hostile or corrupted path
    /// would arrive, but NOT enforced at deserialisation: one bad entry must
    /// not make a whole catalogue unreadable and lock a user out of the files
    /// that are fine. The caller that is about to write refuses; the catalogue
    /// still loads.
    pub fn has_a_safe_path(&self) -> bool {
        let path = std::path::Path::new(&self.path);
        !self.path.is_empty()
            && path.is_relative()
            && path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_)))
            // A Windows drive-relative path such as `C:foo` parses as relative
            // on Unix and as something else entirely on Windows. Refused on
            // both, so a manifest means one thing everywhere.
            && !self.path.contains(':')
            && !self.path.starts_with('\\')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).unwrap()
    }

    fn project(byte: u8) -> ProjectId {
        ProjectId::parse(&format!("{byte:02x}").repeat(16)).unwrap()
    }

    fn sample() -> Manifest {
        let mut manifest = Manifest::new();
        manifest.projects.insert(
            project(0x11),
            ManifestProject {
                name: "acme-api".into(),
                entries: BTreeMap::from([(
                    entry(0xa1),
                    ManifestEntry {
                        path: "services/api/.env.production".into(),
                        seq: 7,
                    },
                )]),
            },
        );
        manifest
    }

    #[test]
    fn round_trips_through_json() {
        assert_eq!(
            Manifest::from_json(&sample().to_json().unwrap()).unwrap(),
            sample()
        );
    }

    #[test]
    fn the_ids_are_object_keys_not_nested_objects() {
        // The wire shape is part of the published protocol, so it is asserted
        // rather than left to whatever serde happens to do with a newtype.
        let json = String::from_utf8(sample().to_json().unwrap()).unwrap();
        assert!(json.contains(&format!("\"{}\":", project(0x11))), "{json}");
        assert!(json.contains(&format!("\"{}\":", entry(0xa1))), "{json}");
    }

    #[test]
    fn an_unknown_field_does_not_break_loading() {
        // Forward compatibility: an older build must survive a manifest
        // written by a newer one rather than refuse the whole catalogue.
        let raw = br#"{"v":1,"projects":{},"somethingNew":true}"#;
        assert_eq!(Manifest::from_json(raw).unwrap().projects.len(), 0);
    }

    #[test]
    fn a_future_version_is_refused_rather_than_guessed() {
        assert!(matches!(
            Manifest::from_json(br#"{"v":2,"projects":{}}"#),
            Err(EnvCryptoError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn a_malformed_id_is_refused() {
        let raw = br#"{"v":1,"projects":{"not-an-id":{"name":"x","entries":{}}}}"#;
        assert!(Manifest::from_json(raw).is_err());
    }

    #[test]
    fn an_escaping_path_is_not_safe() {
        for path in [
            "../../../etc/passwd",
            "/etc/passwd",
            "a/../../b",
            "",
            "C:/Windows/system32",
            "\\\\server\\share",
        ] {
            let entry = ManifestEntry {
                path: path.into(),
                seq: 1,
            };
            assert!(!entry.has_a_safe_path(), "{path:?} was accepted");
        }
    }

    #[test]
    fn an_ordinary_relative_path_is_safe() {
        for path in [".env", ".env.local", "services/api/.env.production"] {
            let entry = ManifestEntry {
                path: path.into(),
                seq: 1,
            };
            assert!(entry.has_a_safe_path(), "{path:?} was refused");
        }
    }

    #[test]
    fn an_entry_can_be_traced_back_to_its_project() {
        let manifest = sample();
        let (id, found) = manifest.project_of(&entry(0xa1)).unwrap();
        assert_eq!(*id, project(0x11));
        assert_eq!(found.name, "acme-api");
        assert!(manifest.project_of(&entry(0xff)).is_none());
    }

    #[test]
    fn a_name_already_in_the_project_gets_the_next_index() {
        // Counting from 2 so the first duplicate reads as the second copy.
        let mut manifest = sample();
        let project = project(0x11);

        assert_eq!(manifest.unique_path(project, ".env", None), ".env");

        manifest
            .projects
            .get_mut(&project)
            .expect("the sample project")
            .entries
            .insert(
                entry(0xb1),
                ManifestEntry {
                    path: ".env".into(),
                    seq: 0,
                },
            );
        assert_eq!(manifest.unique_path(project, ".env", None), ".env (2)");

        manifest
            .projects
            .get_mut(&project)
            .expect("the sample project")
            .entries
            .insert(
                entry(0xb2),
                ManifestEntry {
                    path: ".env (2)".into(),
                    seq: 0,
                },
            );
        assert_eq!(manifest.unique_path(project, ".env", None), ".env (3)");
    }

    #[test]
    fn an_entry_does_not_collide_with_itself() {
        // Renaming something to the name it already has must not walk it to
        // `.env (2)` for colliding with itself.
        let manifest = sample();
        assert_eq!(
            manifest.unique_path(
                project(0x11),
                "services/api/.env.production",
                Some(entry(0xa1)),
            ),
            "services/api/.env.production",
        );
    }

    #[test]
    fn a_name_in_another_project_is_not_a_collision() {
        // Projects are separate namespaces; two checkouts both having a `.env`
        // is the normal case, not a clash.
        let manifest = sample();
        assert_eq!(
            manifest.unique_path(project(0x22), "services/api/.env.production", None),
            "services/api/.env.production",
        );
    }
}
