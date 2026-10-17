/// The largest message Noise permits, ciphertext and tag included.
pub const MAX_NOISE_MESSAGE_LEN: usize = 65_535;

/// Plaintext carried by one Noise message, inner header included.
///
/// Held well under [`MAX_NOISE_MESSAGE_LEN`] so the tag and any future header
/// growth never push a frame over the limit.
pub const MAX_INNER_FRAME_LEN: usize = 60 * 1024;

/// `kind (1) | stream_id (4)`.
pub const INNER_HEADER_LEN: usize = 5;

pub const MAX_INNER_PAYLOAD_LEN: usize = MAX_INNER_FRAME_LEN - INNER_HEADER_LEN;

/// `session_id (4)`.
pub const RELAY_HEADER_LEN: usize = 4;

pub const MAX_RELAY_PAYLOAD_LEN: usize = MAX_NOISE_MESSAGE_LEN;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame is {actual} bytes, shorter than its {needed}-byte header")]
    Truncated { actual: usize, needed: usize },
    #[error("frame carries no payload")]
    EmptyPayload,
    #[error("payload is {actual} bytes, over the {limit}-byte limit")]
    TooLarge { actual: usize, limit: usize },
    #[error("unknown inner frame kind {0}")]
    UnknownKind(u8),
    #[error("{0}")]
    InvalidStream(&'static str),
}

/// What the relay server sees on a binary WebSocket frame: which session the
/// bytes belong to, then bytes it cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayFrame {
    pub session_id: u32,
    pub payload: Vec<u8>,
}

pub fn encode_relay_frame(session_id: u32, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.is_empty() {
        return Err(FrameError::EmptyPayload);
    }
    if payload.len() > MAX_RELAY_PAYLOAD_LEN {
        return Err(FrameError::TooLarge {
            actual: payload.len(),
            limit: MAX_RELAY_PAYLOAD_LEN,
        });
    }
    let mut bytes = Vec::with_capacity(RELAY_HEADER_LEN + payload.len());
    bytes.extend_from_slice(&session_id.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

pub fn decode_relay_frame(bytes: &[u8]) -> Result<RelayFrame, FrameError> {
    let Some((header, payload)) = bytes.split_first_chunk::<RELAY_HEADER_LEN>() else {
        return Err(FrameError::Truncated {
            actual: bytes.len(),
            needed: RELAY_HEADER_LEN,
        });
    };
    if payload.is_empty() {
        return Err(FrameError::EmptyPayload);
    }
    if payload.len() > MAX_RELAY_PAYLOAD_LEN {
        return Err(FrameError::TooLarge {
            actual: payload.len(),
            limit: MAX_RELAY_PAYLOAD_LEN,
        });
    }
    Ok(RelayFrame {
        session_id: u32::from_be_bytes(*header),
        payload: payload.to_vec(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InnerKind {
    Control = 0,
    Data = 1,
}

impl InnerKind {
    fn from_byte(byte: u8) -> Result<Self, FrameError> {
        match byte {
            0 => Ok(Self::Control),
            1 => Ok(Self::Data),
            other => Err(FrameError::UnknownKind(other)),
        }
    }
}

/// A frame inside the encrypted channel.
///
/// Control frames always use stream 0 and carry one JSON message. Data frames
/// use a non-zero stream id chosen by the side that opens the stream, and an
/// empty data payload marks the end of that stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerFrame {
    pub kind: InnerKind,
    pub stream_id: u32,
    pub payload: Vec<u8>,
}

impl InnerFrame {
    pub fn control(payload: Vec<u8>) -> Result<Self, FrameError> {
        Self::validated(InnerKind::Control, 0, payload)
    }

    pub fn data(stream_id: u32, payload: Vec<u8>) -> Result<Self, FrameError> {
        Self::validated(InnerKind::Data, stream_id, payload)
    }

    pub fn end_of_stream(stream_id: u32) -> Result<Self, FrameError> {
        Self::validated(InnerKind::Data, stream_id, Vec::new())
    }

    fn validated(kind: InnerKind, stream_id: u32, payload: Vec<u8>) -> Result<Self, FrameError> {
        match kind {
            InnerKind::Control if stream_id != 0 => {
                return Err(FrameError::InvalidStream("control frames use stream 0"));
            }
            InnerKind::Control if payload.is_empty() => return Err(FrameError::EmptyPayload),
            InnerKind::Data if stream_id == 0 => {
                return Err(FrameError::InvalidStream(
                    "stream 0 is reserved for control frames",
                ));
            }
            _ => {}
        }
        if payload.len() > MAX_INNER_PAYLOAD_LEN {
            return Err(FrameError::TooLarge {
                actual: payload.len(),
                limit: MAX_INNER_PAYLOAD_LEN,
            });
        }
        Ok(Self {
            kind,
            stream_id,
            payload,
        })
    }
}

pub fn encode_inner_frame(frame: &InnerFrame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(INNER_HEADER_LEN + frame.payload.len());
    bytes.push(frame.kind as u8);
    bytes.extend_from_slice(&frame.stream_id.to_be_bytes());
    bytes.extend_from_slice(&frame.payload);
    bytes
}

pub fn decode_inner_frame(bytes: &[u8]) -> Result<InnerFrame, FrameError> {
    let Some((&kind, rest)) = bytes.split_first() else {
        return Err(FrameError::Truncated {
            actual: 0,
            needed: INNER_HEADER_LEN,
        });
    };
    let Some((stream_id, payload)) = rest.split_first_chunk::<4>() else {
        return Err(FrameError::Truncated {
            actual: bytes.len(),
            needed: INNER_HEADER_LEN,
        });
    };
    InnerFrame::validated(
        InnerKind::from_byte(kind)?,
        u32::from_be_bytes(*stream_id),
        payload.to_vec(),
    )
}

/// Cuts a stream's bytes into data frames that each fit one Noise message.
///
/// Empty input yields no frames: an empty data frame means end of stream, so
/// producing one here would end a stream the caller had not finished.
pub fn split_data(stream_id: u32, data: &[u8]) -> Result<Vec<InnerFrame>, FrameError> {
    data.chunks(MAX_INNER_PAYLOAD_LEN)
        .map(|chunk| InnerFrame::data(stream_id, chunk.to_vec()))
        .collect()
}
