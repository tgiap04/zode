//! Decides what a control message from a connected device means, and nothing
//! more: it holds no sockets and touches no terminal, so every rule can be
//! asserted against plain values.
//!
//! The host carries out what comes back.

use remote_relay_protocol::{
    Control, RELAY_PROTOCOL_VERSION, RPC_PROTOCOL_VERSION, error_code, reject_unknown_version,
};

use crate::file_browse::FileRequest;

/// What this host always does for a device. Files and the project server are
/// not in the list: advertising what is not served would only invite requests
/// that have to be refused. They are added by [`HostFacts::files_available`]
/// and [`HostFacts::ide_available`].
pub const HOST_CAPABILITIES: [&str; 1] = ["terminal"];

/// The capability that says this host lists, reads and diffs the folders open
/// on it for a device.
pub const FILES_CAPABILITY: &str = "files";

/// The capability that says this host can run the project server for a device.
pub const IDE_CAPABILITY: &str = "ide";

/// The code for a request that is understood and not served. Not one of the
/// protocol's seven: a recognised message the host declines is a different
/// answer from one it could not parse, and a client that does not know the
/// code treats it as a generic refusal.
pub const UNSUPPORTED: &str = "unsupported";

/// Streams the host opens for file contents and diffs count up from here, so
/// they never meet the ids a device picks for its own streams.
pub(crate) const HOST_STREAM_BASE: u32 = 0x8000_0000;

/// Why `stream_id` cannot be one a device picks for a stream of its own.
fn client_stream_problem(stream_id: u32) -> Option<&'static str> {
    if stream_id == 0 {
        Some("stream 0 is reserved for control messages")
    } else if stream_id >= HOST_STREAM_BASE {
        Some("streams from 2147483648 up are the host's to open")
    } else {
        None
    }
}

/// What the router needs to know about this host that is not in a message.
#[derive(Debug, Clone, Copy)]
pub struct HostFacts<'a> {
    pub app_version: &'a str,
    /// Whether a project server binary can be found, which is what decides if
    /// the `ide` capability is truthful.
    pub ide_available: bool,
    /// Whether this host can browse its open folders, which is what decides if
    /// the `files` capability is truthful.
    pub files_available: bool,
    /// The version of the project server's own protocol, which only identical
    /// builds speak to each other.
    pub proto_version: u32,
}

/// The most terminals one device may be attached to at once.
pub const MAX_ATTACHMENTS_PER_SESSION: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterAction {
    Reply(Control),
    /// Reply with the error, then end the session.
    Refuse(Control),
    Attach {
        terminal_id: String,
        stream_id: u32,
    },
    Detach {
        terminal_id: String,
    },
    /// Tell the device the terminal's current size. The host's size wins, so a
    /// device asking for another one is answered with the real one.
    ReportSize {
        terminal_id: String,
    },
    /// Start the project server for this device and carry it on `stream_id`.
    /// The versions have already been checked against this host's.
    OpenIde {
        request_id: u32,
        stream_id: u32,
    },
    /// Read something of the open folders and answer `request_id`.
    Files {
        request_id: u32,
        request: FileRequest,
    },
}

/// What the router needs to remember about one session.
#[derive(Debug, Default)]
pub struct RouterState {
    hello_done: bool,
}

impl RouterState {
    pub fn hello_done(&self) -> bool {
        self.hello_done
    }
}

fn error(code: &str, message: &str) -> Control {
    Control::Error {
        code: code.to_string(),
        message: message.to_string(),
        request_id: None,
    }
}

fn request_error(code: &str, message: &str, request_id: u32) -> Control {
    Control::Error {
        code: code.to_string(),
        message: message.to_string(),
        request_id: Some(request_id),
    }
}

/// The `hello_ack` this host answers with.
pub fn hello_ack(facts: &HostFacts<'_>) -> Control {
    let mut capabilities: Vec<String> = HOST_CAPABILITIES
        .iter()
        .map(|name| name.to_string())
        .collect();
    if facts.files_available {
        capabilities.push(FILES_CAPABILITY.to_string());
    }
    if facts.ide_available {
        capabilities.push(IDE_CAPABILITY.to_string());
    }
    Control::HelloAck {
        relay_protocol: RELAY_PROTOCOL_VERSION,
        app_version: facts.app_version.to_string(),
        rpc_protocol: RPC_PROTOCOL_VERSION,
        capabilities,
    }
}

fn ide_open_action(
    facts: &HostFacts<'_>,
    request_id: u32,
    stream_id: Option<u32>,
    app_version: Option<String>,
    proto_version: Option<u32>,
) -> RouterAction {
    if !facts.ide_available {
        return RouterAction::Reply(request_error(
            UNSUPPORTED,
            "this Zode was installed without its project server",
            request_id,
        ));
    }
    let (Some(stream_id), Some(app_version), Some(proto_version)) =
        (stream_id, app_version, proto_version)
    else {
        return RouterAction::Reply(request_error(
            error_code::MALFORMED,
            "a project request names a stream and the versions it speaks",
            request_id,
        ));
    };
    if let Some(problem) = client_stream_problem(stream_id) {
        return RouterAction::Reply(request_error(error_code::MALFORMED, problem, request_id));
    }
    if app_version != facts.app_version || proto_version != facts.proto_version {
        return RouterAction::Reply(request_error(
            error_code::VERSION,
            &format!(
                "This Zode is version {} (protocol {}) and the one asking is version {} \
                 (protocol {}). Update both to the same version to open a project.",
                facts.app_version, facts.proto_version, app_version, proto_version
            ),
            request_id,
        ));
    }
    RouterAction::OpenIde {
        request_id,
        stream_id,
    }
}

/// `request` is `None` when the message left out a folder it cannot do without.
fn files_action(
    facts: &HostFacts<'_>,
    request_id: u32,
    request: Option<FileRequest>,
) -> RouterAction {
    if !facts.files_available {
        return RouterAction::Reply(request_error(
            UNSUPPORTED,
            "this Zode does not serve that request",
            request_id,
        ));
    }
    match request {
        Some(request) => RouterAction::Files {
            request_id,
            request,
        },
        None => RouterAction::Reply(request_error(
            error_code::MALFORMED,
            "that request names a folder",
            request_id,
        )),
    }
}

pub fn route_control(
    state: &mut RouterState,
    control: Control,
    facts: &HostFacts<'_>,
) -> Vec<RouterAction> {
    // The first thing a device says is hello, and nothing else is acted on
    // before it has: a session that skips it is not speaking this protocol.
    if !state.hello_done && !matches!(control, Control::Hello { .. } | Control::Ping) {
        return vec![RouterAction::Refuse(error(
            error_code::MALFORMED,
            "the first message must be hello",
        ))];
    }

    match control {
        Control::Hello { relay_protocol, .. } => {
            if let Some(refusal) = reject_unknown_version(relay_protocol) {
                return vec![RouterAction::Refuse(refusal)];
            }
            state.hello_done = true;
            vec![RouterAction::Reply(hello_ack(facts))]
        }
        Control::Ping => vec![RouterAction::Reply(Control::Pong)],
        Control::Pong => Vec::new(),
        Control::TerminalAttach {
            terminal_id,
            stream_id,
        } => {
            if let Some(problem) = client_stream_problem(stream_id) {
                return vec![RouterAction::Reply(error(error_code::MALFORMED, problem))];
            }
            vec![RouterAction::Attach {
                terminal_id,
                stream_id,
            }]
        }
        Control::TerminalDetach { terminal_id } => vec![RouterAction::Detach { terminal_id }],
        Control::TerminalResized { terminal_id, .. } => {
            vec![RouterAction::ReportSize { terminal_id }]
        }
        Control::IdeOpen {
            request_id,
            stream_id,
            app_version,
            proto_version,
            ..
        } => vec![ide_open_action(
            facts,
            request_id,
            stream_id,
            app_version,
            proto_version,
        )],
        Control::FilesList {
            request_id,
            worktree_id,
            path,
        } => vec![files_action(
            facts,
            request_id,
            Some(FileRequest::List { worktree_id, path }),
        )],
        Control::FileRead {
            request_id,
            worktree_id,
            path,
        } => vec![files_action(
            facts,
            request_id,
            worktree_id.map(|worktree_id| FileRequest::Read { worktree_id, path }),
        )],
        Control::DiffRequest {
            request_id,
            worktree_id,
        } => vec![files_action(
            facts,
            request_id,
            worktree_id.map(|worktree_id| FileRequest::Diff { worktree_id }),
        )],
        // A peer's own error is its business; there is nothing to answer.
        Control::Error { .. } => Vec::new(),
        // Messages only this side sends. A device sending one is confused
        // about which end it is.
        Control::HelloAck { .. }
        | Control::AgentList { .. }
        | Control::AgentUpdate { .. }
        | Control::TerminalList { .. }
        | Control::TerminalAttached { .. }
        | Control::TerminalClosed { .. }
        | Control::IdeOpened { .. }
        | Control::FilesListReply { .. }
        | Control::FileReadReply { .. }
        | Control::DiffReply { .. } => vec![RouterAction::Reply(error(
            error_code::MALFORMED,
            "that message is not one a device sends",
        ))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(ide_available: bool) -> HostFacts<'static> {
        HostFacts {
            app_version: "9.9.9",
            ide_available,
            files_available: false,
            proto_version: 68,
        }
    }

    fn files_facts() -> HostFacts<'static> {
        HostFacts {
            files_available: true,
            ..facts(false)
        }
    }

    fn hello(relay_protocol: u32) -> Control {
        Control::Hello {
            relay_protocol,
            app_version: "0.1.5".into(),
            rpc_protocol: 1,
            capabilities: vec!["terminal".into(), "files".into()],
        }
    }

    fn greeted() -> RouterState {
        let mut state = RouterState::default();
        route_control(&mut state, hello(RELAY_PROTOCOL_VERSION), &facts(false));
        state
    }

    fn error_code_of(action: &RouterAction) -> &str {
        match action {
            RouterAction::Reply(Control::Error { code, .. })
            | RouterAction::Refuse(Control::Error { code, .. }) => code,
            other => panic!("not an error: {other:?}"),
        }
    }

    #[test]
    fn hello_is_answered_with_this_hosts_version_and_only_what_it_serves() {
        let mut state = RouterState::default();
        let actions = route_control(&mut state, hello(1), &facts(false));
        assert_eq!(
            actions,
            vec![RouterAction::Reply(Control::HelloAck {
                relay_protocol: 1,
                app_version: "9.9.9".into(),
                rpc_protocol: 1,
                capabilities: vec!["terminal".into()],
            })]
        );
        assert!(state.hello_done());
    }

    #[test]
    fn a_different_relay_protocol_is_refused_with_a_version_error() {
        let mut state = RouterState::default();
        let actions = route_control(&mut state, hello(2), &facts(false));
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], RouterAction::Refuse(_)));
        assert_eq!(error_code_of(&actions[0]), error_code::VERSION);
        assert!(!state.hello_done(), "a refused hello opens nothing");
    }

    #[test]
    fn nothing_is_acted_on_before_hello() {
        for message in [
            Control::TerminalAttach {
                terminal_id: "terminal-1".into(),
                stream_id: 1,
            },
            Control::TerminalDetach {
                terminal_id: "terminal-1".into(),
            },
            Control::FilesList {
                request_id: 1,
                worktree_id: None,
                path: "/".into(),
            },
        ] {
            let mut state = RouterState::default();
            let actions = route_control(&mut state, message, &facts(false));
            assert_eq!(actions.len(), 1);
            assert!(matches!(&actions[0], RouterAction::Refuse(_)));
            assert_eq!(error_code_of(&actions[0]), error_code::MALFORMED);
        }
    }

    #[test]
    fn a_ping_is_always_answered() {
        let mut fresh = RouterState::default();
        assert_eq!(
            route_control(&mut fresh, Control::Ping, &facts(false)),
            vec![RouterAction::Reply(Control::Pong)]
        );
        let mut greeted = greeted();
        assert_eq!(
            route_control(&mut greeted, Control::Ping, &facts(false)),
            vec![RouterAction::Reply(Control::Pong)]
        );
        assert!(route_control(&mut greeted, Control::Pong, &facts(false)).is_empty());
    }

    #[test]
    fn attach_and_detach_become_actions() {
        let mut state = greeted();
        assert_eq!(
            route_control(
                &mut state,
                Control::TerminalAttach {
                    terminal_id: "terminal-4".into(),
                    stream_id: 7
                },
                &facts(false)
            ),
            vec![RouterAction::Attach {
                terminal_id: "terminal-4".into(),
                stream_id: 7
            }]
        );
        assert_eq!(
            route_control(
                &mut state,
                Control::TerminalDetach {
                    terminal_id: "terminal-4".into()
                },
                &facts(false)
            ),
            vec![RouterAction::Detach {
                terminal_id: "terminal-4".into()
            }]
        );
    }

    #[test]
    fn stream_zero_is_not_a_data_stream() {
        let mut state = greeted();
        let actions = route_control(
            &mut state,
            Control::TerminalAttach {
                terminal_id: "terminal-4".into(),
                stream_id: 0,
            },
            &facts(false),
        );
        assert_eq!(error_code_of(&actions[0]), error_code::MALFORMED);
        assert!(
            matches!(&actions[0], RouterAction::Reply(_)),
            "the session survives"
        );
    }

    #[test]
    fn a_stream_in_the_hosts_range_is_not_one_a_device_may_pick() {
        let mut state = greeted();
        for stream_id in [HOST_STREAM_BASE, HOST_STREAM_BASE + 1, u32::MAX] {
            let actions = route_control(
                &mut state,
                Control::TerminalAttach {
                    terminal_id: "terminal-4".into(),
                    stream_id,
                },
                &facts(false),
            );
            assert_eq!(
                error_code_of(&actions[0]),
                error_code::MALFORMED,
                "{stream_id}"
            );
            assert!(matches!(&actions[0], RouterAction::Reply(_)));

            let actions = route_control(
                &mut state,
                ide_open(Some(stream_id), Some("9.9.9"), Some(68)),
                &facts(true),
            );
            assert_eq!(
                error_code_of(&actions[0]),
                error_code::MALFORMED,
                "{stream_id}"
            );
            assert!(matches!(
                &actions[0],
                RouterAction::Reply(Control::Error {
                    request_id: Some(5),
                    ..
                })
            ));
        }
        assert!(matches!(
            route_control(
                &mut state,
                Control::TerminalAttach {
                    terminal_id: "terminal-4".into(),
                    stream_id: HOST_STREAM_BASE - 1,
                },
                &facts(false),
            )[0],
            RouterAction::Attach { .. }
        ));
    }

    #[test]
    fn a_resize_request_is_answered_with_the_real_size_not_obeyed() {
        let mut state = greeted();
        assert_eq!(
            route_control(
                &mut state,
                Control::TerminalResized {
                    terminal_id: "terminal-4".into(),
                    columns: 10,
                    rows: 3
                },
                &facts(false)
            ),
            vec![RouterAction::ReportSize {
                terminal_id: "terminal-4".into()
            }]
        );
    }

    #[test]
    fn requests_this_host_does_not_serve_are_refused_by_name() {
        let mut state = greeted();
        let requests = [
            Control::FilesList {
                request_id: 2,
                worktree_id: None,
                path: "a".into(),
            },
            Control::FileRead {
                request_id: 3,
                worktree_id: Some("1".into()),
                path: "a".into(),
            },
            Control::DiffRequest {
                request_id: 4,
                worktree_id: Some("1".into()),
            },
        ];
        for (index, request) in requests.into_iter().enumerate() {
            let actions = route_control(&mut state, request, &facts(true));
            assert_eq!(actions.len(), 1);
            assert_eq!(error_code_of(&actions[0]), UNSUPPORTED);
            assert!(matches!(
                &actions[0],
                RouterAction::Reply(Control::Error { request_id: Some(id), .. }) if *id == index as u32 + 2
            ));
        }
    }

    #[test]
    fn messages_only_a_host_sends_are_refused_from_a_device() {
        let mut state = greeted();
        for message in [
            Control::AgentList { agents: vec![] },
            Control::TerminalList { terminals: vec![] },
            Control::TerminalClosed {
                terminal_id: "t".into(),
                exit_code: None,
            },
            hello_ack(&facts(false)),
        ] {
            let actions = route_control(&mut state, message, &facts(false));
            assert_eq!(error_code_of(&actions[0]), error_code::MALFORMED);
        }
    }

    #[test]
    fn an_error_from_the_device_is_not_answered() {
        let mut state = greeted();
        assert!(
            route_control(
                &mut state,
                Control::Error {
                    code: "internal".into(),
                    message: "x".into(),
                    request_id: None
                },
                &facts(false)
            )
            .is_empty()
        );
    }

    fn ide_open(stream_id: Option<u32>, app_version: Option<&str>, proto: Option<u32>) -> Control {
        Control::IdeOpen {
            request_id: 5,
            path: String::new(),
            line: None,
            stream_id,
            app_version: app_version.map(str::to_string),
            proto_version: proto,
        }
    }

    #[test]
    fn the_ide_capability_is_advertised_only_when_the_project_server_exists() {
        let mut with = RouterState::default();
        let actions = route_control(&mut with, hello(1), &facts(true));
        assert!(matches!(
            &actions[0],
            RouterAction::Reply(Control::HelloAck { capabilities, .. })
                if capabilities == &["terminal".to_string(), "ide".to_string()]
        ));
        let mut without = RouterState::default();
        let actions = route_control(&mut without, hello(1), &facts(false));
        assert!(matches!(
            &actions[0],
            RouterAction::Reply(Control::HelloAck { capabilities, .. })
                if capabilities == &["terminal".to_string()]
        ));
    }

    #[test]
    fn a_matching_project_request_becomes_an_action() {
        let mut state = greeted();
        assert_eq!(
            route_control(
                &mut state,
                ide_open(Some(3), Some("9.9.9"), Some(68)),
                &facts(true)
            ),
            vec![RouterAction::OpenIde {
                request_id: 5,
                stream_id: 3
            }]
        );
    }

    #[test]
    fn a_project_request_without_the_capability_is_unsupported_and_names_its_request() {
        let mut state = greeted();
        let actions = route_control(
            &mut state,
            ide_open(Some(3), Some("9.9.9"), Some(68)),
            &facts(false),
        );
        assert_eq!(error_code_of(&actions[0]), UNSUPPORTED);
        assert!(matches!(
            &actions[0],
            RouterAction::Reply(Control::Error {
                request_id: Some(5),
                ..
            })
        ));
    }

    #[test]
    fn a_project_request_from_another_build_gets_a_version_error_and_the_session_lives() {
        let mut state = greeted();
        for (app_version, proto) in [("9.9.8", 68), ("9.9.9", 67)] {
            let actions = route_control(
                &mut state,
                ide_open(Some(3), Some(app_version), Some(proto)),
                &facts(true),
            );
            assert_eq!(actions.len(), 1);
            assert!(
                matches!(&actions[0], RouterAction::Reply(_)),
                "a mismatch must not end the session, which still mirrors"
            );
            assert_eq!(error_code_of(&actions[0]), error_code::VERSION);
            let RouterAction::Reply(Control::Error {
                message,
                request_id,
                ..
            }) = &actions[0]
            else {
                panic!("not an error");
            };
            assert_eq!(*request_id, Some(5));
            assert!(
                message.contains("9.9.9") && message.contains("Update"),
                "{message}"
            );
            assert!(message.contains(app_version));
        }
    }

    #[test]
    fn a_project_request_missing_its_stream_or_versions_is_malformed() {
        let mut state = greeted();
        for request in [
            ide_open(None, Some("9.9.9"), Some(68)),
            ide_open(Some(0), Some("9.9.9"), Some(68)),
            ide_open(Some(3), None, Some(68)),
            ide_open(Some(3), Some("9.9.9"), None),
        ] {
            let actions = route_control(&mut state, request, &facts(true));
            assert_eq!(error_code_of(&actions[0]), error_code::MALFORMED);
            assert!(matches!(&actions[0], RouterAction::Reply(_)));
        }
    }

    #[test]
    fn the_files_capability_is_advertised_only_when_files_are_served() {
        let mut with = RouterState::default();
        let actions = route_control(&mut with, hello(1), &files_facts());
        assert!(matches!(
            &actions[0],
            RouterAction::Reply(Control::HelloAck { capabilities, .. })
                if capabilities == &["terminal".to_string(), "files".to_string()]
        ));
        let mut without = RouterState::default();
        let actions = route_control(&mut without, hello(1), &facts(false));
        assert!(matches!(
            &actions[0],
            RouterAction::Reply(Control::HelloAck { capabilities, .. })
                if capabilities == &["terminal".to_string()]
        ));
    }

    #[test]
    fn file_requests_become_actions_when_files_are_served() {
        let mut state = greeted();
        let list = route_control(
            &mut state,
            Control::FilesList {
                request_id: 2,
                worktree_id: Some("7".into()),
                path: "src".into(),
            },
            &files_facts(),
        );
        assert_eq!(
            list,
            vec![RouterAction::Files {
                request_id: 2,
                request: FileRequest::List {
                    worktree_id: Some("7".into()),
                    path: "src".into()
                }
            }]
        );
        let read = route_control(
            &mut state,
            Control::FileRead {
                request_id: 3,
                worktree_id: Some("7".into()),
                path: "a".into(),
            },
            &files_facts(),
        );
        assert!(matches!(
            &read[0],
            RouterAction::Files {
                request: FileRequest::Read { .. },
                ..
            }
        ));
        let diff = route_control(
            &mut state,
            Control::DiffRequest {
                request_id: 4,
                worktree_id: Some("7".into()),
            },
            &files_facts(),
        );
        assert!(matches!(
            &diff[0],
            RouterAction::Files {
                request: FileRequest::Diff { .. },
                ..
            }
        ));
    }

    #[test]
    fn a_read_or_diff_that_names_no_folder_is_malformed_and_names_its_request() {
        let mut state = greeted();
        for (request_id, request) in [
            (
                3,
                Control::FileRead {
                    request_id: 3,
                    worktree_id: None,
                    path: "a".into(),
                },
            ),
            (
                4,
                Control::DiffRequest {
                    request_id: 4,
                    worktree_id: None,
                },
            ),
        ] {
            let actions = route_control(&mut state, request, &files_facts());
            assert_eq!(error_code_of(&actions[0]), error_code::MALFORMED);
            assert!(matches!(
                &actions[0],
                RouterAction::Reply(Control::Error { request_id: Some(id), .. }) if *id == request_id
            ));
        }
    }
}
