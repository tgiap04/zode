use std::collections::HashSet;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use sha2::{Digest as _, Sha256};

use crate::noise_session::chunks_fit;
use crate::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd-length hex");
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("valid hex"))
        .collect()
}

// ---- frames ----

#[test]
fn relay_frames_round_trip() {
    let bytes = encode_relay_frame(0x0102_0304, b"opaque").expect("encodes");
    assert_eq!(&bytes[..4], &[1, 2, 3, 4]);
    let frame = decode_relay_frame(&bytes).expect("decodes");
    assert_eq!(frame.session_id, 0x0102_0304);
    assert_eq!(frame.payload, b"opaque");
}

#[test]
fn relay_frames_reject_short_empty_and_oversized_input() {
    assert_eq!(
        decode_relay_frame(&[0, 0, 1]),
        Err(FrameError::Truncated {
            actual: 3,
            needed: RELAY_HEADER_LEN
        })
    );
    assert_eq!(
        decode_relay_frame(&[0, 0, 0, 1]),
        Err(FrameError::EmptyPayload)
    );
    assert_eq!(encode_relay_frame(1, &[]), Err(FrameError::EmptyPayload));

    let oversized = vec![0u8; MAX_RELAY_PAYLOAD_LEN + 1];
    assert!(matches!(
        encode_relay_frame(1, &oversized),
        Err(FrameError::TooLarge { .. })
    ));
    let mut wire = vec![0, 0, 0, 1];
    wire.extend_from_slice(&oversized);
    assert!(matches!(
        decode_relay_frame(&wire),
        Err(FrameError::TooLarge { .. })
    ));

    let largest = vec![0u8; MAX_RELAY_PAYLOAD_LEN];
    assert!(encode_relay_frame(1, &largest).is_ok());
}

#[test]
fn inner_frames_round_trip_and_enforce_stream_rules() {
    let data = InnerFrame::data(7, b"bytes".to_vec()).expect("valid");
    assert_eq!(
        decode_inner_frame(&encode_inner_frame(&data)).expect("decodes"),
        data
    );

    let control = InnerFrame::control(b"{}".to_vec()).expect("valid");
    assert_eq!(encode_inner_frame(&control)[..5], [0, 0, 0, 0, 0]);

    assert!(InnerFrame::data(0, b"x".to_vec()).is_err());
    assert!(InnerFrame::control(Vec::new()).is_err());
    assert_eq!(
        decode_inner_frame(&[0, 0, 0, 0, 9, b'x']),
        Err(FrameError::InvalidStream("control frames use stream 0"))
    );
    assert_eq!(
        decode_inner_frame(&[9, 0, 0, 0, 1]),
        Err(FrameError::UnknownKind(9))
    );
    assert!(matches!(
        decode_inner_frame(&[1, 0, 0]),
        Err(FrameError::Truncated { .. })
    ));
    assert!(matches!(
        decode_inner_frame(&[]),
        Err(FrameError::Truncated { .. })
    ));
}

#[test]
fn an_empty_data_frame_marks_the_end_of_a_stream() {
    let end = InnerFrame::end_of_stream(3).expect("valid");
    let decoded = decode_inner_frame(&encode_inner_frame(&end)).expect("decodes");
    assert!(decoded.payload.is_empty());
    assert_eq!(decoded.stream_id, 3);
}

#[test]
fn splitting_data_respects_the_per_message_limit() {
    assert!(split_data(1, &[]).expect("splits").is_empty());

    let data = vec![7u8; MAX_INNER_PAYLOAD_LEN * 2 + 10];
    let frames = split_data(1, &data).expect("splits");
    assert_eq!(frames.len(), 3);
    assert!(
        frames
            .iter()
            .all(|frame| encode_inner_frame(frame).len() <= MAX_INNER_FRAME_LEN)
    );
    let rejoined: Vec<u8> = frames.into_iter().flat_map(|frame| frame.payload).collect();
    assert_eq!(rejoined, data);

    assert!(split_data(0, b"x").is_err());
}

// ---- messages ----

fn sample_controls() -> Vec<Control> {
    let agent = AgentSummary {
        id: "agent-1".into(),
        name: "Claude".into(),
        status: AgentStatus::WaitingForInput,
        title: Some("Fix the build".into()),
    };
    vec![
        Control::Hello {
            relay_protocol: 1,
            app_version: "0.1.5".into(),
            rpc_protocol: 1,
            capabilities: vec!["terminal".into(), "files".into()],
        },
        Control::HelloAck {
            relay_protocol: 1,
            app_version: "0.1.5".into(),
            rpc_protocol: 1,
            capabilities: Vec::new(),
        },
        Control::Error {
            code: error_code::NOT_FOUND.into(),
            message: "no such terminal".into(),
            request_id: Some(4),
        },
        Control::Ping,
        Control::Pong,
        Control::AgentList {
            agents: vec![agent.clone()],
        },
        Control::AgentUpdate { agent },
        Control::TerminalList {
            terminals: vec![TerminalSummary {
                id: "t1".into(),
                title: "zsh".into(),
                columns: 80,
                rows: 24,
            }],
        },
        Control::TerminalAttach {
            terminal_id: "t1".into(),
            stream_id: 5,
        },
        Control::TerminalAttached {
            terminal_id: "t1".into(),
            stream_id: 5,
            columns: 80,
            rows: 24,
        },
        Control::TerminalDetach {
            terminal_id: "t1".into(),
        },
        Control::TerminalResized {
            terminal_id: "t1".into(),
            columns: 120,
            rows: 40,
        },
        Control::TerminalClosed {
            terminal_id: "t1".into(),
            exit_code: Some(0),
        },
        Control::IdeOpen {
            request_id: 1,
            path: "src/main.rs".into(),
            line: Some(12),
            stream_id: None,
            app_version: None,
            proto_version: None,
        },
        Control::IdeOpened {
            request_id: 1,
            stream_id: None,
            path_style: None,
            shell: None,
            default_shell: None,
        },
        Control::FilesList {
            request_id: 2,
            worktree_id: Some("7".into()),
            path: ".".into(),
        },
        Control::FilesListReply {
            request_id: 2,
            entries: vec![FileEntry {
                name: "Cargo.toml".into(),
                kind: FileKind::File,
                size: Some(10),
                worktree_id: None,
            }],
            truncated: true,
            more: true,
        },
        Control::FileRead {
            request_id: 3,
            worktree_id: Some("7".into()),
            path: "Cargo.toml".into(),
        },
        Control::FileReadReply {
            request_id: 3,
            size: 10,
            stream_id: 9,
        },
        Control::DiffRequest {
            request_id: 6,
            worktree_id: Some("7".into()),
        },
        Control::DiffReply {
            request_id: 6,
            stream_id: 11,
            truncated: true,
        },
    ]
}

#[test]
fn every_control_message_round_trips() {
    let mut tags = HashSet::new();
    for control in sample_controls() {
        let bytes = encode_control(&control).expect("encodes");
        assert_eq!(decode_control(&bytes).expect("decodes"), control);
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let tag = value["t"].as_str().expect("tagged").to_string();
        assert!(tags.insert(tag), "two messages share a tag");
    }
    assert_eq!(tags.len(), 21);
}

#[test]
fn the_project_server_request_and_its_answer_round_trip_with_every_field() {
    for control in [
        Control::IdeOpen {
            request_id: 7,
            path: String::new(),
            line: None,
            stream_id: Some(3),
            app_version: Some("0.1.5".into()),
            proto_version: Some(68),
        },
        Control::IdeOpened {
            request_id: 7,
            stream_id: Some(3),
            path_style: Some("posix".into()),
            shell: Some("/bin/zsh".into()),
            default_shell: Some("/bin/sh".into()),
        },
    ] {
        let bytes = encode_control(&control).expect("encodes");
        assert_eq!(decode_control(&bytes).expect("decodes"), control);
    }
}

#[test]
fn a_project_server_request_from_an_older_peer_has_no_stream_or_versions() {
    let control =
        decode_control(br#"{"t":"ide_open","request_id":1,"path":"a"}"#).expect("decodes");
    assert_eq!(
        control,
        Control::IdeOpen {
            request_id: 1,
            path: "a".into(),
            line: None,
            stream_id: None,
            app_version: None,
            proto_version: None,
        }
    );
}

#[test]
fn an_unknown_message_type_is_an_error_that_names_it() {
    let error = decode_control(br#"{"t":"launch_missiles"}"#).expect_err("unknown type");
    assert!(error.to_string().contains("launch_missiles"), "{error}");
}

#[test]
fn unknown_fields_are_ignored_so_the_format_can_grow() {
    let control = decode_control(br#"{"t":"ide_opened","request_id":2,"added_later":true}"#)
        .expect("decodes");
    assert_eq!(
        control,
        Control::IdeOpened {
            request_id: 2,
            stream_id: None,
            path_style: None,
            shell: None,
            default_shell: None,
        }
    );
}

#[test]
fn file_messages_written_before_their_new_fields_still_decode() {
    assert_eq!(
        decode_control(br#"{"t":"files_list","request_id":1,"path":""}"#).expect("decodes"),
        Control::FilesList {
            request_id: 1,
            worktree_id: None,
            path: String::new(),
        }
    );
    assert_eq!(
        decode_control(
            br#"{"t":"files_list_reply","request_id":1,"entries":[{"name":"a","kind":"file"}]}"#
        )
        .expect("decodes"),
        Control::FilesListReply {
            request_id: 1,
            entries: vec![FileEntry {
                name: "a".into(),
                kind: FileKind::File,
                size: None,
                worktree_id: None,
            }],
            truncated: false,
            more: false,
        }
    );
    assert_eq!(
        decode_control(br#"{"t":"diff_reply","request_id":1,"stream_id":3}"#).expect("decodes"),
        Control::DiffReply {
            request_id: 1,
            stream_id: 3,
            truncated: false,
        }
    );
}

#[test]
fn an_unknown_status_degrades_instead_of_failing_the_list() {
    let control = decode_control(
        br#"{"t":"agent_update","agent":{"id":"a","name":"n","status":"levitating"}}"#,
    )
    .expect("decodes");
    let Control::AgentUpdate { agent } = control else {
        panic!("wrong variant");
    };
    assert_eq!(agent.status, AgentStatus::Unknown);
    assert_eq!(agent.title, None);
}

#[test]
fn malformed_and_oversized_control_messages_are_rejected() {
    assert!(decode_control(b"not json").is_err());
    assert!(decode_control(br#"{"t":"ide_opened"}"#).is_err());

    let huge = Control::Error {
        code: "x".into(),
        message: "y".repeat(MAX_INNER_PAYLOAD_LEN),
        request_id: None,
    };
    assert!(matches!(
        encode_control(&huge),
        Err(MessageError::Frame(FrameError::TooLarge { .. }))
    ));
}

#[test]
fn only_a_foreign_version_is_rejected() {
    assert_eq!(reject_unknown_version(RELAY_PROTOCOL_VERSION), None);
    let Some(Control::Error { code, .. }) = reject_unknown_version(RELAY_PROTOCOL_VERSION + 1)
    else {
        panic!("expected a version error");
    };
    assert_eq!(code, error_code::VERSION);
}

#[test]
fn pairing_messages_round_trip_and_check_lengths() {
    let request = PairingMessage::PairRequest {
        public_key: [1; KEY_LEN],
        commitment: [2; COMMITMENT_LEN],
    };
    let text = serde_json::to_string(&request).expect("serialises");
    assert_eq!(
        serde_json::from_str::<PairingMessage>(&text).expect("parses"),
        request
    );

    let short = r#"{"t":"pair_reveal","nonce":"AAAA"}"#;
    let error = serde_json::from_str::<PairingMessage>(short).expect_err("short nonce");
    assert!(error.to_string().contains("expected 32 bytes"), "{error}");
    assert!(serde_json::from_str::<PairingMessage>(r#"{"t":"pair_reveal","nonce":"!!"}"#).is_err());
}

// ---- pairing ----

fn pair(
    requester_key: [u8; KEY_LEN],
    acceptor_key: [u8; KEY_LEN],
) -> (PairingOutcome, PairingOutcome) {
    let (requester, request) = PairingRequester::start(requester_key, [0xB0; NONCE_LEN]);
    let PairingMessage::PairRequest {
        public_key,
        commitment,
    } = request
    else {
        panic!("not a request");
    };
    let (acceptor, accept) = PairingAcceptor::receive_request(
        acceptor_key,
        [0xA0; NONCE_LEN],
        public_key,
        commitment,
        Instant::now(),
    )
    .expect("accepts");
    let PairingMessage::PairAccept { public_key, nonce } = accept else {
        panic!("not an accept");
    };
    let (reveal, requester_outcome) = requester
        .receive_accept(public_key, nonce)
        .expect("reveals");
    let PairingMessage::PairReveal { nonce } = reveal else {
        panic!("not a reveal");
    };
    (
        requester_outcome,
        acceptor
            .receive_reveal(nonce, Instant::now())
            .expect("verifies"),
    )
}

#[test]
fn both_sides_derive_the_same_six_digits() {
    let (requester_outcome, acceptor_outcome) = pair([1; KEY_LEN], [2; KEY_LEN]);
    assert_eq!(
        requester_outcome.short_authentication_string,
        acceptor_outcome.short_authentication_string
    );
    assert_eq!(requester_outcome.short_authentication_string.len(), 6);
    assert!(
        requester_outcome
            .short_authentication_string
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    );
    assert_eq!(requester_outcome.peer_public_key, [2; KEY_LEN]);
    assert_eq!(acceptor_outcome.peer_public_key, [1; KEY_LEN]);
}

#[test]
fn a_different_key_or_nonce_changes_the_string() {
    let base = short_authentication_string(&[1; 32], &[2; 32], &[3; 32], &[4; 32]);
    assert_ne!(
        base,
        short_authentication_string(&[9; 32], &[2; 32], &[3; 32], &[4; 32])
    );
    assert_ne!(
        base,
        short_authentication_string(&[1; 32], &[2; 32], &[3; 32], &[9; 32])
    );
    // Swapping the roles must not give the same string: the order is part of
    // what both sides agree on.
    assert_ne!(
        base,
        short_authentication_string(&[2; 32], &[1; 32], &[4; 32], &[3; 32])
    );
}

#[test]
fn a_reveal_that_does_not_match_the_commitment_is_refused() {
    let (_requester, request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    let PairingMessage::PairRequest {
        public_key,
        commitment,
    } = request
    else {
        panic!("not a request");
    };
    let (acceptor, _accept) = PairingAcceptor::receive_request(
        [2; KEY_LEN],
        [0xA0; NONCE_LEN],
        public_key,
        commitment,
        Instant::now(),
    )
    .expect("accepts");
    assert_eq!(
        acceptor.receive_reveal([0xB1; NONCE_LEN], Instant::now()),
        Err(PairingError::CommitmentMismatch)
    );
}

#[test]
fn a_swapped_requester_key_fails_the_commitment() {
    // The commitment covers the key as well as the nonce, so a relay that
    // swaps the key between request and reveal is caught, not just a nonce.
    let (_requester, request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    let PairingMessage::PairRequest { commitment, .. } = request else {
        panic!("not a request");
    };
    let (acceptor, _accept) = PairingAcceptor::receive_request(
        [2; KEY_LEN],
        [0xA0; NONCE_LEN],
        [7; KEY_LEN],
        commitment,
        Instant::now(),
    )
    .expect("accepts");
    assert_eq!(
        acceptor.receive_reveal([0xB0; NONCE_LEN], Instant::now()),
        Err(PairingError::CommitmentMismatch)
    );
}

#[test]
fn pairing_with_ones_own_key_is_refused() {
    let (requester, _request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    assert!(matches!(
        requester.receive_accept([1; KEY_LEN], [0xA0; NONCE_LEN]),
        Err(PairingError::IdenticalKeys)
    ));
    assert!(matches!(
        PairingAcceptor::receive_request(
            [1; KEY_LEN],
            [0xA0; NONCE_LEN],
            [1; KEY_LEN],
            [0; COMMITMENT_LEN],
            Instant::now()
        ),
        Err(PairingError::IdenticalKeys)
    ));
}

#[test]
fn random_nonces_differ() {
    assert_ne!(
        random_nonce().expect("nonce"),
        random_nonce().expect("nonce")
    );
}

// ---- noise ----

struct Pair {
    initiator: Session,
    responder: Session,
}

fn handshake(
    initiator_keys: &DeviceKeypair,
    responder_keys: &DeviceKeypair,
    prologue: &[u8],
) -> Result<Pair, NoiseError> {
    let mut initiator = Handshake::initiator(&HandshakeParameters {
        local_private_key: initiator_keys.private_key(),
        remote_public_key: responder_keys.public_key(),
        prologue,
    })?;
    let mut responder = Handshake::responder(&HandshakeParameters {
        local_private_key: responder_keys.private_key(),
        remote_public_key: initiator_keys.public_key(),
        prologue,
    })?;
    let first = initiator.write_message(b"hello")?;
    assert_eq!(responder.read_message(&first)?, b"hello");
    let second = responder.write_message(b"ack")?;
    assert_eq!(initiator.read_message(&second)?, b"ack");
    assert!(initiator.is_finished() && responder.is_finished());
    assert_eq!(initiator.handshake_hash(), responder.handshake_hash());
    Ok(Pair {
        initiator: initiator.into_session()?,
        responder: responder.into_session()?,
    })
}

fn prologue() -> Vec<u8> {
    build_prologue("user-1", "browser", "desktop").expect("prologue")
}

#[test]
fn a_session_carries_frames_in_both_directions() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");

    let control = InnerFrame::control(b"{\"t\":\"ping\"}".to_vec()).expect("frame");
    let sealed = initiator.encrypt_frame(&control).expect("seals");
    assert_eq!(responder.decrypt_frame(&sealed).expect("opens"), control);

    let reply = InnerFrame::data(2, b"output".to_vec()).expect("frame");
    let sealed = responder.encrypt_frame(&reply).expect("seals");
    assert_eq!(initiator.decrypt_frame(&sealed).expect("opens"), reply);
}

#[test]
fn large_streams_are_chunked_and_reassemble() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");

    let data: Vec<u8> = (0..200_000u32).map(|value| (value % 251) as u8).collect();
    let sealed = initiator.encrypt_data_chunks(4, &data).expect("seals");
    assert!(sealed.len() > 3);
    assert!(
        sealed
            .iter()
            .all(|message| message.len() <= MAX_NOISE_MESSAGE_LEN)
    );

    let mut rejoined = Vec::new();
    for message in &sealed {
        let frame = responder.decrypt_frame(message).expect("opens");
        assert_eq!(frame.stream_id, 4);
        rejoined.extend_from_slice(&frame.payload);
    }
    assert_eq!(rejoined, data);
}

#[test]
fn a_different_prologue_breaks_the_handshake() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let mut initiator = Handshake::initiator(&HandshakeParameters {
        local_private_key: initiator_keys.private_key(),
        remote_public_key: responder_keys.public_key(),
        prologue: &prologue(),
    })
    .expect("initiator");
    let mut responder = Handshake::responder(&HandshakeParameters {
        local_private_key: responder_keys.private_key(),
        remote_public_key: initiator_keys.public_key(),
        prologue: &build_prologue("user-2", "browser", "desktop").expect("prologue"),
    })
    .expect("responder");
    let first = initiator.write_message(b"hello").expect("writes");
    assert!(responder.read_message(&first).is_err());
}

#[test]
fn a_stranger_who_knows_no_pinned_key_cannot_complete_the_handshake() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let impostor_keys = DeviceKeypair::generate().expect("keys");
    let mut initiator = Handshake::initiator(&HandshakeParameters {
        local_private_key: initiator_keys.private_key(),
        remote_public_key: responder_keys.public_key(),
        prologue: &prologue(),
    })
    .expect("initiator");
    let mut impostor = Handshake::responder(&HandshakeParameters {
        local_private_key: impostor_keys.private_key(),
        remote_public_key: initiator_keys.public_key(),
        prologue: &prologue(),
    })
    .expect("impostor");
    let first = initiator.write_message(b"hello").expect("writes");
    assert!(impostor.read_message(&first).is_err());
}

#[test]
fn a_tampered_message_closes_the_session() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");

    let mut sealed = initiator.encrypt(b"first").expect("seals");
    let untouched = sealed.clone();
    sealed[0] ^= 1;
    assert!(responder.decrypt(&sealed).is_err());
    assert!(responder.is_closed());
    assert!(matches!(
        responder.decrypt(&untouched),
        Err(NoiseError::Closed)
    ));
    assert!(matches!(responder.encrypt(b"x"), Err(NoiseError::Closed)));
}

#[test]
fn a_replayed_message_is_refused() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");
    let sealed = initiator.encrypt(b"once").expect("seals");
    assert!(responder.decrypt(&sealed).is_ok());
    assert!(responder.decrypt(&sealed).is_err());
}

#[test]
fn a_session_stops_at_its_message_limit() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");
    responder.set_receiving_nonce_for_test(MAX_MESSAGES_PER_DIRECTION);
    let sealed = initiator.encrypt(b"late").expect("seals");
    assert!(matches!(
        responder.decrypt(&sealed),
        Err(NoiseError::Exhausted)
    ));
}

#[test]
fn oversized_plaintext_is_refused_before_encryption() {
    let initiator_keys = DeviceKeypair::generate().expect("keys");
    let responder_keys = DeviceKeypair::generate().expect("keys");
    let Pair { mut initiator, .. } =
        handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");
    assert!(matches!(
        initiator.encrypt(&vec![0u8; MAX_INNER_FRAME_LEN + 1]),
        Err(NoiseError::TooLarge { .. })
    ));
}

#[test]
fn the_prologue_binds_account_and_both_devices_unambiguously() {
    let prologue = build_prologue("u", "i", "r").expect("prologue");
    assert_eq!(prologue, b"zode-remote/1\x1fu\x1fi\x1fr");
    assert!(build_prologue("", "i", "r").is_err());
    assert!(build_prologue("u\x1f", "i", "r").is_err());
    assert!(build_prologue("u", "i", "r\x1fx").is_err());
    assert_ne!(
        build_prologue("ab", "c", "d").expect("prologue"),
        build_prologue("a", "bc", "d").expect("prologue")
    );
}

// ---- pairing hardening ----

const SMALL_ORDER_POINT: &str = "e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800";

#[test]
fn low_order_public_keys_are_recognised_with_or_without_the_high_bit() {
    let mut high_bit = key(SMALL_ORDER_POINT);
    high_bit[KEY_LEN - 1] |= 0x80;
    for point in [[0; KEY_LEN], key(SMALL_ORDER_POINT), high_bit] {
        assert!(is_low_order_public_key(&point));
    }
    assert!(!is_low_order_public_key(&key(RESPONDER_STATIC_PUBLIC)));
    assert!(!is_low_order_public_key(&[1; KEY_LEN]));
}

#[test]
fn pairing_refuses_low_order_keys_on_both_sides() {
    let (_requester, request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    for point in [[0; KEY_LEN], key(SMALL_ORDER_POINT)] {
        assert!(matches!(
            requester_accepts(point),
            Err(PairingError::InvalidPublicKey)
        ));
        let PairingMessage::PairRequest { commitment, .. } = request.clone() else {
            panic!("not a request");
        };
        assert!(matches!(
            PairingAcceptor::receive_request(
                [2; KEY_LEN],
                [0xA0; NONCE_LEN],
                point,
                commitment,
                Instant::now()
            ),
            Err(PairingError::InvalidPublicKey)
        ));
    }
}

fn requester_accepts(acceptor_key: [u8; KEY_LEN]) -> Result<(), PairingError> {
    // `receive_accept` consumes the requester, so each probe builds its own.
    let (requester, _request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    requester
        .receive_accept(acceptor_key, [0xA0; NONCE_LEN])
        .map(|_| ())
}

fn begin(gate: &mut PairingGate, now: Instant) -> Result<(), PairingError> {
    let (_requester, request) = PairingRequester::start([1; KEY_LEN], [0xB0; NONCE_LEN]);
    let PairingMessage::PairRequest {
        public_key,
        commitment,
    } = request
    else {
        panic!("not a request");
    };
    gate.begin([2; KEY_LEN], [0xA0; NONCE_LEN], public_key, commitment, now)
        .map(|_| ())
}

#[test]
fn an_exchange_expires() {
    let start = Instant::now();
    let mut gate = PairingGate::new();
    begin(&mut gate, start).expect("begins");
    assert_eq!(
        gate.reveal([0xB0; NONCE_LEN], start + PAIRING_EXPIRY),
        Err(PairingError::Expired)
    );
    assert_eq!(
        gate.reveal([0xB0; NONCE_LEN], start),
        Err(PairingError::NoPendingExchange)
    );
}

#[test]
fn only_one_exchange_is_pending_until_it_expires() {
    let start = Instant::now();
    let mut gate = PairingGate::new();
    begin(&mut gate, start).expect("begins");
    assert_eq!(
        begin(&mut gate, start + Duration::from_secs(1)),
        Err(PairingError::Busy)
    );
    begin(&mut gate, start + PAIRING_EXPIRY).expect("an expired exchange is replaced");
}

#[test]
fn a_reveal_is_one_try() {
    let start = Instant::now();
    let mut gate = PairingGate::new();
    begin(&mut gate, start).expect("begins");
    gate.reveal([0xB0; NONCE_LEN], start).expect("reveals");
    assert_eq!(
        gate.reveal([0xB0; NONCE_LEN], start),
        Err(PairingError::NoPendingExchange)
    );
}

#[test]
fn repeated_failures_lock_pairing_until_the_user_resets_it() {
    let start = Instant::now();
    let mut gate = PairingGate::new();
    for _ in 0..MAX_FAILED_PAIRING_ATTEMPTS {
        begin(&mut gate, start).expect("begins");
        assert_eq!(
            gate.reveal([0xB1; NONCE_LEN], start),
            Err(PairingError::CommitmentMismatch)
        );
    }
    assert!(gate.is_locked_out());
    assert_eq!(begin(&mut gate, start), Err(PairingError::LockedOut));
    assert_eq!(
        begin(&mut gate, start + PAIRING_EXPIRY * 100),
        Err(PairingError::LockedOut),
        "time does not unlock it"
    );
    gate.reset_lockout();
    begin(&mut gate, start).expect("begins again after an explicit reset");
}

#[test]
fn rejected_digits_count_and_confirmed_digits_clear_the_count() {
    let mut gate = PairingGate::new();
    for _ in 0..MAX_FAILED_PAIRING_ATTEMPTS - 1 {
        gate.reject_digits();
    }
    gate.confirm_digits();
    for _ in 0..MAX_FAILED_PAIRING_ATTEMPTS - 1 {
        gate.reject_digits();
    }
    assert!(!gate.is_locked_out());
    gate.reject_digits();
    assert!(gate.is_locked_out());
}

// ---- handshake hardening ----

fn keys() -> (DeviceKeypair, DeviceKeypair) {
    (
        DeviceKeypair::generate().expect("keys"),
        DeviceKeypair::generate().expect("keys"),
    )
}

fn parameters<'a>(
    own: &'a DeviceKeypair,
    peer: &'a DeviceKeypair,
    prologue: &'a [u8],
) -> HandshakeParameters<'a> {
    HandshakeParameters {
        local_private_key: own.private_key(),
        remote_public_key: peer.public_key(),
        prologue,
    }
}

#[test]
fn a_failed_read_poisons_the_handshake() {
    let (initiator_keys, responder_keys) = keys();
    let prologue = prologue();
    let mut initiator =
        Handshake::initiator(&parameters(&initiator_keys, &responder_keys, &prologue))
            .expect("initiator");
    let mut responder =
        Handshake::responder(&parameters(&responder_keys, &initiator_keys, &prologue))
            .expect("responder");
    let mut first = initiator.write_message(b"hello").expect("writes");
    let genuine = first.clone();
    first[40] ^= 1;
    assert!(responder.read_message(&first).is_err());
    assert!(matches!(
        responder.read_message(&genuine),
        Err(NoiseError::HandshakeFailed)
    ));
    assert!(matches!(
        responder.write_message(b"x"),
        Err(NoiseError::HandshakeFailed)
    ));
    assert!(matches!(
        responder.into_session(),
        Err(NoiseError::HandshakeFailed)
    ));
}

#[test]
fn handshake_payloads_are_capped_like_snow_caps_them() {
    let (initiator_keys, responder_keys) = keys();
    let prologue = prologue();
    let mut initiator =
        Handshake::initiator(&parameters(&initiator_keys, &responder_keys, &prologue))
            .expect("initiator");
    assert!(matches!(
        initiator.write_message(&vec![0; MAX_HANDSHAKE_PAYLOAD_LEN + 1]),
        Err(NoiseError::TooLarge { .. })
    ));
    let message = initiator
        .write_message(&vec![0; MAX_HANDSHAKE_PAYLOAD_LEN])
        .expect("the largest payload still fits, and the refusal did not poison the handshake");
    assert_eq!(message.len(), MAX_NOISE_MESSAGE_LEN);
}

#[test]
fn a_low_order_key_is_refused_as_a_pinned_key_or_an_ephemeral_key() {
    let (own, _) = keys();
    let prologue = prologue();
    let zero = DeviceKeypair::from_parts(Zeroizing::new([7; KEY_LEN]), [0; KEY_LEN]);
    assert!(matches!(
        Handshake::initiator(&parameters(&own, &zero, &prologue)),
        Err(NoiseError::LowOrderKey)
    ));

    let (initiator_keys, responder_keys) = keys();
    let mut responder =
        Handshake::responder(&parameters(&responder_keys, &initiator_keys, &prologue))
            .expect("responder");
    let mut forged = vec![0u8; 32 + 5 + 16];
    forged[40] = 1;
    assert!(matches!(
        responder.read_message(&forged),
        Err(NoiseError::LowOrderKey)
    ));
}

#[test]
fn only_the_initiator_session_starts_confirmed() {
    let (initiator_keys, responder_keys) = keys();
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");
    assert!(initiator.is_confirmed());
    assert!(!responder.is_confirmed());

    let tampered = {
        let mut sealed = initiator.encrypt(b"first").expect("seals");
        sealed[0] ^= 1;
        sealed
    };
    assert!(responder.decrypt(&tampered).is_err());
    assert!(!responder.is_confirmed());
}

#[test]
fn the_first_authentic_message_confirms_the_responder() {
    let (initiator_keys, responder_keys) = keys();
    let Pair {
        mut initiator,
        mut responder,
    } = handshake(&initiator_keys, &responder_keys, &prologue()).expect("handshake");
    let sealed = initiator.encrypt(b"hello").expect("seals");
    responder.decrypt(&sealed).expect("opens");
    assert!(responder.is_confirmed());
}

#[test]
fn a_stream_that_would_outrun_the_message_limit_is_refused_whole() {
    assert!(chunks_fit(0, 3));
    assert!(chunks_fit(MAX_MESSAGES_PER_DIRECTION - 3, 3));
    assert!(!chunks_fit(MAX_MESSAGES_PER_DIRECTION - 2, 3));
    assert!(!chunks_fit(MAX_MESSAGES_PER_DIRECTION, 1));
    assert!(chunks_fit(MAX_MESSAGES_PER_DIRECTION, 0));
}

// ---- published vectors ----

const SPEC: &str = include_str!("../../../docs/src/remote-control-protocol.md");

const INITIATOR_STATIC_PRIVATE: &str =
    "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
const INITIATOR_STATIC_PUBLIC: &str =
    "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";
const RESPONDER_STATIC_PRIVATE: &str =
    "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb";
const RESPONDER_STATIC_PUBLIC: &str =
    "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f";

fn key(text: &str) -> [u8; KEY_LEN] {
    unhex(text).try_into().expect("32-byte key")
}

/// Every vector the spec publishes, computed from fixed inputs. The first
/// element of each pair is the name used in the spec's vector block.
fn computed_vectors() -> Vec<(String, String)> {
    let mut vectors: Vec<(String, String)> = Vec::new();
    let mut push = |name: &str, value: String| vectors.push((name.to_string(), value));

    push("noise.protocol", NOISE_PROTOCOL_NAME.to_string());
    push("noise.user_id", "user-0001".to_string());
    push("noise.initiator_device_id", "device-browser".to_string());
    push("noise.responder_device_id", "device-desktop".to_string());
    let prologue =
        build_prologue("user-0001", "device-browser", "device-desktop").expect("prologue");
    push("noise.prologue", hex(&prologue));
    push("initiator.static.private", INITIATOR_STATIC_PRIVATE.into());
    push("initiator.static.public", INITIATOR_STATIC_PUBLIC.into());
    push("responder.static.private", RESPONDER_STATIC_PRIVATE.into());
    push("responder.static.public", RESPONDER_STATIC_PUBLIC.into());
    let initiator_ephemeral = [0x11u8; KEY_LEN];
    let responder_ephemeral = [0x22u8; KEY_LEN];
    push("initiator.ephemeral.private", hex(&initiator_ephemeral));
    push("responder.ephemeral.private", hex(&responder_ephemeral));

    let initiator_private = key(INITIATOR_STATIC_PRIVATE);
    let initiator_public = key(INITIATOR_STATIC_PUBLIC);
    let responder_private = key(RESPONDER_STATIC_PRIVATE);
    let responder_public = key(RESPONDER_STATIC_PUBLIC);

    let mut initiator = Handshake::with_fixed_ephemeral(
        &HandshakeParameters {
            local_private_key: &initiator_private,
            remote_public_key: &responder_public,
            prologue: &prologue,
        },
        &initiator_ephemeral,
        true,
    )
    .expect("initiator");
    let mut responder = Handshake::with_fixed_ephemeral(
        &HandshakeParameters {
            local_private_key: &responder_private,
            remote_public_key: &initiator_public,
            prologue: &prologue,
        },
        &responder_ephemeral,
        false,
    )
    .expect("responder");

    push("handshake.message1.payload.utf8", "hello".into());
    let first = initiator.write_message(b"hello").expect("writes");
    push("handshake.message1.ciphertext", hex(&first));
    responder.read_message(&first).expect("reads");

    push("handshake.message2.payload.utf8", "hello-ack".into());
    let second = responder.write_message(b"hello-ack").expect("writes");
    push("handshake.message2.ciphertext", hex(&second));
    initiator.read_message(&second).expect("reads");
    push("handshake.hash", hex(initiator.handshake_hash()));

    // The same keys again with empty payloads: an empty payload still carries
    // a 16-byte tag, and an implementation that skips encrypting it differs here.
    let mut empty_initiator = Handshake::with_fixed_ephemeral(
        &HandshakeParameters {
            local_private_key: &initiator_private,
            remote_public_key: &responder_public,
            prologue: &prologue,
        },
        &initiator_ephemeral,
        true,
    )
    .expect("initiator");
    let mut empty_responder = Handshake::with_fixed_ephemeral(
        &HandshakeParameters {
            local_private_key: &responder_private,
            remote_public_key: &initiator_public,
            prologue: &prologue,
        },
        &responder_ephemeral,
        false,
    )
    .expect("responder");
    let empty_first = empty_initiator.write_message(&[]).expect("writes");
    push("handshake.empty.message1.ciphertext", hex(&empty_first));
    empty_responder.read_message(&empty_first).expect("reads");
    let empty_second = empty_responder.write_message(&[]).expect("writes");
    push("handshake.empty.message2.ciphertext", hex(&empty_second));
    empty_initiator.read_message(&empty_second).expect("reads");
    push(
        "handshake.empty.hash",
        hex(empty_initiator.handshake_hash()),
    );

    let mut initiator = initiator.into_session().expect("session");
    let mut responder = responder.into_session().expect("session");

    let control = InnerFrame::control(br#"{"t":"ping"}"#.to_vec()).expect("frame");
    let sealed = initiator.encrypt_frame(&control).expect("seals");
    push(
        "transport.initiator_to_responder.0.frame",
        hex(&encode_inner_frame(&control)),
    );
    push(
        "transport.initiator_to_responder.0.ciphertext",
        hex(&sealed),
    );
    responder.decrypt_frame(&sealed).expect("opens");

    let data = InnerFrame::data(2, b"ls -la\n".to_vec()).expect("frame");
    let sealed = initiator.encrypt_frame(&data).expect("seals");
    push(
        "transport.initiator_to_responder.1.frame",
        hex(&encode_inner_frame(&data)),
    );
    push(
        "transport.initiator_to_responder.1.ciphertext",
        hex(&sealed),
    );
    responder.decrypt_frame(&sealed).expect("opens");

    let reply = InnerFrame::control(br#"{"t":"pong"}"#.to_vec()).expect("frame");
    let sealed = responder.encrypt_frame(&reply).expect("seals");
    push(
        "transport.responder_to_initiator.0.frame",
        hex(&encode_inner_frame(&reply)),
    );
    push(
        "transport.responder_to_initiator.0.ciphertext",
        hex(&sealed),
    );
    initiator.decrypt_frame(&sealed).expect("opens");

    // Counters 2 to 299 are pings, so message 300 is the first whose counter
    // needs more than one byte in its big-endian nonce encoding.
    for _ in 2..300 {
        let sealed = initiator.encrypt_frame(&control).expect("seals");
        responder.decrypt_frame(&sealed).expect("opens");
    }
    let late = InnerFrame::data(2, b"counter-300".to_vec()).expect("frame");
    let sealed = initiator.encrypt_frame(&late).expect("seals");
    push(
        "transport.initiator_to_responder.300.frame",
        hex(&encode_inner_frame(&late)),
    );
    push(
        "transport.initiator_to_responder.300.ciphertext",
        hex(&sealed),
    );
    responder.decrypt_frame(&sealed).expect("opens");

    let relay = encode_relay_frame(0x1234_5678, &[0xde, 0xad, 0xbe, 0xef]).expect("frame");
    push("frame.relay.session_id", "305419896".into());
    push("frame.relay.bytes", hex(&relay));

    let hello = Control::Hello {
        relay_protocol: RELAY_PROTOCOL_VERSION,
        app_version: "0.1.5".into(),
        rpc_protocol: RPC_PROTOCOL_VERSION,
        capabilities: vec!["terminal".into(), "files".into()],
    };
    push(
        "control.hello.json",
        String::from_utf8(encode_control(&hello).expect("encodes")).expect("utf8"),
    );

    let requester_key = [0xB1u8; KEY_LEN];
    let acceptor_key = [0xA1u8; KEY_LEN];
    let requester_nonce = [0xB2u8; NONCE_LEN];
    let acceptor_nonce = [0xA2u8; NONCE_LEN];
    push("pairing.requester.public", hex(&requester_key));
    push("pairing.acceptor.public", hex(&acceptor_key));
    push("pairing.requester.nonce", hex(&requester_nonce));
    push("pairing.acceptor.nonce", hex(&acceptor_nonce));
    push(
        "pairing.commitment",
        hex(&commitment(&requester_key, &requester_nonce)),
    );
    push(
        "pairing.sas",
        short_authentication_string(
            &acceptor_key,
            &requester_key,
            &acceptor_nonce,
            &requester_nonce,
        ),
    );
    vectors
}

fn fixture_digest(vectors: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    for (name, value) in vectors {
        hasher.update(format!("{name} = {value}\n"));
    }
    hex(&hasher.finalize())
}

fn published_vectors() -> Vec<(String, String)> {
    let mut inside = false;
    let mut vectors = Vec::new();
    for line in SPEC.lines() {
        if line.trim_start().starts_with("```") {
            if inside {
                break;
            }
            inside = line.contains("zode-remote-vectors");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((name, value)) = line.split_once(" = ") {
            vectors.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    assert!(
        !vectors.is_empty(),
        "the spec no longer carries a `zode-remote-vectors` block — it is the only \
         thing keeping the published format honest, so it may not simply go away",
    );
    vectors
}

#[test]
fn the_published_vectors_are_what_the_code_computes() {
    let mut published = published_vectors();
    let digest = published
        .iter()
        .position(|(name, _)| name == "fixture.sha256")
        .map(|index| published.remove(index).1)
        .expect("the spec publishes the digest of the browser fixture");
    let computed = computed_vectors();

    assert_eq!(
        published.len(),
        computed.len(),
        "the spec and the code disagree on how many vectors exist"
    );
    for ((published_name, published_value), (computed_name, computed_value)) in
        published.iter().zip(&computed)
    {
        assert_eq!(published_name, computed_name);
        assert_eq!(
            published_value, computed_value,
            "the published `{published_name}` is not what this build produces"
        );
    }
    assert_eq!(digest, fixture_digest(&computed));
}

#[test]
fn the_published_ciphertext_opens_under_an_independent_responder() {
    let published: std::collections::HashMap<_, _> = published_vectors().into_iter().collect();
    let prologue = unhex(&published["noise.prologue"]);
    let responder_private = key(&published["responder.static.private"]);
    let initiator_public = key(&published["initiator.static.public"]);

    // No fixed ephemeral: the responder here generates its own, so this opens
    // the published first message the way any other implementation would.
    let mut responder = Handshake::responder(&HandshakeParameters {
        local_private_key: &responder_private,
        remote_public_key: &initiator_public,
        prologue: &prologue,
    })
    .expect("responder");
    let payload = responder
        .read_message(&unhex(&published["handshake.message1.ciphertext"]))
        .expect("opens the published first message");
    assert_eq!(payload, b"hello");
}

/// Prints the spec's vector block. Run after a deliberate format change:
/// `cargo test -p remote_relay_protocol print_vectors -- --ignored --nocapture`.
#[test]
#[ignore = "regenerates the published vectors"]
fn print_vectors() {
    let computed = computed_vectors();
    for (name, value) in &computed {
        println!("{name} = {value}");
    }
    println!("fixture.sha256 = {}", fixture_digest(&computed));
}
