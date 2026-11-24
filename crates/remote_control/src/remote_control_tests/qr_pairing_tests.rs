//! Pairing by QR code through the real host: the code on screen, a phone that
//! read it, and what the host does with the proof.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gpui::TestAppContext;
use remote_relay_protocol::{
    PairingMessage, PairingRequester, QR_PROOF_LEN, QR_SECRET_LEN, QrSecret, host_key_fingerprint,
    qr_confirm_code, qr_proof, random_nonce,
};

use super::*;
use crate::RemoteHostEvent;

/// What a phone gets out of the QR code.
struct ScannedCode {
    host_device_id: String,
    fingerprint: [u8; 32],
    secret: [u8; QR_SECRET_LEN],
    expires_at: u64,
}

fn parse_code(uri: &str) -> ScannedCode {
    let query = uri
        .strip_prefix("zode://pair?")
        .expect("the zode scheme and the pair host");
    let field = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .unwrap_or_else(|| panic!("{name} is in the code"))
    };
    assert_eq!(field("v"), "1");
    ScannedCode {
        host_device_id: field("h").to_string(),
        fingerprint: URL_SAFE_NO_PAD
            .decode(field("k"))
            .expect("base64url")
            .try_into()
            .expect("32 bytes"),
        secret: URL_SAFE_NO_PAD
            .decode(field("s"))
            .expect("base64url")
            .try_into()
            .expect("32 bytes"),
        expires_at: field("e").parse().expect("whole seconds"),
    }
}

/// The modal opens: the host shows a code and a phone reads it.
fn show_and_scan(rig: &Rig, cx: &mut TestAppContext) -> ScannedCode {
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    cx.run_until_parked();
    let code = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("a code is shown while the host runs");
    parse_code(&code.uri)
}

fn shown_code(rig: &Rig, cx: &mut TestAppContext) -> Option<ScannedCode> {
    rig.host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .map(|code| parse_code(&code.uri))
}

/// Asks to pair as `browser` and answers the host's nonce with a reveal that
/// carries a proof made from `secret`. Returns the pairing session.
fn pair_with_proof(
    rig: &Rig,
    browser: &Browser,
    secret: [u8; QR_SECRET_LEN],
    cx: &mut TestAppContext,
) -> u32 {
    let (session_id, answered) = try_pair_with_proof(rig, browser, secret, cx);
    assert!(answered, "the host answered the request");
    session_id
}

/// [`pair_with_proof`] for a host that may not answer the request at all, in
/// which case there is no reveal to send.
fn try_pair_with_proof(
    rig: &Rig,
    browser: &Browser,
    secret: [u8; QR_SECRET_LEN],
    cx: &mut TestAppContext,
) -> (u32, bool) {
    try_pair_with_proof_for(rig, browser, secret, &browser.id, cx)
}

/// [`try_pair_with_proof`] with a proof made for `proof_device_id`, which is
/// not the device the relay reports when it is not `browser.id`.
fn try_pair_with_proof_for(
    rig: &Rig,
    browser: &Browser,
    secret: [u8; QR_SECRET_LEN],
    proof_device_id: &str,
    cx: &mut TestAppContext,
) -> (u32, bool) {
    let host_key = rig.host_public_key.lock().expect("the host's key");
    let session_id = browser.client.open(HOST_DEVICE, OpenMode::Pair);
    let nonce = random_nonce().expect("randomness");
    let (requester, request) = PairingRequester::start(*browser.keypair.public_key(), nonce);
    browser
        .client
        .send_text(&encode_pairing(session_id, &request).expect("encodes"));
    cx.run_until_parked();
    let Some(PairingMessage::PairAccept {
        public_key,
        nonce: host_nonce,
    }) = browser.client.take_pairing().pop()
    else {
        return (session_id, false);
    };
    assert_eq!(public_key, host_key);
    let (reveal, _digits) = requester
        .receive_accept(public_key, host_nonce)
        .expect("accepts");
    let PairingMessage::PairReveal { nonce, .. } = reveal else {
        panic!("expected a reveal");
    };
    let proof: [u8; QR_PROOF_LEN] = qr_proof(
        &QrSecret::from_bytes(secret),
        browser.keypair.public_key(),
        &host_key,
        &nonce,
        &host_nonce,
        proof_device_id,
    )
    .expect("a device id that fits");
    let reveal = PairingMessage::PairReveal {
        nonce,
        qr_proof: Some(proof),
    };
    browser
        .client
        .send_text(&encode_pairing(session_id, &reveal).expect("encodes"));
    cx.run_until_parked();
    (session_id, true)
}

fn paired_names(events: &std::cell::RefCell<Vec<RemoteHostEvent>>) -> Vec<String> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            RemoteHostEvent::DevicePaired { device_name } => Some(device_name.clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
async fn the_code_names_this_host_and_its_key_and_expires_in_two_minutes(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let host_key = rig.host_public_key.lock().expect("the host's key");
    let code = show_and_scan(&rig, cx);
    assert_eq!(code.host_device_id, HOST_DEVICE);
    assert_eq!(code.fingerprint, host_key_fingerprint(&host_key));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    assert!(
        (now + 110..=now + 130).contains(&code.expires_at),
        "expires about 120 s from now: {} vs {now}",
        code.expires_at
    );
    let shown = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("shown");
    assert_eq!(shown.remaining, Duration::from_secs(120));
    assert!(
        !format!("{shown:?}").contains("zode://"),
        "the code's text, which holds the secret, is not printed"
    );
}

#[gpui::test]
async fn each_code_comes_with_a_short_confirmation_made_from_its_secret_and_this_hosts_key(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let host_key = rig.host_public_key.lock().expect("the host's key");
    let first = show_and_scan(&rig, cx);
    let shown = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("shown");
    assert_eq!(
        shown.confirm_code,
        qr_confirm_code(&QrSecret::from_bytes(first.secret), &host_key)
    );
    assert_eq!(shown.confirm_code.len(), 4);

    cx.executor().advance_clock(Duration::from_secs(121));
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    let second = shown_code(&rig, cx).expect("a new code");
    let next = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("shown");
    assert_eq!(
        next.confirm_code,
        qr_confirm_code(&QrSecret::from_bytes(second.secret), &host_key),
        "it follows the code on screen"
    );
}

#[gpui::test]
async fn a_phone_that_read_the_code_is_trusted_without_digits(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);

    let session_id = pair_with_proof(&rig, &browser, code.secret, cx);

    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_none()),
        "no question: the QR code was the confirmation"
    );
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0].device_id, "browser-1");
    assert_eq!(trusted[0].public_key, *browser.keypair.public_key());
    assert_eq!(trusted[0].name, "Ada's browser");
    assert!(browser.client.was_told_closed(session_id));
    assert_eq!(paired_names(&events), vec!["Ada's browser".to_string()]);
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "the code is spent"
    );
    assert!(problems(&events).is_empty(), "{:?}", problems(&events));

    // And it can connect straight away, which is the point.
    let mut connected = open_session(&rig, &browser, cx);
    connected.send(&browser, &hello(1));
    cx.run_until_parked();
    connected.send(&browser, &Control::Ping);
    cx.run_until_parked();
    assert!(connected.controls(&browser).contains(&Control::Pong));
}

#[gpui::test]
async fn a_second_phone_with_the_same_code_is_turned_away(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let first = Browser::new(&rig, "browser-1");
    let second = Browser::new(&rig, "browser-2");
    let code = show_and_scan(&rig, cx);
    pair_with_proof(&rig, &first, code.secret, cx);

    let session_id = pair_with_proof(&rig, &second, code.secret, cx);
    assert!(second.client.was_told_closed(session_id));
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted.len(), 1, "only the first phone is trusted");
    assert_eq!(trusted[0].device_id, "browser-1");
}

#[gpui::test]
async fn a_wrong_proof_pins_nothing_and_three_lock_pairing(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    show_and_scan(&rig, cx);
    let code_uri = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("shown")
        .uri;
    let guess = [0x42; QR_SECRET_LEN];

    for attempt in 0..3 {
        let session_id = pair_with_proof(&rig, &browser, guess, cx);
        assert!(
            browser.client.was_told_closed(session_id),
            "attempt {attempt} is turned away"
        );
        assert!(
            rig.host
                .read_with(cx, |host, _| host.pending_pairing().is_none()),
            "a bad proof does not fall back to asking for digits"
        );
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert!(paired_names(&events).is_empty());
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
    let told = problems(&events);
    assert_eq!(told.len(), 1, "the lockout is said once: {told:?}");
    assert!(told[0].0.contains("locked"), "{told:?}");
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "locked, so no code is drawn"
    );
    rig.host.update(cx, |host, cx| host.unlock_pairing(cx));
    assert_eq!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx).map(|code| code.uri)),
        Some(code_uri),
        "wrong guesses do not spend the real code"
    );
}

#[gpui::test]
async fn a_locked_host_refuses_a_phone_that_has_the_right_code(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    for _ in 0..3 {
        pair_with_proof(&rig, &browser, [0x42; QR_SECRET_LEN], cx);
    }
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

    let (session_id, answered) = try_pair_with_proof(&rig, &browser, code.secret, cx);
    assert!(!answered, "a locked host answers nobody");
    assert!(browser.client.was_told_closed(session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty()),
        "unlocking is the person's call, whatever the phone knows"
    );
}

#[gpui::test]
async fn a_proof_after_the_code_was_taken_down_is_refused(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    rig.host.update(cx, |host, cx| host.stop_qr_pairing(cx));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none()
    );

    let session_id = pair_with_proof(&rig, &browser, code.secret, cx);
    assert!(browser.client.was_told_closed(session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
}

#[gpui::test]
async fn a_proof_after_the_code_ran_out_is_refused_and_a_new_code_can_be_shown(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    cx.executor().advance_clock(Duration::from_secs(121));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "an expired code is not shown as good"
    );

    let session_id = pair_with_proof(&rig, &browser, code.secret, cx);
    assert!(browser.client.was_told_closed(session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );

    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    let fresh = rig
        .host
        .read_with(cx, |host, cx| host.qr_pairing(cx))
        .expect("a new code on request");
    assert_ne!(parse_code(&fresh.uri).secret, code.secret);
}

#[gpui::test]
async fn showing_a_code_twice_keeps_the_one_that_is_good(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let first = show_and_scan(&rig, cx);
    let again = show_and_scan(&rig, cx);
    assert_eq!(first.secret, again.secret);
}

#[gpui::test]
async fn no_code_is_made_while_remote_control_is_off(cx: &mut TestAppContext) {
    let rig = rig(cx, false).await;
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    cx.run_until_parked();
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none()
    );
}

#[gpui::test]
async fn a_reveal_without_a_proof_still_asks_for_the_digits_while_a_code_is_shown(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    show_and_scan(&rig, cx);
    begin_pairing_until_asked(&rig, &browser, cx);
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.pending_pairing().is_some()
                && host.qr_pairing(cx).is_some()),
        "the old flow is untouched and the code is still good"
    );
}

#[gpui::test]
async fn a_device_trusted_under_another_key_is_asked_about_not_trusted_by_qr(
    cx: &mut TestAppContext,
) {
    let (rig, browser, mut connected) = paired_and_connected(cx).await;
    connected.controls(&browser);
    let old_key = *browser.keypair.public_key();
    let (events, _subscription) = record_host_events(&rig, cx);
    let rekeyed = Browser {
        id: browser.id,
        keypair: DeviceKeypair::generate().expect("a key"),
        client: browser.client,
    };
    let code = show_and_scan(&rig, cx);

    let session_id = pair_with_proof(&rig, &rekeyed, code.secret, cx);

    let question = rig
        .host
        .read_with(cx, |host, _| host.pending_pairing().cloned())
        .expect("the code does not vouch for a replaced key: the person is asked");
    assert_eq!(question.session_id, session_id);
    assert_eq!(question.replaces.as_deref(), Some("Ada's browser"));
    assert_eq!(question.peer_public_key, *rekeyed.keypair.public_key());
    assert!(question.via_qr, "the phone shows no digits to compare");
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted.len(), 1);
    assert_eq!(
        trusted[0].public_key, old_key,
        "nothing is replaced before the person says so"
    );
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "the code is not drawn beside the question"
    );
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "and no new one is made while it is up"
    );
    assert!(paired_names(&events).is_empty(), "nobody is told it paired");
    assert!(!rekeyed.client.was_told_closed(session_id));

    // The person compares the digits and trusts it, as in the old flow, and
    // only then do sessions made under the old key end.
    let old_session = connected.session_id;
    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, true, cx));
    cx.run_until_parked();
    assert!(rig.host.read_with(cx, |host, _| host.sessions().is_empty()));
    assert!(rekeyed.client.was_told_closed(old_session));
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted[0].public_key, *rekeyed.keypair.public_key());
    assert_eq!(
        paired_names(&events),
        vec!["Ada's browser".to_string()],
        "a trusted QR pairing is announced like any other, which closes the code"
    );
}

#[gpui::test]
async fn refusing_to_replace_a_qr_devices_key_keeps_the_old_key_and_does_not_count(
    cx: &mut TestAppContext,
) {
    let (rig, browser, _connected) = paired_and_connected(cx).await;
    let old_key = *browser.keypair.public_key();
    let rekeyed = Browser {
        id: browser.id,
        keypair: DeviceKeypair::generate().expect("a key"),
        client: browser.client,
    };
    let code = show_and_scan(&rig, cx);
    let session_id = pair_with_proof(&rig, &rekeyed, code.secret, cx);

    rig.host
        .update(cx, |host, cx| host.decide_pairing(session_id, false, cx));
    cx.run_until_parked();

    assert!(rekeyed.client.was_told_closed(session_id));
    let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
    assert_eq!(trusted[0].public_key, old_key, "the old key stays");
    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    assert!(
        shown_code(&rig, cx).is_some(),
        "a code can be shown again once the question is answered"
    );
}

#[gpui::test]
async fn a_proof_from_the_code_before_the_current_one_is_turned_away_without_locking_pairing(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let old = show_and_scan(&rig, cx);
    // The modal makes the next code as the first one runs out.
    cx.executor().advance_clock(Duration::from_secs(121));
    cx.run_until_parked();
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    let current = shown_code(&rig, cx).expect("a new code");
    assert_ne!(current.secret, old.secret);

    // A slow tap on the confirm screen, an old photo: three of them would
    // lock pairing if they counted.
    for attempt in 0..3 {
        let session_id = pair_with_proof(&rig, &browser, old.secret, cx);
        assert!(
            browser.client.was_told_closed(session_id),
            "attempt {attempt} is turned away"
        );
    }

    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert_eq!(
        shown_code(&rig, cx).expect("still shown").secret,
        current.secret,
        "the code on screen was not spent"
    );
    // And that code still pairs (another phone: this one has used the hour's
    // allowance of answered requests).
    let other = Browser::new(&rig, "browser-2");
    pair_with_proof(&rig, &other, current.secret, cx);
    assert_eq!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().len()),
        1
    );
}

#[gpui::test]
async fn a_device_trusted_under_the_same_key_is_trusted_by_qr_without_a_question(
    cx: &mut TestAppContext,
) {
    let (rig, browser, _connected) = paired_and_connected(cx).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let code = show_and_scan(&rig, cx);

    pair_with_proof(&rig, &browser, code.secret, cx);

    assert!(
        rig.host
            .read_with(cx, |host, _| host.pending_pairing().is_none()),
        "the same key is not a replacement"
    );
    assert_eq!(paired_names(&events), vec!["Ada's browser".to_string()]);
}

#[gpui::test]
async fn the_hourly_allowance_applies_to_phones_that_read_the_code(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    // A device may have three requests answered an hour, however they end.
    for _ in 0..3 {
        let session_id = send_pairing_request(&browser, cx);
        browser.client.take_pairing();
        browser.client.close(session_id);
        cx.run_until_parked();
    }

    let (session_id, answered) = try_pair_with_proof(&rig, &browser, code.secret, cx);
    assert!(
        !answered,
        "not even answered, so the proof was never judged"
    );
    assert!(browser.client.was_told_closed(session_id));
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_some(),
        "and the code is not spent by a request that was refused"
    );
}

#[gpui::test]
async fn proofs_with_no_live_code_are_turned_away_without_locking_pairing(cx: &mut TestAppContext) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    rig.host.update(cx, |host, cx| host.stop_qr_pairing(cx));
    cx.run_until_parked();

    // An old photo, a link opened twice: as often as the device may ask.
    for attempt in 0..3 {
        let session_id = pair_with_proof(&rig, &browser, code.secret, cx);
        assert!(
            browser.client.was_told_closed(session_id),
            "attempt {attempt} is turned away"
        );
    }

    assert!(
        !rig.host.read_with(cx, |host, _| host.pairing_locked_out()),
        "none of that was a guess"
    );
    assert!(
        problems(&events)
            .iter()
            .all(|(message, _)| !message.contains("locked")),
        "{:?}",
        problems(&events)
    );
    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
}

#[gpui::test]
async fn a_proof_made_for_another_device_id_is_refused_and_counts_as_a_guess(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);

    // The relay reports the phone as "browser-1"; the proof was made for the
    // id the phone really has.
    for _ in 0..3 {
        let (session_id, answered) =
            try_pair_with_proof_for(&rig, &browser, code.secret, "phone-device", cx);
        assert!(answered);
        assert!(browser.client.was_told_closed(session_id));
    }

    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty())
    );
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
}

#[gpui::test]
async fn no_code_is_shown_while_pairing_is_locked_and_one_comes_back_when_it_is_unlocked(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let browser = Browser::new(&rig, "browser-1");
    show_and_scan(&rig, cx);
    for _ in 0..3 {
        pair_with_proof(&rig, &browser, [0x42; QR_SECRET_LEN], cx);
    }
    assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_none(),
        "a code nobody can use is not drawn"
    );

    rig.host.update(cx, |host, cx| host.unlock_pairing(cx));
    rig.host.update(cx, |host, cx| host.show_qr_pairing(cx));
    assert!(
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .is_some()
    );
}

#[gpui::test]
async fn closing_the_code_while_the_phones_name_is_looked_up_trusts_nobody(
    cx: &mut TestAppContext,
) {
    let rig = rig(cx, true).await;
    let (events, _subscription) = record_host_events(&rig, cx);
    let browser = Browser::new(&rig, "browser-1");
    let code = show_and_scan(&rig, cx);
    let release = rig.hold_device_list();

    let session_id = pair_with_proof(&rig, &browser, code.secret, cx);
    assert!(
        !browser.client.was_told_closed(session_id),
        "the proof was good and the name is being looked up"
    );

    rig.host.update(cx, |host, cx| host.stop_qr_pairing(cx));
    release.send(()).ok();
    cx.run_until_parked();

    assert!(
        rig.host
            .read_with(cx, |host, _| host.trusted_devices().is_empty()),
        "a code that was closed pairs nobody, even one already proven"
    );
    assert!(browser.client.was_told_closed(session_id));
    assert!(paired_names(&events).is_empty());
    assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
}

mod in_a_window {
    use gpui::{Entity, VisualTestContext};
    use workspace::Workspace;

    use super::super::with_a_window::click;
    use super::*;
    use crate::{PairPhoneWithQr, qr_pairing_modal::QrPairingModal};

    /// The workspace window, with the host the actions look for installed as
    /// the app's, as the running app has it.
    async fn window<'a>(
        cx: &'a mut TestAppContext,
        rig: &Rig,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.update(|cx| RemoteHost::set_global(rig.host.clone(), cx));
        super::super::with_a_window::window(cx, rig).await
    }

    fn modal_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
        workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<QrPairingModal>(cx)
            })
            .is_some()
    }

    fn open_modal(cx: &mut VisualTestContext) {
        cx.dispatch_action(PairPhoneWithQr);
        cx.run_until_parked();
    }

    fn shown_secret(rig: &Rig, cx: &mut VisualTestContext) -> Option<[u8; QR_SECRET_LEN]> {
        rig.host
            .read_with(cx, |host, cx| host.qr_pairing(cx))
            .map(|code| parse_code(&code.uri).secret)
    }

    #[gpui::test]
    async fn the_action_opens_a_modal_that_draws_the_code(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        open_modal(cx);

        assert!(modal_is_open(&workspace, cx));
        assert!(shown_secret(&rig, cx).is_some(), "a code is made for it");
        let drawn = cx
            .debug_bounds("remote-qr-code")
            .expect("the code is drawn");
        // At least a version 1 code and its quiet zone at 5 px a module, and
        // square: a scanner finds the corners of a square.
        assert!(drawn.size.width >= gpui::px((21. + 8.) * 5.), "{drawn:?}");
        assert_eq!(drawn.size.width, drawn.size.height);
    }

    #[gpui::test]
    async fn the_modal_shows_the_short_confirmation_under_the_code(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (_workspace, cx) = window(cx, &rig).await;
        open_modal(cx);

        assert!(
            cx.debug_bounds("remote-qr-confirm-code").is_some(),
            "the person compares it with the phone's"
        );
    }

    #[gpui::test]
    async fn closing_the_modal_takes_the_code_down(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        open_modal(cx);
        let secret = shown_secret(&rig, cx).expect("shown");

        workspace.update_in(cx, |workspace, window, cx| workspace.hide_modal(window, cx));
        cx.run_until_parked();
        assert!(!modal_is_open(&workspace, cx));
        assert!(shown_secret(&rig, cx).is_none(), "nobody can see it now");

        let session_id = pair_with_proof(&rig, &browser, secret, cx);
        assert!(browser.client.was_told_closed(session_id));
        assert!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().is_empty()),
            "a code that is not on screen pairs nobody"
        );
    }

    #[gpui::test]
    async fn an_open_modal_replaces_a_code_that_ran_out(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        open_modal(cx);
        let first = shown_secret(&rig, cx).expect("shown");

        cx.executor().advance_clock(Duration::from_secs(121));
        cx.run_until_parked();

        assert!(modal_is_open(&workspace, cx), "it stays open");
        let second = shown_secret(&rig, cx).expect("a new code, without being asked");
        assert_ne!(first, second);
    }

    #[gpui::test]
    async fn a_phone_that_pairs_closes_the_modal_and_the_code_is_not_replaced(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        open_modal(cx);
        let secret = shown_secret(&rig, cx).expect("shown");

        pair_with_proof(&rig, &browser, secret, cx);
        cx.run_until_parked();

        assert!(!modal_is_open(&workspace, cx), "paired, so done");
        assert_eq!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().len()),
            1
        );
        assert!(
            shown_secret(&rig, cx).is_none(),
            "no new code once somebody paired"
        );
    }

    #[gpui::test]
    async fn a_replaced_key_by_qr_is_asked_without_digits_and_no_does_not_count(
        cx: &mut TestAppContext,
    ) {
        let (rig, browser, _connected) = paired_and_connected(cx).await;
        let old_key = *browser.keypair.public_key();
        let rekeyed = Browser {
            id: browser.id,
            keypair: DeviceKeypair::generate().expect("a key"),
            client: browser.client,
        };
        let (workspace, cx) = window(cx, &rig).await;
        open_modal(cx);
        let secret = shown_secret(&rig, cx).expect("shown");

        let session_id = pair_with_proof(&rig, &rekeyed, secret, cx);
        cx.run_until_parked();

        assert!(
            cx.debug_bounds("remote-pairing-replace-question").is_some(),
            "the person is asked a plain question about replacing the key"
        );
        assert!(
            cx.debug_bounds("remote-pairing-digits").is_none(),
            "the phone shows no digits, so none are shown here"
        );
        assert!(
            cx.debug_bounds("remote-qr-code").is_none(),
            "the code is not drawn while the question is up"
        );
        assert!(!modal_is_open(&workspace, cx));

        click("remote-pairing-reject", cx);

        assert!(rekeyed.client.was_told_closed(session_id));
        let trusted = rig.host.read_with(cx, |host, _| host.trusted_devices());
        assert_eq!(trusted[0].public_key, old_key);
        assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
    }

    #[gpui::test]
    async fn signed_out_the_modal_asks_for_a_sign_in_and_shows_no_code(cx: &mut TestAppContext) {
        let rig = rig_with_status(cx, true, AccountStatus::SignedOut).await;
        let (workspace, cx) = window(cx, &rig).await;
        open_modal(cx);

        assert!(modal_is_open(&workspace, cx));
        assert!(cx.debug_bounds("remote-qr-code").is_none());
        assert!(shown_secret(&rig, cx).is_none());
        assert_eq!(
            rig.host.read_with(cx, |host, _| host.state().clone()),
            HostState::SignedOut
        );
    }

    #[gpui::test]
    async fn the_action_turns_remote_control_on_when_it_is_off(cx: &mut TestAppContext) {
        let rig = rig(cx, false).await;
        cx.update(|cx| {
            <dyn fs::Fs>::set_global(fs::FakeFs::new(cx.background_executor().clone()), cx);
        });
        let (workspace, cx) = window(cx, &rig).await;
        assert_eq!(
            rig.host.read_with(cx, |host, _| host.state().clone()),
            HostState::Disabled
        );
        open_modal(cx);
        cx.run_until_parked();

        assert!(rig.relay.is_host_connected(), "hosting is on");
        assert!(modal_is_open(&workspace, cx));
        assert!(
            shown_secret(&rig, cx).is_some(),
            "the code appears once hosting is up"
        );
        assert!(cx.debug_bounds("remote-qr-code").is_some());
    }

    #[gpui::test]
    async fn closing_the_modal_while_the_phones_name_is_looked_up_trusts_nobody(
        cx: &mut TestAppContext,
    ) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        open_modal(cx);
        let secret = shown_secret(&rig, cx).expect("shown");
        let release = rig.hold_device_list();

        pair_with_proof(&rig, &browser, secret, cx);
        workspace.update_in(cx, |workspace, window, cx| workspace.hide_modal(window, cx));
        release.send(()).ok();
        cx.run_until_parked();

        assert!(
            rig.host
                .read_with(cx, |host, _| host.trusted_devices().is_empty()),
            "the window was closed before trust completed"
        );
    }

    #[gpui::test]
    async fn a_locked_host_shows_how_to_unlock_instead_of_a_code(cx: &mut TestAppContext) {
        let rig = rig(cx, true).await;
        let (workspace, cx) = window(cx, &rig).await;
        let browser = Browser::new(&rig, "browser-1");
        open_modal(cx);
        for _ in 0..3 {
            pair_with_proof(&rig, &browser, [0x42; QR_SECRET_LEN], cx);
        }
        cx.run_until_parked();
        assert!(rig.host.read_with(cx, |host, _| host.pairing_locked_out()));

        assert!(modal_is_open(&workspace, cx));
        assert!(
            cx.debug_bounds("remote-qr-code").is_none(),
            "no code is drawn while every scan would be refused"
        );
        super::super::with_a_window::click("remote-qr-unlock", cx);

        assert!(!rig.host.read_with(cx, |host, _| host.pairing_locked_out()));
        assert!(modal_is_open(&workspace, cx), "it stays open");
        assert!(
            cx.debug_bounds("remote-qr-code").is_some(),
            "unlocked, a code is drawn again"
        );
    }
}
