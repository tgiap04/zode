//! Where a build looks for its drivers, now that they are served from Zode's
//! own API rather than a GitHub release.
//!
//! The manifest comes from `latest`; an archive comes from the immutable path
//! its manifest names. Both halves of one install therefore describe the same
//! publish, which they could not if an archive were also read from a URL that
//! moves. A driver speaks a pinned protocol (`crate::PROTOCOL_VERSION`), but the
//! only place that is checked is the handshake, not anywhere in this path.

use anyhow::{Context as _, Result};
use url::{Host, Url};

use crate::install::manifest::MANIFEST_ASSET;

/// The path segment the server answers with whichever version was published
/// last.
///
/// No published version can be called this: the server requires a version to
/// start with a digit.
const LATEST: &str = "latest";

/// `{base}/drivers/{version}/{asset}`, plus `{base}/drivers/latest/` for the
/// manifest alone -- the one contract this crate shares with the server and
/// with `script/publish-drivers-to-web` on the other side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverEndpoint {
    base: String,
}

impl DriverEndpoint {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    pub fn latest_manifest_url(&self) -> String {
        format!(
            "{}/drivers/{LATEST}/{MANIFEST_ASSET}",
            self.base.trim_end_matches('/')
        )
    }

    pub fn asset_url(&self, version: &str, asset: &str) -> String {
        format!(
            "{}/drivers/{version}/{asset}",
            self.base.trim_end_matches('/')
        )
    }

    /// Rejects a `url` that does not point at exactly the origin this
    /// endpoint was configured with.
    ///
    /// Unlike `ensure_release_host_is_trusted` in `http_client`, which checks a
    /// fixed allowlist of GitHub hosts shared with `auto_update`, this compares
    /// against a base the caller supplied -- so no host is trusted except the
    /// one actually configured. Widening the GitHub allowlist to include a web
    /// host would loosen auto-update's own trust check too, which is why this
    /// lives here instead.
    pub fn ensure_trusted(&self, url: &str) -> Result<()> {
        let base = Url::parse(&self.base).with_context(|| {
            format!(
                "the configured driver base url is unparsable: {:?}",
                self.base
            )
        })?;
        let target =
            Url::parse(url).with_context(|| format!("unparsable driver asset url: {url:?}"))?;

        anyhow::ensure!(
            target.username().is_empty() && target.password().is_none(),
            "driver asset url carries userinfo, which can make it read as a different \
             host than it is: {url:?}"
        );

        let same_origin = target.scheme() == base.scheme()
            && target.host() == base.host()
            && target.port_or_known_default() == base.port_or_known_default();
        anyhow::ensure!(
            same_origin,
            "driver asset url {:?} does not match the configured driver origin {:?}",
            origin_of(&target),
            origin_of(&base),
        );

        anyhow::ensure!(
            target.scheme() == "https" || is_loopback(&target),
            "driver asset url is not https: {url:?}"
        );

        Ok(())
    }
}

/// Whether the url names the machine's own loopback, for which plain HTTP is
/// allowed.
///
/// A developer pointing `ZODE_API_URL` at `http://localhost:8000/api` must still
/// be able to install a driver; nothing else gets that exemption. Judged on the
/// parsed host rather than on its text: `host_str` writes an IPv6 address inside
/// brackets, so `[::1]` would never equal a bare `::1` in a list of names.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain == "localhost",
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

/// `scheme://host:port`, for an error that names what was compared rather than
/// only that the comparison failed.
fn origin_of(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), url.host_str().unwrap_or("")),
        None => format!("{}://{}", url.scheme(), url.host_str().unwrap_or("")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_is_read_from_latest_and_an_archive_from_its_own_version() {
        let endpoint = DriverEndpoint::new("https://api.zodekit.site/api");
        assert_eq!(
            endpoint.latest_manifest_url(),
            "https://api.zodekit.site/api/drivers/latest/zode-db-drivers-manifest.json"
        );
        assert_eq!(
            endpoint.asset_url("0.1.4", "zode-db-postgres-aarch64-apple-darwin.tar.gz"),
            "https://api.zodekit.site/api/drivers/0.1.4/zode-db-postgres-aarch64-apple-darwin.tar.gz"
        );
    }

    /// A trailing slash on the configured base must not double up in the URL.
    #[test]
    fn a_trailing_slash_on_the_base_does_not_duplicate() {
        let endpoint = DriverEndpoint::new("https://api.zodekit.site/api/");
        assert_eq!(
            endpoint.asset_url("0.1.4", "asset.tar.gz"),
            "https://api.zodekit.site/api/drivers/0.1.4/asset.tar.gz"
        );
        assert_eq!(
            endpoint.latest_manifest_url(),
            "https://api.zodekit.site/api/drivers/latest/zode-db-drivers-manifest.json"
        );
    }

    fn endpoint() -> DriverEndpoint {
        DriverEndpoint::new("https://api.zodekit.site/api")
    }

    #[test]
    fn a_url_at_the_configured_origin_is_trusted() {
        endpoint()
            .ensure_trusted("https://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz")
            .unwrap();
    }

    #[test]
    fn a_different_host_is_refused() {
        let error = endpoint()
            .ensure_trusted("https://evil.example/drivers/0.1.1/asset.tar.gz")
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("api.zodekit.site"), "{message}");
        assert!(message.contains("evil.example"), "{message}");
    }

    #[test]
    fn a_different_scheme_is_refused() {
        assert!(
            endpoint()
                .ensure_trusted("http://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }

    #[test]
    fn a_different_port_is_refused() {
        assert!(
            endpoint()
                .ensure_trusted("https://api.zodekit.site:8443/api/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }

    /// `https://api.zodekit.site@evil.com/` parses with the real host in the
    /// userinfo slot and `evil.com` as the actual host -- the textbook trick
    /// this check exists to catch.
    #[test]
    fn userinfo_dressed_up_as_the_trusted_host_is_refused() {
        assert!(
            endpoint()
                .ensure_trusted("https://api.zodekit.site@evil.com/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }

    #[test]
    fn plain_http_on_a_public_host_is_refused_even_if_same_origin() {
        let endpoint = DriverEndpoint::new("http://api.zodekit.site/api");
        assert!(
            endpoint
                .ensure_trusted("http://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }

    /// `ZODE_API_URL=http://localhost:8000/api` must still work for a
    /// developer running the API themselves.
    #[test]
    fn plain_http_on_loopback_is_allowed() {
        let endpoint = DriverEndpoint::new("http://localhost:8000/api");
        endpoint
            .ensure_trusted("http://localhost:8000/api/drivers/0.1.1/asset.tar.gz")
            .unwrap();
    }

    /// `Url::host_str` spells an IPv6 host with its brackets, so comparing it
    /// against a list of bare addresses never matched `[::1]`.
    #[test]
    fn plain_http_on_every_spelling_of_loopback_is_allowed() {
        for base in [
            "http://localhost:8000/api",
            "http://LOCALHOST:8000/api",
            "http://127.0.0.1:8000/api",
            "http://127.0.0.2:8000/api",
            "http://[::1]:8000/api",
        ] {
            let endpoint = DriverEndpoint::new(base);
            let url = format!("{base}/drivers/0.1.1/asset.tar.gz");
            endpoint
                .ensure_trusted(&url)
                .unwrap_or_else(|error| panic!("{base} must be allowed: {error}"));
        }
    }

    /// Only the machine's own loopback gets the exemption. These all look
    /// local to a person and none of them is.
    #[test]
    fn plain_http_on_anything_but_loopback_is_refused() {
        for base in [
            "http://[::2]:8000/api",
            "http://[fe80::1]:8000/api",
            "http://192.168.1.10:8000/api",
            "http://localhost.example:8000/api",
            "http://0.0.0.0:8000/api",
        ] {
            let endpoint = DriverEndpoint::new(base);
            let url = format!("{base}/drivers/0.1.1/asset.tar.gz");
            assert!(
                endpoint.ensure_trusted(&url).is_err(),
                "{base} must be refused"
            );
        }
    }

    #[test]
    fn an_unparsable_url_is_refused_rather_than_ignored() {
        assert!(endpoint().ensure_trusted("not a url").is_err());
    }

    #[test]
    fn an_unparsable_base_is_refused_rather_than_ignored() {
        let endpoint = DriverEndpoint::new("not a url");
        assert!(
            endpoint
                .ensure_trusted("https://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }
}
