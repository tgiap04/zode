use anyhow::{Context as _, Result};
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use prost::Message as _;
use rpc::proto::Envelope;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct MessageId(pub u32);

pub type MessageLen = u32;
pub const MESSAGE_LEN_SIZE: usize = size_of::<MessageLen>();

/// The longest message that is read. The length comes from the other end of a
/// stream, which may be a device across the relay and not a server this Zode
/// started, so it is not taken on trust.
pub const MAX_MESSAGE_LEN: MessageLen = 256 * 1024 * 1024;

/// The most room made for a message body at a time. A body is read a step at a
/// time, so a length that is a lie costs one step, and no later message pays to
/// clear the room an earlier, longer one left in a buffer that is reused.
const READ_STEP: usize = 1024 * 1024;

pub fn message_len_from_buffer(buffer: &[u8]) -> MessageLen {
    MessageLen::from_le_bytes(buffer.try_into().unwrap())
}

/// Reads the `message_len` bytes of a message into `buffer`, replacing what it
/// held. Refuses a length over [`MAX_MESSAGE_LEN`], and holds only as much as
/// has arrived, give or take a step.
async fn read_body<S: AsyncRead + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
    message_len: MessageLen,
) -> Result<()> {
    anyhow::ensure!(
        message_len <= MAX_MESSAGE_LEN,
        "a message of {message_len} bytes is over the limit of {MAX_MESSAGE_LEN}"
    );
    let expected = message_len as usize;
    buffer.clear();
    let mut filled = 0;
    while filled < expected {
        buffer.resize(filled + (expected - filled).min(READ_STEP), 0);
        let step = buffer
            .get_mut(filled..)
            .context("the buffer is shorter than it was just made")?;
        let step_len = step.len();
        if let Err(error) = stream.read_exact(step).await {
            buffer.truncate(filled);
            return Err(std::io::Error::new(
                error.kind(),
                format!("could not read a message of {expected} bytes: {error}"),
            )
            .into());
        }
        filled += step_len;
    }
    Ok(())
}

pub async fn read_message_with_len<S: AsyncRead + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
    message_len: MessageLen,
) -> Result<Envelope> {
    read_body(stream, buffer, message_len).await?;
    Ok(Envelope::decode(buffer.as_slice())?)
}

pub async fn read_message<S: AsyncRead + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
) -> Result<Envelope> {
    buffer.resize(MESSAGE_LEN_SIZE, 0);
    stream.read_exact(buffer).await?;

    let len = message_len_from_buffer(buffer);

    read_message_with_len(stream, buffer, len).await
}

pub async fn write_message<S: AsyncWrite + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
    message: Envelope,
) -> Result<()> {
    let message_len = message.encoded_len() as u32;
    stream
        .write_all(message_len.to_le_bytes().as_slice())
        .await?;
    buffer.clear();
    buffer.reserve(message_len as usize);
    message.encode(buffer)?;
    stream.write_all(buffer).await?;
    Ok(())
}

pub async fn write_size_prefixed_buffer<S: AsyncWrite + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
) -> Result<()> {
    let len = buffer.len() as u32;
    stream.write_all(len.to_le_bytes().as_slice()).await?;
    stream.write_all(buffer).await?;
    Ok(())
}

pub async fn read_message_raw<S: AsyncRead + Unpin>(
    stream: &mut S,
    buffer: &mut Vec<u8>,
) -> Result<()> {
    buffer.resize(MESSAGE_LEN_SIZE, 0);
    stream.read_exact(buffer).await?;

    let message_len = message_len_from_buffer(buffer);
    read_body(stream, buffer, message_len).await
}

#[cfg(test)]
mod tests {
    use futures::io::Cursor;

    use super::*;

    fn framed(len: MessageLen, body: &[u8]) -> Vec<u8> {
        let mut bytes = len.to_le_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn a_message_is_read_back_whole() {
        let envelope = Envelope {
            id: 7,
            ..Envelope::default()
        };
        let mut wire = Vec::new();
        smol::block_on(write_message(&mut wire, &mut Vec::new(), envelope.clone()))
            .expect("writes");

        let mut buffer = Vec::new();
        let read = smol::block_on(read_message(&mut Cursor::new(wire.clone()), &mut buffer))
            .expect("reads");
        assert_eq!(read, envelope);

        let mut raw = Vec::new();
        smol::block_on(read_message_raw(&mut Cursor::new(wire), &mut raw)).expect("reads");
        assert_eq!(raw, envelope.encode_to_vec());
    }

    #[test]
    fn a_length_over_the_limit_is_refused_without_reserving_memory_for_it() {
        let wire = framed(MAX_MESSAGE_LEN + 1, &[0; 16]);
        let mut buffer = Vec::new();
        let error = smol::block_on(read_message_raw(
            &mut Cursor::new(wire.clone()),
            &mut buffer,
        ))
        .expect_err("over the limit");
        assert!(format!("{error:#}").contains("over the limit"), "{error:#}");
        assert!(buffer.capacity() < 1024 * 1024, "{}", buffer.capacity());

        let error = smol::block_on(read_message(&mut Cursor::new(wire), &mut buffer))
            .expect_err("over the limit");
        assert!(format!("{error:#}").contains("over the limit"), "{error:#}");

        let error = smol::block_on(read_message_with_len(
            &mut Cursor::new(Vec::new()),
            &mut buffer,
            u32::MAX,
        ))
        .expect_err("over the limit");
        assert!(format!("{error:#}").contains("over the limit"), "{error:#}");
    }

    #[test]
    fn a_length_that_promises_more_than_arrives_costs_at_most_one_step() {
        let wire = framed(MAX_MESSAGE_LEN, &[1; 10]);
        let mut buffer = Vec::new();
        let error = smol::block_on(read_message_raw(&mut Cursor::new(wire), &mut buffer))
            .expect_err("the stream ends first");
        let ended = error
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind);
        assert_eq!(ended, Some(std::io::ErrorKind::UnexpectedEof));
        assert!(
            buffer.capacity() <= READ_STEP,
            "a lie costs one step: {}",
            buffer.capacity()
        );
    }

    /// Remembers the most room it was offered in one read, which is what
    /// had to be cleared for it.
    struct Recording {
        inner: Cursor<Vec<u8>>,
        most_offered: usize,
    }

    impl AsyncRead for Recording {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buffer: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            self.most_offered = self.most_offered.max(buffer.len());
            std::pin::Pin::new(&mut self.inner).poll_read(cx, buffer)
        }
    }

    #[test]
    fn a_short_message_after_a_long_one_does_not_pay_for_the_room_the_long_one_left() {
        let mut buffer = Vec::new();
        let long_body = vec![7u8; 5 * 1024 * 1024];
        let mut stream = Recording {
            inner: Cursor::new(long_body.clone()),
            most_offered: 0,
        };
        smol::block_on(read_body(
            &mut stream,
            &mut buffer,
            long_body.len() as MessageLen,
        ))
        .expect("reads");
        assert_eq!(buffer, long_body);
        assert!(
            stream.most_offered <= READ_STEP,
            "{} bytes offered at once",
            stream.most_offered
        );
        assert!(buffer.capacity() >= long_body.len(), "the room is kept");

        let mut stream = Recording {
            inner: Cursor::new(b"short".to_vec()),
            most_offered: 0,
        };
        smol::block_on(read_body(&mut stream, &mut buffer, 5)).expect("reads");
        assert_eq!(buffer, b"short");
        assert!(
            stream.most_offered <= 5,
            "a 5 byte message was offered {} bytes of room",
            stream.most_offered
        );
    }

    #[test]
    fn a_message_of_exactly_one_step_and_one_past_it_are_read_whole() {
        for length in [READ_STEP - 1, READ_STEP, READ_STEP + 1, 2 * READ_STEP] {
            let body: Vec<u8> = (0..length).map(|index| (index % 251) as u8).collect();
            let mut buffer = vec![9u8; 10];
            smol::block_on(read_body(
                &mut Cursor::new(body.clone()),
                &mut buffer,
                length as MessageLen,
            ))
            .expect("reads");
            assert_eq!(buffer, body, "{length} bytes");
        }
    }

    #[test]
    fn a_message_at_the_limit_is_not_refused_for_its_length() {
        let wire = framed(MAX_MESSAGE_LEN, &[]);
        let error = smol::block_on(read_message_raw(&mut Cursor::new(wire), &mut Vec::new()))
            .expect_err("the stream ends first");
        assert!(
            !format!("{error:#}").contains("over the limit"),
            "{error:#}"
        );
    }
}
