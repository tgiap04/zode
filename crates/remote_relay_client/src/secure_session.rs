//! The responder's side of the encrypted channel: which handshakes it is
//! willing to be in the middle of, and how an established session turns
//! control messages and stream bytes into sealed frames and back.
//!
//! No clock and no sockets: callers pass the time in, so the 10-second
//! handshake limit can be asserted without waiting for it.

use std::{collections::HashMap, time::Instant};

use remote_relay_protocol::{
    Control, FrameError, HANDSHAKE_TIMEOUT, Handshake, HandshakeParameters, InnerFrame, KEY_LEN,
    MAX_HALF_OPEN_HANDSHAKES, MessageError, NoiseError, Session, decode_control, encode_control,
};

#[derive(Debug, thiserror::Error)]
pub enum HandshakeRefusal {
    /// A relay can open sessions on a device's behalf for free, so only so many
    /// may be waiting at once.
    #[error("too many handshakes are already waiting")]
    TooManyOpen,
    #[error("no handshake is expected on this session")]
    NotExpected,
    #[error("the handshake took too long")]
    Expired,
    #[error(transparent)]
    Noise(#[from] NoiseError),
}

struct Awaited {
    remote_public_key: [u8; KEY_LEN],
    prologue: Vec<u8>,
    deadline: Instant,
}

/// Sessions the relay has opened that have not yet sent their first
/// handshake message.
///
/// The handshake itself is built only when that message arrives, so a waiting
/// session costs a key and a prologue, not a cipher state.
#[derive(Default)]
pub struct ResponderHandshakes {
    awaited: HashMap<u32, Awaited>,
}

impl ResponderHandshakes {
    pub fn len(&self) -> usize {
        self.awaited.len()
    }

    pub fn is_empty(&self) -> bool {
        self.awaited.is_empty()
    }

    pub fn is_expecting(&self, session_id: u32) -> bool {
        self.awaited.contains_key(&session_id)
    }

    /// Starts waiting for `session_id`'s first message, from the one device
    /// whose pinned key is `remote_public_key`.
    pub fn expect(
        &mut self,
        session_id: u32,
        remote_public_key: [u8; KEY_LEN],
        prologue: Vec<u8>,
        now: Instant,
    ) -> Result<(), HandshakeRefusal> {
        self.awaited.retain(|_, awaited| now < awaited.deadline);
        if !self.awaited.contains_key(&session_id) && self.awaited.len() >= MAX_HALF_OPEN_HANDSHAKES
        {
            return Err(HandshakeRefusal::TooManyOpen);
        }
        // A relay that announces the same session again must not get a fresh
        // 10 seconds out of it, or it could keep a slot occupied for good.
        let deadline = self
            .awaited
            .get(&session_id)
            .map_or(now + HANDSHAKE_TIMEOUT, |existing| existing.deadline);
        self.awaited.insert(
            session_id,
            Awaited {
                remote_public_key,
                prologue,
                deadline,
            },
        );
        Ok(())
    }

    /// Runs the handshake on its first message and returns the session it
    /// yields with the reply to send.
    ///
    /// A handshake is one try: it leaves the table whatever happens, so a
    /// message that fails to authenticate ends it rather than being retried.
    /// The returned session is not confirmed; see [`Session::is_confirmed`].
    pub fn accept(
        &mut self,
        session_id: u32,
        local_private_key: &[u8; KEY_LEN],
        message: &[u8],
        now: Instant,
    ) -> Result<(Session, Vec<u8>), HandshakeRefusal> {
        let awaited = self
            .awaited
            .remove(&session_id)
            .ok_or(HandshakeRefusal::NotExpected)?;
        if now >= awaited.deadline {
            return Err(HandshakeRefusal::Expired);
        }
        let mut handshake = Handshake::responder(&HandshakeParameters {
            local_private_key,
            remote_public_key: &awaited.remote_public_key,
            prologue: &awaited.prologue,
        })?;
        handshake.read_message(message)?;
        let reply = handshake.write_message(&[])?;
        Ok((handshake.into_session()?, reply))
    }

    /// Drops handshakes past their deadline and returns their session ids, so
    /// the caller can close them at the relay.
    pub fn expire(&mut self, now: Instant) -> Vec<u32> {
        let expired: Vec<u32> = self
            .awaited
            .iter()
            .filter(|(_, awaited)| now >= awaited.deadline)
            .map(|(session_id, _)| *session_id)
            .collect();
        for session_id in &expired {
            self.awaited.remove(session_id);
        }
        expired
    }

    pub fn cancel(&mut self, session_id: u32) {
        self.awaited.remove(&session_id);
    }

    pub fn clear(&mut self) {
        self.awaited.clear();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error(transparent)]
    Noise(#[from] NoiseError),
    #[error(transparent)]
    Message(#[from] MessageError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// The peer has not yet proven it holds the session keys, so nothing may
    /// be sent to it: a replayed first handshake message would otherwise be
    /// answered with live data.
    #[error("the session is not confirmed yet")]
    NotConfirmed,
}

/// An established session, speaking in control messages and streams.
pub struct SecureChannel {
    session: Session,
}

impl SecureChannel {
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// Whether the peer has proven it holds the session keys. Nothing may be
    /// acted on before this is true.
    pub fn is_confirmed(&self) -> bool {
        self.session.is_confirmed()
    }

    /// A session that failed to authenticate a message is closed for good.
    pub fn is_closed(&self) -> bool {
        self.session.is_closed()
    }

    pub fn open(&mut self, ciphertext: &[u8]) -> Result<InnerFrame, NoiseError> {
        self.session.decrypt_frame(ciphertext)
    }

    fn require_confirmed(&self) -> Result<(), ChannelError> {
        if self.session.is_confirmed() {
            Ok(())
        } else {
            Err(ChannelError::NotConfirmed)
        }
    }

    pub fn seal_control(&mut self, control: &Control) -> Result<Vec<u8>, ChannelError> {
        self.require_confirmed()?;
        let frame = InnerFrame::control(encode_control(control)?)?;
        Ok(self.session.encrypt_frame(&frame)?)
    }

    /// Seals one data frame. `data` must already fit one frame.
    pub fn seal_data(&mut self, stream_id: u32, data: Vec<u8>) -> Result<Vec<u8>, ChannelError> {
        self.require_confirmed()?;
        let frame = InnerFrame::data(stream_id, data)?;
        Ok(self.session.encrypt_frame(&frame)?)
    }

    /// The empty data frame that ends a stream.
    pub fn seal_end_of_stream(&mut self, stream_id: u32) -> Result<Vec<u8>, ChannelError> {
        self.require_confirmed()?;
        let frame = InnerFrame::end_of_stream(stream_id)?;
        Ok(self.session.encrypt_frame(&frame)?)
    }
}

/// Reads a control frame's payload. Separate from [`SecureChannel::open`]
/// because only a frame of the control kind carries one.
pub fn read_control(frame: &InnerFrame) -> Result<Control, MessageError> {
    decode_control(&frame.payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_relay_protocol::{DeviceKeypair, InnerKind, MAX_INNER_PAYLOAD_LEN, build_prologue};
    use std::time::Duration;

    struct Pair {
        initiator: DeviceKeypair,
        responder: DeviceKeypair,
        prologue: Vec<u8>,
    }

    fn pair() -> Pair {
        Pair {
            initiator: DeviceKeypair::generate().unwrap(),
            responder: DeviceKeypair::generate().unwrap(),
            prologue: build_prologue("user", "browser", "desktop").unwrap(),
        }
    }

    fn initiator_message(pair: &Pair) -> (Handshake, Vec<u8>) {
        let mut handshake = Handshake::initiator(&HandshakeParameters {
            local_private_key: pair.initiator.private_key(),
            remote_public_key: pair.responder.public_key(),
            prologue: &pair.prologue,
        })
        .unwrap();
        let message = handshake.write_message(&[]).unwrap();
        (handshake, message)
    }

    #[test]
    fn a_handshake_yields_two_sessions_that_talk_but_the_responder_waits_for_proof() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), pair.prologue.clone(), now)
            .unwrap();

        let (mut initiator, first) = initiator_message(&pair);
        let (session, reply) = table
            .accept(1, pair.responder.private_key(), &first, now)
            .unwrap();
        assert!(table.is_empty(), "a handshake is one try");

        initiator.read_message(&reply).unwrap();
        let mut initiator = SecureChannel::new(initiator.into_session().unwrap());
        let mut responder = SecureChannel::new(session);
        assert!(initiator.is_confirmed());
        assert!(
            !responder.is_confirmed(),
            "message 1 alone proves nothing: it can be replayed"
        );

        let hello = initiator
            .seal_control(&Control::Hello {
                relay_protocol: 1,
                app_version: "0.1.5".into(),
                rpc_protocol: 1,
                capabilities: vec!["terminal".into()],
            })
            .unwrap();
        let frame = responder.open(&hello).unwrap();
        assert!(responder.is_confirmed());
        assert_eq!(frame.kind, InnerKind::Control);
        assert!(matches!(
            read_control(&frame).unwrap(),
            Control::Hello {
                rpc_protocol: 1,
                ..
            }
        ));

        let pong = responder.seal_control(&Control::Pong).unwrap();
        assert_eq!(
            read_control(&initiator.open(&pong).unwrap()).unwrap(),
            Control::Pong
        );
    }

    #[test]
    fn data_and_end_of_stream_frames_round_trip() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), pair.prologue.clone(), now)
            .unwrap();
        let (mut initiator, first) = initiator_message(&pair);
        let (session, reply) = table
            .accept(1, pair.responder.private_key(), &first, now)
            .unwrap();
        initiator.read_message(&reply).unwrap();
        let mut initiator = SecureChannel::new(initiator.into_session().unwrap());
        let mut responder = SecureChannel::new(session);
        let ping = initiator.seal_control(&Control::Ping).unwrap();
        responder.open(&ping).unwrap();

        let sealed = responder.seal_data(9, b"output".to_vec()).unwrap();
        let frame = initiator.open(&sealed).unwrap();
        assert_eq!((frame.kind, frame.stream_id), (InnerKind::Data, 9));
        assert_eq!(frame.payload, b"output");

        let end = responder.seal_end_of_stream(9).unwrap();
        assert!(initiator.open(&end).unwrap().payload.is_empty());

        assert!(
            matches!(
                responder.seal_data(9, vec![0; MAX_INNER_PAYLOAD_LEN + 1]),
                Err(ChannelError::Frame(_))
            ),
            "a frame over the limit is refused, not truncated"
        );
    }

    #[test]
    fn nothing_is_sealed_for_a_peer_that_has_not_proven_itself() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), pair.prologue.clone(), now)
            .unwrap();
        let (_, first) = initiator_message(&pair);
        let (session, _) = table
            .accept(1, pair.responder.private_key(), &first, now)
            .unwrap();
        let mut responder = SecureChannel::new(session);

        assert!(matches!(
            responder.seal_control(&Control::Pong),
            Err(ChannelError::NotConfirmed)
        ));
        assert!(matches!(
            responder.seal_data(1, b"output".to_vec()),
            Err(ChannelError::NotConfirmed)
        ));
        assert!(matches!(
            responder.seal_end_of_stream(1),
            Err(ChannelError::NotConfirmed)
        ));
    }

    #[test]
    fn announcing_a_session_again_does_not_extend_its_deadline() {
        let pair = pair();
        let start = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), vec![1], start)
            .unwrap();
        table
            .expect(
                1,
                *pair.initiator.public_key(),
                vec![1],
                start + Duration::from_secs(8),
            )
            .unwrap();
        assert_eq!(table.expire(start + HANDSHAKE_TIMEOUT), vec![1]);
    }

    #[test]
    fn a_message_from_the_wrong_device_is_refused_and_ends_the_handshake() {
        let pair = pair();
        let stranger = DeviceKeypair::generate().unwrap();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), pair.prologue.clone(), now)
            .unwrap();

        let mut handshake = Handshake::initiator(&HandshakeParameters {
            local_private_key: stranger.private_key(),
            remote_public_key: pair.responder.public_key(),
            prologue: &pair.prologue,
        })
        .unwrap();
        let message = handshake.write_message(&[]).unwrap();
        assert!(matches!(
            table.accept(1, pair.responder.private_key(), &message, now),
            Err(HandshakeRefusal::Noise(_))
        ));
        assert!(!table.is_expecting(1), "no second try on the same session");
    }

    #[test]
    fn a_handshake_for_another_account_or_device_pair_does_not_open() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(
                1,
                *pair.initiator.public_key(),
                build_prologue("someone-else", "browser", "desktop").unwrap(),
                now,
            )
            .unwrap();
        let (_, message) = initiator_message(&pair);
        assert!(
            table
                .accept(1, pair.responder.private_key(), &message, now)
                .is_err()
        );
    }

    #[test]
    fn at_most_eight_handshakes_wait_at_once() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        for session_id in 0..MAX_HALF_OPEN_HANDSHAKES as u32 {
            table
                .expect(session_id, *pair.initiator.public_key(), vec![1], now)
                .unwrap();
        }
        assert!(matches!(
            table.expect(100, *pair.initiator.public_key(), vec![1], now),
            Err(HandshakeRefusal::TooManyOpen)
        ));
        // Starting over on a session already waiting is not an extra one.
        table
            .expect(0, *pair.initiator.public_key(), vec![1], now)
            .unwrap();
        assert_eq!(table.len(), MAX_HALF_OPEN_HANDSHAKES);

        table.cancel(3);
        table
            .expect(100, *pair.initiator.public_key(), vec![1], now)
            .unwrap();
    }

    #[test]
    fn a_handshake_not_finished_in_ten_seconds_is_dropped() {
        let pair = pair();
        let start = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(
                1,
                *pair.initiator.public_key(),
                pair.prologue.clone(),
                start,
            )
            .unwrap();
        table
            .expect(
                2,
                *pair.initiator.public_key(),
                pair.prologue.clone(),
                start + Duration::from_secs(6),
            )
            .unwrap();

        let expired = table.expire(start + HANDSHAKE_TIMEOUT);
        assert_eq!(expired, vec![1]);
        assert!(table.is_expecting(2));

        let (_, message) = initiator_message(&pair);
        assert!(matches!(
            table.accept(
                2,
                pair.responder.private_key(),
                &message,
                start + Duration::from_secs(6) + HANDSHAKE_TIMEOUT
            ),
            Err(HandshakeRefusal::Expired)
        ));
    }

    #[test]
    fn expired_entries_make_room_for_new_ones() {
        let pair = pair();
        let start = Instant::now();
        let mut table = ResponderHandshakes::default();
        for session_id in 0..MAX_HALF_OPEN_HANDSHAKES as u32 {
            table
                .expect(session_id, *pair.initiator.public_key(), vec![1], start)
                .unwrap();
        }
        table
            .expect(
                99,
                *pair.initiator.public_key(),
                vec![1],
                start + HANDSHAKE_TIMEOUT,
            )
            .unwrap();
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn a_session_nobody_expected_is_not_accepted() {
        let pair = pair();
        let mut table = ResponderHandshakes::default();
        let (_, message) = initiator_message(&pair);
        assert!(matches!(
            table.accept(7, pair.responder.private_key(), &message, Instant::now()),
            Err(HandshakeRefusal::NotExpected)
        ));
    }

    #[test]
    fn a_tampered_message_closes_the_channel_for_good() {
        let pair = pair();
        let now = Instant::now();
        let mut table = ResponderHandshakes::default();
        table
            .expect(1, *pair.initiator.public_key(), pair.prologue.clone(), now)
            .unwrap();
        let (mut initiator, first) = initiator_message(&pair);
        let (session, reply) = table
            .accept(1, pair.responder.private_key(), &first, now)
            .unwrap();
        initiator.read_message(&reply).unwrap();
        let mut initiator = SecureChannel::new(initiator.into_session().unwrap());
        let mut responder = SecureChannel::new(session);

        let mut sealed = initiator.seal_control(&Control::Ping).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(responder.open(&sealed).is_err());
        assert!(responder.is_closed());
        assert!(!responder.is_confirmed(), "a forged frame confirms nothing");
    }
}
