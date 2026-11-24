//! The edits behind the markdown toolbar: each format wraps the selection,
//! or prefixes the lines it touches, and undoes itself when applied again.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownFormat {
    Heading,
    Bold,
    Italic,
    Quote,
    Code,
    Link,
    BulletList,
    NumberedList,
    TaskList,
}

/// One replacement, and what is selected once it is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatEdit {
    /// The bytes replaced, in the text before the edit.
    pub replaced: Range<usize>,
    pub text: String,
    /// In the text after the edit.
    pub selection: Range<usize>,
}

/// `None` when the selection is not a range of `text`.
pub fn format_edit(
    text: &str,
    selection: Range<usize>,
    format: MarkdownFormat,
) -> Option<FormatEdit> {
    if selection.start > selection.end
        || !text.is_char_boundary(selection.start)
        || !text.is_char_boundary(selection.end)
    {
        return None;
    }
    Some(match format {
        MarkdownFormat::Bold => wrap(text, selection, "**"),
        MarkdownFormat::Italic => wrap(text, selection, "_"),
        MarkdownFormat::Code if text[selection.clone()].contains('\n') => fence(text, selection),
        MarkdownFormat::Code => wrap(text, selection, "`"),
        MarkdownFormat::Link => link(text, selection),
        MarkdownFormat::Heading => prefix_lines(text, selection, LinePrefix::Heading),
        MarkdownFormat::Quote => prefix_lines(text, selection, LinePrefix::Fixed("> ")),
        MarkdownFormat::BulletList => prefix_lines(text, selection, LinePrefix::Fixed("- ")),
        MarkdownFormat::TaskList => prefix_lines(text, selection, LinePrefix::Fixed("- [ ] ")),
        MarkdownFormat::NumberedList => prefix_lines(text, selection, LinePrefix::Numbered),
    })
}

/// `marker` around the selection; taken off when it is there already,
/// just outside the selection or as its own ends.
fn wrap(text: &str, selection: Range<usize>, marker: &str) -> FormatEdit {
    let Range { start, end } = selection;
    let selected = &text[start..end];
    let length = marker.len();
    if wrapped_outside(text, start, end, marker) {
        return FormatEdit {
            replaced: start - length..end + length,
            text: selected.to_string(),
            selection: start - length..end - length,
        };
    }
    if selected.len() >= 2 * length && selected.starts_with(marker) && selected.ends_with(marker) {
        let inner = &selected[length..selected.len() - length];
        if marker_positions(inner, marker).is_empty() {
            return FormatEdit {
                replaced: start..end,
                text: inner.to_string(),
                selection: start..start + inner.len(),
            };
        }
    }
    FormatEdit {
        replaced: start..end,
        text: format!("{marker}{selected}{marker}"),
        selection: start + length..end + length,
    }
}

/// Whether the markers right before and after the selection are one span's
/// own: not part of a longer run or a word, and the one before opens a span
/// (an even number of markers stand earlier on its line) rather than closing
/// the span before, as in `**a** and **b**` around ` and `.
fn wrapped_outside(text: &str, start: usize, end: usize, marker: &str) -> bool {
    let Some(opening) = start.checked_sub(marker.len()) else {
        return false;
    };
    if !text[..start].ends_with(marker)
        || !text[end..].starts_with(marker)
        || !is_marker_at(text, opening, marker)
        || !is_marker_at(text, end, marker)
        || !marker_positions(&text[start..end], marker).is_empty()
    {
        return false;
    }
    let line_start = text[..opening].rfind('\n').map_or(0, |index| index + 1);
    marker_positions(&text[line_start..opening], marker)
        .len()
        .is_multiple_of(2)
}

/// Where `marker` stands in `text` as a marker would: see [`is_marker_at`].
fn marker_positions(text: &str, marker: &str) -> Vec<usize> {
    text.match_indices(marker)
        .map(|(index, _)| index)
        .filter(|index| is_marker_at(text, *index, marker))
        .collect()
}

/// Whether the `marker` at `index` is one, rather than part of a longer run
/// of the same character (`***`) or, for `_`, the middle of a word
/// (`snake_case`).
fn is_marker_at(text: &str, index: usize, marker: &str) -> bool {
    let before = text[..index].chars().next_back();
    let after = text[index + marker.len()..].chars().next();
    if before == marker.chars().next_back() || after == marker.chars().next() {
        return false;
    }
    let inside_word =
        before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric);
    !(marker == "_" && inside_word)
}

/// The whole lines the selection touches; a selection ending at the start
/// of a line does not take that line.
fn line_span(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let end = if selection.end > selection.start && text[..selection.end].ends_with('\n') {
        selection.end - 1
    } else {
        text[selection.end..]
            .find('\n')
            .map_or(text.len(), |index| selection.end + index)
    };
    start..end.max(start)
}

fn fence(text: &str, selection: Range<usize>) -> FormatEdit {
    let span = line_span(text, &selection);
    let lines = &text[span.clone()];
    const OPEN: &str = "```\n";
    const CLOSE: &str = "\n```";
    if text[..span.start].ends_with(OPEN) && text[span.end..].starts_with(CLOSE) {
        let start = span.start - OPEN.len();
        return FormatEdit {
            replaced: start..span.end + CLOSE.len(),
            text: lines.to_string(),
            selection: start..start + lines.len(),
        };
    }
    let inner = span.start + OPEN.len();
    FormatEdit {
        replaced: span,
        text: format!("{OPEN}{lines}{CLOSE}"),
        selection: inner..inner + lines.len(),
    }
}

/// `[text](url)` with `url` selected to type over; a selected address
/// becomes the link's target, with the cursor left in its text.
fn link(text: &str, selection: Range<usize>) -> FormatEdit {
    let Range { start, end } = selection;
    let selected = &text[start..end];
    if selected.starts_with("https://") || selected.starts_with("http://") {
        return FormatEdit {
            replaced: start..end,
            text: format!("[]({selected})"),
            selection: start + 1..start + 1,
        };
    }
    if selected.is_empty() {
        return FormatEdit {
            replaced: start..end,
            text: "[](url)".to_string(),
            selection: start + 1..start + 1,
        };
    }
    let address = start + selected.len() + 3;
    FormatEdit {
        replaced: start..end,
        text: format!("[{selected}](url)"),
        selection: address..address + 3,
    }
}

/// What goes in front of each line a line format touches.
#[derive(Clone, Copy)]
enum LinePrefix {
    Fixed(&'static str),
    /// `### `, standing in for a heading of any level, which it replaces.
    Heading,
    /// `1. `, `2. `, … counting the lines that get one.
    Numbered,
}

impl LinePrefix {
    fn nth(self, number: usize) -> String {
        match self {
            Self::Fixed(prefix) => prefix.to_string(),
            Self::Heading => HEADING.to_string(),
            Self::Numbered => format!("{number}. "),
        }
    }

    /// How long the prefix `line` already starts with, if it has one.
    fn present(self, line: &str) -> Option<usize> {
        match self {
            Self::Fixed(prefix) => line.starts_with(prefix).then_some(prefix.len()),
            Self::Heading => heading_prefix_length(line),
            Self::Numbered => numbered_prefix_length(line),
        }
    }

    /// Whether `line` has this very prefix, which applying it again takes
    /// off. A heading of another level is changed to this one instead.
    fn is_exactly_on(self, line: &str) -> bool {
        match self {
            Self::Heading => line.starts_with(HEADING),
            _ => self.present(line).is_some(),
        }
    }

    /// How much of `line` a prefix being added takes the place of.
    fn replaced_by_adding(self, line: &str) -> usize {
        match self {
            Self::Heading => self.present(line).unwrap_or(0),
            _ => 0,
        }
    }
}

/// Each line touched gets the prefix; blank lines inside a longer selection
/// are left alone. When every other line has it already, it comes off.
fn prefix_lines(text: &str, selection: Range<usize>, prefix: LinePrefix) -> FormatEdit {
    let span = line_span(text, &selection);
    let lines = text[span.clone()].split('\n').collect::<Vec<_>>();
    let several = lines.len() > 1;
    let skipped = |line: &str| several && line.trim().is_empty();
    let removing = lines
        .iter()
        .all(|line| skipped(line) || prefix.is_exactly_on(line));
    let mut number = 0;
    let mut first_line_shift: isize = 0;
    let replaced_lines = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if skipped(line) {
                return line.to_string();
            }
            number += 1;
            let (new_line, shift) = match prefix.present(line).filter(|_| removing) {
                Some(length) => (line[length..].to_string(), -(length as isize)),
                None => {
                    let added = prefix.nth(number);
                    let replaced = prefix.replaced_by_adding(line);
                    let shift = added.len() as isize - replaced as isize;
                    (format!("{added}{}", &line[replaced..]), shift)
                }
            };
            if index == 0 {
                first_line_shift = shift;
            }
            new_line
        })
        .collect::<Vec<_>>()
        .join("\n");
    let selection = if selection.is_empty() {
        let cursor =
            (selection.start as isize + first_line_shift).max(span.start as isize) as usize;
        cursor..cursor
    } else {
        span.start..span.start + replaced_lines.len()
    };
    FormatEdit {
        replaced: span,
        text: replaced_lines,
        selection,
    }
}

/// The length of a `12. ` at the start of `line`.
fn numbered_prefix_length(line: &str) -> Option<usize> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    (digits > 0 && line[digits..].starts_with(". ")).then_some(digits + 2)
}

const HEADING: &str = "### ";

/// The length of a `## ` at the start of `line`: one to six hashes and a
/// space.
fn heading_prefix_length(line: &str) -> Option<usize> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    ((1..=6).contains(&hashes) && line[hashes..].starts_with(' ')).then_some(hashes + 1)
}
