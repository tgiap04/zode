use crate::RemoteConnectionOptions;

/// A normalized remote identity for matching live remote hosts against
/// persisted remote metadata.
///
/// This mirrors workspace persistence identity semantics rather than full
/// `RemoteConnectionOptions` equality, so runtime-only fields like SSH
/// nicknames or Docker environment overrides do not affect matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RemoteConnectionIdentity {
    Ssh {
        host: String,
        username: Option<String>,
        port: Option<u16>,
    },
    Wsl {
        distro_name: String,
        user: Option<String>,
    },
    Docker {
        container_id: String,
        name: String,
        remote_user: String,
    },
    Relay {
        host_device_id: String,
    },
    #[cfg(any(test, feature = "test-support"))]
    Mock {
        id: u64,
    },
}

impl From<&RemoteConnectionOptions> for RemoteConnectionIdentity {
    fn from(options: &RemoteConnectionOptions) -> Self {
        match options {
            RemoteConnectionOptions::Ssh(options) => Self::Ssh {
                host: options.host.to_string(),
                username: options.username.clone(),
                port: options.port,
            },
            RemoteConnectionOptions::Wsl(options) => Self::Wsl {
                distro_name: options.distro_name.clone(),
                user: options.user.clone(),
            },
            RemoteConnectionOptions::Docker(options) => Self::Docker {
                container_id: options.container_id.clone(),
                name: options.name.clone(),
                remote_user: options.remote_user.clone(),
            },
            RemoteConnectionOptions::Relay(options) => Self::Relay {
                host_device_id: options.host_device_id.clone(),
            },
            #[cfg(any(test, feature = "test-support"))]
            RemoteConnectionOptions::Mock(options) => Self::Mock { id: options.id },
        }
    }
}

impl RemoteConnectionIdentity {
    /// The spelling this host is recorded and recognised under, the same on
    /// every run and every machine.
    ///
    /// Written out field by field rather than derived: it keys persisted
    /// records, so a renamed field must not quietly re-key them.
    pub fn stable_key(&self) -> String {
        let parts = match self {
            Self::Ssh {
                host,
                username,
                port,
            } => serde_json::json!(["ssh", host, username, port]),
            Self::Wsl { distro_name, user } => serde_json::json!(["wsl", distro_name, user]),
            Self::Docker {
                container_id,
                name,
                remote_user,
            } => serde_json::json!(["docker", container_id, name, remote_user]),
            Self::Relay { host_device_id } => serde_json::json!(["relay", host_device_id]),
            // The spelling records were made under before this lived here.
            #[cfg(any(test, feature = "test-support"))]
            Self::Mock { .. } => serde_json::json!([format!("{self:?}")]),
        };
        parts.to_string()
    }
}

pub fn remote_connection_identity(options: &RemoteConnectionOptions) -> RemoteConnectionIdentity {
    options.into()
}

pub fn same_remote_connection_identity(
    left: Option<&RemoteConnectionOptions>,
    right: Option<&RemoteConnectionOptions>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            remote_connection_identity(left) == remote_connection_identity(right)
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{DockerConnectionOptions, SshConnectionOptions, WslConnectionOptions};

    #[test]
    fn ssh_identity_ignores_non_persisted_runtime_fields() {
        let left = RemoteConnectionOptions::Ssh(SshConnectionOptions {
            host: "example.com".into(),
            username: Some("anth".to_string()),
            port: Some(2222),
            password: Some("secret".to_string()),
            args: Some(vec!["-v".to_string()]),
            connection_timeout: Some(30),
            nickname: Some("work".to_string()),
            upload_binary_over_ssh: true,
            ..Default::default()
        });
        let right = RemoteConnectionOptions::Ssh(SshConnectionOptions {
            host: "example.com".into(),
            username: Some("anth".to_string()),
            port: Some(2222),
            password: None,
            args: None,
            connection_timeout: None,
            nickname: None,
            upload_binary_over_ssh: false,
            ..Default::default()
        });

        assert!(same_remote_connection_identity(Some(&left), Some(&right),));
    }

    #[test]
    fn ssh_identity_distinguishes_persistence_key_fields() {
        let left = RemoteConnectionOptions::Ssh(SshConnectionOptions {
            host: "example.com".into(),
            username: Some("anth".to_string()),
            port: Some(2222),
            ..Default::default()
        });
        let right = RemoteConnectionOptions::Ssh(SshConnectionOptions {
            host: "example.com".into(),
            username: Some("anth".to_string()),
            port: Some(2223),
            ..Default::default()
        });

        assert!(!same_remote_connection_identity(Some(&left), Some(&right),));
    }

    #[test]
    fn wsl_identity_includes_user() {
        let left = RemoteConnectionOptions::Wsl(WslConnectionOptions {
            distro_name: "Ubuntu".to_string(),
            user: Some("anth".to_string()),
        });
        let right = RemoteConnectionOptions::Wsl(WslConnectionOptions {
            distro_name: "Ubuntu".to_string(),
            user: Some("root".to_string()),
        });

        assert!(!same_remote_connection_identity(Some(&left), Some(&right),));
    }

    #[test]
    fn docker_identity_ignores_non_persisted_runtime_fields() {
        let left = RemoteConnectionOptions::Docker(DockerConnectionOptions {
            name: "zed-dev".to_string(),
            container_id: "container-123".to_string(),
            remote_user: "anth".to_string(),
            upload_binary_over_docker_exec: true,
            use_podman: true,
            remote_env: BTreeMap::from([("FOO".to_string(), "BAR".to_string())]),
        });
        let right = RemoteConnectionOptions::Docker(DockerConnectionOptions {
            name: "zed-dev".to_string(),
            container_id: "container-123".to_string(),
            remote_user: "anth".to_string(),
            upload_binary_over_docker_exec: false,
            use_podman: false,
            remote_env: BTreeMap::new(),
        });

        assert!(same_remote_connection_identity(Some(&left), Some(&right),));
    }

    /// Persisted records are keyed by these spellings: a change to any of them
    /// silently forgets everything recorded for that host.
    #[test]
    fn stable_keys_are_spelled_as_recorded() {
        use RemoteConnectionIdentity as Identity;
        let cases = [
            (
                Identity::Wsl {
                    distro_name: "Ubuntu".into(),
                    user: None,
                },
                r#"["wsl","Ubuntu",null]"#,
            ),
            (
                Identity::Wsl {
                    distro_name: "Ubuntu".into(),
                    user: Some("me".into()),
                },
                r#"["wsl","Ubuntu","me"]"#,
            ),
            (
                Identity::Ssh {
                    host: "box".into(),
                    username: Some("me".into()),
                    port: Some(2222),
                },
                r#"["ssh","box","me",2222]"#,
            ),
            (
                Identity::Ssh {
                    host: "box".into(),
                    username: None,
                    port: None,
                },
                r#"["ssh","box",null,null]"#,
            ),
            (
                Identity::Docker {
                    container_id: "c0ffee".into(),
                    name: "dev".into(),
                    remote_user: "root".into(),
                },
                r#"["docker","c0ffee","dev","root"]"#,
            ),
            (
                Identity::Relay {
                    host_device_id: "device-1".into(),
                },
                r#"["relay","device-1"]"#,
            ),
            (Identity::Mock { id: 7 }, r#"["Mock { id: 7 }"]"#),
        ];
        for (identity, spelling) in cases {
            assert_eq!(identity.stable_key(), spelling, "{identity:?}");
        }
    }

    #[test]
    fn local_identity_matches_only_local_identity() {
        let remote = RemoteConnectionOptions::Wsl(WslConnectionOptions {
            distro_name: "Ubuntu".to_string(),
            user: Some("anth".to_string()),
        });

        assert!(same_remote_connection_identity(None, None));
        assert!(!same_remote_connection_identity(None, Some(&remote)));
    }
}
