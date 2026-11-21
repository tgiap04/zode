pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Zode is running in stateless mode.
/// When true, Zode will use in-memory databases instead of persistent storage.
pub static ZODE_STATELESS: LazyLock<bool> = bool_env_var!("ZODE_STATELESS");
