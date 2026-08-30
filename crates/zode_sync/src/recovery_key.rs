use crate::dek::{DEK_LEN, Dek};

/// Crockford base32: no `I`, `L`, `O`, or `U`.
///
/// Must stay identical to `ALPHABET` in
/// `web/backend/src/auth/device/user-code.util.ts` and its frontend mirror.
/// P1 already shipped one copy of this table; a second copy that drifts is the
/// same class of bug as the two cookie paths that disagreed on 2026-08-30 —
/// two halves of one rule, neither knowing the other changed.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Data characters: 32 bytes is 256 bits, and base32 carries 5 bits each.
const DATA_CHARS: usize = 52; // ceil(256 / 5)
const GROUP: usize = 5;
const PREFIX: &str = "ZODE";

#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryKeyError {
    /// Not the right number of characters once separators are removed.
    WrongLength { found: usize },
    /// A character that is not in the alphabet, even after folding.
    BadCharacter(char),
    /// Right shape, wrong check character — almost always a typo.
    ChecksumMismatch,
    /// The trailing bits carry data they should not, so this string was not
    /// produced by `encode`.
    NonCanonical,
}

impl std::fmt::Display for RecoveryKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLength { found } => {
                write!(
                    f,
                    "a recovery key has {} characters, this one has {found}",
                    DATA_CHARS + 1
                )
            }
            Self::BadCharacter(c) => write!(f, "'{c}' is not part of a recovery key"),
            Self::ChecksumMismatch => write!(f, "that recovery key has a typo in it"),
            Self::NonCanonical => write!(f, "that is not a well-formed recovery key"),
        }
    }
}

impl std::error::Error for RecoveryKeyError {}

/// Folds the characters people reliably mistype. Same three as the device
/// user code — the alphabet excludes the targets, so the mapping is lossless.
fn fold(c: char) -> char {
    match c {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    }
}

fn symbol_value(c: char) -> Option<u8> {
    ALPHABET
        .iter()
        .position(|&s| s as char == c)
        .map(|index| index as u8)
}

/// Renders a key as the string the user writes down.
///
/// 52 data characters plus one check character, in groups of five behind a
/// `ZODE-` prefix. Long, deliberately: the alternative is deriving the key
/// from something short enough to type comfortably, which is also short enough
/// for the server holding the ciphertext to brute-force.
pub fn encode(dek: &Dek) -> String {
    let bytes = dek.bytes();
    let mut symbols = String::with_capacity(DATA_CHARS + 1);

    // MSB-first, five bits at a time. The last symbol carries four real bits
    // and one zero of padding — `decode` insists that padding stays zero.
    for index in 0..DATA_CHARS {
        let bit_offset = index * 5;
        let mut value = 0u8;
        for bit in 0..5 {
            let absolute = bit_offset + bit;
            let set = if absolute < DEK_LEN * 8 {
                (bytes[absolute / 8] >> (7 - (absolute % 8))) & 1
            } else {
                0
            };
            value = (value << 1) | set;
        }
        symbols.push(ALPHABET[value as usize] as char);
    }
    symbols.push(ALPHABET[dek.check_bits() as usize] as char);

    let grouped = symbols
        .as_bytes()
        .chunks(GROUP)
        .map(|chunk| std::str::from_utf8(chunk).expect("alphabet is ASCII"))
        .collect::<Vec<_>>()
        .join("-");

    format!("{PREFIX}-{grouped}")
}

/// Reads a key the user typed or pasted.
///
/// Tolerant about presentation — any whitespace, any hyphenation, any case,
/// with or without the prefix — and strict about content. A wrong character is
/// rejected rather than dropped: silently stripping one would turn a typo into
/// a different valid-looking key.
pub fn decode(input: &str) -> Result<Dek, RecoveryKeyError> {
    // Separators and case go first, but NOT the confusable folding: the
    // `ZODE` prefix contains an `O`, and folding before stripping would turn
    // it into `Z0DE` and leave the prefix unrecognised. Folding applies to the
    // key body only, which is the only part that is base32.
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .map(|c| c.to_ascii_uppercase())
        .collect();

    let body = cleaned.strip_prefix(PREFIX).unwrap_or(&cleaned);

    if body.chars().count() != DATA_CHARS + 1 {
        return Err(RecoveryKeyError::WrongLength {
            found: body.chars().count(),
        });
    }

    let mut values = Vec::with_capacity(DATA_CHARS + 1);
    for raw in body.chars() {
        let c = fold(raw);
        values.push(symbol_value(c).ok_or(RecoveryKeyError::BadCharacter(raw))?);
    }

    let check = values.pop().expect("length was verified above");

    let mut bytes = [0u8; DEK_LEN];
    for (index, value) in values.iter().enumerate() {
        for bit in 0..5 {
            let absolute = index * 5 + bit;
            if absolute >= DEK_LEN * 8 {
                // Padding territory. Anything set here means the string did
                // not come from `encode`, so refuse rather than truncate.
                if (value >> (4 - bit)) & 1 == 1 {
                    return Err(RecoveryKeyError::NonCanonical);
                }
                continue;
            }
            let bit_value = (value >> (4 - bit)) & 1;
            bytes[absolute / 8] |= bit_value << (7 - (absolute % 8));
        }
    }

    let dek = Dek::from_bytes(bytes);
    if dek.check_bits() != check {
        return Err(RecoveryKeyError::ChecksumMismatch);
    }
    Ok(dek)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let dek = Dek::generate().unwrap();
        let encoded = encode(&dek);
        let decoded = decode(&encoded).expect("a freshly encoded key must decode");
        assert_eq!(decoded.bytes(), dek.bytes());
    }

    #[test]
    fn round_trips_every_edge_pattern() {
        for pattern in [
            [0x00u8; DEK_LEN],
            [0xFF; DEK_LEN],
            [0xAA; DEK_LEN],
            [0x55; DEK_LEN],
        ] {
            let dek = Dek::from_bytes(pattern);
            let decoded = decode(&encode(&dek)).expect("edge pattern must round trip");
            assert_eq!(decoded.bytes(), &pattern);
        }
    }

    #[test]
    fn the_rendered_shape_is_stable() {
        let encoded = encode(&Dek::from_bytes([0u8; DEK_LEN]));
        assert!(encoded.starts_with("ZODE-"));
        // 53 symbols plus the prefix, separators excluded.
        assert_eq!(
            encoded.chars().filter(|c| *c != '-').count(),
            DATA_CHARS + 1 + PREFIX.len()
        );
    }

    #[test]
    fn accepts_any_presentation_of_the_same_key() {
        let dek = Dek::generate().unwrap();
        let canonical = encode(&dek);
        let stripped = canonical.replace('-', "");
        let spaced = stripped
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ");

        for variant in [
            canonical.clone(),
            stripped,
            spaced,
            canonical.to_lowercase(),
        ] {
            assert_eq!(
                decode(&variant).unwrap().bytes(),
                dek.bytes(),
                "variant: {variant}"
            );
        }
    }

    #[test]
    fn folds_the_confusable_characters() {
        // A key whose encoding contains 0 and 1, typed as O and I.
        let dek = Dek::generate().unwrap();
        let canonical = encode(&dek).replace('-', "");
        let mistyped: String = canonical
            .chars()
            .map(|c| match c {
                '0' => 'O',
                '1' => 'I',
                other => other,
            })
            .collect();
        assert_eq!(decode(&mistyped).unwrap().bytes(), dek.bytes());
    }

    #[test]
    fn rejects_a_single_character_typo() {
        let dek = Dek::from_bytes([1u8; DEK_LEN]);
        let encoded = encode(&dek).replace('-', "");
        let mut wrong: Vec<char> = encoded.chars().collect();
        // Change one data character to a different valid symbol.
        let position = PREFIX.len() + 3;
        wrong[position] = if wrong[position] == '2' { '3' } else { '2' };
        let mistyped: String = wrong.into_iter().collect();

        // `matches!` rather than `assert_eq!` because `Dek` deliberately has
        // no `PartialEq` — comparing keys is not something this crate should
        // make convenient.
        assert!(matches!(
            decode(&mistyped),
            Err(RecoveryKeyError::ChecksumMismatch)
        ));
    }

    #[test]
    fn rejects_rather_than_strips_an_unknown_character() {
        let encoded = encode(&Dek::generate().unwrap()).replace('-', "");
        let body: String = encoded.chars().skip(PREFIX.len()).collect();
        // `U` is excluded from Crockford's alphabet and has no fold, so this
        // must be an error rather than a silently shorter key. Substituted
        // inside the body, keeping the length correct — otherwise the length
        // check would answer first and this would prove nothing.
        let bad = format!("{PREFIX}U{}", body.chars().skip(1).collect::<String>());

        assert!(
            matches!(decode(&bad), Err(RecoveryKeyError::BadCharacter('U'))),
            "got {:?}",
            decode(&bad)
        );
    }

    #[test]
    fn rejects_the_wrong_length() {
        assert!(matches!(
            decode("ZODE-ABC"),
            Err(RecoveryKeyError::WrongLength { .. })
        ));
    }

    #[test]
    fn rejects_non_canonical_padding() {
        let dek = Dek::from_bytes([0u8; DEK_LEN]);
        let mut chars: Vec<char> = encode(&dek).replace('-', "").chars().collect();
        // Last DATA char carries 4 real bits + 1 padding bit. Setting the
        // padding bit gives a string `encode` could never produce.
        let last_data = PREFIX.len() + DATA_CHARS - 1;
        chars[last_data] = '1'; // value 1 -> lowest bit set, which is the pad
        let tampered: String = chars.into_iter().collect();
        assert!(matches!(
            decode(&tampered),
            Err(RecoveryKeyError::NonCanonical)
        ));
    }

    /// Cross-language guard against the alphabet drifting apart.
    ///
    /// Skips when the `web` repo is not checked out beside this one — the two
    /// live in separate repositories, so this cannot be a hard requirement in
    /// CI. A guard that only fires locally is weaker than one that always
    /// fires, but it is true, and the alternative is no check at all.
    #[test]
    fn the_alphabet_matches_the_backend_copy() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../web/backend/src/auth/device/user-code.util.ts");
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("skipped: {} is not present", path.display());
            return;
        };
        let line = source
            .lines()
            .find(|line| line.contains("const ALPHABET"))
            .expect("the backend must still declare ALPHABET");
        let backend = line
            .split('\'')
            .nth(1)
            .expect("ALPHABET must be a quoted literal");
        assert_eq!(
            backend,
            std::str::from_utf8(ALPHABET).unwrap(),
            "the Rust and TypeScript alphabets have drifted apart",
        );
    }
}
