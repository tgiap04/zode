//! A provider's answer is untrusted text: a prompt-injected model or a
//! compromised server can hide bidi overrides (Trojan Source), zero-width
//! characters, control codes or terminal escapes in it. They do not show in a
//! label, so they are removed before the answer is shown, copied or written
//! over the user's text.

/// The answer without hidden characters, and how many were removed. Newlines
/// and tabs stay; a `\r\n` line ending becomes `\n` and is not counted.
pub(crate) fn remove_hidden_characters(text: &str) -> (String, usize) {
    let mut cleaned = String::with_capacity(text.len());
    let mut removed = 0;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\n' | '\t' => cleaned.push(character),
            '\r' if characters.peek() == Some(&'\n') => {}
            '\u{1b}' => {
                skip_escape_sequence(&mut characters);
                removed += 1;
            }
            character if is_hidden(character) => removed += 1,
            character => cleaned.push(character),
        }
    }
    (cleaned, removed)
}

fn is_hidden(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200B}'..='\u{200D}'
                | '\u{2060}'
                | '\u{FEFF}'
        )
}

/// Skips what follows an ESC: a CSI sequence (`ESC [ … final`), an OSC
/// sequence (`ESC ] … BEL` or `ESC ] … ESC \`), or a single character.
fn skip_escape_sequence(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match characters.next() {
        Some('[') => {
            for character in characters.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&character) {
                    break;
                }
            }
        }
        Some(']') => {
            while let Some(character) = characters.next() {
                if character == '\u{7}' {
                    break;
                }
                if character == '\u{1b}' && characters.peek() == Some(&'\\') {
                    characters.next();
                    break;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::remove_hidden_characters;

    #[test]
    fn plain_text_newlines_and_tabs_are_kept() {
        let text = "Hello\tworld\nXin chào 👋🏽 مرحبا";
        assert_eq!(remove_hidden_characters(text), (text.to_string(), 0));
    }

    #[test]
    fn bidi_controls_and_zero_width_characters_are_removed() {
        for hidden in [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}',
        ] {
            assert_eq!(
                remove_hidden_characters(&format!("a{hidden}b")),
                ("ab".to_string(), 1),
                "U+{:04X}",
                hidden as u32
            );
        }
    }

    #[test]
    fn control_characters_are_removed_except_line_breaks() {
        assert_eq!(
            remove_hidden_characters("a\u{0}b\u{7}c\u{7f}d\u{85}e\u{9b}f"),
            ("abcdef".to_string(), 5)
        );
        assert_eq!(
            remove_hidden_characters("one\r\ntwo\rthree"),
            ("one\ntwothree".to_string(), 1)
        );
    }

    #[test]
    fn terminal_escape_sequences_are_removed_whole() {
        assert_eq!(
            remove_hidden_characters("\u{1b}[1;31mred\u{1b}[0m"),
            ("red".to_string(), 2)
        );
        assert_eq!(
            remove_hidden_characters("\u{1b}]8;;https://evil\u{7}link\u{1b}]8;;\u{1b}\\"),
            ("link".to_string(), 2)
        );
        assert_eq!(remove_hidden_characters("a\u{1b}Mb"), ("ab".to_string(), 1));
        assert_eq!(
            remove_hidden_characters("end\u{1b}"),
            ("end".to_string(), 1)
        );
        assert_eq!(
            remove_hidden_characters("cut\u{1b}[31"),
            ("cut".to_string(), 1)
        );
    }
}
