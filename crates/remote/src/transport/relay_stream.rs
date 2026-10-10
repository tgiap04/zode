//! The bytes of the project-server stream, cut back into messages.
//!
//! A stream carries the same length-prefixed envelopes a child process's
//! stdio does, but relay frames end wherever the encrypted channel's frame
//! limit falls, so a message may arrive in several pieces or share a frame
//! with the next.

use anyhow::{Result, anyhow};
use prost::Message as _;
use rpc::proto::Envelope;

use crate::protocol::{MESSAGE_LEN_SIZE, message_len_from_buffer, write_message};

/// The largest message accepted from the other Zode. Far above anything the
/// project protocol sends, and what stops a peer from making this one buffer
/// without end.
pub(crate) const MAX_MESSAGE_LEN: usize = 256 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct MessageDecoder {
    buffer: Vec<u8>,
}

impl MessageDecoder {
    /// Adds `bytes` and returns every message that is now complete.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Envelope>> {
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        let mut consumed = 0;
        loop {
            let rest = &self.buffer[consumed..];
            let Some((length_bytes, body)) = rest.split_first_chunk::<MESSAGE_LEN_SIZE>() else {
                break;
            };
            let length = message_len_from_buffer(length_bytes) as usize;
            if length > MAX_MESSAGE_LEN {
                return Err(anyhow!(
                    "the other Zode sent a message of {length} bytes, over the {MAX_MESSAGE_LEN} limit"
                ));
            }
            let Some(message) = body.get(..length) else {
                break;
            };
            messages.push(Envelope::decode(message)?);
            consumed += MESSAGE_LEN_SIZE + length;
        }
        self.buffer.drain(..consumed);
        Ok(messages)
    }
}

pub(crate) async fn encode_message(envelope: Envelope) -> Result<Vec<u8>> {
    let mut encoded = Vec::with_capacity(MESSAGE_LEN_SIZE + envelope.encoded_len());
    let mut scratch = Vec::new();
    write_message(&mut encoded, &mut scratch, envelope).await?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(id: u32) -> Envelope {
        Envelope {
            id,
            ..Default::default()
        }
    }

    #[test]
    fn messages_split_across_frames_are_reassembled() {
        let first = smol::block_on(encode_message(envelope(1))).unwrap();
        let second = smol::block_on(encode_message(envelope(2))).unwrap();
        let mut all = first.clone();
        all.extend_from_slice(&second);

        let mut decoder = MessageDecoder::default();
        let (head, tail) = all.split_at(first.len() + 2);
        let mut ids: Vec<u32> = decoder
            .push(head)
            .unwrap()
            .iter()
            .map(|message| message.id)
            .collect();
        assert_eq!(ids, vec![1], "the second message is still incomplete");
        ids.extend(decoder.push(tail).unwrap().iter().map(|message| message.id));
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn one_byte_at_a_time_yields_every_message_once() {
        let bytes = smol::block_on(encode_message(envelope(7))).unwrap();
        let mut decoder = MessageDecoder::default();
        let mut seen = Vec::new();
        for byte in bytes {
            seen.extend(decoder.push(&[byte]).unwrap());
        }
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].id, 7);
    }

    #[test]
    fn a_length_over_the_limit_is_refused_before_anything_is_buffered_for_it() {
        let mut decoder = MessageDecoder::default();
        let huge = (MAX_MESSAGE_LEN as u32 + 1).to_le_bytes();
        assert!(decoder.push(&huge).is_err());
    }

    #[test]
    fn garbage_that_is_not_an_envelope_is_an_error() {
        let mut decoder = MessageDecoder::default();
        let mut bytes = 3u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0xff, 0xff, 0xff]);
        assert!(decoder.push(&bytes).is_err());
    }
}
