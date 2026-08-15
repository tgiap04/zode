//! Talking to databases through driver sidecars.
//!
//! Zode holds no database driver of its own. Each engine is a separate process
//! speaking line-delimited JSON-RPC over its stdio, the way language servers and
//! debug adapters already do here -- so a driver that hangs or crashes costs a
//! process rather than the editor, and a third party can add an engine without
//! Zode being rebuilt.
//!
//! Layers, bottom up:
//!
//! - [`protocol`] is the eight calls and their payloads. This is the part third
//!   parties pin to, so it changes only with [`protocol::PROTOCOL_VERSION`].
//! - [`server`] is the driver's half: framing and dispatch, so each driver
//!   writes only the eight answers.
//! - `transport` moves lines. `transport::StdioTransport` runs a real child
//!   process; [`fake_driver`] answers in process for tests.
//! - `client` turns those calls into typed methods and enforces the version
//!   handshake and the request timeout.
//! - `registry` says which drivers exist, shipped or from an extension.
//!
//! The last three sit behind the default `client` feature. A driver binary
//! takes this crate with `default-features = false` and gets [`protocol`] and
//! [`server`] alone -- otherwise building a driver would compile gpui.
//!
//! Read-only is not enforced here. It is the driver's job, done by opening a
//! read-only connection, so the engine itself refuses a write rather than Zode
//! guessing at the meaning of a statement.

pub mod protocol;
pub mod server;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub mod registry;
#[cfg(feature = "client")]
pub mod transport;

#[cfg(any(test, feature = "test-support"))]
pub mod fake_driver;

/// Kept apart from `test-support`, which pulls the client half in with it: this
/// one runs a driver binary and needs nothing but std, so a driver crate can
/// take it without compiling gpui to test itself.
#[cfg(feature = "driver-test-suite")]
pub mod driver_test_suite;

#[cfg(test)]
mod client_tests;

pub use protocol::{ErrorCode, PROTOCOL_VERSION};
pub use server::{Driver, serve, typed};

#[cfg(feature = "client")]
pub use client::{DEFAULT_REQUEST_TIMEOUT, DriverClient, DriverError};
#[cfg(feature = "client")]
pub use registry::{DriverDescriptor, DriverId, DriverRegistry, DriverSource};
#[cfg(feature = "client")]
pub use transport::{DriverBinary, Transport};
