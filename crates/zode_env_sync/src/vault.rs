use serde::Deserialize;

use crate::EnvCryptoError;
use crate::ids::EntryId;
use crate::manifest::Manifest;

/// One row of `GET /api/env`. No ciphertext — see the backend docs.
#[derive(Clone, Debug, Deserialize)]
pub struct RemoteEntry {
    #[serde(rename = "entryId")]
    pub entry_id: String,
    pub revision: String,
    #[serde(rename = "byteLength")]
    pub byte_length: usize,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    entries: Vec<RemoteEntry>,
}

/// Reads the listing the server answered with.
pub fn parse_listing(body: &str) -> Result<Vec<RemoteEntry>, EnvCryptoError> {
    let parsed: ListResponse = serde_json::from_str(body)
        .map_err(|error| EnvCryptoError::Malformed(format!("the entry listing: {error}")))?;
    Ok(parsed.entries)
}

/// Where the local catalogue and the server disagree about which files exist.
///
/// Both directions matter and they mean different things, which is why this is
/// two lists rather than a boolean.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reconciliation {
    /// In this machine's catalogue, absent from the server.
    ///
    /// Deleted on another machine. The local file is NOT removed on the
    /// strength of this — a server that forgot an entry, or answered a
    /// truncated listing, would otherwise delete a user's credentials. It is
    /// reported so the user decides.
    pub deleted_elsewhere: Vec<EntryId>,
    /// On the server, absent from this machine's catalogue.
    ///
    /// The catalogue is stale — another machine added a file and this one has
    /// not pulled the manifest since. Fixed by pulling it, not by guessing.
    pub unknown_here: Vec<EntryId>,
    /// Identifiers the server sent that are not well-formed.
    ///
    /// Kept rather than dropped: a server inventing identifiers is worth
    /// surfacing, and silently ignoring them would hide it.
    pub unreadable: Vec<String>,
}

/// Compares the encrypted catalogue against what the server says it holds.
pub fn reconcile(manifest: &Manifest, remote: &[RemoteEntry]) -> Reconciliation {
    let mut result = Reconciliation::default();

    let mut on_server = Vec::with_capacity(remote.len());
    for entry in remote {
        match EntryId::parse(&entry.entry_id) {
            Ok(id) => on_server.push(id),
            Err(_) => result.unreadable.push(entry.entry_id.clone()),
        }
    }

    let known: Vec<EntryId> = manifest.entry_ids().copied().collect();

    result.deleted_elsewhere = known
        .iter()
        .filter(|id| !on_server.contains(id))
        .copied()
        .collect();
    result.unknown_here = on_server
        .iter()
        .filter(|id| !known.contains(id))
        .copied()
        .collect();

    result
}

impl Reconciliation {
    pub fn is_agreed(&self) -> bool {
        self.deleted_elsewhere.is_empty()
            && self.unknown_here.is_empty()
            && self.unreadable.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProjectId;
    use crate::manifest::{ManifestEntry, ManifestProject};
    use std::collections::BTreeMap;

    fn entry(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("a valid fixture id")
    }

    fn remote(id: &EntryId) -> RemoteEntry {
        RemoteEntry {
            entry_id: id.as_hex(),
            revision: "rev".into(),
            byte_length: 4096,
        }
    }

    fn manifest_with(ids: &[EntryId]) -> Manifest {
        let mut manifest = Manifest::new();
        manifest.projects.insert(
            ProjectId::parse(&"11".repeat(16)).unwrap(),
            ManifestProject {
                name: "acme".into(),
                entries: ids
                    .iter()
                    .map(|id| {
                        (
                            *id,
                            ManifestEntry {
                                path: ".env".into(),
                                seq: 1,
                            },
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
            },
        );
        manifest
    }

    #[test]
    fn agreement_reports_nothing() {
        let reconciliation = reconcile(
            &manifest_with(&[entry(0xa1), entry(0xb2)]),
            &[remote(&entry(0xa1)), remote(&entry(0xb2))],
        );
        assert!(reconciliation.is_agreed(), "{reconciliation:?}");
    }

    #[test]
    fn a_file_removed_on_another_machine_is_reported() {
        let reconciliation = reconcile(
            &manifest_with(&[entry(0xa1), entry(0xb2)]),
            &[remote(&entry(0xa1))],
        );
        assert_eq!(reconciliation.deleted_elsewhere, vec![entry(0xb2)]);
        assert!(reconciliation.unknown_here.is_empty());
    }

    #[test]
    fn a_file_added_on_another_machine_is_reported() {
        let reconciliation = reconcile(
            &manifest_with(&[entry(0xa1)]),
            &[remote(&entry(0xa1)), remote(&entry(0xb2))],
        );
        assert_eq!(reconciliation.unknown_here, vec![entry(0xb2)]);
        assert!(reconciliation.deleted_elsewhere.is_empty());
    }

    #[test]
    fn an_identifier_the_server_invented_is_surfaced_not_swallowed() {
        let reconciliation = reconcile(
            &manifest_with(&[]),
            &[RemoteEntry {
                entry_id: "../../etc/passwd".into(),
                revision: "rev".into(),
                byte_length: 1,
            }],
        );
        assert_eq!(
            reconciliation.unreadable,
            vec!["../../etc/passwd".to_string()]
        );
        assert!(!reconciliation.is_agreed());
    }

    #[test]
    fn a_listing_round_trips_from_the_wire() {
        let body = r#"{"entries":[{"entryId":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","revision":"r","updatedAt":"2026-09-17T00:00:00.000Z","byteLength":4096}]}"#;
        let entries = parse_listing(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].byte_length, 4096);
    }

    #[test]
    fn an_unusable_listing_is_an_error_rather_than_an_empty_vault() {
        // Treating a broken listing as "the server holds nothing" would report
        // every file the user has as deleted elsewhere.
        assert!(parse_listing("not json").is_err());
        assert!(parse_listing("{}").is_err());
    }
}
