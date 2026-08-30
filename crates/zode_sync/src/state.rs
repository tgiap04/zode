use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::artifact;
use crate::envelope::Kind;

/// What this machine believes about one artifact as of the last successful
/// sync.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedAt {
    /// The server revision that was current when this machine last agreed with
    /// it.
    pub revision: String,
    /// Hash of the local file at that same moment. This is the field that
    /// makes "local was edited since" answerable.
    pub local_hash: String,
}

/// The whole of `sync_state.json`.
///
/// A `BTreeMap` rather than three named fields so a future artifact kind does
/// not invalidate files written by older builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    entries: BTreeMap<String, SyncedAt>,
}

impl SyncState {
    /// Loads the file, treating anything unreadable as "never synced".
    ///
    /// Absent, truncated, hand-edited, or written by an incompatible build all
    /// mean the same thing to the user: the next pull asks before it writes.
    /// None of them is worth refusing to sync over.
    pub fn load(path: &Path) -> Self {
        let Ok(Some(raw)) = artifact::read(path) else {
            return Self::default();
        };
        match serde_json::from_str(&raw) {
            Ok(state) => state,
            Err(error) => {
                log::warn!("sync_state.json could not be read, starting fresh: {error}");
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = serde_json::to_string_pretty(self)?;
        artifact::write_atomic(path, &raw)?;
        Ok(())
    }

    pub fn get(&self, kind: Kind) -> Option<&SyncedAt> {
        self.entries.get(kind.as_str())
    }

    pub fn record(&mut self, kind: Kind, revision: String, local_hash: String) {
        self.entries.insert(
            kind.as_str().to_string(),
            SyncedAt {
                revision,
                local_hash,
            },
        );
    }

    pub fn forget(&mut self, kind: Kind) {
        self.entries.remove(kind.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("zode-sync-state-{name}.json"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn round_trips_through_the_file() {
        let path = temp("roundtrip");
        let mut state = SyncState::default();
        state.record(Kind::Settings, "rev-1".into(), "hash-1".into());
        state.save(&path).unwrap();

        let loaded = SyncState::load(&path);
        assert_eq!(loaded.get(Kind::Settings).unwrap().revision, "rev-1");
        assert_eq!(loaded.get(Kind::Keymap), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn an_absent_file_means_never_synced() {
        assert_eq!(SyncState::load(&temp("absent")), SyncState::default());
    }

    #[test]
    fn a_corrupt_file_means_never_synced_rather_than_an_error() {
        let path = temp("corrupt");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(SyncState::load(&path), SyncState::default());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn unknown_kinds_in_the_file_do_not_break_loading() {
        let path = temp("forward");
        std::fs::write(
            &path,
            r#"{"entries":{"snippets":{"revision":"r","local_hash":"h"}}}"#,
        )
        .unwrap();
        let loaded = SyncState::load(&path);
        assert_eq!(loaded.get(Kind::Settings), None);
        std::fs::remove_file(&path).unwrap();
    }
}
