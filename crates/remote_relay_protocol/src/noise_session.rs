use std::time::Duration;

use snow::{Builder, HandshakeState, TransportState};
use zeroize::{Zeroize as _, Zeroizing};

use crate::{
    FIELD_SEPARATOR, FrameError, InnerFrame, MAX_INNER_FRAME_LEN, MAX_NOISE_MESSAGE_LEN,
    NOISE_PROTOCOL_NAME, decode_inner_frame, encode_inner_frame, split_data,
};

pub const KEY_LEN: usize = 32;

const NOISE_TAG_LEN: usize = 16;

/// A session ends after this many messages in one direction.
///
/// Far below the 2^64 where AES-GCM's counter nonce would wrap, and far above
/// anything a terminal session sends in a day. Past it the peers pair a fresh
/// handshake rather than rekey, so there is one state machine to get right.
pub const MAX_MESSAGES_PER_DIRECTION: u64 = 1 << 32;

/// The longest payload a handshake message can carry: the ephemeral key and
/// the tag come out of the same 65535 bytes.
pub const MAX_HANDSHAKE_PAYLOAD_LEN: usize = MAX_NOISE_MESSAGE_LEN - KEY_LEN - NOISE_TAG_LEN;

/// How long a responder waits for the rest of a handshake before dropping it.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How many handshakes a responder holds open at once. A relay can start
/// handshakes on a device's behalf for free; without a cap each one is memory
/// the relay gets to spend.
pub const MAX_HALF_OPEN_HANDSHAKES: usize = 8;

const PROLOGUE_LABEL: &str = "zode-remote/1";

/// U-coordinates of the curve's small-order points, with the high bit
/// cleared because X25519 ignores it. Any of these as a public key makes the
/// shared secret independent of the private key.
const SMALL_ORDER_KEYS: [[u8; KEY_LEN]; 7] = [
    [0; KEY_LEN],
    {
        let mut one = [0; KEY_LEN];
        one[0] = 1;
        one
    },
    [
        0xe0, 0xeb, 0x7a, 0x7c, 0x3b, 0x41, 0xb8, 0xae, 0x16, 0x56, 0xe3, 0xfa, 0xf1, 0x9f, 0xc4,
        0x6a, 0xda, 0x09, 0x8d, 0xeb, 0x9c, 0x32, 0xb1, 0xfd, 0x86, 0x62, 0x05, 0x16, 0x5f, 0x49,
        0xb8, 0x00,
    ],
    [
        0x5f, 0x9c, 0x95, 0xbc, 0xa3, 0x50, 0x8c, 0x24, 0xb1, 0xd0, 0xb1, 0x55, 0x9c, 0x83, 0xef,
        0x5b, 0x04, 0x44, 0x5c, 0xc4, 0x58, 0x1c, 0x8e, 0x86, 0xd8, 0x22, 0x4e, 0xdd, 0xd0, 0x9f,
        0x11, 0x57,
    ],
    prime_plus(0xec),
    prime_plus(0xed),
    prime_plus(0xee),
];

const fn prime_plus(first_byte: u8) -> [u8; KEY_LEN] {
    let mut key = [0xff; KEY_LEN];
    key[0] = first_byte;
    key[KEY_LEN - 1] = 0x7f;
    key
}

/// Whether `public_key` is one of the points a Diffie–Hellman against which
/// yields a secret an observer can compute. Refused at every entry point
/// because snow does not check the result of its DH.
pub fn is_low_order_public_key(public_key: &[u8; KEY_LEN]) -> bool {
    let mut masked = *public_key;
    masked[KEY_LEN - 1] &= 0x7f;
    SMALL_ORDER_KEYS.contains(&masked)
}

#[derive(Debug, thiserror::Error)]
pub enum NoiseError {
    #[error("noise: {0}")]
    Noise(#[from] snow::Error),
    #[error("{field} must be non-empty and must not contain a unit separator")]
    InvalidIdentity { field: &'static str },
    #[error("a key must be {KEY_LEN} bytes, found {0}")]
    InvalidKeyLength(usize),
    #[error("message is {actual} bytes, over the {limit}-byte limit")]
    TooLarge { actual: usize, limit: usize },
    #[error("the session has sent or received its maximum number of messages")]
    Exhausted,
    #[error("the session failed to authenticate a message and is closed")]
    Closed,
    #[error("the handshake failed and cannot be continued")]
    HandshakeFailed,
    #[error("a low-order public key was presented")]
    LowOrderKey,
    #[error(transparent)]
    Frame(#[from] FrameError),
}

/// The bytes both sides must agree on before any key is derived.
///
/// Binding the account and both device ids here means a relay cannot take a
/// handshake meant for one pair of devices and replay it into another: the
/// transcript hash would differ and the first message would fail to open.
pub fn build_prologue(
    user_id: &str,
    initiator_device_id: &str,
    responder_device_id: &str,
) -> Result<Vec<u8>, NoiseError> {
    let fields = [
        ("user id", user_id),
        ("initiator device id", initiator_device_id),
        ("responder device id", responder_device_id),
    ];
    let mut prologue = PROLOGUE_LABEL.as_bytes().to_vec();
    for (field, value) in fields {
        if value.is_empty() || value.bytes().any(|byte| byte == FIELD_SEPARATOR) {
            return Err(NoiseError::InvalidIdentity { field });
        }
        prologue.push(FIELD_SEPARATOR);
        prologue.extend_from_slice(value.as_bytes());
    }
    Ok(prologue)
}

/// A device's long-term X25519 identity.
///
/// The private half is wiped on drop. It is the one secret in this protocol
/// that outlives a process, so it never lives longer in memory than it must.
/// Copies made inside snow while a handshake runs are outside our control.
pub struct DeviceKeypair {
    private_key: Zeroizing<[u8; KEY_LEN]>,
    public_key: [u8; KEY_LEN],
}

impl DeviceKeypair {
    pub fn generate() -> Result<Self, NoiseError> {
        let mut keypair = Builder::new(NOISE_PROTOCOL_NAME.parse()?).generate_keypair()?;
        let private_key = key_from_slice(&keypair.private).map(Zeroizing::new);
        keypair.private.zeroize();
        Ok(Self {
            private_key: private_key?,
            public_key: key_from_slice(&keypair.public)?,
        })
    }

    /// Rebuilds a keypair read back from the keychain.
    pub fn from_parts(private_key: Zeroizing<[u8; KEY_LEN]>, public_key: [u8; KEY_LEN]) -> Self {
        Self {
            private_key,
            public_key,
        }
    }

    pub fn private_key(&self) -> &[u8; KEY_LEN] {
        &self.private_key
    }

    pub fn public_key(&self) -> &[u8; KEY_LEN] {
        &self.public_key
    }
}

impl std::fmt::Debug for DeviceKeypair {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceKeypair")
            .field("public_key", &self.public_key)
            .finish_non_exhaustive()
    }
}

fn key_from_slice(bytes: &[u8]) -> Result<[u8; KEY_LEN], NoiseError> {
    bytes
        .try_into()
        .map_err(|_| NoiseError::InvalidKeyLength(bytes.len()))
}

pub struct HandshakeParameters<'a> {
    pub local_private_key: &'a [u8; KEY_LEN],
    pub remote_public_key: &'a [u8; KEY_LEN],
    pub prologue: &'a [u8],
}

/// A Noise KK handshake in progress.
///
/// KK means both sides already know each other's static key, which pairing
/// established, so the handshake is two messages and authenticates both ends.
/// The browser is always the initiator.
///
/// A handshake that fails is dead: snow may have advanced its transcript
/// before noticing, so carrying on would mean using a half-updated state.
pub struct Handshake {
    state: HandshakeState,
    failed: bool,
}

impl Handshake {
    pub fn initiator(parameters: &HandshakeParameters<'_>) -> Result<Self, NoiseError> {
        Self::build(parameters, None, true)
    }

    pub fn responder(parameters: &HandshakeParameters<'_>) -> Result<Self, NoiseError> {
        Self::build(parameters, None, false)
    }

    /// Pins the ephemeral key so the published vectors are reproducible.
    #[cfg(test)]
    pub(crate) fn with_fixed_ephemeral(
        parameters: &HandshakeParameters<'_>,
        ephemeral_private_key: &[u8; KEY_LEN],
        initiator: bool,
    ) -> Result<Self, NoiseError> {
        Self::build(parameters, Some(ephemeral_private_key), initiator)
    }

    fn build(
        parameters: &HandshakeParameters<'_>,
        ephemeral_private_key: Option<&[u8; KEY_LEN]>,
        initiator: bool,
    ) -> Result<Self, NoiseError> {
        if is_low_order_public_key(parameters.remote_public_key) {
            return Err(NoiseError::LowOrderKey);
        }
        let mut builder = Builder::new(NOISE_PROTOCOL_NAME.parse()?)
            .prologue(parameters.prologue)?
            .local_private_key(parameters.local_private_key)?
            .remote_public_key(parameters.remote_public_key)?;
        if let Some(ephemeral_private_key) = ephemeral_private_key {
            builder = builder.fixed_ephemeral_key_for_testing_only(ephemeral_private_key);
        }
        let state = if initiator {
            builder.build_initiator()?
        } else {
            builder.build_responder()?
        };
        Ok(Self {
            state,
            failed: false,
        })
    }

    fn ensure_usable(&self) -> Result<(), NoiseError> {
        if self.failed {
            Err(NoiseError::HandshakeFailed)
        } else {
            Ok(())
        }
    }

    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
        self.ensure_usable()?;
        // Refused before snow sees it, so a caller's oversized payload is not
        // allowed to kill a handshake that never started writing.
        if payload.len() > MAX_HANDSHAKE_PAYLOAD_LEN {
            return Err(NoiseError::TooLarge {
                actual: payload.len(),
                limit: MAX_HANDSHAKE_PAYLOAD_LEN,
            });
        }
        let mut buffer = vec![0u8; MAX_NOISE_MESSAGE_LEN];
        match self.state.write_message(payload, &mut buffer) {
            Ok(length) => {
                buffer.truncate(length);
                Ok(buffer)
            }
            Err(error) => {
                self.failed = true;
                Err(error.into())
            }
        }
    }

    pub fn read_message(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        self.ensure_usable()?;
        let result = self.read_checked(message);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn read_checked(&mut self, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
        if message.len() > MAX_NOISE_MESSAGE_LEN {
            return Err(NoiseError::TooLarge {
                actual: message.len(),
                limit: MAX_NOISE_MESSAGE_LEN,
            });
        }
        // Every handshake message opens with the sender's ephemeral key. snow
        // does not look at the result of its Diffie–Hellman, so a low-order
        // ephemeral key is refused here, before it can make one all-zero.
        if let Some((ephemeral, _)) = message.split_first_chunk::<KEY_LEN>()
            && is_low_order_public_key(ephemeral)
        {
            return Err(NoiseError::LowOrderKey);
        }
        let mut buffer = vec![0u8; MAX_NOISE_MESSAGE_LEN];
        let length = self.state.read_message(message, &mut buffer)?;
        buffer.truncate(length);
        Ok(buffer)
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    pub fn handshake_hash(&self) -> &[u8] {
        self.state.get_handshake_hash()
    }

    /// The initiator's session is confirmed at once: the responder's reply
    /// carries a fresh ephemeral key, so it cannot be a replay. The
    /// responder's is not — message 1 can be replayed by anyone who recorded
    /// it — so see [`Session::is_confirmed`].
    pub fn into_session(self) -> Result<Session, NoiseError> {
        self.ensure_usable()?;
        let confirmed = self.state.is_initiator();
        Ok(Session {
            transport: self.state.into_transport_mode()?,
            closed: false,
            confirmed,
        })
    }
}

/// The transport half, after the handshake.
///
/// Nonces are implicit counters, so messages must be delivered in order —
/// which the relay's single WebSocket per peer guarantees. A message that
/// fails to authenticate closes the session for good rather than skipping it:
/// a relay that can make a session limp on after tampering is a relay that
/// can probe it.
pub struct Session {
    transport: TransportState,
    closed: bool,
    confirmed: bool,
}

impl Session {
    /// Whether the peer has proven it is live, not a replay.
    ///
    /// Always true for the initiator. For the responder it turns true when the
    /// first transport message authenticates; until then a recorded handshake
    /// message 1 replayed by the relay looks identical to a real one, so the
    /// responder must not act on the session — open a terminal, answer a
    /// request — before this is true.
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        self.ensure_open()?;
        if plaintext.len() > MAX_INNER_FRAME_LEN {
            return Err(NoiseError::TooLarge {
                actual: plaintext.len(),
                limit: MAX_INNER_FRAME_LEN,
            });
        }
        if self.transport.sending_nonce() >= MAX_MESSAGES_PER_DIRECTION {
            self.closed = true;
            return Err(NoiseError::Exhausted);
        }
        let mut buffer = vec![0u8; plaintext.len() + NOISE_TAG_LEN];
        match self.transport.write_message(plaintext, &mut buffer) {
            Ok(length) => {
                buffer.truncate(length);
                Ok(buffer)
            }
            Err(error) => {
                self.closed = true;
                Err(error.into())
            }
        }
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, NoiseError> {
        self.ensure_open()?;
        if ciphertext.len() > MAX_NOISE_MESSAGE_LEN {
            return Err(NoiseError::TooLarge {
                actual: ciphertext.len(),
                limit: MAX_NOISE_MESSAGE_LEN,
            });
        }
        if self.transport.receiving_nonce() >= MAX_MESSAGES_PER_DIRECTION {
            self.closed = true;
            return Err(NoiseError::Exhausted);
        }
        let mut buffer = vec![0u8; ciphertext.len()];
        match self.transport.read_message(ciphertext, &mut buffer) {
            Ok(length) => {
                buffer.truncate(length);
                self.confirmed = true;
                Ok(buffer)
            }
            Err(error) => {
                self.closed = true;
                Err(error.into())
            }
        }
    }

    pub fn encrypt_frame(&mut self, frame: &InnerFrame) -> Result<Vec<u8>, NoiseError> {
        self.encrypt(&encode_inner_frame(frame))
    }

    pub fn decrypt_frame(&mut self, ciphertext: &[u8]) -> Result<InnerFrame, NoiseError> {
        let plaintext = self.decrypt(ciphertext)?;
        Ok(decode_inner_frame(&plaintext)?)
    }

    /// Cuts one stream's bytes into data frames and seals each.
    ///
    /// Checks up front that the session has nonces left for every chunk: a
    /// stream cut off halfway by exhaustion leaves the peer with a truncated
    /// file it cannot tell from a complete one.
    pub fn encrypt_data_chunks(
        &mut self,
        stream_id: u32,
        data: &[u8],
    ) -> Result<Vec<Vec<u8>>, NoiseError> {
        self.ensure_open()?;
        let frames = split_data(stream_id, data)?;
        if !chunks_fit(self.transport.sending_nonce(), frames.len()) {
            self.closed = true;
            return Err(NoiseError::Exhausted);
        }
        frames
            .iter()
            .map(|frame| self.encrypt_frame(frame))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn set_receiving_nonce_for_test(&mut self, nonce: u64) {
        self.transport.set_receiving_nonce(nonce);
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn ensure_open(&self) -> Result<(), NoiseError> {
        if self.closed {
            Err(NoiseError::Closed)
        } else {
            Ok(())
        }
    }
}

/// Whether `chunks` more messages fit before the session's message limit.
pub(crate) fn chunks_fit(sending_nonce: u64, chunks: usize) -> bool {
    let remaining = MAX_MESSAGES_PER_DIRECTION.saturating_sub(sending_nonce);
    (chunks as u64) <= remaining
}
