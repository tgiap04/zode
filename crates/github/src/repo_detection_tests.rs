use crate::model::RepositoryId;
use crate::repo_detection::{
    DetectedRepository, RemoteName, RepositoryRemotes, detect_repositories, parse_github_remote,
};

fn repository(owner: &str, name: &str) -> RepositoryId {
    RepositoryId::new(owner, name).expect("a valid repository")
}

#[test]
fn https_and_ssh_remotes_on_github_are_read() {
    for url in [
        "https://github.com/zode-kit/zode.git",
        "https://github.com/zode-kit/zode",
        "https://github.com/zode-kit/zode/",
        "https://GitHub.com/zode-kit/zode.git",
        "https://someone:secret@github.com/zode-kit/zode.git",
        "git@github.com:zode-kit/zode.git",
        "git@github.com:zode-kit/zode",
        "ssh://git@github.com/zode-kit/zode.git",
        "ssh://git@github.com:22/zode-kit/zode.git",
        "git://github.com/zode-kit/zode.git",
    ] {
        assert_eq!(
            parse_github_remote(url),
            Some(repository("zode-kit", "zode")),
            "{url}"
        );
    }
}

#[test]
fn names_with_dots_underscores_and_dashes_are_kept() {
    assert_eq!(
        parse_github_remote("git@github.com:my_org/my.repo-name.git"),
        Some(repository("my_org", "my.repo-name"))
    );
}

#[test]
fn hosts_other_than_exactly_github_com_are_refused() {
    for url in [
        "git@github.com-work:zode-kit/zode.git",
        "git@ssh.github.com:zode-kit/zode.git",
        "https://github.com.evil.example/zode-kit/zode.git",
        "https://evil.example/github.com/zode-kit/zode.git",
        "https://gitlab.com/zode-kit/zode.git",
        "https://www.github.com/zode-kit/zode.git",
    ] {
        assert_eq!(parse_github_remote(url), None, "{url}");
    }
}

#[test]
fn odd_owners_names_and_paths_are_refused() {
    for url in [
        "https://github.com/zode-kit/zode/tree/main",
        "https://github.com/zode-kit",
        "https://github.com/zode%2Fkit/zode",
        "https://github.com/../zode",
        "git@github.com:zode kit/zode.git",
        "git@github.com:zode-kit/..",
        "git@github.com:zode-kit/zode?x=1",
        "file:///github.com/zode-kit/zode",
        "",
    ] {
        assert_eq!(parse_github_remote(url), None, "{url:?}");
    }
}

#[test]
fn every_repository_is_listed_once_with_origin_first() {
    let remotes = vec![
        RepositoryRemotes {
            origin_url: Some("git@github.com:me/zode.git".to_string()),
            upstream_url: Some("https://github.com/zode-kit/zode.git".to_string()),
        },
        RepositoryRemotes {
            origin_url: Some("https://github.com/ME/Zode".to_string()),
            upstream_url: None,
        },
        RepositoryRemotes {
            origin_url: Some("git@gitlab.com:me/other.git".to_string()),
            upstream_url: Some("https://github.com/me/tools.git".to_string()),
        },
        RepositoryRemotes::default(),
    ];
    assert_eq!(
        detect_repositories(&remotes),
        vec![
            DetectedRepository {
                repository: repository("me", "zode"),
                remote: RemoteName::Origin,
            },
            DetectedRepository {
                repository: repository("zode-kit", "zode"),
                remote: RemoteName::Upstream,
            },
            DetectedRepository {
                repository: repository("me", "tools"),
                remote: RemoteName::Upstream,
            },
        ]
    );
}

#[test]
fn repository_ids_refuse_path_walking_parts() {
    assert_eq!(RepositoryId::new(".", "zode"), None);
    assert_eq!(RepositoryId::new("zode-kit", ".."), None);
    assert_eq!(RepositoryId::new("zode/kit", "zode"), None);
    assert_eq!(RepositoryId::new("", "zode"), None);
    assert_eq!(
        repository("zode-kit", "zode").full_name(),
        "zode-kit/zode".to_string()
    );
}
