//! Wire format, end-to-end encryption and device pairing for remote control.
//!
//! Pure and synchronous on purpose: no sockets, no gpui, no clock. Everything
//! here is bytes in, bytes out, so the same code can be held against the
//! published vectors in `docs/src/remote-control-protocol.md` and against the
//! browser implementation of the same spec.

mod frame;
mod messages;
mod noise_session;
mod pairing;

#[cfg(test)]
mod remote_relay_protocol_tests;

pub use frame::{
    FrameError, INNER_HEADER_LEN, InnerFrame, InnerKind, MAX_INNER_FRAME_LEN,
    MAX_INNER_PAYLOAD_LEN, MAX_NOISE_MESSAGE_LEN, MAX_RELAY_PAYLOAD_LEN, RELAY_HEADER_LEN,
    RelayFrame, decode_inner_frame, decode_relay_frame, encode_inner_frame, encode_relay_frame,
    split_data,
};
pub use messages::{
    AgentStatus, AgentSummary, Control, FileEntry, FileKind, MessageError, PairingMessage,
    TerminalSummary, decode_control, encode_control, error_code, reject_unknown_version,
};
pub use noise_session::{
    DeviceKeypair, HANDSHAKE_TIMEOUT, Handshake, HandshakeParameters, KEY_LEN,
    MAX_HALF_OPEN_HANDSHAKES, MAX_HANDSHAKE_PAYLOAD_LEN, MAX_MESSAGES_PER_DIRECTION, NoiseError,
    Session, build_prologue, is_low_order_public_key,
};
pub use pairing::{
    COMMITMENT_LEN, MAX_FAILED_PAIRING_ATTEMPTS, NONCE_LEN, PAIRING_EXPIRY, PairingAcceptor,
    PairingError, PairingGate, PairingOutcome, PairingRequester, commitment, random_nonce,
    short_authentication_string,
};

/// Version of the relay framing and of the encrypted channel.
///
/// A peer that meets a version it does not know answers with a `version` error
/// and closes. It does not guess: reading a newer framing as an older one
/// means misinterpreting ciphertext as control messages.
pub const RELAY_PROTOCOL_VERSION: u32 = 1;

/// Version of the control and data messages carried inside the channel.
pub const RPC_PROTOCOL_VERSION: u32 = 1;

/// Noise protocol name both ends build their handshake from.
pub const NOISE_PROTOCOL_NAME: &str = "Noise_KK_25519_AESGCM_SHA256";

/// Separator for every field that is concatenated into a hashed or
/// authenticated string. A unit separator, because no user id, device id or
/// label can contain it, so two distinct inputs cannot assemble the same bytes.
pub const FIELD_SEPARATOR: u8 = 0x1F;
