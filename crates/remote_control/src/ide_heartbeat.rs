//! Telling the project server's heartbeat from a request on its stream.
//!
//! A controlling Zode pings its project server every few seconds, and the ping
//! crosses the relay as a few bytes on the project stream whether or not anyone
//! is at the keyboard. Counting every byte as use would make the idle limit
//! unreachable for as long as a project is open, so the host reads just enough
//! of the stream to see which messages are pings. It never acts on what it
//! reads: the bytes go on to the project server untouched.

use rpc::proto::{Envelope, Message as _, envelope::Payload};

/// Bytes in the length that precedes every message, little-endian.
const LENGTH_PREFIX_LEN: usize = 4;

/// A ping is a handful of bytes. A message longer than this is a request
/// without being decoded, which also bounds what is held to read a message.
const MAX_HEARTBEAT_LEN: usize = 64;

enum Reading {
    /// The length of the next message, as far as it has arrived.
    Length {
        prefix: [u8; LENGTH_PREFIX_LEN],
        filled: usize,
    },
    /// The bytes of a message still to come. `held` is how many have been kept
    /// in the detector's buffer: only a message short enough to be a ping is
    /// kept, and a longer one has `None`.
    Message {
        remaining: usize,
        held: Option<usize>,
    },
}

impl Reading {
    const START: Self = Self::Length {
        prefix: [0; LENGTH_PREFIX_LEN],
        filled: 0,
    };
}

/// Follows the device's side of one project stream.
pub(crate) struct RequestDetector {
    reading: Reading,
    /// The bytes of the short message being read, so none is allocated per
    /// message.
    short_message: [u8; MAX_HEARTBEAT_LEN],
}

impl RequestDetector {
    pub(crate) fn new() -> Self {
        Self {
            reading: Reading::START,
            short_message: [0; MAX_HEARTBEAT_LEN],
        }
    }

    /// Takes the next bytes the device sent on the stream. `true` if they hold
    /// any of a message that is not a ping, however far into a long message
    /// they fall: somebody sending a large file is at the other end for as long
    /// as it takes. Bytes need not fall on message boundaries.
    pub(crate) fn sees_a_request(&mut self, bytes: &[u8]) -> bool {
        let mut request = false;
        let mut rest = bytes;
        while !rest.is_empty() {
            match &mut self.reading {
                Reading::Length { prefix, filled } => {
                    let (head, tail) = rest.split_at((LENGTH_PREFIX_LEN - *filled).min(rest.len()));
                    rest = tail;
                    for byte in head {
                        if let Some(slot) = prefix.get_mut(*filled) {
                            *slot = *byte;
                        }
                        *filled += 1;
                    }
                    if *filled < LENGTH_PREFIX_LEN {
                        continue;
                    }
                    let length = usize::try_from(u32::from_le_bytes(*prefix)).unwrap_or(usize::MAX);
                    self.reading = if length == 0 {
                        // A message with nothing in it has nothing to read, and
                        // none to wait for.
                        Reading::START
                    } else {
                        Reading::Message {
                            remaining: length,
                            held: (length <= MAX_HEARTBEAT_LEN).then_some(0),
                        }
                    };
                }
                Reading::Message { remaining, held } => {
                    let (head, tail) = rest.split_at((*remaining).min(rest.len()));
                    rest = tail;
                    *remaining -= head.len();
                    let Some(kept) = held else {
                        // Too long to be a ping, from its first byte to its last.
                        request = true;
                        if *remaining == 0 {
                            self.reading = Reading::START;
                        }
                        continue;
                    };
                    if let Some(slot) = self.short_message.get_mut(*kept..*kept + head.len()) {
                        slot.copy_from_slice(head);
                    }
                    *kept += head.len();
                    if *remaining == 0 {
                        let message = self.short_message.get(..*kept).unwrap_or_default();
                        request |= !is_ping(message);
                        self.reading = Reading::START;
                    }
                }
            }
        }
        request
    }
}

fn is_ping(message: &[u8]) -> bool {
    Envelope::decode(message)
        .is_ok_and(|envelope| matches!(envelope.payload, Some(Payload::Ping(_))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rpc::proto::{Ack, Ping};

    fn framed(envelope: &Envelope) -> Vec<u8> {
        let mut bytes = u32::try_from(envelope.encoded_len())
            .expect("a short message")
            .to_le_bytes()
            .to_vec();
        envelope.encode(&mut bytes).expect("encodes");
        bytes
    }

    fn ping(id: u32) -> Vec<u8> {
        framed(&Envelope {
            id,
            ack_id: Some(id.saturating_sub(1)),
            payload: Some(Payload::Ping(Ping {})),
            ..Default::default()
        })
    }

    fn request(id: u32) -> Vec<u8> {
        framed(&Envelope {
            id,
            responding_to: Some(1),
            payload: Some(Payload::Ack(Ack {})),
            ..Default::default()
        })
    }

    fn large_request() -> Vec<u8> {
        framed(&Envelope {
            id: 9,
            payload: Some(Payload::Error(rpc::proto::Error {
                message: "x".repeat(4096),
                ..Default::default()
            })),
            ..Default::default()
        })
    }

    #[test]
    fn a_ping_is_not_a_request() {
        let mut detector = RequestDetector::new();
        for id in [1, 300, 70_000, u32::MAX] {
            assert!(!detector.sees_a_request(&ping(id)), "ping {id}");
        }
    }

    #[test]
    fn any_other_message_is_a_request() {
        let mut detector = RequestDetector::new();
        assert!(detector.sees_a_request(&request(2)));
        assert!(detector.sees_a_request(&large_request()));
    }

    #[test]
    fn a_message_that_does_not_decode_is_a_request() {
        let mut detector = RequestDetector::new();
        let mut garbage = 3u32.to_le_bytes().to_vec();
        garbage.extend_from_slice(&[0xff, 0xff, 0xff]);
        assert!(detector.sees_a_request(&garbage));
    }

    #[test]
    fn a_message_cut_into_pieces_is_judged_whole() {
        let mut detector = RequestDetector::new();
        for byte in ping(5) {
            assert!(!detector.sees_a_request(&[byte]));
        }
        let pieces: Vec<bool> = request(6)
            .into_iter()
            .map(|byte| detector.sees_a_request(&[byte]))
            .collect();
        assert_eq!(
            pieces.iter().filter(|seen| **seen).count(),
            1,
            "seen once, when the message is complete: {pieces:?}"
        );
    }

    #[test]
    fn pings_around_a_long_message_stay_pings() {
        let mut detector = RequestDetector::new();
        let mut stream = ping(1);
        stream.extend(large_request());
        stream.extend(ping(2));
        assert!(
            detector.sees_a_request(&stream),
            "the long message is a request"
        );
        assert!(
            !detector.sees_a_request(&ping(3)),
            "and the framing is still in step afterwards"
        );
    }

    #[test]
    fn every_piece_of_a_long_message_is_a_request() {
        let mut detector = RequestDetector::new();
        let message = large_request();
        for piece in message.chunks(500) {
            assert!(detector.sees_a_request(piece));
        }
        assert!(
            !detector.sees_a_request(&ping(4)),
            "and then pings are pings again"
        );
    }

    #[test]
    fn a_long_message_is_a_request_before_its_end_arrives() {
        let mut detector = RequestDetector::new();
        let message = large_request();
        let (first, rest) = message.split_at(LENGTH_PREFIX_LEN + 10);
        assert!(detector.sees_a_request(first));
        assert!(
            detector.sees_a_request(rest),
            "the rest of a long message is somebody still sending it"
        );
        assert!(!detector.sees_a_request(&ping(4)));
    }

    #[test]
    fn several_pings_in_one_piece_are_still_only_pings() {
        let mut detector = RequestDetector::new();
        let mut stream = Vec::new();
        for id in 1..20 {
            stream.extend(ping(id));
        }
        assert!(!detector.sees_a_request(&stream));
    }

    #[test]
    fn a_message_with_nothing_in_it_is_not_a_request() {
        let mut detector = RequestDetector::new();
        let mut stream = 0u32.to_le_bytes().to_vec();
        stream.extend(ping(1));
        assert!(!detector.sees_a_request(&stream));
    }
}
