use std::path::Path;

use remote::RemoteConnectionIdentity;
use util::paths::SanitizedPath;

/// Namespace holding one dock size per project and panel.
///
/// Separate from `dock::PANEL_SIZE_STATE_KEY` rather than sharing it under new
/// keys: the older namespace stays frozen and readable by a build from before
/// projects had records of their own, and the prune pass gets a namespace it
/// can sweep without having to tell the two kinds of row apart.
pub(crate) const PROJECT_PANEL_SIZE_STATE_KEY: &str = "dock_panel_size_by_project";

/// The key one project's record for one panel is stored under.
///
/// Two shapes, told apart by a one-character tag:
///
/// ```text
/// local   {panel_key}:L:{project_path}
/// remote  {panel_key}:R:{host_token}:{project_path}
/// ```
///
/// Panel keys are CamelCase identifiers -- `ProjectPanel`, `GitPanel` -- and
/// carry no colon, so the first colon always ends one. The tag is a single
/// character, so the second always ends that. `host_token` is escaped free of
/// colons, so the third ends it in turn, which leaves the path last and opaque:
/// free to contain colons, as on some platforms it does.
///
/// Answers `None` for a path that is not UTF-8. Such a project keeps the shared
/// record, which is the behaviour it had before it could have one of its own --
/// better than a lossy key two different paths could both produce.
pub(crate) fn panel_size_key(
    panel_key: &str,
    host: Option<&RemoteConnectionIdentity>,
    project_path: &Path,
) -> Option<String> {
    debug_assert!(
        !panel_key.contains(':'),
        "a panel key must not contain a colon, or the first colon stops ending \
         it and the key no longer parses: {panel_key}"
    );
    let project_path = SanitizedPath::new(project_path).as_path().to_str()?;
    Some(match host {
        None => format!("{panel_key}:L:{project_path}"),
        Some(host) => format!("{panel_key}:R:{}:{project_path}", host_token(host)),
    })
}

/// The project path a key names, for a local project only.
///
/// A remote row answers `None` on purpose: its path lives on a host that may be
/// unreachable, so nothing offline can establish whether it is still there. The
/// prune pass reads rows through this function and therefore skips remote rows
/// by construction rather than by remembering to.
pub(crate) fn local_project_path_from_key(key: &str) -> Option<&str> {
    let (_panel_key, tagged) = key.split_once(':')?;
    tagged.strip_prefix("L:")
}

/// A remote host rendered as text, stably.
///
/// Formatted rather than hashed deliberately: `std`'s default hasher carries no
/// guarantee of stability across releases, so a hashed host would silently
/// reset every stored width on the day the toolchain moved. Each field is
/// escaped free of the three characters this format reserves, so the token
/// never contains a colon and the key above stays parseable.
fn host_token(host: &RemoteConnectionIdentity) -> String {
    let mut token = String::new();
    match host {
        RemoteConnectionIdentity::Ssh {
            host,
            username,
            port,
        } => {
            token.push_str("ssh");
            push_field(&mut token, host);
            push_optional_field(&mut token, username.as_deref());
            let port = port.map(|port| port.to_string());
            push_optional_field(&mut token, port.as_deref());
        }
        RemoteConnectionIdentity::Wsl { distro_name, user } => {
            token.push_str("wsl");
            push_field(&mut token, distro_name);
            push_optional_field(&mut token, user.as_deref());
        }
        RemoteConnectionIdentity::Docker {
            container_id,
            name,
            remote_user,
        } => {
            token.push_str("docker");
            push_field(&mut token, container_id);
            push_field(&mut token, name);
            push_field(&mut token, remote_user);
        }
        #[cfg(any(test, feature = "test-support"))]
        RemoteConnectionIdentity::Mock { id } => {
            token.push_str("mock");
            push_field(&mut token, &id.to_string());
        }
    }
    token
}

fn push_field(token: &mut String, field: &str) {
    token.push('|');
    push_escaped(token, field);
}

fn push_optional_field(token: &mut String, field: Option<&str>) {
    match field {
        Some(field) => {
            token.push_str("|+");
            push_escaped(token, field);
        }
        // Marked rather than left empty, so an absent field and a present but
        // empty one cannot render alike.
        None => token.push_str("|-"),
    }
}

fn push_escaped(token: &mut String, field: &str) {
    for character in field.chars() {
        match character {
            '%' => token.push_str("%25"),
            ':' => token.push_str("%3A"),
            '|' => token.push_str("%7C"),
            _ => token.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ssh(host: &str, username: Option<&str>, port: Option<u16>) -> RemoteConnectionIdentity {
        RemoteConnectionIdentity::Ssh {
            host: host.to_string(),
            username: username.map(str::to_string),
            port,
        }
    }

    #[test]
    fn local_key_round_trips() {
        let key = panel_size_key("GitPanel", None, Path::new("/Users/me/repo")).unwrap();
        assert_eq!(key, "GitPanel:L:/Users/me/repo");
        assert_eq!(local_project_path_from_key(&key), Some("/Users/me/repo"));
    }

    #[test]
    fn a_path_containing_a_colon_survives() {
        let path = PathBuf::from("/Users/me/odd:name/repo");
        let key = panel_size_key("ProjectPanel", None, &path).unwrap();
        assert_eq!(
            local_project_path_from_key(&key),
            Some("/Users/me/odd:name/repo")
        );
    }

    #[test]
    fn a_path_containing_a_newline_survives() {
        let path = PathBuf::from("/Users/me/odd\nname");
        let key = panel_size_key("ProjectPanel", None, &path).unwrap();
        assert_eq!(local_project_path_from_key(&key), Some("/Users/me/odd\nname"));
    }

    #[test]
    fn a_malformed_key_answers_none_rather_than_panicking() {
        assert_eq!(local_project_path_from_key("no-colon-at-all"), None);
        assert_eq!(local_project_path_from_key(""), None);
        assert_eq!(local_project_path_from_key("GitPanel:"), None);
        assert_eq!(local_project_path_from_key("GitPanel:X:/repo"), None);
    }

    #[test]
    fn a_remote_key_is_not_read_as_a_local_path() {
        let key =
            panel_size_key("GitPanel", Some(&ssh("example.com", None, None)), Path::new("/repo"))
                .unwrap();
        assert!(key.starts_with("GitPanel:R:"));
        assert_eq!(local_project_path_from_key(&key), None);
    }

    #[test]
    fn two_hosts_sharing_a_path_are_two_projects() {
        let path = Path::new("/srv/app");
        let left = panel_size_key("GitPanel", Some(&ssh("one.example", None, None)), path);
        let right = panel_size_key("GitPanel", Some(&ssh("two.example", None, None)), path);
        assert_ne!(left, right);
    }

    #[test]
    fn a_host_and_a_path_cannot_trade_characters() {
        // An IPv6 literal puts colons in the host, and a path may hold them
        // too. Without escaping, `{host}:{path}` would let one borrow from the
        // other and two different projects would land on one key.
        let path = Path::new("/srv/app");
        let left = panel_size_key("GitPanel", Some(&ssh("::1", None, None)), path);
        let right = panel_size_key("GitPanel", Some(&ssh(":", None, None)), Path::new("1:/srv/app"));
        assert_ne!(left, right);
    }

    #[test]
    fn an_absent_field_differs_from_an_empty_one() {
        let path = Path::new("/srv/app");
        let absent = panel_size_key("GitPanel", Some(&ssh("host", None, None)), path);
        let empty = panel_size_key("GitPanel", Some(&ssh("host", Some(""), None)), path);
        assert_ne!(absent, empty);
    }

    #[test]
    fn a_port_separates_two_otherwise_equal_hosts() {
        let path = Path::new("/srv/app");
        let left = panel_size_key("GitPanel", Some(&ssh("host", Some("me"), Some(22))), path);
        let right = panel_size_key("GitPanel", Some(&ssh("host", Some("me"), Some(2222))), path);
        assert_ne!(left, right);
    }

    #[test]
    #[cfg(unix)]
    fn a_non_utf8_path_answers_none() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = PathBuf::from(OsStr::from_bytes(&[0x2f, 0xff, 0xfe]));
        assert_eq!(panel_size_key("GitPanel", None, &path), None);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn a_unc_path_normalises_to_its_plain_form() {
        let key = panel_size_key("GitPanel", None, Path::new(r"\\?\C:\repo")).unwrap();
        assert_eq!(local_project_path_from_key(&key), Some(r"C:\repo"));
    }
}
