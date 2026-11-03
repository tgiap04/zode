use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use zode_env_sync::EntryId;

use crate::TeamCryptoError;

/// The reserved entry id the manifest lives at: 32 zeros. A client
/// convention; the server never learns which entry is which.
pub const MANIFEST_ENTRY_ID: &str = "00000000000000000000000000000000";

/// Manifest format version. A reader that meets another refuses rather than
/// rewriting a catalogue it only half understands.
pub const MANIFEST_VERSION: u32 = 1;

pub fn manifest_entry_id() -> Result<EntryId, TeamCryptoError> {
    Ok(EntryId::parse(MANIFEST_ENTRY_ID)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEntryKind {
    Env,
    DbConnection,
}

/// What a team calls one entry. Sealed with the rest of the manifest, so the
/// server sees only opaque ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub entry_id: EntryId,
    pub kind: ManifestEntryKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_hint: Option<String>,
}

/// The team's catalogue of shared entries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamManifest {
    entries: Vec<ManifestEntry>,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    entries: Vec<ManifestEntry>,
}

impl TeamManifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    pub fn get(&self, entry_id: &EntryId) -> Option<&ManifestEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.entry_id == entry_id)
    }

    /// Adds an entry, or replaces the one with the same id.
    pub fn upsert(&mut self, entry: ManifestEntry) -> Result<(), TeamCryptoError> {
        if entry.entry_id == manifest_entry_id()? {
            return Err(TeamCryptoError::Malformed(
                "the manifest cannot list its own slot".into(),
            ));
        }
        match self
            .entries
            .iter_mut()
            .find(|existing| existing.entry_id == entry.entry_id)
        {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
        Ok(())
    }

    pub fn remove(&mut self, entry_id: &EntryId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| &entry.entry_id != entry_id);
        self.entries.len() != before
    }

    pub fn to_json(&self) -> Result<Vec<u8>, TeamCryptoError> {
        serde_json::to_vec(&Wire {
            v: MANIFEST_VERSION,
            entries: self.entries.clone(),
        })
        .map_err(|error| TeamCryptoError::Malformed(error.to_string()))
    }

    /// Strict: another version, the reserved id, or one id listed twice all
    /// mean this was not written by Zode, and are refused rather than repaired.
    pub fn from_json(bytes: &[u8]) -> Result<Self, TeamCryptoError> {
        let wire: Wire = serde_json::from_slice(bytes)
            .map_err(|error| TeamCryptoError::Malformed(format!("team manifest: {error}")))?;
        if wire.v != MANIFEST_VERSION {
            return Err(TeamCryptoError::Malformed(format!(
                "team manifest version {} is not supported",
                wire.v
            )));
        }
        let reserved = manifest_entry_id()?;
        let mut seen = HashSet::with_capacity(wire.entries.len());
        for entry in &wire.entries {
            if entry.entry_id == reserved || !seen.insert(entry.entry_id) {
                return Err(TeamCryptoError::Malformed(format!(
                    "team manifest lists {} where it may not",
                    entry.entry_id
                )));
            }
        }
        Ok(Self {
            entries: wire.entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(byte: u8, kind: ManifestEntryKind) -> ManifestEntry {
        ManifestEntry {
            entry_id: EntryId::parse(&format!("{byte:02x}").repeat(16)).unwrap(),
            kind,
            label: format!("entry {byte}"),
            project_hint: (kind == ManifestEntryKind::Env).then(|| "acme-api".into()),
        }
    }

    #[test]
    fn a_manifest_round_trips_through_its_wire_form() {
        let mut manifest = TeamManifest::new();
        manifest
            .upsert(entry(0xa1, ManifestEntryKind::Env))
            .unwrap();
        manifest
            .upsert(entry(0xb2, ManifestEntryKind::DbConnection))
            .unwrap();
        let json = manifest.to_json().unwrap();
        let text = String::from_utf8(json.clone()).unwrap();
        assert!(text.contains("\"kind\":\"db_connection\""), "{text}");
        assert!(text.contains("\"entryId\""), "{text}");
        assert_eq!(TeamManifest::from_json(&json).unwrap(), manifest);
    }

    #[test]
    fn upsert_replaces_and_remove_forgets() {
        let mut manifest = TeamManifest::new();
        manifest
            .upsert(entry(0xa1, ManifestEntryKind::Env))
            .unwrap();
        let mut renamed = entry(0xa1, ManifestEntryKind::Env);
        renamed.label = "renamed".into();
        manifest.upsert(renamed.clone()).unwrap();
        assert_eq!(manifest.entries(), &[renamed.clone()]);
        assert!(manifest.remove(&renamed.entry_id));
        assert!(!manifest.remove(&renamed.entry_id));
    }

    #[test]
    fn the_reserved_slot_and_duplicates_are_refused() {
        let mut manifest = TeamManifest::new();
        let mut own_slot = entry(0, ManifestEntryKind::Env);
        own_slot.entry_id = manifest_entry_id().unwrap();
        assert!(manifest.upsert(own_slot).is_err());

        let duplicate = serde_json::json!({
            "v": 1,
            "entries": [
                { "entryId": "a1".repeat(16), "kind": "env", "label": "a" },
                { "entryId": "a1".repeat(16), "kind": "env", "label": "b" },
            ]
        });
        assert!(TeamManifest::from_json(duplicate.to_string().as_bytes()).is_err());
    }

    #[test]
    fn another_version_or_kind_is_refused() {
        for body in [
            serde_json::json!({ "v": 2, "entries": [] }),
            serde_json::json!({ "v": 1, "entries": [
                { "entryId": "a1".repeat(16), "kind": "ssh_key", "label": "a" }
            ] }),
        ] {
            assert!(
                TeamManifest::from_json(body.to_string().as_bytes()).is_err(),
                "{body}"
            );
        }
    }
}
