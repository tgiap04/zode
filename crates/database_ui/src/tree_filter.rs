//! The text typed above the tree, and how a name is held against it.
//!
//! Pure on purpose: `rows()` asks it once per row, so the query is lowercased
//! once when it is typed and never again, and the tests need no window.

/// A case-insensitive substring query. Empty means "everything matches".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeFilter {
    text: String,
    needle: Vec<char>,
}

impl TreeFilter {
    pub(crate) fn new(text: &str) -> Self {
        let text = text.trim().to_string();
        let needle = lowercase_characters(&text)
            .map(|(_, lowered)| lowered)
            .collect();
        Self { text, needle }
    }

    /// What was typed, trimmed, as it was typed.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.needle.is_empty()
    }

    pub(crate) fn matches(&self, name: &str) -> bool {
        !self.is_active() || self.find(name).is_some()
    }

    /// The byte offset of each character of `name` the query covers, for
    /// `HighlightedLabel`, which wants offsets that sit on a character
    /// boundary. Empty when the query is empty or `name` does not match.
    pub(crate) fn highlight(&self, name: &str) -> Vec<usize> {
        if !self.is_active() {
            return Vec::new();
        }
        let Some((start, end)) = self.find(name) else {
            return Vec::new();
        };
        name.char_indices()
            .map(|(offset, _)| offset)
            .filter(|offset| (start..end).contains(offset))
            .collect()
    }

    /// The byte span of the first match, `end` exclusive and on a boundary.
    ///
    /// Lowercasing can turn one character into several, so the match runs over
    /// lowered characters that each remember the character they came from;
    /// searching a lowercased copy of the string would hand back offsets into
    /// a string that is not the one on screen.
    fn find(&self, name: &str) -> Option<(usize, usize)> {
        let lowered: Vec<(usize, char)> = lowercase_characters(name).collect();
        let needle_length = self.needle.len();
        if lowered.len() < needle_length {
            return None;
        }
        (0..=lowered.len() - needle_length).find_map(|start| {
            let window = &lowered[start..start + needle_length];
            let equal = window
                .iter()
                .zip(&self.needle)
                .all(|((_, lowered), wanted)| lowered == wanted);
            if !equal {
                return None;
            }
            let begin = window.first()?.0;
            let last = window.last()?.0;
            let last_width = name.get(last..)?.chars().next()?.len_utf8();
            Some((begin, last + last_width))
        })
    }
}

/// Each lowercased character with the byte offset of the character it came from.
fn lowercase_characters(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    text.char_indices().flat_map(|(offset, character)| {
        character
            .to_lowercase()
            .map(move |lowered| (offset, lowered))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything_and_highlights_nothing() {
        for text in ["", "   "] {
            let filter = TreeFilter::new(text);
            assert!(!filter.is_active());
            assert!(filter.matches("users"));
            assert!(filter.matches(""));
            assert!(filter.highlight("users").is_empty());
        }
    }

    #[test]
    fn a_name_matches_anywhere_in_it_whatever_the_case() {
        let filter = TreeFilter::new("  ErS ");
        assert!(filter.matches("users"));
        assert!(filter.matches("USERS_TABLE"));
        assert!(!filter.matches("items"));
        assert!(!filter.matches(""));
    }

    #[test]
    fn the_covered_characters_are_highlighted() {
        let filter = TreeFilter::new("er");
        assert_eq!(filter.highlight("Users"), vec![2, 3]);
        assert_eq!(filter.highlight("orders"), vec![3, 4]);
        assert!(filter.highlight("items").is_empty());
    }

    /// A byte offset taken from a lowercased copy would land inside a
    /// multi-byte character and make `HighlightedLabel` panic.
    #[test]
    fn highlights_sit_on_character_boundaries_for_non_ascii_names() {
        let name = "Ünïcode_ñame_日本語";
        for query in ["ünï", "ñame", "本語", "ÑAME", "e_ñ"] {
            let filter = TreeFilter::new(query);
            let indices = filter.highlight(name);
            assert!(!indices.is_empty(), "`{query}` matches `{name}`");
            for index in indices {
                assert!(
                    name.is_char_boundary(index),
                    "`{query}` highlighted inside a character at {index}"
                );
            }
        }
        assert_eq!(TreeFilter::new("日本").highlight(name).len(), 2);
    }

    /// `İ` lowercases to two characters; the name must still highlight the one
    /// it came from.
    #[test]
    fn a_character_that_lowercases_to_several_is_one_highlight() {
        let name = "İstanbul";
        let filter = TreeFilter::new("stan");
        assert_eq!(filter.highlight(name), vec![2, 3, 4, 5]);
        assert!(name.is_char_boundary(2));
        let whole = TreeFilter::new("i\u{307}s");
        assert_eq!(whole.highlight(name), vec![0, 2]);
    }
}
