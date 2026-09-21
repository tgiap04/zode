use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ids::EntryId;

/// What this machine believes about one environment file as of the last
/// successful sync.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedEntry {
    /// The server revision current when this machine last agreed with it.
    pub revision: String,
    /// Hash of the local file at that same moment. This is what makes "the
    /// local file has been edited since" answerable without keeping a copy.
    pub local_hash: String,
    /// The highest version this machine has applied.
    ///
    /// The field `sync_state.json` has no equivalent of, and the reason this
    /// file exists separately. A server can hand back an old blob; it cannot
    /// manufacture a newer one. Comparing against this is what turns that
    /// asymmetry into a refusal.
    pub seq: u64,
}

/// The whole of `env_state.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvState {
    #[serde(default)]
    entries: BTreeMap<String, SyncedEntry>,
    /// The manifest is versioned too — a rolled-back catalogue hides files
    /// rather than changing them, which is quieter and just as bad.
    #[serde(default)]
    manifest_seq: u64,
}

impl EnvState {
    /// Loads the file, treating anything unreadable as "never synced".
    ///
    /// Absent, truncated, hand-edited or written by an incompatible build all
    /// mean the same thing to the user — the next pull asks before it writes —
    /// and none is worth refusing to sync over.
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(error) => {
                log::warn!("env_state.json could not be read, starting fresh: {error}");
                Self::default()
            }
        }
    }

    /// Saves it, with the same permissions as the files it describes.
    ///
    /// It holds no plaintext — hashes, revisions and counters only — but it
    /// does say which entries this machine has, and there is no reason for
    /// that to be world-readable when everything beside it is not.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = serde_json::to_vec_pretty(self)?;
        crate::secret_file::write_secret_atomic(path, &raw)?;
        Ok(())
    }

    pub fn get(&self, entry: &EntryId) -> Option<&SyncedEntry> {
        self.entries.get(&entry.as_hex())
    }

    /// The last version applied for an entry, for the rollback check.
    pub fn seen_seq(&self, entry: &EntryId) -> Option<u64> {
        self.get(entry).map(|synced| synced.seq)
    }

    pub fn manifest_seq(&self) -> Option<u64> {
        (self.manifest_seq > 0).then_some(self.manifest_seq)
    }

    pub fn record(&mut self, entry: &EntryId, revision: String, local_hash: String, seq: u64) {
        self.entries.insert(
            entry.as_hex(),
            SyncedEntry {
                revision,
                local_hash,
                seq,
            },
        );
    }

    pub fn record_manifest(&mut self, seq: u64) {
        self.manifest_seq = seq;
    }

    pub fn forget(&mut self, entry: &EntryId) {
        self.entries.remove(&entry.as_hex());
    }

    pub fn known_entries(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }
}

/// Content hash, for comparing local against remote without keeping either.
pub fn hash(content: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    format!("{:x}", Sha256::digest(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("zode-env-state-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("a valid fixture id")
    }

    #[test]
    fn round_trips_through_the_file() {
        let path = temp("roundtrip");
        let mut state = EnvState::default();
        state.record(&entry(0xa1), "rev-1".into(), "hash-1".into(), 4);
        state.save(&path).unwrap();

        let loaded = EnvState::load(&path);
        assert_eq!(loaded.get(&entry(0xa1)).unwrap().revision, "rev-1");
        assert_eq!(loaded.seen_seq(&entry(0xa1)), Some(4));
        assert_eq!(loaded.get(&entry(0xb2)), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_absent_file_means_never_synced() {
        assert_eq!(EnvState::load(&temp("absent")), EnvState::default());
    }

    #[test]
    fn a_corrupt_file_means_never_synced_rather_than_an_error() {
        let path = temp("corrupt");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(EnvState::load(&path), EnvState::default());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn the_state_file_is_not_world_readable() {
        let path = temp("mode");
        EnvState::default().save(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_machine_with_no_record_reports_no_seen_version() {
        // `None` and `Some(0)` must not be confused: the first accepts any
        // version, the second would refuse everything below one.
        assert_eq!(EnvState::default().seen_seq(&entry(0xa1)), None);
        assert_eq!(EnvState::default().manifest_seq(), None);
    }

    #[test]
    fn forgetting_an_entry_clears_its_version_too() {
        let mut state = EnvState::default();
        state.record(&entry(0xa1), "r".into(), "h".into(), 9);
        state.forget(&entry(0xa1));
        assert_eq!(state.seen_seq(&entry(0xa1)), None);
    }
}
