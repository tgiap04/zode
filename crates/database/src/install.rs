//! Getting a driver onto the machine.
//!
//! Zode ships no driver binaries. Each one arrives when an engine is first
//! connected to, so an install carries only what its user asked for rather than
//! four engines on the chance one of them is wanted.
//!
//! [`store`] owns the on-disk shape, [`manifest`] what the release says it
//! published, and [`download`] the fetch itself. The order inside a download is
//! the part that matters: bytes are hashed as they stream, compared against the
//! manifest, and only then unpacked -- nothing unverified reaches an archive
//! reader, and nothing half-installed is ever visible where the resolver looks.
//!
//! Behind the `install` feature, which `client` enables, so a driver binary --
//! which takes this crate with `default-features = false` for the protocol
//! alone -- does not compile any of it. Bundling the downloader into the very
//! binaries being downloaded would grow exactly what this work exists to shrink.

pub mod download;
pub mod manifest;
pub mod store;
mod unpack;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use download::{DriverInstaller, InstallProgress, InstallResult};
pub use manifest::{DriverAsset, DriverManifest, ReleaseCoordinates};
