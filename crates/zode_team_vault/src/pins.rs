//! What this machine has decided to trust about a team, and what it has seen.
//!
//! Three things, all of them defences against the server rather than secrets:
//!
//! - **pins**: each member's sharing key fingerprint, per (team, member), in
//!   one of two states. *Seen*: remembered without a comparison, only when
//!   this machine opens a team for the very first time and takes the sealer
//!   of its envelopes on trust. *Verified*: the user compared the
//!   fingerprint out of band and pressed Trust Key. Only a verified key is
//!   sealed to (grant, rotation); a seen key only opens envelopes. A later
//!   change is reported, never absorbed: the server is the one party that
//!   could substitute a key.
//! - **the key version floor**: the highest team key version seen per team.
//!   A server naming a lower one is refused, or the next write would be sealed
//!   under a key a removed member still holds.
//! - **seen seqs**: the highest `seq` opened per entry, so a replayed older
//!   entry is refused, as for `.env`.
//!
//! Stored the way `remote_relay_client::TrustStore` stores its pins: one
//! key-value record per account. A record that cannot be read is an error,
//! never an empty record — an empty record would trust the next key it sees.

use std::collections::BTreeMap;

use db::kvp::KeyValueStore;
use futures::FutureExt as _;
use futures::future::Shared;
use gpui::{App, AppContext as _, Task};
use serde::{Deserialize, Serialize};

use crate::sharing_key::Fingerprint;
use crate::{TeamCryptoError, TeamVaultError};

const RECORD_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedKey {
    pub fingerprint: Fingerprint,
    /// Who the key belonged to when it was pinned, for the change warning.
    pub email: String,
    /// Seconds since the Unix epoch.
    pub pinned_at: u64,
    /// The user compared this fingerprint and trusted it. False for a key
    /// remembered on first sight, which is never sealed to.
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinCheck {
    /// Never seen in this team. Trusting it is a decision the user makes.
    New,
    /// Matches a key remembered on first sight, never compared. Envelopes it
    /// sealed open; nothing is sealed to it until the user verifies it.
    Seen,
    /// Matches a key the user compared and trusted.
    Verified,
    /// Differs from the pin. Nothing is sealed to it, and nothing sealed by it
    /// is opened, until the user accepts the change.
    Changed { pinned: PinnedKey },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamTrust {
    pins: BTreeMap<(String, String), PinnedKey>,
    key_version_floor: BTreeMap<String, u32>,
    seen_seq: BTreeMap<(String, String), u64>,
}

impl TeamTrust {
    pub fn check(&self, team_id: &str, user_id: &str, fingerprint: &Fingerprint) -> PinCheck {
        match self.pinned(team_id, user_id) {
            None => PinCheck::New,
            Some(pinned) if &pinned.fingerprint == fingerprint && pinned.verified => {
                PinCheck::Verified
            }
            Some(pinned) if &pinned.fingerprint == fingerprint => PinCheck::Seen,
            Some(pinned) => PinCheck::Changed {
                pinned: pinned.clone(),
            },
        }
    }

    pub fn pinned(&self, team_id: &str, user_id: &str) -> Option<&PinnedKey> {
        self.pins.get(&(team_id.to_string(), user_id.to_string()))
    }

    /// The user compared this key and trusts it: a first look, or a change
    /// they accept. The only way a pin becomes verified.
    pub fn verify(
        &mut self,
        team_id: &str,
        user_id: &str,
        email: &str,
        fingerprint: Fingerprint,
        now: u64,
    ) {
        self.insert_pin(team_id, user_id, email, fingerprint, now, true);
    }

    /// Remembers a sealer's key on first sight, unverified. Only for a team
    /// this machine has never opened ([`Self::has_opened`]), and never over
    /// an existing pin.
    pub fn remember_first_sight(
        &mut self,
        team_id: &str,
        user_id: &str,
        email: &str,
        fingerprint: Fingerprint,
        now: u64,
    ) {
        if self.pinned(team_id, user_id).is_none() {
            self.insert_pin(team_id, user_id, email, fingerprint, now, false);
        }
    }

    fn insert_pin(
        &mut self,
        team_id: &str,
        user_id: &str,
        email: &str,
        fingerprint: Fingerprint,
        now: u64,
        verified: bool,
    ) {
        self.pins.insert(
            (team_id.to_string(), user_id.to_string()),
            PinnedKey {
                fingerprint,
                email: email.to_string(),
                pinned_at: now,
                verified,
            },
        );
    }

    /// Whether this machine has opened, or decided anything about, this team
    /// before. Once it has, a sealer it has never seen is not taken on trust:
    /// that is exactly what a server inventing a member would look like.
    pub fn has_opened(&self, team_id: &str) -> bool {
        self.key_version_floor(team_id) > 0
            || self
                .pins
                .keys()
                .any(|(pinned_team, _)| pinned_team == team_id)
    }

    pub fn key_version_floor(&self, team_id: &str) -> u32 {
        self.key_version_floor.get(team_id).copied().unwrap_or(0)
    }

    /// Takes the server's word for the current key version only if it does
    /// not go backwards. Returns whether the floor moved.
    pub fn observe_key_version(
        &mut self,
        team_id: &str,
        version: u32,
    ) -> Result<bool, TeamCryptoError> {
        let seen = self.key_version_floor(team_id);
        if version < seen {
            return Err(TeamCryptoError::KeyVersionRollback { seen, got: version });
        }
        if version > seen {
            self.key_version_floor.insert(team_id.to_string(), version);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn seen_seq(&self, team_id: &str, entry_id: &str) -> Option<u64> {
        self.seen_seq
            .get(&(team_id.to_string(), entry_id.to_string()))
            .copied()
    }

    /// Records an opened or written `seq`. Never lowers it. Returns whether it
    /// moved.
    pub fn record_seq(&mut self, team_id: &str, entry_id: &str, seq: u64) -> bool {
        let slot = self
            .seen_seq
            .entry((team_id.to_string(), entry_id.to_string()))
            .or_insert(0);
        if seq > *slot {
            *slot = seq;
            return true;
        }
        false
    }

    pub fn to_json(&self) -> Result<String, TeamVaultError> {
        let record = StoredRecord {
            v: RECORD_VERSION,
            pins: self
                .pins
                .iter()
                .map(|((team_id, user_id), pin)| StoredPin {
                    team_id: team_id.clone(),
                    user_id: user_id.clone(),
                    email: pin.email.clone(),
                    fingerprint: pin.fingerprint.to_hex(),
                    pinned_at: pin.pinned_at,
                    verified: pin.verified,
                })
                .collect(),
            floors: self.key_version_floor.clone(),
            seen: self
                .seen_seq
                .iter()
                .map(|((team_id, entry_id), seq)| StoredSeq {
                    team_id: team_id.clone(),
                    entry_id: entry_id.clone(),
                    seq: *seq,
                })
                .collect(),
        };
        serde_json::to_string(&record).map_err(|error| TeamVaultError::Storage(error.to_string()))
    }

    /// Strict: a record with one unreadable pin is unreadable. Dropping that
    /// pin would turn its member back into `New`, which is exactly what a key
    /// substitution needs.
    pub fn from_json(raw: &str) -> Result<Self, TeamVaultError> {
        let record: StoredRecord = serde_json::from_str(raw)
            .map_err(|error| TeamVaultError::Storage(format!("unreadable: {error}")))?;
        if record.v != RECORD_VERSION {
            return Err(TeamVaultError::Storage(format!(
                "record version {} is not supported",
                record.v
            )));
        }
        let mut trust = Self {
            key_version_floor: record.floors,
            ..Self::default()
        };
        for pin in record.pins {
            let fingerprint = Fingerprint::from_hex(&pin.fingerprint).ok_or_else(|| {
                TeamVaultError::Storage(format!("an unreadable pin for {}", pin.user_id))
            })?;
            trust.insert_pin(
                &pin.team_id,
                &pin.user_id,
                &pin.email,
                fingerprint,
                pin.pinned_at,
                pin.verified,
            );
        }
        for seen in record.seen {
            trust.record_seq(&seen.team_id, &seen.entry_id, seen.seq);
        }
        Ok(trust)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRecord {
    v: u32,
    pins: Vec<StoredPin>,
    floors: BTreeMap<String, u32>,
    seen: Vec<StoredSeq>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPin {
    team_id: String,
    user_id: String,
    email: String,
    fingerprint: String,
    pinned_at: u64,
    /// Absent in a record written before pins had two states: such a pin was
    /// never compared by anyone, so it reads as seen, not verified.
    #[serde(default)]
    verified: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSeq {
    team_id: String,
    entry_id: String,
    seq: u64,
}

fn storage_key(user_id: &str) -> String {
    format!("zode_team_trust:{user_id}")
}

/// Reads this account's record. Nothing stored is an empty record; anything
/// unreadable is an error.
pub fn load(user_id: &str, cx: &App) -> Task<Result<TeamTrust, TeamVaultError>> {
    let kvp = KeyValueStore::global(cx);
    let key = storage_key(user_id);
    cx.background_spawn(async move {
        match kvp.read_kvp(&key) {
            Ok(None) => Ok(TeamTrust::default()),
            Ok(Some(raw)) => TeamTrust::from_json(&raw),
            Err(error) => Err(TeamVaultError::Storage(format!("{error:#}"))),
        }
    })
}

/// A write in flight. Shared so the next write can wait for it.
pub type SaveHandle = Shared<Task<Result<(), TeamVaultError>>>;

/// Writes this account's record, after `previous` has finished, so the store
/// sees snapshots in the order they were taken. Callers await the handle
/// before acting on the change: a pin that did not reach the disk is never
/// relied on.
pub fn save(
    user_id: &str,
    trust: &TeamTrust,
    previous: Option<SaveHandle>,
    cx: &App,
) -> SaveHandle {
    let value = trust.to_json();
    let kvp = KeyValueStore::global(cx);
    let key = storage_key(user_id);
    cx.background_spawn(async move {
        // The earlier write's outcome belongs to its own caller; this one
        // only needs it to be over.
        if let Some(previous) = previous
            && let Err(error) = previous.await
        {
            log::debug!("an earlier team trust write failed: {error}");
        }
        kvp.write_kvp(key, value?)
            .await
            .map_err(|error| TeamVaultError::Storage(format!("{error:#}")))
    })
    .shared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";
    const OTHER_TEAM: &str = "68b1f0c2a4d3e5f607182911";
    const MEMBER: &str = "68b1f0c2a4d3e5f60718293b";

    fn fingerprint(byte: u8) -> Fingerprint {
        Fingerprint::of(&[byte; 32])
    }

    fn pinned() -> TeamTrust {
        let mut trust = TeamTrust::default();
        trust.verify(
            TEAM,
            MEMBER,
            "bo@example.com",
            fingerprint(1),
            1_700_000_000,
        );
        trust
    }

    #[test]
    fn a_pin_answers_new_verified_and_changed() {
        let trust = pinned();
        assert_eq!(
            trust.check(TEAM, "someone-else", &fingerprint(1)),
            PinCheck::New
        );
        assert_eq!(
            trust.check(TEAM, MEMBER, &fingerprint(1)),
            PinCheck::Verified
        );
        assert_eq!(
            trust.check(TEAM, MEMBER, &fingerprint(2)),
            PinCheck::Changed {
                pinned: PinnedKey {
                    fingerprint: fingerprint(1),
                    email: "bo@example.com".into(),
                    pinned_at: 1_700_000_000,
                    verified: true,
                }
            }
        );
    }

    #[test]
    fn a_pin_in_one_team_says_nothing_about_another() {
        assert_eq!(
            pinned().check(OTHER_TEAM, MEMBER, &fingerprint(1)),
            PinCheck::New
        );
    }

    #[test]
    fn accepting_a_change_is_re_pinning() {
        let mut trust = pinned();
        trust.verify(
            TEAM,
            MEMBER,
            "bo@example.com",
            fingerprint(2),
            1_800_000_000,
        );
        assert_eq!(
            trust.check(TEAM, MEMBER, &fingerprint(2)),
            PinCheck::Verified
        );
    }

    #[test]
    fn a_first_sight_is_seen_and_only_trusting_it_verifies_it() {
        let mut trust = TeamTrust::default();
        assert!(!trust.has_opened(TEAM));
        trust.remember_first_sight(TEAM, MEMBER, "bo@example.com", fingerprint(1), 1);
        assert_eq!(trust.check(TEAM, MEMBER, &fingerprint(1)), PinCheck::Seen);
        assert!(trust.has_opened(TEAM));
        assert!(!trust.has_opened(OTHER_TEAM));

        // A first sight never overwrites what is pinned.
        trust.remember_first_sight(TEAM, MEMBER, "bo@example.com", fingerprint(2), 2);
        assert!(matches!(
            trust.check(TEAM, MEMBER, &fingerprint(2)),
            PinCheck::Changed { .. }
        ));

        trust.verify(TEAM, MEMBER, "bo@example.com", fingerprint(1), 3);
        assert_eq!(
            trust.check(TEAM, MEMBER, &fingerprint(1)),
            PinCheck::Verified
        );

        let mut floor_only = TeamTrust::default();
        floor_only.observe_key_version(TEAM, 1).unwrap();
        assert!(floor_only.has_opened(TEAM));
    }

    #[test]
    fn the_key_version_floor_only_rises() {
        let mut trust = TeamTrust::default();
        assert_eq!(trust.observe_key_version(TEAM, 2), Ok(true));
        assert_eq!(trust.observe_key_version(TEAM, 2), Ok(false));
        assert_eq!(
            trust.observe_key_version(TEAM, 1),
            Err(TeamCryptoError::KeyVersionRollback { seen: 2, got: 1 })
        );
        assert_eq!(trust.key_version_floor(TEAM), 2);
        assert_eq!(trust.key_version_floor(OTHER_TEAM), 0);
    }

    #[test]
    fn a_seen_seq_never_goes_down() {
        let mut trust = TeamTrust::default();
        assert!(trust.record_seq(TEAM, "e1", 5));
        assert!(!trust.record_seq(TEAM, "e1", 3));
        assert_eq!(trust.seen_seq(TEAM, "e1"), Some(5));
        assert_eq!(trust.seen_seq(OTHER_TEAM, "e1"), None);
    }

    #[test]
    fn the_record_round_trips_and_an_unreadable_one_is_an_error() {
        let mut trust = pinned();
        trust.observe_key_version(TEAM, 3).unwrap();
        trust.record_seq(TEAM, "e1", 7);
        let json = trust.to_json().unwrap();
        assert_eq!(TeamTrust::from_json(&json).unwrap(), trust);

        assert!(TeamTrust::from_json("{").is_err());
        let damaged = json.replace(&fingerprint(1).to_hex(), "zz");
        assert!(
            TeamTrust::from_json(&damaged).is_err(),
            "a damaged pin must not quietly become no pin"
        );

        let mut seen = TeamTrust::default();
        seen.remember_first_sight(TEAM, MEMBER, "bo@example.com", fingerprint(1), 1);
        let reloaded = TeamTrust::from_json(&seen.to_json().unwrap()).unwrap();
        assert_eq!(
            reloaded.check(TEAM, MEMBER, &fingerprint(1)),
            PinCheck::Seen
        );

        // A pin from before the two states has no flag: it was never compared.
        let legacy = json.replace(",\"verified\":true", "");
        assert_ne!(legacy, json);
        assert_eq!(
            TeamTrust::from_json(&legacy)
                .unwrap()
                .check(TEAM, MEMBER, &fingerprint(1)),
            PinCheck::Seen
        );
    }

    #[gpui::test]
    async fn the_record_survives_a_reload_per_account(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
        let trust = pinned();
        let first = cx.update(|cx| save("user-a", &TeamTrust::default(), None, cx));
        cx.update(|cx| save("user-a", &trust, Some(first), cx))
            .await
            .unwrap();

        let reloaded = cx.update(|cx| load("user-a", cx)).await.unwrap();
        assert_eq!(reloaded, trust);
        let other = cx.update(|cx| load("user-b", cx)).await.unwrap();
        assert_eq!(other, TeamTrust::default());
    }
}
