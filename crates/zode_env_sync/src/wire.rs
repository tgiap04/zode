use zode_sync::{Resource, to_blob};

use crate::env_dek::EnvDek;
use crate::{EnvCryptoError, seal};

/// Exactly what a push puts on the wire.
///
/// Both fields, not one: the JSON is what a person can read and check, the
/// base64 is what the request body actually carries, and showing the first
/// while sending the second would make the panel a decoration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireBytes {
    /// The envelope, serialised. Ciphertext, never plaintext.
    pub envelope_json: String,
    /// The `blob` field of the request, character for character.
    pub blob_base64: String,
}

/// Builds the bytes for one push.
///
/// **The only place env bytes are built for the network.** `push` calls it and
/// so does the panel that shows a user what is about to leave their machine;
/// `tests/wire_is_what_is_sent.rs` asserts the two get the same string. Two
/// functions producing "the same" bytes is the shape of a promise that quietly
/// stops being true — a panel that renders its own idea of the payload proves
/// nothing about the payload.
pub fn wire_bytes(
    env_dek: &EnvDek,
    user_id: &str,
    resource: &Resource,
    seq: u64,
    payload: &[u8],
) -> Result<WireBytes, EnvCryptoError> {
    let envelope = seal::seal(env_dek, user_id, resource, seq, payload)?;
    let envelope_json = serde_json::to_string_pretty(&envelope)
        .map_err(|error| EnvCryptoError::Malformed(error.to_string()))?;
    Ok(WireBytes {
        envelope_json,
        blob_base64: to_blob(&envelope)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_sync::DEK_LEN;

    const USER: &str = "68b1f0c2a4d3e5f60718293a";

    fn built() -> WireBytes {
        let entry = crate::EntryId::parse("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1").unwrap();
        wire_bytes(
            &EnvDek::from_bytes([0x33; DEK_LEN]),
            USER,
            &seal::entry_resource(&entry).unwrap(),
            3,
            b"STRIPE_SECRET_KEY=sk_live_abcdef",
        )
        .unwrap()
    }

    #[test]
    fn neither_field_holds_plaintext() {
        let wire = built();
        for needle in ["STRIPE_SECRET_KEY", "sk_live_abcdef"] {
            assert!(
                !wire.envelope_json.contains(needle),
                "{}",
                wire.envelope_json
            );
            assert!(!wire.blob_base64.contains(needle));
        }
    }

    #[test]
    fn the_json_is_readable_by_a_person() {
        // The panel's whole value is that someone can look at this and see
        // five fields, none of which is their file.
        let wire = built();
        for field in ["\"v\"", "\"alg\"", "\"kid\"", "\"nonce\"", "\"ct\""] {
            assert!(wire.envelope_json.contains(field), "{}", wire.envelope_json);
        }
        assert!(
            wire.envelope_json.contains('\n'),
            "pretty-printed for reading"
        );
    }

    #[test]
    fn the_two_fields_describe_one_envelope() {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        let wire = built();
        let decoded = BASE64
            .decode(&wire.blob_base64)
            .expect("the blob is base64");
        let round_tripped: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        let shown: serde_json::Value = serde_json::from_str(&wire.envelope_json).unwrap();
        assert_eq!(
            round_tripped, shown,
            "the panel would be showing something other than what is sent",
        );
    }
}
