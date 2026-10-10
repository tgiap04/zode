use std::time::{Duration, Instant};

use ring::rand::{SecureRandom as _, SystemRandom};
use sha2::{Digest as _, Sha256};

use crate::{KEY_LEN, PairingMessage, is_low_order_public_key};

pub const NONCE_LEN: usize = 32;
pub const COMMITMENT_LEN: usize = 32;

/// How long an acceptor waits between a pairing request and its reveal.
///
/// Long enough for a person to walk to the other screen, short enough that an
/// abandoned exchange does not keep the acceptor occupied.
pub const PAIRING_EXPIRY: Duration = Duration::from_secs(120);

/// Failed commitment checks or rejected digits before pairing locks. Only an
/// explicit user action unlocks it: a lockout that expires on its own is a
/// rate limit on guessing, not a stop.
pub const MAX_FAILED_PAIRING_ATTEMPTS: u32 = 3;

const SAS_LABEL: &[u8] = b"zode-pair/1";
const SAS_MODULUS: u32 = 1_000_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("the revealed nonce does not match the commitment made before it")]
    CommitmentMismatch,
    #[error("both devices presented the same public key")]
    IdenticalKeys,
    #[error("no randomness available")]
    NoRandomness,
    #[error("the peer presented a public key that cannot be used")]
    InvalidPublicKey,
    #[error("the pairing exchange expired")]
    Expired,
    #[error("another pairing exchange is already in progress")]
    Busy,
    #[error("pairing is locked after repeated failures")]
    LockedOut,
    #[error("there is no pairing exchange to continue")]
    NoPendingExchange,
}

pub fn random_nonce() -> Result<[u8; NONCE_LEN], PairingError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| PairingError::NoRandomness)?;
    Ok(nonce)
}

/// `SHA-256(requester public key ‖ requester nonce)`.
///
/// Sent before the other side shows its nonce, so the requester cannot pick
/// its nonce after seeing the other's. That ordering is what stops someone in
/// the middle from grinding keypairs until the six digits happen to match:
/// without it a million tries is cheap, with it each try needs the victim's
/// cooperation.
pub fn commitment(
    requester_public_key: &[u8; KEY_LEN],
    requester_nonce: &[u8; NONCE_LEN],
) -> [u8; COMMITMENT_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(requester_public_key);
    hasher.update(requester_nonce);
    hasher.finalize().into()
}

/// The six digits both people compare.
///
/// The first 20 bits of the digest, reduced modulo 10^6 and zero-padded. The
/// requester is B and the acceptor is A in the hashed order, regardless of who
/// computes it.
pub fn short_authentication_string(
    acceptor_public_key: &[u8; KEY_LEN],
    requester_public_key: &[u8; KEY_LEN],
    acceptor_nonce: &[u8; NONCE_LEN],
    requester_nonce: &[u8; NONCE_LEN],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SAS_LABEL);
    hasher.update(acceptor_public_key);
    hasher.update(requester_public_key);
    hasher.update(acceptor_nonce);
    hasher.update(requester_nonce);
    let digest = hasher.finalize();
    let first_twenty_bits =
        (u32::from(digest[0]) << 12) | (u32::from(digest[1]) << 4) | (u32::from(digest[2]) >> 4);
    format!("{:06}", first_twenty_bits % SAS_MODULUS)
}

/// What a completed exchange leaves behind. The key is trusted only once the
/// people at both screens have confirmed the string matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingOutcome {
    pub peer_public_key: [u8; KEY_LEN],
    pub short_authentication_string: String,
}

/// The side that starts pairing — the browser.
pub struct PairingRequester {
    public_key: [u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
}

impl PairingRequester {
    pub fn start(public_key: [u8; KEY_LEN], nonce: [u8; NONCE_LEN]) -> (Self, PairingMessage) {
        let request = PairingMessage::PairRequest {
            public_key,
            commitment: commitment(&public_key, &nonce),
        };
        (Self { public_key, nonce }, request)
    }

    /// Answers the acceptor's nonce by revealing our own, and derives the
    /// string to show.
    pub fn receive_accept(
        self,
        acceptor_public_key: [u8; KEY_LEN],
        acceptor_nonce: [u8; NONCE_LEN],
    ) -> Result<(PairingMessage, PairingOutcome), PairingError> {
        if acceptor_public_key == self.public_key {
            return Err(PairingError::IdenticalKeys);
        }
        if is_low_order_public_key(&acceptor_public_key) {
            return Err(PairingError::InvalidPublicKey);
        }
        let outcome = PairingOutcome {
            peer_public_key: acceptor_public_key,
            short_authentication_string: short_authentication_string(
                &acceptor_public_key,
                &self.public_key,
                &acceptor_nonce,
                &self.nonce,
            ),
        };
        Ok((PairingMessage::PairReveal { nonce: self.nonce }, outcome))
    }
}

/// The side that is asked to pair — the desktop.
pub struct PairingAcceptor {
    public_key: [u8; KEY_LEN],
    nonce: [u8; NONCE_LEN],
    requester_public_key: [u8; KEY_LEN],
    requester_commitment: [u8; COMMITMENT_LEN],
    deadline: Instant,
}

impl PairingAcceptor {
    pub fn receive_request(
        public_key: [u8; KEY_LEN],
        nonce: [u8; NONCE_LEN],
        requester_public_key: [u8; KEY_LEN],
        requester_commitment: [u8; COMMITMENT_LEN],
        now: Instant,
    ) -> Result<(Self, PairingMessage), PairingError> {
        if requester_public_key == public_key {
            return Err(PairingError::IdenticalKeys);
        }
        if is_low_order_public_key(&requester_public_key) {
            return Err(PairingError::InvalidPublicKey);
        }
        let accept = PairingMessage::PairAccept { public_key, nonce };
        Ok((
            Self {
                public_key,
                nonce,
                requester_public_key,
                requester_commitment,
                deadline: now + PAIRING_EXPIRY,
            },
            accept,
        ))
    }

    /// Checks the revealed nonce against the commitment from the first message.
    /// A mismatch means the request was not made by whoever now reveals — the
    /// exchange is abandoned, and the string is never shown.
    pub fn receive_reveal(
        self,
        requester_nonce: [u8; NONCE_LEN],
        now: Instant,
    ) -> Result<PairingOutcome, PairingError> {
        if now >= self.deadline {
            return Err(PairingError::Expired);
        }
        let expected = commitment(&self.requester_public_key, &requester_nonce);
        // Not constant-time: the commitment is a public hash the relay already
        // saw, so there is no secret for timing to leak.
        if expected != self.requester_commitment {
            return Err(PairingError::CommitmentMismatch);
        }
        Ok(PairingOutcome {
            peer_public_key: self.requester_public_key,
            short_authentication_string: short_authentication_string(
                &self.public_key,
                &self.requester_public_key,
                &self.nonce,
                &requester_nonce,
            ),
        })
    }
}

/// Holds the acceptor's pairing state across exchanges: one exchange at a
/// time, each with an expiry, and a lockout after repeated failures.
///
/// Without the single-exchange rule a relay can open exchanges faster than a
/// person can look at one, and without the lockout it can retry until the
/// digits happen to agree.
#[derive(Default)]
pub struct PairingGate {
    pending: Option<PairingAcceptor>,
    failed_attempts: u32,
}

impl PairingGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_locked_out(&self) -> bool {
        self.failed_attempts >= MAX_FAILED_PAIRING_ATTEMPTS
    }

    pub fn begin(
        &mut self,
        public_key: [u8; KEY_LEN],
        nonce: [u8; NONCE_LEN],
        requester_public_key: [u8; KEY_LEN],
        requester_commitment: [u8; COMMITMENT_LEN],
        now: Instant,
    ) -> Result<PairingMessage, PairingError> {
        if self.is_locked_out() {
            return Err(PairingError::LockedOut);
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now < pending.deadline)
        {
            return Err(PairingError::Busy);
        }
        let (acceptor, accept) = PairingAcceptor::receive_request(
            public_key,
            nonce,
            requester_public_key,
            requester_commitment,
            now,
        )?;
        self.pending = Some(acceptor);
        Ok(accept)
    }

    /// Ends the pending exchange whatever the outcome: a reveal is one try.
    pub fn reveal(
        &mut self,
        requester_nonce: [u8; NONCE_LEN],
        now: Instant,
    ) -> Result<PairingOutcome, PairingError> {
        if self.is_locked_out() {
            return Err(PairingError::LockedOut);
        }
        let acceptor = self.pending.take().ok_or(PairingError::NoPendingExchange)?;
        let outcome = acceptor.receive_reveal(requester_nonce, now);
        if outcome == Err(PairingError::CommitmentMismatch) {
            self.failed_attempts += 1;
        }
        outcome
    }

    /// The person compared the digits and they did not match.
    pub fn reject_digits(&mut self) {
        self.failed_attempts += 1;
    }

    /// The person compared the digits and they matched.
    pub fn confirm_digits(&mut self) {
        self.failed_attempts = 0;
    }

    /// An explicit user action, and the only way out of a lockout.
    pub fn reset_lockout(&mut self) {
        self.failed_attempts = 0;
        self.pending = None;
    }
}
