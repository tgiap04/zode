//! The relay's own text frames: the JSON the relay itself reads and writes,
//! as opposed to the encrypted channel it carries between two devices.
//!
//! Only the shapes this side of the connection meets are modelled. A tag
//! nobody here knows is ignored rather than refused, so the relay can grow a
//! message without breaking a build that predates it.

use remote_relay_protocol::PairingMessage;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenMode {
    /// Two devices exchanging public keys and comparing a short string.
    Pair,
    /// An already-paired device opening an encrypted session.
    Session,
}

impl OpenMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pair => "pair",
            Self::Session => "session",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("relay message is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("relay pairing message names no usable session id")]
    MissingSession,
}

/// A text frame from the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayText {
    /// Sent once on connect.
    Hello {
        relay: u32,
        role: String,
    },
    Presence {
        hosts: Vec<String>,
    },
    /// A session the relay set up between this device and `peer`.
    Opened {
        sid: u32,
        peer: String,
        peer_kind: String,
        mode: OpenMode,
    },
    Close {
        sid: u32,
        reason: String,
    },
    /// One of the account's devices was revoked; sent to hosts so they can
    /// drop what they pinned for it.
    DeviceRevoked {
        device_id: String,
    },
    Error {
        code: String,
    },
    /// A pairing message the other side of a session sent, unread by the relay.
    Pairing {
        sid: u32,
        message: PairingMessage,
    },
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Incoming {
    Hello {
        relay: u32,
        role: String,
    },
    Presence {
        hosts: Vec<String>,
    },
    Opened {
        sid: u32,
        peer: String,
        #[serde(rename = "peerKind", default)]
        peer_kind: String,
        mode: OpenMode,
    },
    Close {
        sid: u32,
        #[serde(default)]
        reason: String,
    },
    DeviceRevoked {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    Error {
        code: String,
    },
    #[serde(other)]
    Unknown,
}

/// Reads one text frame. `Ok(None)` is a tag this build does not know.
pub fn parse_relay_text(raw: &str) -> Result<Option<RelayText>, WireError> {
    let value: Value = serde_json::from_str(raw)?;
    let tag = value.get("t").and_then(Value::as_str).unwrap_or_default();
    if tag.starts_with("pair_") {
        let sid = value
            .get("sid")
            .and_then(Value::as_u64)
            .and_then(|sid| u32::try_from(sid).ok())
            .filter(|sid| *sid != 0)
            .ok_or(WireError::MissingSession)?;
        let message = serde_json::from_value::<PairingMessage>(value)?;
        return Ok(Some(RelayText::Pairing { sid, message }));
    }
    Ok(match serde_json::from_value::<Incoming>(value)? {
        Incoming::Hello { relay, role } => Some(RelayText::Hello { relay, role }),
        Incoming::Presence { hosts } => Some(RelayText::Presence { hosts }),
        Incoming::Opened {
            sid,
            peer,
            peer_kind,
            mode,
        } => Some(RelayText::Opened {
            sid,
            peer,
            peer_kind,
            mode,
        }),
        Incoming::Close { sid, reason } => Some(RelayText::Close { sid, reason }),
        Incoming::DeviceRevoked { device_id } => Some(RelayText::DeviceRevoked { device_id }),
        Incoming::Error { code } => Some(RelayText::Error { code }),
        Incoming::Unknown => None,
    })
}

/// A pairing message addressed to the session it belongs to. The relay routes
/// it on `sid` alone and forwards the rest untouched.
pub fn encode_pairing(sid: u32, message: &PairingMessage) -> Result<String, WireError> {
    let mut value = serde_json::to_value(message)?;
    if let Value::Object(fields) = &mut value {
        fields.insert("sid".to_string(), json!(sid));
    }
    Ok(serde_json::to_string(&value)?)
}

pub fn encode_close(sid: u32) -> String {
    json!({ "t": "close", "sid": sid }).to_string()
}

/// What the initiating side sends to start a session with `host`. A host never
/// sends it; it exists so the other end of a connection can be written, and
/// tested against, from this same crate.
pub fn encode_open(host_device_id: &str, mode: OpenMode) -> String {
    json!({ "t": "open", "host": host_device_id, "mode": mode.as_str() }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_relays_own_messages_parse() {
        assert_eq!(
            parse_relay_text(r#"{"t":"hello","relay":1,"role":"host"}"#)
                .unwrap()
                .unwrap(),
            RelayText::Hello {
                relay: 1,
                role: "host".into()
            }
        );
        assert_eq!(
            parse_relay_text(
                r#"{"t":"opened","sid":7,"peer":"dev-1","peerKind":"web","mode":"pair"}"#
            )
            .unwrap()
            .unwrap(),
            RelayText::Opened {
                sid: 7,
                peer: "dev-1".into(),
                peer_kind: "web".into(),
                mode: OpenMode::Pair
            }
        );
        assert_eq!(
            parse_relay_text(r#"{"t":"close","sid":7,"reason":"idle"}"#)
                .unwrap()
                .unwrap(),
            RelayText::Close {
                sid: 7,
                reason: "idle".into()
            }
        );
        assert_eq!(
            parse_relay_text(r#"{"t":"device_revoked","deviceId":"dev-9"}"#)
                .unwrap()
                .unwrap(),
            RelayText::DeviceRevoked {
                device_id: "dev-9".into()
            }
        );
        assert_eq!(
            parse_relay_text(r#"{"t":"presence","hosts":["a","b"]}"#)
                .unwrap()
                .unwrap(),
            RelayText::Presence {
                hosts: vec!["a".into(), "b".into()]
            }
        );
    }

    #[test]
    fn an_unknown_tag_is_ignored_not_refused() {
        assert_eq!(
            parse_relay_text(r#"{"t":"something_new","x":1}"#).unwrap(),
            None
        );
    }

    #[test]
    fn text_that_is_not_json_is_an_error() {
        assert!(parse_relay_text("not json").is_err());
        assert!(parse_relay_text(r#"{"t":"close"}"#).is_err());
    }

    #[test]
    fn a_pairing_message_round_trips_with_its_session_id() {
        let message = PairingMessage::PairReveal { nonce: [9; 32] };
        let encoded = encode_pairing(41, &message).unwrap();
        assert_eq!(
            parse_relay_text(&encoded).unwrap().unwrap(),
            RelayText::Pairing { sid: 41, message }
        );
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["t"], "pair_reveal");
        assert_eq!(value["sid"], 41);
    }

    #[test]
    fn a_pairing_message_without_a_session_is_refused() {
        let message = PairingMessage::PairReveal { nonce: [9; 32] };
        let mut value = serde_json::to_value(&message).unwrap();
        assert!(matches!(
            parse_relay_text(&value.to_string()),
            Err(WireError::MissingSession)
        ));
        value["sid"] = json!(0);
        assert!(matches!(
            parse_relay_text(&value.to_string()),
            Err(WireError::MissingSession)
        ));
    }

    #[test]
    fn outbound_messages_match_what_the_relay_reads() {
        let close: Value = serde_json::from_str(&encode_close(5)).unwrap();
        assert_eq!(close, json!({"t": "close", "sid": 5}));
        let open: Value = serde_json::from_str(&encode_open("host-1", OpenMode::Session)).unwrap();
        assert_eq!(
            open,
            json!({"t": "open", "host": "host-1", "mode": "session"})
        );
    }
}
