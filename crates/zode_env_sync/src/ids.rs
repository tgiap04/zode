use ring::rand::{SecureRandom as _, SystemRandom};

use crate::EnvCryptoError;

/// 128 bits. Long enough that two clients never collide without coordination,
/// short enough to read in a log line.
const ID_BYTES: usize = 16;
const ID_CHARS: usize = ID_BYTES * 2;

/// Declares one opaque 128-bit identifier.
///
/// Two of these exist and they differ only in what they name, so the
/// implementation is written once. Keeping them as distinct types is the
/// point: a project id must never be usable where an entry id is expected, and
/// the compiler is a better guard for that than care.
macro_rules! id_type {
    ($name:ident, $what:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; ID_BYTES]);

        impl $name {
            /// A fresh identifier from the OS CSPRNG.
            ///
            /// Random rather than derived. Binding a project to a machine is a
            /// manual step in Zode, so nothing ever needs to re-derive this id
            /// from a path or a git remote — and an id with no derivation
            /// input is an id the server cannot guess its way back from.
            pub fn generate() -> anyhow::Result<Self> {
                let mut bytes = [0u8; ID_BYTES];
                SystemRandom::new()
                    .fill(&mut bytes)
                    .map_err(|_| anyhow::anyhow!("the system random number generator refused"))?;
                Ok(Self(bytes))
            }

            /// Reads one back, strictly.
            ///
            /// Lowercase only, exactly `ID_CHARS` characters, nothing else.
            /// Uppercase is refused rather than folded: these strings become
            /// URL path segments, and two spellings of one id would mean two
            /// slots on the server for one file.
            pub fn parse(raw: &str) -> Result<Self, EnvCryptoError> {
                if raw.len() != ID_CHARS {
                    return Err(EnvCryptoError::Malformed(format!(
                        concat!("a ", $what, " has {} characters, this one has {}"),
                        ID_CHARS,
                        raw.len()
                    )));
                }
                let mut bytes = [0u8; ID_BYTES];
                for (index, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
                    let high = hex_value(pair[0])?;
                    let low = hex_value(pair[1])?;
                    bytes[index] = (high << 4) | low;
                }
                Ok(Self(bytes))
            }

            pub fn as_hex(&self) -> String {
                let mut out = String::with_capacity(ID_CHARS);
                for byte in self.0 {
                    out.push(HEX[(byte >> 4) as usize] as char);
                    out.push(HEX[(byte & 0x0f) as usize] as char);
                }
                out
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.as_hex())
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.as_hex())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.as_hex())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn hex_value(byte: u8) -> Result<u8, EnvCryptoError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        other => Err(EnvCryptoError::Malformed(format!(
            "'{}' is not lowercase hexadecimal",
            other as char
        ))),
    }
}

id_type!(EntryId, "entry id");
id_type!(ProjectId, "project id");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_its_text_form() {
        let id = EntryId::generate().unwrap();
        assert_eq!(EntryId::parse(&id.as_hex()).unwrap(), id);
    }

    #[test]
    fn generate_produces_distinct_ids() {
        assert_ne!(
            EntryId::generate().unwrap(),
            EntryId::generate().unwrap(),
            "two generated ids must not collide"
        );
    }

    #[test]
    fn uppercase_is_refused_rather_than_folded() {
        let id = EntryId::generate().unwrap().as_hex().to_uppercase();
        assert!(EntryId::parse(&id).is_err(), "uppercase must not parse");
    }

    #[test]
    fn the_wrong_length_is_refused() {
        assert!(EntryId::parse("").is_err());
        assert!(EntryId::parse("0123456789abcdef").is_err());
        assert!(EntryId::parse(&"0".repeat(33)).is_err());
    }

    #[test]
    fn non_hexadecimal_is_refused() {
        assert!(EntryId::parse(&format!("{}z", "0".repeat(31))).is_err());
        assert!(EntryId::parse(&format!("{}/", "0".repeat(31))).is_err());
        assert!(EntryId::parse(&format!("{}.", "0".repeat(31))).is_err());
    }

    #[test]
    fn the_two_id_types_do_not_mix() {
        // Not an assertion so much as a note: this is held by the type system,
        // and the test exists so a refactor that collapses them into one alias
        // has something to break.
        let entry = EntryId::generate().unwrap();
        let project = ProjectId::parse(&entry.as_hex()).unwrap();
        assert_eq!(project.as_hex(), entry.as_hex());
    }
}
