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

pub fn connector() -> MakeRustlsConnect {
    MakeRustlsConnect::new(ClientConfig::with_platform_verifier())
}
