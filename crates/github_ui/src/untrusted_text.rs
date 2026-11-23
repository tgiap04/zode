//! Short text that strangers wrote — titles, label names, logins, branch
//! names — made safe to read.
//!
//! Bidirectional controls and invisible characters let a title read
//! differently from what it holds (Trojan Source), so each one is shown as a
//! visible `<U+XXXX>` marker instead of being drawn or hidden.

use std::borrow::Cow;

/// `text` with every bidirectional control, invisible character and control
/// character replaced by a `<U+XXXX>` marker. Borrowed when there was none.
///
/// A zero-width joiner between two pictographs is kept, since that is how
/// emoji such as 👩‍💻 are built; so are tag characters following a black flag,
/// which spell the subdivision flags (🏴 England).
pub fn reveal_hidden_characters(text: &str) -> Cow<'_, str> {
    let characters = text.chars().collect::<Vec<_>>();
    let needs_marker = |index: usize| {
        let character = characters[index];
        if character == ZERO_WIDTH_JOINER {
            let joins_pictographs = index
                .checked_sub(1)
                .and_then(|previous| characters.get(previous))
                .zip(characters.get(index + 1))
                .is_some_and(|(previous, next)| is_pictograph(*previous) && is_pictograph(*next));
            return !joins_pictographs;
        }
        if is_tag_character(character) {
            return !follows_black_flag(&characters, index);
        }
        // `❤️`: the emoji presentation selector right after a pictograph.
        if character == EMOJI_PRESENTATION {
            return !index
                .checked_sub(1)
                .and_then(|previous| characters.get(previous))
                .is_some_and(|previous| is_pictograph(*previous) && *previous != character);
        }
        is_hidden_character(character)
    };
    if !(0..characters.len()).any(needs_marker) {
        return Cow::Borrowed(text);
    }
    let mut revealed = String::with_capacity(text.len() + 16);
    for (index, character) in characters.iter().enumerate() {
        if needs_marker(index) {
            revealed.push_str(&format!("<U+{:04X}>", u32::from(*character)));
        } else {
            revealed.push(*character);
        }
    }
    Cow::Owned(revealed)
}

const ZERO_WIDTH_JOINER: char = '\u{200D}';
const BLACK_FLAG: char = '\u{1F3F4}';
const EMOJI_PRESENTATION: char = '\u{FE0F}';

/// Characters that change how the text around them reads, or that take no
/// room at all. The zero-width joiner and tag characters are judged by their
/// neighbours in [`reveal_hidden_characters`].
pub fn is_hidden_character(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0100}'..='\u{E01EF}'
    ) || is_tag_character(character)
        || character.is_control()
}

fn is_tag_character(character: char) -> bool {
    ('\u{E0000}'..='\u{E007F}').contains(&character)
}

/// Whether the run of tag characters `index` is in starts right after 🏴.
fn follows_black_flag(characters: &[char], index: usize) -> bool {
    characters[..index]
        .iter()
        .rev()
        .find(|character| !is_tag_character(**character))
        == Some(&BLACK_FLAG)
}

fn is_pictograph(character: char) -> bool {
    matches!(
        character,
        '\u{2600}'..='\u{27BF}' | '\u{2B00}'..='\u{2BFF}' | '\u{FE0F}' | '\u{1F000}'..='\u{1FAFF}'
    )
}
