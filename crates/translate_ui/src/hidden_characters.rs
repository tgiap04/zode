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
    let mut previous = None;
    while let Some(character) = characters.next() {
        let before = previous.replace(character);
        match character {
            '\n' | '\t' => cleaned.push(character),
            '\r' if characters.peek() == Some(&'\n') => {}
            '\u{1b}' => {
                skip_escape_sequence(&mut characters);
                removed += 1;
            }
            // Text or emoji presentation of the symbol before it; kept only
            // there, since anywhere else it is invisible and carries nothing.
            '\u{FE0E}' | '\u{FE0F}'
                if before.is_some_and(|before| !before.is_ascii() && !is_hidden(before)) =>
            {
                cleaned.push(character)
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
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'
                | '\u{1160}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                // Invisible operators, the isolates, and the deprecated
                // format characters that follow them.
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
                | '\u{E0100}'..='\u{E01EF}'
        )
}

/// The first non-blank line of `text` for showing the user what is about to
/// be sent: cleaned of hidden characters, cut to `max_characters`, with an
/// ellipsis when more follows.
pub(crate) fn preview_line(text: &str, max_characters: usize) -> String {
    let (cleaned, _) = remove_hidden_characters(text);
    let mut lines = cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let first = lines.next().unwrap_or_default();
    let mut preview: String = first.chars().take(max_characters).collect();
    if first.chars().nth(max_characters).is_some() || lines.next().is_some() {
        preview.push('…');
    }
    preview
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
    use super::{preview_line, remove_hidden_characters};

    #[test]
    fn plain_text_newlines_and_tabs_are_kept() {
        let text = "Hello\tworld\nXin chào 👋🏽 مرحبا";
        assert_eq!(remove_hidden_characters(text), (text.to_string(), 0));
    }

    #[test]
    fn invisible_operators_and_blank_fillers_are_removed() {
        for hidden in [
            '\u{2061}', '\u{2062}', '\u{2063}', '\u{2064}', '\u{206A}', '\u{206F}', '\u{180E}',
            '\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{FFF9}', '\u{FFFA}', '\u{FFFB}',
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
    fn invisible_marks_and_separators_are_removed() {
        for hidden in [
            '\u{200E}',
            '\u{200F}',
            '\u{061C}',
            '\u{2028}',
            '\u{2029}',
            '\u{00AD}',
            '\u{034F}',
            '\u{E0000}',
            '\u{E0001}',
            '\u{E0020}',
            '\u{E007F}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{FE00}',
            '\u{FE0D}',
        ] {
            assert_eq!(
                remove_hidden_characters(&format!("a{hidden}b")),
                ("ab".to_string(), 1),
                "U+{:04X}",
                hidden as u32
            );
        }
    }

    /// A hidden payload can ride on variation selectors, but the emoji
    /// presentation selector after a symbol is ordinary text.
    #[test]
    fn emoji_presentation_selectors_stay_after_a_symbol_and_nowhere_else() {
        for text in [
            "I \u{2764}\u{FE0F} you",
            "\u{2714}\u{FE0E}",
            "\u{1F441}\u{FE0F}",
        ] {
            assert_eq!(
                remove_hidden_characters(text),
                (text.to_string(), 0),
                "{text}"
            );
        }
        for (text, cleaned, removed) in [
            ("a\u{FE0F}b", "ab", 1),
            ("\u{FE0F}b", "b", 1),
            (" \u{FE0F}", " ", 1),
            ("\u{2764}\u{FE0F}\u{FE0F}", "\u{2764}\u{FE0F}", 1),
            ("\u{2764}\u{FE00}\u{FE0F}", "\u{2764}", 2),
            ("\u{2764}\u{E0100}", "\u{2764}", 1),
        ] {
            assert_eq!(
                remove_hidden_characters(text),
                (cleaned.to_string(), removed),
                "{text:?}"
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

    #[test]
    fn the_preview_is_the_first_line_cleaned_and_cut() {
        assert_eq!(preview_line("Xin chao", 80), "Xin chao");
        assert_eq!(preview_line("  \n\n  Xin chao  ", 80), "Xin chao");
        assert_eq!(preview_line("one\ntwo", 80), "one…");
        assert_eq!(preview_line("abcdef", 3), "abc…");
        assert_eq!(preview_line("abc", 3), "abc");
        assert_eq!(preview_line("ệệệệ", 2), "ệệ…");
        assert_eq!(
            preview_line("\u{202E}evil\u{1b}[31m red", 80),
            "evil red",
            "bidi controls and escapes are not shown"
        );
        assert_eq!(preview_line(" \n ", 80), "");
    }
}
