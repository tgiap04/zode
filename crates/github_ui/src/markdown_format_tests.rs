use std::ops::Range;

use crate::markdown_format::{MarkdownFormat, format_edit};

/// The text and the selection after applying `format`, the selection
/// written as `[` and `]` (a cursor as `|`).
fn apply(text: &str, selection: Range<usize>, format: MarkdownFormat) -> String {
    let edit = format_edit(text, selection, format).expect("a valid selection");
    let mut result = text.to_string();
    result.replace_range(edit.replaced.clone(), &edit.text);
    let Range { start, end } = edit.selection;
    if start == end {
        result.insert(start, '|');
    } else {
        result.insert(end, ']');
        result.insert(start, '[');
    }
    result
}

#[test]
fn bold_wraps_the_selection_and_unwraps_it_again() {
    assert_eq!(
        apply("a word here", 2..6, MarkdownFormat::Bold),
        "a **[word]** here"
    );
    assert_eq!(
        apply("a **word** here", 4..8, MarkdownFormat::Bold),
        "a [word] here"
    );
    assert_eq!(
        apply("a **word** here", 2..10, MarkdownFormat::Bold),
        "a [word] here"
    );
    assert_eq!(apply("ab", 1..1, MarkdownFormat::Bold), "a**|**b");
}

#[test]
fn italic_and_inline_code_wrap_like_bold() {
    assert_eq!(
        apply("so é good", 3..5, MarkdownFormat::Italic),
        "so _[é]_ good"
    );
    assert_eq!(
        apply("run ls now", 4..6, MarkdownFormat::Code),
        "run `[ls]` now"
    );
    assert_eq!(
        apply("run `ls` now", 5..7, MarkdownFormat::Code),
        "run [ls] now"
    );
}

#[test]
fn code_over_several_lines_is_a_fenced_block_of_whole_lines() {
    assert_eq!(
        apply(
            "intro\nfn a() {}\nfn b() {}\nend",
            8..18,
            MarkdownFormat::Code
        ),
        "intro\n```\n[fn a() {}\nfn b() {}]\n```\nend"
    );
    assert_eq!(
        apply(
            "```\nfn a() {}\nfn b() {}\n```",
            4..23,
            MarkdownFormat::Code
        ),
        "[fn a() {}\nfn b() {}]"
    );
}

#[test]
fn a_link_selects_the_address_to_type_over() {
    assert_eq!(
        apply("see docs", 4..8, MarkdownFormat::Link),
        "see [docs]([url])"
    );
    assert_eq!(apply("see ", 4..4, MarkdownFormat::Link), "see [|](url)");
    assert_eq!(
        apply("at https://x.io", 3..15, MarkdownFormat::Link),
        "at [|](https://x.io)"
    );
}

#[test]
fn line_formats_prefix_every_line_touched_and_toggle_off() {
    assert_eq!(apply("title", 2..2, MarkdownFormat::Heading), "### ti|tle");
    assert_eq!(apply("### title", 6..6, MarkdownFormat::Heading), "ti|tle");
    assert_eq!(
        apply("one\ntwo", 1..5, MarkdownFormat::Quote),
        "[> one\n> two]"
    );
    assert_eq!(
        apply("a\nb\n\nc", 0..6, MarkdownFormat::NumberedList),
        "[1. a\n2. b\n\n3. c]",
        "blank lines between items are kept as they are"
    );
    assert_eq!(
        apply("1. a\n2. b", 0..9, MarkdownFormat::NumberedList),
        "[a\nb]"
    );
    assert_eq!(
        apply("x\ny", 0..3, MarkdownFormat::BulletList),
        "[- x\n- y]"
    );
    assert_eq!(apply("", 0..0, MarkdownFormat::TaskList), "- [ ] |");
    assert_eq!(apply("- [ ] do", 8..8, MarkdownFormat::TaskList), "do|");
}

#[test]
fn a_selection_ending_at_a_line_start_leaves_that_line_alone() {
    assert_eq!(
        apply("one\ntwo", 0..4, MarkdownFormat::Quote),
        "[> one]\ntwo"
    );
}

#[test]
fn a_selection_that_is_not_in_the_text_changes_nothing() {
    assert_eq!(format_edit("abc", 2..9, MarkdownFormat::Bold), None);
    assert_eq!(
        format_edit("abc", Range { start: 2, end: 1 }, MarkdownFormat::Bold),
        None
    );
    assert_eq!(format_edit("é", 1..1, MarkdownFormat::Bold), None);
}

#[test]
fn a_marker_belonging_to_a_neighbouring_span_is_not_taken_off() {
    // " and " sits between two bold words: the `**` on each side close one
    // span and open the next, they do not wrap the selection.
    assert_eq!(
        apply("**a** and **b**", 5..10, MarkdownFormat::Bold),
        "**a****[ and ]****b**"
    );
    assert_eq!(
        apply("**a** and **b**", 2..3, MarkdownFormat::Bold),
        "[a] and **b**"
    );
    assert_eq!(
        apply("**a** and **b**", 12..13, MarkdownFormat::Bold),
        "**a** and [b]"
    );
    // The whole line starts and ends with `**` but is two spans, not one.
    assert_eq!(
        apply("**a** and **b**", 0..15, MarkdownFormat::Bold),
        "[**a** and **b**]"
            .replacen('[', "**[", 1)
            .replacen(']', "]**", 1)
    );
}

#[test]
fn italic_ignores_underscores_inside_a_word() {
    assert_eq!(
        apply("a _word_ here", 3..7, MarkdownFormat::Italic),
        "a [word] here"
    );
    assert_eq!(
        apply("snake_case_word", 6..10, MarkdownFormat::Italic),
        "snake__[case]__word",
        "the underscores of an identifier are not italics"
    );
}

#[test]
fn a_heading_prefix_of_any_level_is_replaced_not_stacked() {
    assert_eq!(
        apply("## title", 5..5, MarkdownFormat::Heading),
        "### ti|tle"
    );
    assert_eq!(
        apply("# title", 3..3, MarkdownFormat::Heading),
        "### t|itle"
    );
    assert_eq!(
        apply("###### title", 8..8, MarkdownFormat::Heading),
        "### t|itle"
    );
    assert_eq!(
        apply("## a\n### b", 0..10, MarkdownFormat::Heading),
        "[### a\n### b]"
    );
    assert_eq!(
        apply("#hashtag", 0..0, MarkdownFormat::Heading),
        "### |#hashtag",
        "no space after the hashes: not a heading"
    );
    assert_eq!(
        apply("####### seven", 0..0, MarkdownFormat::Heading),
        "### |####### seven",
        "seven hashes is not a heading either"
    );
}
