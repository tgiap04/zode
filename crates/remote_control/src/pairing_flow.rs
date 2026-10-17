//! The host's side of pairing, as a state machine over plain values.
//!
//! `remote_relay_protocol` supplies the cryptography and the single-exchange,
//! expiry and lockout rules; this adds what a person at the screen adds to
//! them: only one question on screen at a time, a cap on how often the
//! question may be asked, and a deadline that covers the whole conversation
//! rather than just the first half of it.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use remote_relay_protocol::{
    COMMITMENT_LEN, KEY_LEN, NONCE_LEN, PAIRING_EXPIRY, PairingError, PairingGate, PairingMessage,
};

/// How many times an unpaired device may ask in an hour. A relay that wants to
/// wear a person down by putting the same question in front of them
/// repeatedly gets three tries.
pub const MAX_PAIRING_REQUESTS_PER_HOUR: usize = 3;

const REQUEST_WINDOW: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingRefusal {
    RateLimited,
    /// Another pairing is under way, or waiting to be answered.
    Busy,
    /// Too many failures; only [`PairingFlow::unlock`] ends it.
    LockedOut,
    /// The device's key is unusable.
    InvalidRequest,
    /// A reveal arrived with no request before it.
    NoPendingExchange,
    CommitmentMismatch,
    Expired,
}

impl From<PairingError> for PairingRefusal {
    fn from(error: PairingError) -> Self {
        match error {
            PairingError::Busy => Self::Busy,
            PairingError::LockedOut => Self::LockedOut,
            PairingError::Expired => Self::Expired,
            PairingError::CommitmentMismatch => Self::CommitmentMismatch,
            PairingError::NoPendingExchange => Self::NoPendingExchange,
            PairingError::IdenticalKeys
            | PairingError::InvalidPublicKey
            | PairingError::NoRandomness => Self::InvalidRequest,
        }
    }
}

/// The question put to the person at this screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDecision {
    pub session_id: u32,
    pub peer_device_id: String,
    pub peer_name: String,
    pub peer_public_key: [u8; KEY_LEN],
    /// The six digits both people compare.
    pub code: String,
    pub started: Instant,
}

/// What a reveal that checked out leaves to be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revealed {
    pub peer_device_id: String,
}

enum Stage {
    Idle,
    AwaitingReveal {
        session_id: u32,
        peer_device_id: String,
        started: Instant,
    },
    /// The code is known and the device's name is being looked up.
    AwaitingName {
        session_id: u32,
        peer_device_id: String,
        peer_public_key: [u8; KEY_LEN],
        code: String,
        started: Instant,
    },
    Deciding(PendingDecision),
}

pub struct PairingFlow {
    gate: PairingGate,
    stage: Stage,
    requests: VecDeque<Instant>,
}

impl Default for PairingFlow {
    fn default() -> Self {
        Self {
            gate: PairingGate::new(),
            stage: Stage::Idle,
            requests: VecDeque::new(),
        }
    }
}

impl PairingFlow {
    pub fn is_locked_out(&self) -> bool {
        self.gate.is_locked_out()
    }

    pub fn pending_decision(&self) -> Option<&PendingDecision> {
        match &self.stage {
            Stage::Deciding(decision) => Some(decision),
            _ => None,
        }
    }

    /// The session pairing is currently using, if any.
    pub fn active_session(&self) -> Option<u32> {
        match &self.stage {
            Stage::Idle => None,
            Stage::AwaitingReveal { session_id, .. } | Stage::AwaitingName { session_id, .. } => {
                Some(*session_id)
            }
            Stage::Deciding(decision) => Some(decision.session_id),
        }
    }

    fn started(&self) -> Option<Instant> {
        match &self.stage {
            Stage::Idle => None,
            Stage::AwaitingReveal { started, .. } | Stage::AwaitingName { started, .. } => {
                Some(*started)
            }
            Stage::Deciding(decision) => Some(decision.started),
        }
    }

    /// Ends a pairing that has run past its deadline and returns the session
    /// it used, so the caller can close it.
    pub fn expire(&mut self, now: Instant) -> Option<u32> {
        let started = self.started()?;
        if now < started + PAIRING_EXPIRY {
            return None;
        }
        let session_id = self.active_session();
        self.stage = Stage::Idle;
        session_id
    }

    /// A device asks to pair. On success the message to send back.
    pub fn request(
        &mut self,
        session_id: u32,
        peer_device_id: &str,
        local_public_key: [u8; KEY_LEN],
        nonce: [u8; NONCE_LEN],
        requester_public_key: [u8; KEY_LEN],
        commitment: [u8; COMMITMENT_LEN],
        now: Instant,
    ) -> Result<PairingMessage, PairingRefusal> {
        // An exchange that ran out of time no longer counts as in progress,
        // whether or not anyone has swept it yet.
        self.expire(now);
        if self.gate.is_locked_out() {
            return Err(PairingRefusal::LockedOut);
        }
        if !matches!(self.stage, Stage::Idle) {
            return Err(PairingRefusal::Busy);
        }
        while self
            .requests
            .front()
            .is_some_and(|asked| now.saturating_duration_since(*asked) >= REQUEST_WINDOW)
        {
            self.requests.pop_front();
        }
        if self.requests.len() >= MAX_PAIRING_REQUESTS_PER_HOUR {
            return Err(PairingRefusal::RateLimited);
        }

        let accept = self.gate.begin(
            local_public_key,
            nonce,
            requester_public_key,
            commitment,
            now,
        )?;
        self.requests.push_back(now);
        self.stage = Stage::AwaitingReveal {
            session_id,
            peer_device_id: peer_device_id.to_string(),
            started: now,
        };
        Ok(accept)
    }

    /// The device reveals its nonce. A reveal is one try: whatever the
    /// outcome, there is nothing left to reveal to.
    pub fn reveal(
        &mut self,
        session_id: u32,
        nonce: [u8; NONCE_LEN],
        now: Instant,
    ) -> Result<Revealed, PairingRefusal> {
        // Looked at before anything is taken: a reveal from a session that is
        // not the one being paired must change nothing, whatever stage the
        // flow is in, or a stranger could wipe an exchange it is not part of.
        if !matches!(
            &self.stage,
            Stage::AwaitingReveal { session_id: expected, .. } if *expected == session_id
        ) {
            return Err(PairingRefusal::NoPendingExchange);
        }
        let Stage::AwaitingReveal {
            peer_device_id,
            started,
            ..
        } = std::mem::replace(&mut self.stage, Stage::Idle)
        else {
            return Err(PairingRefusal::NoPendingExchange);
        };
        let outcome = self.gate.reveal(nonce, now)?;
        self.stage = Stage::AwaitingName {
            session_id,
            peer_device_id: peer_device_id.clone(),
            peer_public_key: outcome.peer_public_key,
            code: outcome.short_authentication_string,
            started,
        };
        Ok(Revealed { peer_device_id })
    }

    /// The device's name is known; the question can be asked. `false` if the
    /// exchange ended or ran out of time in the meantime.
    pub fn present(&mut self, session_id: u32, peer_name: String, now: Instant) -> bool {
        self.expire(now);
        match std::mem::replace(&mut self.stage, Stage::Idle) {
            Stage::AwaitingName {
                session_id: expected,
                peer_device_id,
                peer_public_key,
                code,
                started,
            } if expected == session_id => {
                self.stage = Stage::Deciding(PendingDecision {
                    session_id,
                    peer_device_id,
                    peer_name,
                    peer_public_key,
                    code,
                    started,
                });
                true
            }
            other => {
                self.stage = other;
                false
            }
        }
    }

    /// The person compared the digits. `trust` is whether they matched. Only
    /// the question for `session_id`, and only while it is still alive, can be
    /// answered; anything else changes nothing.
    pub fn decide(
        &mut self,
        session_id: u32,
        trust: bool,
        now: Instant,
    ) -> Option<PendingDecision> {
        self.expire(now);
        if !matches!(&self.stage, Stage::Deciding(decision) if decision.session_id == session_id) {
            return None;
        }
        let Stage::Deciding(decision) = std::mem::replace(&mut self.stage, Stage::Idle) else {
            return None;
        };
        if trust {
            self.gate.confirm_digits();
        } else {
            self.gate.reject_digits();
        }
        Some(decision)
    }

    /// The session pairing was using is gone. `true` if pairing was using it.
    pub fn session_gone(&mut self, session_id: u32) -> bool {
        if self.active_session() == Some(session_id) {
            self.stage = Stage::Idle;
            return true;
        }
        false
    }

    /// The only way out of a lockout: an explicit act by the person here.
    /// Returns the session an abandoned pairing was using, to close.
    ///
    /// The hourly request history is kept: unlocking forgives the failures, not
    /// the volume, so it cannot be used to refill the allowance.
    pub fn unlock(&mut self) -> Option<u32> {
        let session_id = self.active_session();
        self.gate.reset_lockout();
        self.stage = Stage::Idle;
        session_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_relay_protocol::{PairingRequester, random_nonce};

    const HOST_KEY: [u8; KEY_LEN] = [0xa1; KEY_LEN];

    struct Browser {
        requester: PairingRequester,
        request: PairingMessage,
        public_key: [u8; KEY_LEN],
        nonce: [u8; NONCE_LEN],
    }

    fn browser(seed: u8) -> Browser {
        let public_key = [seed; KEY_LEN];
        let nonce = [seed.wrapping_add(1); NONCE_LEN];
        let (requester, request) = PairingRequester::start(public_key, nonce);
        Browser {
            requester,
            request,
            public_key,
            nonce,
        }
    }

    fn ask(
        flow: &mut PairingFlow,
        session_id: u32,
        browser: &Browser,
        now: Instant,
    ) -> Result<PairingMessage, PairingRefusal> {
        let PairingMessage::PairRequest {
            public_key,
            commitment,
        } = browser.request.clone()
        else {
            panic!("not a request");
        };
        flow.request(
            session_id,
            "browser-device",
            HOST_KEY,
            random_nonce().unwrap(),
            public_key,
            commitment,
            now,
        )
    }

    #[test]
    fn a_full_exchange_shows_both_people_the_same_six_digits() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let browser = browser(0xb1);
        let accept = ask(&mut flow, 5, &browser, now).unwrap();
        let PairingMessage::PairAccept { public_key, nonce } = accept else {
            panic!("expected an accept");
        };
        assert_eq!(public_key, HOST_KEY);
        let (_reveal, browser_outcome) =
            browser.requester.receive_accept(public_key, nonce).unwrap();

        let revealed = flow
            .reveal(5, browser.nonce, now + Duration::from_secs(10))
            .unwrap();
        assert_eq!(revealed.peer_device_id, "browser-device");
        assert!(
            flow.pending_decision().is_none(),
            "no question before the name is known"
        );
        assert!(flow.present(5, "Ada's laptop".into(), now));

        let decision = flow.pending_decision().unwrap().clone();
        assert_eq!(decision.code, browser_outcome.short_authentication_string);
        assert_eq!(decision.peer_public_key, browser.public_key);
        assert_eq!(decision.peer_name, "Ada's laptop");

        assert_eq!(flow.decide(5, true, now), Some(decision));
        assert!(flow.pending_decision().is_none());
        assert_eq!(flow.active_session(), None);
    }

    #[test]
    fn only_one_pairing_at_a_time_through_every_stage() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let first = browser(1);
        ask(&mut flow, 1, &first, now).unwrap();
        assert_eq!(
            ask(&mut flow, 2, &browser(2), now),
            Err(PairingRefusal::Busy),
            "while awaiting the reveal"
        );

        flow.reveal(1, first.nonce, now).unwrap();
        assert_eq!(
            ask(&mut flow, 2, &browser(2), now),
            Err(PairingRefusal::Busy),
            "while the name is looked up"
        );

        flow.present(1, "n".into(), now);
        assert_eq!(
            ask(&mut flow, 2, &browser(2), now),
            Err(PairingRefusal::Busy),
            "while the question is on screen"
        );

        flow.decide(1, false, now);
        assert!(
            ask(&mut flow, 2, &browser(2), now).is_ok(),
            "free again once answered"
        );
    }

    #[test]
    fn at_most_three_requests_an_hour() {
        let start = Instant::now();
        let mut flow = PairingFlow::default();
        let step = PAIRING_EXPIRY + Duration::from_secs(1);
        for session_id in 0..3u32 {
            let browser = browser(session_id as u8 + 1);
            // Spaced past the gate's own expiry so only the hourly cap is
            // being measured.
            ask(
                &mut flow,
                session_id + 1,
                &browser,
                start + step * session_id,
            )
            .unwrap();
            flow.session_gone(session_id + 1);
        }
        assert_eq!(
            ask(&mut flow, 4, &browser(4), start + step * 3),
            Err(PairingRefusal::RateLimited)
        );
        let next_hour = start + REQUEST_WINDOW;
        assert!(
            ask(&mut flow, 5, &browser(5), next_hour).is_ok(),
            "the window slides"
        );
    }

    #[test]
    fn an_exchange_expires_after_two_minutes_at_any_stage() {
        let start = Instant::now();
        let mut flow = PairingFlow::default();
        let browser = browser(1);
        ask(&mut flow, 9, &browser, start).unwrap();
        assert_eq!(
            flow.expire(start + PAIRING_EXPIRY - Duration::from_secs(1)),
            None
        );
        assert_eq!(flow.expire(start + PAIRING_EXPIRY), Some(9));
        assert_eq!(flow.active_session(), None);

        // The deadline runs from the request, not from the reveal, so a
        // question left on screen does not outlive it.
        let second = self::browser(2);
        let t0 = start + Duration::from_secs(300);
        ask(&mut flow, 10, &second, t0).unwrap();
        flow.reveal(10, second.nonce, t0 + Duration::from_secs(100))
            .unwrap();
        flow.present(10, "n".into(), t0 + Duration::from_secs(100));
        assert_eq!(flow.expire(t0 + Duration::from_secs(119)), None);
        assert_eq!(flow.expire(t0 + PAIRING_EXPIRY), Some(10));
        assert!(flow.pending_decision().is_none());
    }

    #[test]
    fn a_late_reveal_is_refused() {
        let start = Instant::now();
        let mut flow = PairingFlow::default();
        let browser = browser(1);
        ask(&mut flow, 1, &browser, start).unwrap();
        assert_eq!(
            flow.reveal(1, browser.nonce, start + PAIRING_EXPIRY),
            Err(PairingRefusal::Expired)
        );
        assert_eq!(flow.active_session(), None, "a reveal is one try");
    }

    #[test]
    fn a_reveal_that_does_not_match_the_commitment_counts_toward_the_lockout() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        for attempt in 0..3u32 {
            let browser = browser(attempt as u8 + 1);
            // The hourly cap is not what is being tested here.
            flow.requests.clear();
            ask(&mut flow, attempt + 1, &browser, now).unwrap();
            assert_eq!(
                flow.reveal(attempt + 1, [0xee; NONCE_LEN], now),
                Err(PairingRefusal::CommitmentMismatch)
            );
        }
        assert!(flow.is_locked_out());
        assert_eq!(
            ask(&mut flow, 9, &browser(9), now),
            Err(PairingRefusal::LockedOut)
        );
    }

    #[test]
    fn digits_that_do_not_match_count_and_a_match_clears_the_count() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let run = |flow: &mut PairingFlow, session_id: u32, trust: bool| {
            let browser = browser(session_id as u8);
            flow.requests.clear();
            ask(flow, session_id, &browser, now).unwrap();
            flow.reveal(session_id, browser.nonce, now).unwrap();
            flow.present(session_id, "n".into(), now);
            flow.decide(session_id, trust, now);
        };
        run(&mut flow, 1, false);
        run(&mut flow, 2, false);
        run(&mut flow, 3, true);
        assert!(!flow.is_locked_out(), "a match starts the count over");
        run(&mut flow, 4, false);
        run(&mut flow, 5, false);
        run(&mut flow, 6, false);
        assert!(flow.is_locked_out());
    }

    #[test]
    fn a_lockout_does_not_expire_and_only_unlock_ends_it() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        for session_id in 1..=3u32 {
            flow.requests.clear();
            let browser = browser(session_id as u8);
            ask(&mut flow, session_id, &browser, now).unwrap();
            flow.reveal(session_id, [0; NONCE_LEN], now).ok();
        }
        assert!(flow.is_locked_out());
        let much_later = now + Duration::from_secs(60 * 60 * 24 * 30);
        assert_eq!(
            ask(&mut flow, 9, &browser(9), much_later),
            Err(PairingRefusal::LockedOut)
        );

        flow.unlock();
        assert!(!flow.is_locked_out());
        assert!(ask(&mut flow, 9, &browser(9), much_later).is_ok());
    }

    #[test]
    fn a_reveal_on_another_session_does_not_disturb_the_exchange() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let browser = browser(1);
        ask(&mut flow, 1, &browser, now).unwrap();
        assert_eq!(
            flow.reveal(2, browser.nonce, now),
            Err(PairingRefusal::NoPendingExchange)
        );
        assert!(flow.reveal(1, browser.nonce, now).is_ok());
    }

    #[test]
    fn a_session_that_disappears_takes_its_exchange_with_it() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let browser = browser(1);
        ask(&mut flow, 1, &browser, now).unwrap();
        flow.reveal(1, browser.nonce, now).unwrap();
        flow.present(1, "n".into(), now);
        assert!(!flow.session_gone(99));
        assert!(flow.session_gone(1));
        assert!(flow.pending_decision().is_none());
        assert!(
            !flow.present(1, "late".into(), now),
            "a name arriving after is ignored"
        );
    }

    #[test]
    fn a_reveal_with_no_request_is_refused() {
        let mut flow = PairingFlow::default();
        assert_eq!(
            flow.reveal(1, [0; NONCE_LEN], Instant::now()),
            Err(PairingRefusal::NoPendingExchange)
        );
    }

    #[test]
    fn an_unusable_key_is_refused_without_using_up_the_exchange() {
        let now = Instant::now();
        let mut flow = PairingFlow::default();
        let result = flow.request(
            1,
            "d",
            HOST_KEY,
            [1; NONCE_LEN],
            [0; KEY_LEN],
            [0; COMMITMENT_LEN],
            now,
        );
        assert_eq!(result, Err(PairingRefusal::InvalidRequest));
        assert_eq!(flow.active_session(), None);
        assert!(
            flow.requests.is_empty(),
            "a refused key does not spend the hourly allowance"
        );
    }

    /// A flow in each stage, paired with the session id it is using, to aim a
    /// stranger's message at.
    fn flow_at_stage(stage: &str, now: Instant) -> PairingFlow {
        let mut flow = PairingFlow::default();
        if stage == "idle" {
            return flow;
        }
        let browser = browser(1);
        ask(&mut flow, 1, &browser, now).unwrap();
        if stage == "awaiting_reveal" {
            return flow;
        }
        flow.reveal(1, browser.nonce, now).unwrap();
        if stage == "awaiting_name" {
            return flow;
        }
        assert!(flow.present(1, "Ada".into(), now));
        flow
    }

    #[test]
    fn a_reveal_from_a_stranger_leaves_every_stage_as_it_was() {
        let now = Instant::now();
        for stage in ["idle", "awaiting_reveal", "awaiting_name", "deciding"] {
            let mut flow = flow_at_stage(stage, now);
            let before = (flow.active_session(), flow.pending_decision().cloned());
            assert_eq!(
                flow.reveal(77, [3; NONCE_LEN], now),
                Err(PairingRefusal::NoPendingExchange),
                "stage {stage}"
            );
            assert_eq!(
                (flow.active_session(), flow.pending_decision().cloned()),
                before,
                "a reveal on session 77 must not touch the exchange in stage {stage}"
            );
            assert!(!flow.is_locked_out(), "stage {stage}");
        }
    }

    #[test]
    fn a_reveal_for_the_wrong_stage_of_the_right_session_changes_nothing() {
        let now = Instant::now();
        for stage in ["awaiting_name", "deciding"] {
            let mut flow = flow_at_stage(stage, now);
            assert_eq!(
                flow.reveal(1, [3; NONCE_LEN], now),
                Err(PairingRefusal::NoPendingExchange)
            );
            assert_eq!(flow.active_session(), Some(1), "stage {stage}");
        }
    }

    #[test]
    fn an_answer_for_another_session_does_not_end_the_question() {
        let now = Instant::now();
        let mut flow = flow_at_stage("deciding", now);
        assert_eq!(flow.decide(77, true, now), None);
        assert!(
            flow.pending_decision().is_some(),
            "the question is still on screen"
        );
        for stage in ["idle", "awaiting_reveal", "awaiting_name"] {
            let mut flow = flow_at_stage(stage, now);
            let before = flow.active_session();
            assert_eq!(flow.decide(1, true, now), None, "stage {stage}");
            assert_eq!(flow.active_session(), before, "stage {stage}");
        }
    }

    #[test]
    fn a_question_cannot_be_asked_or_answered_after_its_deadline() {
        let start = Instant::now();
        let late = start + PAIRING_EXPIRY + Duration::from_secs(1);

        let mut flow = flow_at_stage("awaiting_name", start);
        assert!(!flow.present(1, "Ada".into(), late), "too late to ask");
        assert_eq!(flow.active_session(), None);

        let mut flow = flow_at_stage("deciding", start);
        assert_eq!(flow.decide(1, true, late), None, "too late to say yes");
        assert_eq!(flow.active_session(), None);
    }

    #[test]
    fn unlocking_does_not_refill_the_hourly_allowance() {
        let start = Instant::now();
        let mut flow = PairingFlow::default();
        let step = PAIRING_EXPIRY + Duration::from_secs(1);
        for session_id in 0..3u32 {
            ask(
                &mut flow,
                session_id + 1,
                &browser(session_id as u8 + 1),
                start + step * session_id,
            )
            .unwrap();
            flow.session_gone(session_id + 1);
        }
        flow.unlock();
        assert_eq!(
            ask(&mut flow, 9, &browser(9), start + step * 3),
            Err(PairingRefusal::RateLimited)
        );
    }
}
