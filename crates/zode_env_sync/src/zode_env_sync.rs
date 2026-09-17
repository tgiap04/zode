//! End-to-end encrypted sync for `.env` files.
//!
//! Sits on top of [`zode_sync`] rather than beside it. The AEAD, the envelope
//! shape and the recovery key all come from there; what this crate adds is the
//! part `.env` needs and settings did not:
//!
//! - a second key ([`EnvDek`]) wrapped under the first, so env can be rotated
//!   without rotating settings and so a future "share with a teammate" has
//!   something to wrap;
//! - one blob per file instead of one per kind, each bound to its own
//!   [`EntryId`] in the AAD — without that the server can serve entry B's blob
//!   from entry A's slot and the tag still verifies, because the tag only
//!   covers the ciphertext;
//! - a monotonic `seq` inside the plaintext, so a server replaying an older
//!   blob is caught. For settings a rollback is an annoyance. For `.env` it is
//!   the restoration of a revoked credential;
//! - padding to a 4 KiB boundary, so the stored length does not say how many
//!   variables a file holds.
//!
//! **There is no password anywhere in this crate, and there must never be
//! one.** The server holds the ciphertext; a key derived from something a
//! person can remember is a key that server can grind offline. The whole
//! argument is in `zode_sync::dek`.
//!
//! Layering, kept as strict as `zode_sync`'s:
//!
//! - [`ids`] / [`env_dek`] / [`padding`] / [`seal`] / [`manifest`] — bytes in,
//!   bytes out. No I/O, so the crypto is testable against fixed vectors
//!   published in `docs/src/env-sync-protocol.md`.
//! - [`keystore`] — the one place the key touches the OS.
//!
//! Like `zode_account` and `zode_sync`, this crate must never be able to reach
//! `telemetry` or the remote-server crates;
//! `script/check-account-no-telemetry` asserts both against the dependency
//! graph.

pub mod bindings;
pub mod env_dek;
pub mod env_state;
pub mod env_sync;
pub mod ids;
pub mod keystore;
pub mod manifest;
pub mod padding;
pub mod seal;
pub mod secret_file;
pub mod session;
pub mod vault;
pub mod wire;

pub use bindings::Bindings;
pub use env_dek::{EnvDek, unwrap_key, wrap_key};
pub use env_state::{EnvState, SyncedEntry};
pub use env_sync::{Divergence, EnvSyncError, PullOutcome, PushOutcome, apply_remote, pull, push};
pub use ids::{EntryId, ProjectId};
pub use manifest::{Manifest, ManifestEntry, ManifestProject};
pub use padding::{MAX_PAYLOAD_BYTES, PAD_BLOCK};
pub use seal::{Opened, open, seal};
pub use secret_file::{BACKUPS_KEPT, back_up_secret, resolve_within, write_secret_atomic};
pub use session::{EnvSession, EnvStatus, EnvStatusChanged, PendingEnvDivergence};
pub use vault::{Reconciliation, RemoteEntry, parse_listing, reconcile};
pub use wire::{WireBytes, wire_bytes};

/// The name of the singleton holding the wrapped [`EnvDek`].
pub const ENV_KEY_RESOURCE: &str = "env-key";

/// The name of the singleton holding the encrypted [`Manifest`].
pub const ENV_MANIFEST_RESOURCE: &str = "env-manifest";

/// What can go wrong between an `.env` file and a blob.
///
/// `Rollback` is the variant that does not exist in `zode_sync`, and it is not
/// a decryption failure: the bytes were authentic, they were simply an older
/// version than this machine has already seen.
#[derive(Debug)]
pub enum EnvCryptoError {
    /// The tag did not verify, or the blob was served from the wrong slot.
    WrongKey,
    /// Written under a different env key — almost always a rotation elsewhere.
    KeyRotated {
        theirs: [u8; zode_sync::KID_LEN],
        ours: [u8; zode_sync::KID_LEN],
    },
    /// Authentic, but older than what this machine has already applied.
    Rollback { seen: u64, got: u64 },
    /// A format version this build does not understand.
    UnsupportedVersion(u32),
    /// Structurally wrong: a bad id, a truncated block, unusable JSON.
    Malformed(String),
    /// The payload is larger than the format carries.
    TooLarge { bytes: usize, limit: usize },
}

impl std::fmt::Display for EnvCryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongKey => write!(f, "that key does not open this environment file"),
            Self::KeyRotated { .. } => write!(
                f,
                "this environment file was encrypted with a different key — it was probably rotated on another machine",
            ),
            Self::Rollback { seen, got } => write!(
                f,
                "the server offered version {got} of this file, but this machine has already seen version {seen} — it was not written"
            ),
            Self::UnsupportedVersion(v) => write!(
                f,
                "this environment file uses format {v}, which this version of Zode cannot read"
            ),
            Self::Malformed(reason) => write!(f, "this is not a Zode environment blob: {reason}"),
            Self::TooLarge { bytes, limit } => {
                write!(f, "this file is {bytes} bytes; the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for EnvCryptoError {}

impl From<zode_sync::SyncCryptoError> for EnvCryptoError {
    fn from(error: zode_sync::SyncCryptoError) -> Self {
        use zode_sync::SyncCryptoError as S;
        match error {
            S::WrongKey => Self::WrongKey,
            S::KeyRotated { theirs, ours } => Self::KeyRotated { theirs, ours },
            S::UnsupportedVersion(v) => Self::UnsupportedVersion(v),
            S::Malformed(reason) => Self::Malformed(reason),
        }
    }
}
