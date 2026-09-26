//! Where a build looks for its drivers, now that they are served from Zode's
//! own API rather than a GitHub release.
//!
//! The version is the app's own. A driver speaks a pinned protocol
//! (`crate::PROTOCOL_VERSION`), and the build that shipped this app is the one
//! whose drivers were built against it -- so an app never fetches from a
//! version other than its own, and never runs a driver left by another.

use anyhow::{Context as _, Result};
use url::Url;

/// The name the manifest is published under, at every version.
pub const MANIFEST_ASSET: &str = "zode-db-drivers-manifest.json";

/// Hosts that are the machine's own loopback, for which plain HTTP is allowed.
///
/// A developer pointing `ZODE_API_URL` at `http://localhost:8000/api` must
/// still be able to install a driver; nothing else gets that exemption.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// `{base}/drivers/{version}/{asset}` -- the one contract this crate shares
/// with `script/build-driver-manifest` on the other side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverEndpoint {
    base: String,
    version: String,
}

impl DriverEndpoint {
    pub fn new(base: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            version: version.into(),
        }
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn asset_url(&self, asset: &str) -> String {
        let Self { base, version } = self;
        format!("{}/drivers/{version}/{asset}", base.trim_end_matches('/'))
    }

    pub fn manifest_url(&self) -> String {
        self.asset_url(MANIFEST_ASSET)
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
            && target.host_str() == base.host_str()
            && target.port_or_known_default() == base.port_or_known_default();
        anyhow::ensure!(
            same_origin,
            "driver asset url {:?} does not match the configured driver origin {:?}",
            origin_of(&target),
            origin_of(&base),
        );

        let loopback = target
            .host_str()
            .map(|host| LOOPBACK_HOSTS.contains(&host))
            .unwrap_or(false);
        anyhow::ensure!(
            target.scheme() == "https" || loopback,
            "driver asset url is not https: {url:?}"
        );

        Ok(())
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
    fn urls_are_built_under_the_configured_api_base() {
        let endpoint = DriverEndpoint::new("https://api.zodekit.site/api", "0.1.1");
        assert_eq!(
            endpoint.manifest_url(),
            "https://api.zodekit.site/api/drivers/0.1.1/zode-db-drivers-manifest.json"
        );
        assert_eq!(
            endpoint.asset_url("zode-db-postgres-aarch64-apple-darwin.tar.gz"),
            "https://api.zodekit.site/api/drivers/0.1.1/zode-db-postgres-aarch64-apple-darwin.tar.gz"
        );
    }

    /// A trailing slash on the configured base must not double up in the URL.
    #[test]
    fn a_trailing_slash_on_the_base_does_not_duplicate() {
        let endpoint = DriverEndpoint::new("https://api.zodekit.site/api/", "0.1.1");
        assert_eq!(
            endpoint.asset_url("asset.tar.gz"),
            "https://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz"
        );
    }

    fn endpoint() -> DriverEndpoint {
        DriverEndpoint::new("https://api.zodekit.site/api", "0.1.1")
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
        let endpoint = DriverEndpoint::new("http://api.zodekit.site/api", "0.1.1");
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
        let endpoint = DriverEndpoint::new("http://localhost:8000/api", "0.1.1");
        endpoint
            .ensure_trusted("http://localhost:8000/api/drivers/0.1.1/asset.tar.gz")
            .unwrap();
    }

    #[test]
    fn an_unparsable_url_is_refused_rather_than_ignored() {
        assert!(endpoint().ensure_trusted("not a url").is_err());
    }

    #[test]
    fn an_unparsable_base_is_refused_rather_than_ignored() {
        let endpoint = DriverEndpoint::new("not a url", "0.1.1");
        assert!(
            endpoint
                .ensure_trusted("https://api.zodekit.site/api/drivers/0.1.1/asset.tar.gz")
                .is_err()
        );
    }
}
