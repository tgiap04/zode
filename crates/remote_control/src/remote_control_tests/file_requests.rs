//! File browsing as a paired browser meets it: the real session, the real
//! encryption, a project on a fake disk.

use std::{path::Path, time::Duration};

use fs::FakeFs;
use project::Project;
use remote_relay_protocol::{
    Control, InnerKind, MAX_INNER_PAYLOAD_LEN, decode_control, error_code,
};
use serde_json::json;
use util::path;

use super::*;
use crate::file_browse;

const HOST_STREAMS_FROM: u32 = 0x8000_0000;

async fn signed_in_rig(cx: &mut TestAppContext) -> Rig {
    rig_with_files(
        cx,
        true,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        true,
    )
    .await
}

async fn greeted(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = signed_in_rig(cx).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    (rig, browser, connected)
}

/// As [`greeted`], with a relay connection that has room for only a few frames
/// at once.
async fn greeted_on_a_narrow_relay(cx: &mut TestAppContext) -> (Rig, Browser, Connected) {
    let rig = rig_with_files(
        cx,
        false,
        AccountStatus::SignedIn(AccountUser {
            id: USER_ID.into(),
            email: "ada@example.com".into(),
            name: None,
            avatar_url: None,
        }),
        true,
    )
    .await;
    rig.relay.set_outbound_capacity(4);
    set_remote_control(cx, true, None);
    cx.run_until_parked();
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    (rig, browser, connected)
}

fn read_request(request_id: u32, worktree_id: &str, path: &str) -> Control {
    Control::FileRead {
        request_id,
        worktree_id: Some(worktree_id.to_string()),
        path: path.to_string(),
    }
}

async fn open_project(cx: &mut TestAppContext) -> (Entity<Project>, String) {
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "src": { "main.rs": "fn main() {}\n" }, "README.md": "# work\n" }),
    )
    .await;
    let big: String = (0..40_000)
        .map(|index| format!("line {index:06}\n"))
        .collect();
    fs.insert_file(path!("/work/big.txt"), big.into_bytes())
        .await;
    let project = Project::test(fs, [Path::new(path!("/work"))], cx).await;
    cx.run_until_parked();
    let id = project.read_with(cx, |project, cx| {
        project
            .visible_worktrees(cx)
            .next()
            .expect("a worktree")
            .read(cx)
            .id()
            .to_proto()
            .to_string()
    });
    (project, id)
}

#[gpui::test]
async fn the_files_capability_is_advertised_when_the_host_can_serve_files(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let controls = connected.controls(&browser);
    assert!(
        matches!(
            &controls[0],
            Control::HelloAck { capabilities, .. }
                if capabilities == &["terminal".to_string(), "files".to_string()]
        ),
        "{controls:?}"
    );
}

#[gpui::test]
async fn a_paired_browser_lists_the_roots_then_reads_a_file_in_frames(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);

    connected.send(
        &browser,
        &Control::FilesList {
            request_id: 1,
            worktree_id: None,
            path: String::new(),
        },
    );
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    let Control::FilesListReply {
        request_id: 1,
        entries,
        more: false,
        ..
    } = &controls[0]
    else {
        panic!("{controls:?}");
    };
    assert_eq!(entries[0].name, "work");
    assert_eq!(
        entries[0].worktree_id.as_deref(),
        Some(worktree_id.as_str())
    );

    connected.send(
        &browser,
        &Control::FileRead {
            request_id: 2,
            worktree_id: Some(worktree_id),
            path: "big.txt".into(),
        },
    );
    cx.run_until_parked();
    let frames = connected.receive(&browser);
    let (kind, _, payload) = &frames[0];
    assert_eq!(*kind, InnerKind::Control);
    let Control::FileReadReply {
        request_id: 2,
        size,
        stream_id,
    } = decode_control(payload).expect("a control message")
    else {
        panic!("not a read reply");
    };
    assert!(stream_id >= HOST_STREAMS_FROM);
    assert_eq!(size, 40_000 * 12);

    let data: Vec<&(InnerKind, u32, Vec<u8>)> = frames[1..].iter().collect();
    let (end, content) = data.split_last().expect("frames");
    assert_eq!((end.0, end.1, end.2.len()), (InnerKind::Data, stream_id, 0));
    assert!(content.len() > 1, "a file that size spans several frames");
    assert!(
        content
            .iter()
            .all(|(kind, stream, bytes)| *kind == InnerKind::Data
                && *stream == stream_id
                && !bytes.is_empty()
                && bytes.len() <= MAX_INNER_PAYLOAD_LEN)
    );
    let received: Vec<u8> = content
        .iter()
        .flat_map(|(_, _, bytes)| bytes.clone())
        .collect();
    assert_eq!(received.len() as u64, size);
    assert!(received.starts_with(b"line 000000\n"));
}

#[gpui::test]
async fn a_path_outside_the_folder_is_not_found_over_the_wire_and_the_session_lives(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);

    for (request_id, path) in [(5, "../../etc/passwd"), (6, "/etc/passwd")] {
        connected.send(
            &browser,
            &Control::FileRead {
                request_id,
                worktree_id: Some(worktree_id.clone()),
                path: path.into(),
            },
        );
        cx.run_until_parked();
        let controls = connected.controls(&browser);
        assert!(
            matches!(
                &controls[0],
                Control::Error { code, request_id: Some(id), .. }
                    if code == error_code::NOT_FOUND && *id == request_id
            ),
            "{path}: {controls:?}"
        );
    }
    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();
    assert_eq!(connected.controls(&browser), vec![Control::Pong]);
}

#[gpui::test]
async fn a_read_without_a_folder_is_malformed(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    connected.controls(&browser);
    connected.send(
        &browser,
        &Control::DiffRequest {
            request_id: 8,
            worktree_id: None,
        },
    );
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    assert!(
        matches!(
            &controls[0],
            Control::Error { code, request_id: Some(8), .. } if code == error_code::MALFORMED
        ),
        "{controls:?}"
    );
}

#[gpui::test]
async fn a_file_request_before_hello_is_refused_and_ends_the_session(cx: &mut TestAppContext) {
    let rig = signed_in_rig(cx).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let (_project, worktree_id) = open_project(cx).await;
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(
        &browser,
        &Control::FileRead {
            request_id: 1,
            worktree_id: Some(worktree_id),
            path: "README.md".into(),
        },
    );
    cx.run_until_parked();

    let controls = connected.controls(&browser);
    assert!(
        matches!(&controls[0], Control::Error { code, .. } if code == error_code::MALFORMED),
        "{controls:?}"
    );
    assert!(
        controls
            .iter()
            .all(|control| !matches!(control, Control::FileReadReply { .. })),
        "nothing is served"
    );
    assert!(browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn a_session_that_has_not_proved_the_keys_is_served_nothing(cx: &mut TestAppContext) {
    let rig = signed_in_rig(cx).await;
    let browser = Browser::new(&rig, "browser-1");
    pair(&rig, &browser, cx);
    let (_project, _worktree_id) = open_project(cx).await;
    // The handshake is answered but no frame under its keys has arrived, so
    // the host has nothing to act on and the session is not counted.
    let mut unproven = open_session(&rig, &browser, cx);
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    assert!(unproven.receive(&browser).is_empty());
}

#[gpui::test]
async fn refusals_wait_their_turn_when_the_relay_is_full_and_the_session_lives(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = greeted_on_a_narrow_relay(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);

    rig.relay.pause_delivery();
    let asked: Vec<u32> = (10..30).collect();
    for request_id in &asked {
        connected.send(
            &browser,
            &read_request(*request_id, &worktree_id, "../nope"),
        );
        cx.run_until_parked();
    }
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.sessions().len()),
        1,
        "a relay with no room is not a reason to drop the device"
    );

    rig.relay.resume_delivery();
    let mut answered = Vec::new();
    for _ in 0..200 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        for control in connected.controls(&browser) {
            if let Control::Error {
                request_id: Some(id),
                code,
                ..
            } = control
            {
                assert_eq!(code, error_code::NOT_FOUND);
                answered.push(id);
            }
        }
        if answered.len() == asked.len() {
            break;
        }
    }
    assert_eq!(answered, asked, "every refusal arrives, in order");
    assert!(!browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn requests_that_never_finish_are_cut_off_and_free_their_places(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected) = greeted(cx).await;
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ ".git": {}, "a.txt": "a" }))
        .await;
    fs.with_git_state(Path::new(path!("/work/.git")), false, |state| {
        state.simulated_scoped_diff_hangs = true;
    })
    .expect("a repository");
    let project = Project::test(fs, [Path::new(path!("/work"))], cx).await;
    cx.run_until_parked();
    let worktree_id = project.read_with(cx, |project, cx| {
        project
            .visible_worktrees(cx)
            .next()
            .expect("a worktree")
            .read(cx)
            .id()
            .to_proto()
            .to_string()
    });
    connected.controls(&browser);

    for request_id in 1..=4 {
        connected.send(
            &browser,
            &Control::DiffRequest {
                request_id,
                worktree_id: Some(worktree_id.clone()),
            },
        );
        cx.run_until_parked();
    }
    assert!(
        connected.controls(&browser).is_empty(),
        "nothing answers yet"
    );

    connected.send(&browser, &read_request(5, &worktree_id, "a.txt"));
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    assert!(
        matches!(
            &controls[..],
            [Control::Error { code, request_id: Some(5), .. }] if code == error_code::RATE_LIMITED
        ),
        "all four places are taken: {controls:?}"
    );

    cx.executor()
        .advance_clock(file_browse::REQUEST_DEADLINE + Duration::from_secs(1));
    cx.run_until_parked();
    let controls = connected.controls(&browser);
    let mut cut_off: Vec<u32> = controls
        .iter()
        .filter_map(|control| match control {
            Control::Error {
                code,
                request_id: Some(id),
                ..
            } if code == error_code::INTERNAL => Some(*id),
            _ => None,
        })
        .collect();
    cut_off.sort_unstable();
    assert_eq!(cut_off, [1, 2, 3, 4], "{controls:?}");

    connected.send(&browser, &read_request(6, &worktree_id, "a.txt"));
    cx.run_until_parked();
    let frames = connected.receive(&browser);
    assert!(
        matches!(
            decode_control(&frames[0].2),
            Ok(Control::FileReadReply { request_id: 6, .. })
        ),
        "the places are free again"
    );
    assert!(!browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn answers_waiting_for_a_slow_device_never_pass_the_allowance(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = greeted_on_a_narrow_relay(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);

    // Each answer is 480,000 bytes. A request is let in only while the answers
    // waiting plus the biggest answer it could yet make fit in 8 MiB, which is
    // fourteen or fifteen of them; counting only what is already waiting would
    // let eighteen in.
    rig.relay.pause_delivery();
    for request_id in 1..=20u32 {
        connected.send(&browser, &read_request(request_id, &worktree_id, "big.txt"));
        cx.run_until_parked();
    }
    rig.relay.resume_delivery();
    let (mut served, mut refused) = (0, 0);
    for _ in 0..500 {
        cx.background_executor
            .timer(Duration::from_millis(10))
            .await;
        cx.run_until_parked();
        for control in connected.controls(&browser) {
            match control {
                Control::FileReadReply { .. } => served += 1,
                Control::Error { code, .. } if code == error_code::RATE_LIMITED => refused += 1,
                other => panic!("{other:?}"),
            }
        }
        if served + refused == 20 {
            break;
        }
    }
    assert_eq!(served + refused, 20);
    assert!(
        (13..=15).contains(&served),
        "{served} served, {refused} refused"
    );
    assert!(!browser.client.was_told_closed(connected.session_id));
}

async fn project_whose_diffs_hang(cx: &mut TestAppContext) -> (Entity<Project>, String) {
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ ".git": {}, "a.txt": "a" }))
        .await;
    fs.with_git_state(Path::new(path!("/work/.git")), false, |state| {
        state.simulated_scoped_diff_hangs = true;
    })
    .expect("a repository");
    let project = Project::test(fs, [Path::new(path!("/work"))], cx).await;
    cx.run_until_parked();
    let worktree_id = project.read_with(cx, |project, cx| {
        project
            .visible_worktrees(cx)
            .next()
            .expect("a worktree")
            .read(cx)
            .id()
            .to_proto()
            .to_string()
    });
    (project, worktree_id)
}

fn ask_for_diffs(
    browser: &Browser,
    connected: &mut Connected,
    worktree_id: &str,
    request_ids: std::ops::RangeInclusive<u32>,
    cx: &mut TestAppContext,
) {
    for request_id in request_ids {
        connected.send(
            browser,
            &Control::DiffRequest {
                request_id,
                worktree_id: Some(worktree_id.to_string()),
            },
        );
        cx.run_until_parked();
    }
}

#[gpui::test]
async fn ending_a_session_drops_the_work_it_was_waiting_on(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = greeted(cx).await;
    let (_project, worktree_id) = project_whose_diffs_hang(cx).await;
    connected.controls(&browser);
    let first_session = connected.session_id;

    ask_for_diffs(&browser, &mut connected, &worktree_id, 1..=4, cx);
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.file_requests_in_flight(first_session)),
        4
    );

    rig.host.update(cx, |host, cx| host.disconnect_all(cx));
    cx.run_until_parked();
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.file_requests_in_flight(first_session)),
        0,
        "nothing is left working for a session that is gone"
    );
    connected.controls(&browser);

    // A new session starts with all four places free.
    let mut second = open_session(&rig, &browser, cx);
    second.send(&browser, &hello(1));
    cx.run_until_parked();
    second.controls(&browser);
    ask_for_diffs(&browser, &mut second, &worktree_id, 1..=4, cx);
    assert!(
        second.controls(&browser).is_empty(),
        "none of the four is refused"
    );
    assert_eq!(
        rig.host.read_with(cx, |host, _| host
            .file_requests_in_flight(second.session_id)),
        4
    );

    cx.executor()
        .advance_clock(file_browse::REQUEST_DEADLINE + Duration::from_secs(1));
    cx.run_until_parked();
    let answered: Vec<u32> = second
        .controls(&browser)
        .into_iter()
        .filter_map(|control| match control {
            Control::Error {
                code,
                request_id: Some(id),
                ..
            } if code == error_code::INTERNAL => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(answered.len(), 4);
    assert!(
        connected.receive(&browser).is_empty(),
        "nothing is sent for the session that ended"
    );
    assert_eq!(
        rig.host.read_with(cx, |host, _| host
            .file_requests_in_flight(second.session_id)),
        0
    );
}

async fn session_with_queue_filled_to(
    queued: usize,
    cx: &mut TestAppContext,
) -> (Rig, Browser, Connected, String) {
    let (rig, browser, mut connected) = greeted(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);
    let session_id = connected.session_id;
    rig.host
        .update(cx, |host, _| host.queue_waiting_bytes(session_id, queued));
    (rig, browser, connected, worktree_id)
}

// The allowance for a full queue is 8 MiB, then 64 KiB more for refusals.
const QUEUE_ALLOWANCE: usize = 8 * 1024 * 1024 + 64 * 1024;

#[gpui::test]
async fn a_device_that_has_used_its_allowance_to_the_last_byte_is_ended(cx: &mut TestAppContext) {
    let (_rig, browser, mut connected, worktree_id) =
        session_with_queue_filled_to(QUEUE_ALLOWANCE, cx).await;
    connected.send(&browser, &read_request(1, &worktree_id, "README.md"));
    cx.run_until_parked();
    assert!(browser.client.was_told_closed(connected.session_id));
}

#[gpui::test]
async fn a_device_one_byte_short_of_its_allowance_is_still_refused_politely(
    cx: &mut TestAppContext,
) {
    let (_rig, browser, mut connected, worktree_id) =
        session_with_queue_filled_to(QUEUE_ALLOWANCE - 1, cx).await;
    connected.send(&browser, &read_request(1, &worktree_id, "README.md"));
    cx.run_until_parked();
    assert!(!browser.client.was_told_closed(connected.session_id));
    let controls = connected.controls(&browser);
    assert!(
        controls.iter().any(|control| matches!(
            control,
            Control::Error { code, request_id: Some(1), .. } if code == error_code::RATE_LIMITED
        )),
        "{controls:?}"
    );
}

#[gpui::test]
async fn host_stream_ids_wrap_to_the_start_of_the_hosts_range(cx: &mut TestAppContext) {
    let (rig, browser, mut connected) = greeted(cx).await;
    let (_project, worktree_id) = open_project(cx).await;
    connected.controls(&browser);
    let session_id = connected.session_id;
    rig.host.update(cx, |host, _| {
        host.set_next_host_stream(session_id, u32::MAX)
    });

    let mut stream_ids = Vec::new();
    for request_id in [1, 2] {
        connected.send(
            &browser,
            &read_request(request_id, &worktree_id, "README.md"),
        );
        cx.run_until_parked();
        for (_, _, payload) in connected.receive(&browser) {
            if let Ok(Control::FileReadReply { stream_id, .. }) = decode_control(&payload) {
                stream_ids.push(stream_id);
            }
        }
    }
    assert_eq!(stream_ids, [u32::MAX, HOST_STREAMS_FROM]);
}
