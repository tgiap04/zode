use std::collections::HashSet;

use url::Url;

use crate::model::RepositoryId;

const GITHUB_HOST: &str = "github.com";

/// The two remotes of one git repository in the project, as the git store
/// reports them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepositoryRemotes {
    pub origin_url: Option<String>,
    pub upstream_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteName {
    Origin,
    Upstream,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedRepository {
    pub repository: RepositoryId,
    pub remote: RemoteName,
}

/// Every github.com repository the project's remotes point at, once each, in
/// the order found (origin before upstream within a repository). Neither is
/// preferred: writing to upstream from a fork could publish private work, so
/// the user picks.
pub fn detect_repositories(remotes: &[RepositoryRemotes]) -> Vec<DetectedRepository> {
    let mut seen = HashSet::new();
    let mut detected = Vec::new();
    for remotes in remotes {
        for (url, remote) in [
            (&remotes.origin_url, RemoteName::Origin),
            (&remotes.upstream_url, RemoteName::Upstream),
        ] {
            let Some(repository) = url.as_deref().and_then(parse_github_remote) else {
                continue;
            };
            // GitHub owner and repository names ignore case.
            if seen.insert(repository.full_name().to_lowercase()) {
                detected.push(DetectedRepository { repository, remote });
            }
        }
    }
    detected
}

/// `owner/name` when `url` points at a repository on exactly `github.com`.
/// SSH host aliases (`github.com-work`) and lookalike hosts are not read as
/// GitHub, since the remote comes from the repository's own config.
pub fn parse_github_remote(url: &str) -> Option<RepositoryId> {
    let url = url.trim();
    let path = if url.contains("://") {
        let parsed = Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "https" | "http" | "ssh" | "git")
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return None;
        }
        if !parsed
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(GITHUB_HOST))
        {
            return None;
        }
        parsed.path().to_string()
    } else {
        // scp-like `[user@]github.com:owner/name.git`.
        let (host, path) = url.split_once(':')?;
        let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
        if !host.eq_ignore_ascii_case(GITHUB_HOST) {
            return None;
        }
        path.to_string()
    };
    let path = path.trim_start_matches('/').trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let (owner, name, rest) = (parts.next()?, parts.next()?, parts.next());
    if rest.is_some() {
        return None;
    }
    RepositoryId::new(owner, name)
}
