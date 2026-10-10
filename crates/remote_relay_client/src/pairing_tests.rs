//! Pairing from the requesting side, against a scripted host.
//!
//! The rules under test are the ones that keep a relay in the middle from
//! getting itself trusted: nothing is pinned before a person confirms the
//! digits, the key must be the one the account's directory lists, and the host
//! is pinned only after an encrypted session to it has been proven.

use std::{sync::Arc, time::Duration};

use gpui::{Entity, TestAppContext};
use remote_relay_protocol::DeviceKeypair;

use crate::{
    PairingAttempt, PairingFailure, PairingStep, PinnedDevice, RelayHost,
    initiator_tests::{CLIENT_DEVICE, HOST_DEVICE, Rig, host_row, rig_with_host_key},
    scripted_host::ScriptedPairing,
    test_support::FakeRelay,
};

/// A rig whose directory lists `directory_key` for the host, which holds `host_key`.
fn pairing_rig(
    cx: &mut TestAppContext,
    host_key: Arc<DeviceKeypair>,
    directory_key: Option<[u8; 32]>,
) -> Rig {
    rig_with_host_key(
        cx,
        FakeRelay::new(),
        vec![host_row(directory_key)],
        host_key,
    )
}

fn honest_rig(cx: &mut TestAppContext) -> Rig {
    let key = Arc::new(DeviceKeypair::generate().expect("a key"));
    let public = *key.public_key();
    pairing_rig(cx, key, Some(public))
}

async fn host_entry(rig: &Rig, cx: &mut TestAppContext) -> RelayHost {
    rig.initiator
        .update(cx, |initiator, cx| initiator.hosts(cx))
        .await
        .expect("a host list")
        .into_iter()
        .find(|host| host.device_id == HOST_DEVICE)
        .expect("the host is listed")
}

async fn start(rig: &Rig, cx: &mut TestAppContext) -> Entity<PairingAttempt> {
    let host = host_entry(rig, cx).await;
    let attempt = rig
        .initiator
        .update(cx, |initiator, cx| initiator.pair(host, cx));
    cx.run_until_parked();
    attempt
}

fn step(attempt: &Entity<PairingAttempt>, cx: &TestAppContext) -> PairingStep {
    attempt.read_with(cx, |attempt, _| attempt.step().clone())
}

fn pinned(rig: &Rig, cx: &TestAppContext) -> usize {
    rig.initiator
        .read_with(cx, |initiator, _| initiator.pinned_hosts().len())
}

fn digits_of(step: &PairingStep) -> String {
    match step {
        PairingStep::Comparing { digits } => digits.clone(),
        other => panic!("not comparing: {other:?}"),
    }
}

#[gpui::test]
async fn both_screens_show_the_same_digits_and_nothing_is_pinned_yet(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    let digits = digits_of(&step(&attempt, cx));
    assert_eq!(digits.len(), 6);
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.pairing()),
        ScriptedPairing::Comparing { digits }
    );
    assert_eq!(pinned(&rig, cx), 0);
}

#[gpui::test]
async fn nothing_is_pinned_and_no_session_opened_without_confirmation(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    // The host's person is quicker and says yes.
    rig.host
        .update(cx, |host, cx| host.decide_pairing(true, cx));
    cx.run_until_parked();

    assert!(matches!(step(&attempt, cx), PairingStep::Comparing { .. }));
    assert_eq!(pinned(&rig, cx), 0);
    assert!(
        rig.host
            .read_with(cx, |host, _| host.received_controls().is_empty()
                && host.session_ids().is_empty()),
        "no session may be opened before the digits are confirmed"
    );

    attempt.update(cx, |attempt, cx| attempt.cancel(cx));
    cx.run_until_parked();
    assert_eq!(pinned(&rig, cx), 0);
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::Cancelled)
    );
}

#[gpui::test]
async fn a_confirmed_pairing_pins_the_host_once_the_channel_is_proven(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
    assert_eq!(step(&attempt, cx), PairingStep::WaitingForHost);
    cx.run_until_parked();
    assert_eq!(pinned(&rig, cx), 0, "the host has not decided");

    rig.host
        .update(cx, |host, cx| host.decide_pairing(true, cx));
    cx.run_until_parked();

    assert_eq!(step(&attempt, cx), PairingStep::Done);
    assert_eq!(pinned(&rig, cx), 1);
    let host_key = rig.host_key;
    rig.initiator.read_with(cx, |initiator, _| {
        let pins = initiator.pinned_hosts();
        assert_eq!(pins[0].device_id, HOST_DEVICE);
        assert_eq!(pins[0].public_key, host_key);
    });
    assert!(rig.host.read_with(cx, |host, _| host.trusts(CLIENT_DEVICE)));

    // The pairing really did leave both ends able to connect.
    let session = rig.connect(cx).await.expect("a session after pairing");
    assert!(!session.read_with(cx, |session, _| session.is_closed()));
}

#[gpui::test]
async fn the_host_may_answer_before_this_person_does(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    rig.host
        .update(cx, |host, cx| host.decide_pairing(true, cx));
    cx.run_until_parked();
    attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
    cx.run_until_parked();
    assert_eq!(step(&attempt, cx), PairingStep::Done);
    assert_eq!(pinned(&rig, cx), 1);
}

#[gpui::test]
async fn digits_that_differ_pin_nothing_and_close_the_session(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    attempt.update(cx, |attempt, cx| attempt.digits_differ(cx));
    cx.run_until_parked();
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::DigitsDiffer)
    );
    assert_eq!(pinned(&rig, cx), 0);
    assert!(
        !rig.host
            .read_with(cx, |host, _| host.ended_sessions().is_empty()),
        "the relay was told to close the pairing session"
    );
}

#[gpui::test]
async fn a_key_the_directory_does_not_list_is_refused_before_any_digits_are_shown(
    cx: &mut TestAppContext,
) {
    let host_key = Arc::new(DeviceKeypair::generate().expect("a key"));
    let other = DeviceKeypair::generate().expect("a key");
    let rig = pairing_rig(cx, host_key, Some(*other.public_key()));
    let attempt = start(&rig, cx).await;

    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::KeyChanged)
    );
    assert_eq!(pinned(&rig, cx), 0);
    assert_eq!(
        rig.host.read_with(cx, |host, _| host.pairing()),
        ScriptedPairing::None,
        "the host was never sent a nonce, so it shows no digits"
    );
    assert!(
        !rig.host
            .read_with(cx, |host, _| host.ended_sessions().is_empty()),
        "the pairing session was closed"
    );
}

#[gpui::test]
async fn a_machine_in_the_middle_that_could_finish_the_handshake_is_still_not_pinned(
    cx: &mut TestAppContext,
) {
    // The directory lists the real host's key; the pairing answers with another.
    let real = DeviceKeypair::generate().expect("a key");
    let impostor = Arc::new(DeviceKeypair::generate().expect("a key"));
    let rig = pairing_rig(cx, impostor, Some(*real.public_key()));
    // The impostor already trusts this device, so a handshake with it would work.
    let initiator_key = rig.initiator_public_key(cx).await;
    rig.host
        .update(cx, |host, _| host.trust(CLIENT_DEVICE, initiator_key));

    let attempt = start(&rig, cx).await;
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::KeyChanged)
    );
    assert_eq!(pinned(&rig, cx), 0);
    assert!(
        rig.host
            .read_with(cx, |host, _| host.received_controls().is_empty()),
        "no channel was ever brought up with the impostor"
    );
    assert_eq!(
        rig.connect(cx).await.err(),
        Some(crate::ConnectError::NotPaired)
    );
}

#[gpui::test]
async fn a_host_that_declines_leaves_nothing_pinned(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    attempt.update(cx, |attempt, cx| attempt.confirm_digits(cx));
    rig.host
        .update(cx, |host, cx| host.decide_pairing(false, cx));
    cx.run_until_parked();
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::NotAccepted)
    );
    assert_eq!(pinned(&rig, cx), 0);
}

#[gpui::test]
async fn cancelling_closes_the_pairing_session_at_the_relay(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    attempt.update(cx, |attempt, cx| attempt.cancel(cx));
    cx.run_until_parked();
    assert!(
        !rig.host
            .read_with(cx, |host, _| host.ended_sessions().is_empty())
    );
    assert_eq!(pinned(&rig, cx), 0);
}

#[gpui::test]
async fn only_one_attempt_runs_at_a_time(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let first = start(&rig, cx).await;
    let second = start(&rig, cx).await;
    assert_eq!(
        step(&second, cx),
        PairingStep::Failed(PairingFailure::AnotherAttempt)
    );
    assert!(matches!(step(&first, cx), PairingStep::Comparing { .. }));
    first.update(cx, |attempt, cx| attempt.cancel(cx));
    let third = start(&rig, cx).await;
    assert!(matches!(step(&third, cx), PairingStep::Comparing { .. }));
}

#[gpui::test]
async fn a_host_that_has_published_no_key_cannot_be_paired(cx: &mut TestAppContext) {
    let key = Arc::new(DeviceKeypair::generate().expect("a key"));
    let rig = pairing_rig(cx, key, None);
    let attempt = start(&rig, cx).await;
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::HostNotReady)
    );
    assert!(
        rig.host
            .read_with(cx, |host, _| host.ended_sessions().is_empty()),
        "no session was even opened"
    );
}

#[gpui::test]
async fn an_unanswered_comparison_expires(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    let attempt = start(&rig, cx).await;
    cx.executor().advance_clock(Duration::from_secs(121));
    cx.run_until_parked();
    assert_eq!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::TimedOut)
    );
    assert_eq!(pinned(&rig, cx), 0);
}

#[gpui::test]
async fn a_full_list_of_pins_is_found_out_before_a_session_is_opened(cx: &mut TestAppContext) {
    let rig = honest_rig(cx);
    rig.initiator.update(cx, |initiator, cx| {
        let trust = initiator.hosts_trust.as_mut().expect("loaded");
        for index in 0..crate::MAX_PINNED_DEVICES {
            trust
                .pin(
                    PinnedDevice {
                        device_id: format!("other-{index}"),
                        public_key: [1; 32],
                        name: "Other".into(),
                        paired_at: 1,
                    },
                    cx,
                )
                .expect("room");
        }
    });
    let attempt = start(&rig, cx).await;
    cx.run_until_parked();
    assert!(matches!(
        step(&attempt, cx),
        PairingStep::Failed(PairingFailure::Relay(_))
    ));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.session_ids().is_empty()),
        "a session was opened for a pin that could not be written"
    );
}
