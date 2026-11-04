//! End-to-end encrypted sharing of `.env` files and database connections
//! inside a team.
//!
//! The server stores, without reading, three things: each member's public
//! sharing key, one sealed copy of the team key per (member, key version), and
//! vault entries encrypted under the team key. Every key is made and opened
//! here, on the member's own machine.
//!
//! The trust model is in `docs/src/team-sharing-protocol.md`, and the one
//! residual risk is stated there rather than hidden: the server could hand an
//! admin a public key that is not the member's. Pins ([`pins`]) are the
//! counter: only a key the user verified is sealed to, a sealer is taken on
//! first sight only by a machine opening the team for the first time, and a
//! later change is surfaced, never silently accepted.
//!
//! Layering, as strict as `zode_sync`'s:
//!
//! - [`sharing_key`] / [`team_key`] / [`seal`] / [`manifest`] /
//!   [`connection_entry`] — bytes in, bytes out. No I/O, so the crypto is
//!   testable against the vectors published in the protocol page.
//! - [`pins`] — the one place trust decisions touch local storage.
//! - [`client`] — the one place this crate touches the network.
//! - [`grant`] / [`rotate`] / [`session`] — the decisions.
//!
//! Like the other crates holding keys, this one must never be able to reach
//! `telemetry` or the remote-server crates; `script/check-account-no-telemetry`
//! asserts both against the dependency graph.

pub mod client;
pub mod connection_entry;
pub mod grant;
pub mod manifest;
pub mod pins;
pub mod rotate;
pub mod seal;
pub mod session;
pub mod sharing_key;
pub mod team_key;

pub use connection_entry::ConnectionEntry;
pub use manifest::{MANIFEST_ENTRY_ID, ManifestEntry, ManifestEntryKind, TeamManifest};
pub use pins::{PinCheck, PinnedKey, TeamTrust};
pub use seal::{OpenedEntry, open_entry, seal_entry};
pub use session::{
    ManifestChange, MemberView, PulledEntry, PushBase, RotationReport, TeamRole, TeamVaultSession,
    TeamVaultStatus, TeamVaultStatusChanged, VaultEntryView, init,
};
pub use sharing_key::{FINGERPRINT_LEN, Fingerprint, SharingKeypair};
pub use team_key::{
    EnvelopeIds, TEAM_KEY_LEN, TeamKey, TeamKeyring, WRAPPED_KEY_LEN, unwrap, wrap,
};

use zode_sync::client::ClientError;

/// What can go wrong between a key, an envelope and a vault entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamCryptoError {
    /// Authentication failed: the wrong key, a tampered byte, or an envelope
    /// meant for another team, member or key version.
    WrongKey,
    /// The entry was sealed under another team key than the one its listing
    /// names. Almost always a rotation the listing has not caught up with.
    KeyRotated,
    /// The envelope opened, but the member who sealed it is not the one this
    /// machine pinned. The key inside is not used.
    WrapperMismatch {
        expected: Fingerprint,
        found: Fingerprint,
    },
    /// A public key against which Diffie–Hellman yields a secret anyone can
    /// compute. Refused wherever one can enter.
    LowOrderKey,
    /// Authentic, but older than what this machine has already seen.
    Rollback { seen: u64, got: u64 },
    /// The server named a team key version lower than one this machine has
    /// already seen. Writing under it would hand the data to whoever still
    /// holds the older key — a removed member, for instance.
    KeyVersionRollback { seen: u32, got: u32 },
    /// The entry says it was sealed under one version and opened under
    /// another.
    KeyVersionMismatch { listed: u32, sealed: u32 },
    /// Structurally wrong: a bad id, a truncated envelope, unusable JSON.
    Malformed(String),
    /// The payload is larger than the format carries.
    TooLarge { bytes: usize, limit: usize },
}

impl std::fmt::Display for TeamCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongKey => write!(
                f,
                "this team data does not open with the keys on this machine"
            ),
            Self::KeyRotated => write!(
                f,
                "this entry was sealed under a different team key — the team key was probably rotated"
            ),
            Self::WrapperMismatch { expected, found } => write!(
                f,
                "the team key was sealed by a key ({found}) that is not the one pinned for that member ({expected}), so it was not used"
            ),
            Self::LowOrderKey => write!(f, "a public key that offers no secrecy was refused"),
            Self::Rollback { seen, got } => write!(
                f,
                "the server offered version {got} of this entry, but this machine has already seen version {seen}"
            ),
            Self::KeyVersionRollback { seen, got } => write!(
                f,
                "the server named team key version {got}, but this machine has already seen version {seen}"
            ),
            Self::KeyVersionMismatch { listed, sealed } => write!(
                f,
                "this entry is listed under team key version {listed} but was sealed under {sealed}"
            ),
            Self::Malformed(reason) => write!(f, "this is not Zode team data: {reason}"),
            Self::TooLarge { bytes, limit } => {
                write!(f, "this entry is {bytes} bytes; the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for TeamCryptoError {}

impl From<zode_sync::SyncCryptoError> for TeamCryptoError {
    fn from(error: zode_sync::SyncCryptoError) -> Self {
        use zode_sync::SyncCryptoError as S;
        match error {
            S::WrongKey => Self::WrongKey,
            S::KeyRotated { .. } => Self::KeyRotated,
            S::UnsupportedVersion(version) => {
                Self::Malformed(format!("envelope format {version} is not supported"))
            }
            S::Malformed(reason) => Self::Malformed(reason),
        }
    }
}

impl From<zode_env_sync::EnvCryptoError> for TeamCryptoError {
    fn from(error: zode_env_sync::EnvCryptoError) -> Self {
        use zode_env_sync::EnvCryptoError as E;
        match error {
            E::WrongKey => Self::WrongKey,
            E::KeyRotated { .. } => Self::KeyRotated,
            E::Rollback { seen, got } => Self::Rollback { seen, got },
            E::UnsupportedVersion(version) => {
                Self::Malformed(format!("format {version} is not supported"))
            }
            E::Malformed(reason) => Self::Malformed(reason),
            E::TooLarge { bytes, limit } => Self::TooLarge { bytes, limit },
        }
    }
}

/// What can go wrong in a team operation: the crypto, the network, or a
/// decision that is the user's to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamVaultError {
    Crypto(TeamCryptoError),
    Client(ClientError),
    /// A member's key differs from the one pinned for them. Nothing is sealed
    /// to it until the user accepts the change.
    KeyChanged {
        user_id: String,
    },
    /// A member's key is not verified on this machine: never compared, or
    /// only remembered on first sight. Nothing is sealed to it (grant,
    /// rotation), and on a team opened before nothing it sealed is opened,
    /// until the user compares the fingerprint and trusts it.
    NotPinned {
        user_id: String,
    },
    /// The member has not registered a sharing key yet.
    NoSharingKey {
        user_id: String,
    },
    /// The operation needs the team key, which this machine does not hold.
    NoTeamKey,
    /// The sharing key in the user's store does not open under the recovery
    /// key on this machine. Nothing is replaced on its own: a new sharing key
    /// costs every team envelope, so it is the user's explicit choice.
    SharingKeyUnreadable,
    /// The team moved to a newer key version since this machine last looked.
    KeyVersionStale,
    /// Someone else wrote first; the current document travels with it.
    Conflict,
    /// The local trust record could not be read or saved.
    Storage(String),
}

impl std::fmt::Display for TeamVaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Crypto(error) => write!(f, "{error}"),
            Self::Client(ClientError::Rejected(slug)) if slug == "billing_disabled" => write!(
                f,
                "team sharing is not available on Zode's service right now"
            ),
            Self::Client(ClientError::Rejected(slug)) if slug == "admin_required" => {
                write!(f, "only a team owner or admin can do this")
            }
            Self::Client(error) => write!(f, "{error}"),
            Self::KeyChanged { user_id } => write!(
                f,
                "the sharing key of member {user_id} changed since it was first seen; check it with them before granting"
            ),
            Self::NotPinned { user_id } => write!(
                f,
                "the sharing key of member {user_id} is not verified on this machine; compare its fingerprint with them, then trust it in the members window"
            ),
            Self::NoSharingKey { user_id } => write!(
                f,
                "member {user_id} has not opened team sharing in Zode yet"
            ),
            Self::NoTeamKey => write!(f, "this machine does not hold the team key yet"),
            Self::SharingKeyUnreadable => write!(
                f,
                "your sharing key was sealed under a different recovery key than the one on this machine"
            ),
            Self::KeyVersionStale => write!(
                f,
                "the team key was rotated since this list was loaded; refresh and try again"
            ),
            Self::Conflict => write!(f, "someone else changed this entry first"),
            Self::Storage(detail) => write!(f, "the local team trust record: {detail}"),
        }
    }
}

impl std::error::Error for TeamVaultError {}

impl From<TeamCryptoError> for TeamVaultError {
    fn from(error: TeamCryptoError) -> Self {
        Self::Crypto(error)
    }
}

impl From<ClientError> for TeamVaultError {
    fn from(error: ClientError) -> Self {
        Self::Client(error)
    }
}

impl From<zode_sync::SyncCryptoError> for TeamVaultError {
    fn from(error: zode_sync::SyncCryptoError) -> Self {
        Self::Crypto(error.into())
    }
}

impl From<zode_env_sync::EnvCryptoError> for TeamVaultError {
    fn from(error: zode_env_sync::EnvCryptoError) -> Self {
        Self::Crypto(error.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What reaches the Team tab is a sentence, not a wire slug, and being
    /// offline or signed out is not reported as a refused session.
    #[test]
    fn refusals_read_as_sentences() {
        let billing = TeamVaultError::Client(ClientError::Rejected("billing_disabled".into()));
        let shown = billing.to_string();
        assert!(!shown.contains("billing_disabled"), "{shown}");
        assert!(shown.contains("not available"), "{shown}");

        let missing = session::no_credential();
        assert_ne!(missing, TeamVaultError::Client(ClientError::Unauthorized));
        let shown = missing.to_string();
        assert!(shown.contains("signed in"), "{shown}");
    }
}
