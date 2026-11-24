#[cfg(not(target_os = "windows"))]
mod install_cli_binary;
mod register_zode_scheme;

#[cfg(not(target_os = "windows"))]
pub use install_cli_binary::{InstallCliBinary, install_cli_binary};
pub use register_zode_scheme::{RegisterZodeScheme, register_zode_scheme};
