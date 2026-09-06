//! End-to-end encrypted sync for Zode's own configuration.
//!
//! The server stores ciphertext and nothing else. It has no key, never
//! receives one, and the API it exposes (`/api/sync/:kind`) treats every blob
//! as opaque bytes. That is not a policy the backend chose to be polite about;
//! it is the reason `settings.json` can be synced at all, because that file
//! holds `language_models.*.api_key`, `terminal.env`, and `remote_env`.
//!
//! The consequence the user pays for is real and must never be softened in the
//! UI: losing the recovery key loses the synced data, and no support ticket
//! can undo that. That IS what zero-knowledge means.
//!
//! Layering, kept deliberately strict so the crypto stays testable with fixed
//! vectors instead of a live server:
//!
//! - [`dek`] / [`recovery_key`] / [`envelope`] / [`diff`] — bytes in, bytes
//!   out. No I/O at all, so the crypto is testable with fixed vectors rather
//!   than against a live server.
//! - [`artifact`] / [`state`] — the local files.
//! - [`keystore`] — the one place the key touches the OS.
//! - [`client`] — the one place it touches the network.
//! - [`sync`] — the decisions, built from all of the above.
//!
//! Sync is local-only by decision D7: the machine running the UI pushes and
//! pulls, and `remote_server` is not involved. Nothing in this crate may
//! depend on `remote`, `remote_server`, or `remote_connection`.
//!
//! Like `zode_account`, this crate must never be able to reach `telemetry`;
//! `script/check-account-no-telemetry` asserts that against the dependency
//! graph.

pub mod artifact;
pub mod client;
pub mod dek;
pub mod diff;
pub mod envelope;
pub mod extensions;
pub mod keystore;
pub mod recovery_key;
pub mod rotate;
pub mod session;
pub mod state;
pub mod sync;

pub use dek::{DEK_LEN, Dek, KID_LEN};
pub use diff::TextDiff;
pub use envelope::{
    ENVELOPE_VERSION, Envelope, Kind, SyncCryptoError, decrypt, encrypt, from_blob, to_blob,
};
pub use extensions::ExtensionComparison;
pub use recovery_key::RecoveryKeyError;
pub use rotate::{RotationOutcome, RotationPlan};
pub use session::{PendingDivergence, SyncSession, SyncStatus, SyncStatusChanged};
pub use state::SyncState;
pub use sync::{PullOutcome, PushOutcome, SyncError, pull, push};

/// Installs the global [`SyncSession`].
///
/// Reads nothing and sends nothing. The encryption key is loaded from the
/// keychain the first time the user opens sync, not here — starting the editor
/// must not touch the keychain for a feature nobody has asked for yet, and it
/// must never touch the network.
pub fn init(account: gpui::Entity<zode_account::Account>, cx: &mut gpui::App) {
    use gpui::AppContext as _;
    let session = cx.new(|_| SyncSession::new(account));
    SyncSession::set_global(session, cx);
}
