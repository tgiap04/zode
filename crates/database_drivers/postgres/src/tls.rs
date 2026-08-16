//! TLS for the connection, verified against the platform's own trust store.
//!
//! `rustls-platform-verifier` rather than a bundled root list: a database
//! server on a company network is very often signed by a certificate authority
//! that only that machine trusts, and a bundled list would refuse it while
//! `psql` on the same machine connects.
//!
//! There is deliberately no "accept any certificate" switch. An unverified TLS
//! connection to a database is worse than a plain one, because it looks safe;
//! anyone who genuinely needs one can say `sslmode=disable` in the URL, which
//! at least says what it is.

use rustls::ClientConfig;
use rustls_platform_verifier::ConfigVerifierExt as _;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Picks the cryptography rustls will use, before anything asks it to.
///
/// Without this, rustls infers the provider from its own crate features and
/// panics when they name two -- which is not a hypothetical: `hyper-rustls` and
/// `tokio-rustls` elsewhere in this workspace ask for `ring`, this driver's
/// rustls asks for `aws-lc-rs`, and cargo unifies features across whatever
/// packages share one `cargo build`. The bundle scripts build this driver in
/// the same invocation as the editor, so the shipped binary had both and died
/// at the first TLS handshake with a message about crate features -- a fault
/// that appears and disappears depending on how the build was invoked.
///
/// Naming the provider outright takes that decision away from the build.
pub fn install_crypto_provider() {
    // The error only means another thread installed one first, and every
    // caller here installs the same one.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();
}

pub fn connector() -> MakeRustlsConnect {
    MakeRustlsConnect::new(ClientConfig::with_platform_verifier())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the config the way a real connection does. Before the provider
    /// was installed outright this panicked, and only in builds where another
    /// package had pulled in a second provider -- so a test that merely checked
    /// a flag would have passed while the shipped binary died.
    #[test]
    fn a_tls_connector_can_be_built() {
        install_crypto_provider();
        let _ = connector();
    }
}
