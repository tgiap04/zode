//! The host's environment is the relay environment: the same pieces serve the
//! Zode that controls another.

pub(crate) use remote_relay_client::RegisteringCredentials;
pub use remote_relay_client::RelayEnvironment as HostEnvironment;
