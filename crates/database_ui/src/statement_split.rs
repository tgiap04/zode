//! Reads SQL the way an engine's lexer does, and finds the statement the
//! cursor is in.
//!
//! A scratch buffer collects statements, and running all of them because the
//! cursor happened to be in one is not what anybody means. The split is on
//! `;`, but not on a `;` inside a string, a quoted name, a comment or (where
//! the engine has them) a dollar-quoted body.
//!
//! The tokens underneath are also what the write guard in `safe_mode` reads, so
//! that the two cannot disagree about where a comment or a string ends.
//!
//! A `BEGIN ... END` body holding its own `;` is not understood: it splits
//! there. Selecting the whole block and running the selection is the way
//! round, and a parser per engine is not worth carrying for it.

use std::ops::Range;

/// What the engine does to the characters the scanner steps over.
///
/// The all-false value is standard SQL: `--` and `/* */` comments, `'` strings,
/// `"` names, and nothing else special.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SplitRules {
    /// `$tag$ ... $tag$` is a string.
    pub(crate) dollar_quotes: bool,
    /// A backslash inside a quoted string escapes the next character, so
    /// `'it\'s'` is one string. Never inside a backtick name.
    pub(crate) backslash_escapes: bool,
    /// `"..."` is a string, and obeys `backslash_escapes`, instead of a name
    /// that never does.
    pub(crate) double_quoted_strings: bool,
    /// `#` starts a comment that runs to the end of the line.
    pub(crate) hash_comments: bool,
    /// `--` starts a comment only when a blank or control character follows
    /// it, so `1--1` is arithmetic.
    pub(crate) dash_comment_needs_space: bool,
    /// `E'...'` is a string that obeys backslashes whatever `backslash_escapes`
    /// says.
    pub(crate) e_strings: bool,
    /// `[...]` is a quoted name, with no escape inside it.
    pub(crate) bracket_names: bool,
    /// A `/*` inside a block comment opens another, and the comment ends only
    /// when every one is closed, so `/* /* */ ; */` is one comment.
    pub(crate) nested_comments: bool,
}

impl SplitRules {
    pub(crate) const STANDARD: Self = Self {
        dollar_quotes: false,
        backslash_escapes: false,
        double_quoted_strings: false,
        hash_comments: false,
        dash_comment_needs_space: false,
        e_strings: false,
        bracket_names: false,
        nested_comments: false,
    };
}

impl Default for SplitRules {
    fn default() -> Self {
        Self::STANDARD
    }
}

/// One stretch between `;`s, with where its statement lies in it.
struct Segment {
    /// From just after the previous `;` to just after this one, so that a
    /// cursor right behind the `;` still belongs to the statement before it.
    span: Range<usize>,
    /// First to last significant byte, `None` when only blanks and comments.
    statement: Option<Range<usize>>,
}

/// The statement `offset` is in: the one it touches, or when it stands in the
/// blank space between two, the one before -- somebody who typed `;` and moved
/// on a line meant to run what they just wrote.
pub(crate) fn statement_at(sql: &str, offset: usize, rules: SplitRules) -> Option<Range<usize>> {
    let segments = segments(sql, rules);
    let offset = offset.min(sql.len());
    let index = segments
        .iter()
        .position(|segment| offset <= segment.span.end)?;

    if let Some(statement) = &segments[index].statement {
        let previous = index.checked_sub(1).and_then(|previous| {
            let previous = &segments[previous];
            let gap = sql.get(previous.span.end..offset)?;
            // Still on the line the previous statement ended on.
            (offset < statement.start && !gap.contains('\n')).then(|| previous.statement.clone())
        });
        return previous.flatten().or_else(|| Some(statement.clone()));
    }
    segments[..index]
        .iter()
        .rev()
        .find_map(|segment| segment.statement.clone())
        .or_else(|| {
            segments[index + 1..]
                .iter()
                .find_map(|segment| segment.statement.clone())
        })
}

/// Every statement in `sql`, in order, with blanks and comments between them
/// left out. What a guard that has to look at *all* of a buffer reads, where
/// `statement_at` answers for the one under a cursor.
pub(crate) fn statement_ranges(sql: &str, rules: SplitRules) -> Vec<Range<usize>> {
    segments(sql, rules)
        .into_iter()
        .filter_map(|segment| segment.statement)
        .collect()
}

fn segments(sql: &str, rules: SplitRules) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut segment_start = 0;
    let mut statement: Option<Range<usize>> = None;
    for token in tokens(sql, rules) {
        match token.kind {
            TokenKind::Semicolon => {
                segments.push(Segment {
                    span: segment_start..token.range.end,
                    statement: statement.take(),
                });
                segment_start = token.range.end;
            }
            TokenKind::Comment | TokenKind::Blank => {}
            TokenKind::Word | TokenKind::Quoted | TokenKind::Symbol => {
                statement.get_or_insert(token.range.clone()).end = token.range.end;
            }
        }
    }
    segments.push(Segment {
        span: segment_start..sql.len(),
        statement,
    });
    segments
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// A keyword, a name or a number.
    Word,
    /// A string, a quoted name or a dollar-quoted body, quotes included.
    Quoted,
    Comment,
    Blank,
    Semicolon,
    /// Any other single character: punctuation and operators.
    Symbol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) range: Range<usize>,
}

/// Reads `sql` the way the engine's lexer does, token by token, without a gap
/// or an overlap between them.
///
/// The one reading both the splitter and the write classifier stand on: where
/// they disagreed about what a comment or a string was, a write could sit in
/// text one of them took for quoted.
pub(crate) fn tokens(sql: &str, rules: SplitRules) -> Tokens<'_> {
    Tokens {
        bytes: sql.as_bytes(),
        rules,
        index: 0,
    }
}

pub(crate) struct Tokens<'a> {
    bytes: &'a [u8],
    rules: SplitRules,
    index: usize,
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let start = self.index;
        let (kind, end) = self.read(start)?;
        self.index = end;
        Some(Token {
            kind,
            range: start..end,
        })
    }
}

impl Tokens<'_> {
    // Every byte this reacts to is ASCII, and no byte of a multi-byte
    // character is, so a token always ends on a character boundary.
    fn read(&self, start: usize) -> Option<(TokenKind, usize)> {
        let bytes = self.bytes;
        let rules = self.rules;
        let byte = *bytes.get(start)?;
        let next = bytes.get(start + 1).copied();
        Some(match byte {
            b';' => (TokenKind::Semicolon, start + 1),
            b'-' if next == Some(b'-')
                && (!rules.dash_comment_needs_space
                    || bytes.get(start + 2).is_none_or(|after| {
                        after.is_ascii_whitespace() || after.is_ascii_control()
                    })) =>
            {
                (TokenKind::Comment, end_of_line(bytes, start))
            }
            b'#' if rules.hash_comments => (TokenKind::Comment, end_of_line(bytes, start)),
            b'/' if next == Some(b'*') => (
                TokenKind::Comment,
                end_of_block_comment(bytes, start, rules.nested_comments),
            ),
            b'\'' => (
                TokenKind::Quoted,
                end_of_quoted(bytes, start, rules.backslash_escapes),
            ),
            b'"' => (
                TokenKind::Quoted,
                end_of_quoted(
                    bytes,
                    start,
                    rules.backslash_escapes && rules.double_quoted_strings,
                ),
            ),
            b'`' => (TokenKind::Quoted, end_of_quoted(bytes, start, false)),
            b'[' if rules.bracket_names => (
                TokenKind::Quoted,
                find(bytes, start + 1, b"]").map_or(bytes.len(), |close| close + 1),
            ),
            b'$' if rules.dollar_quotes => match dollar_quote_end(bytes, start) {
                Some(end) => (TokenKind::Quoted, end),
                None => (TokenKind::Symbol, start + 1),
            },
            byte if byte.is_ascii_whitespace() => (TokenKind::Blank, start + 1),
            byte if is_word_byte(byte) => {
                let length = bytes[start..]
                    .iter()
                    .position(|&byte| !(is_word_byte(byte) || byte == b'$'))
                    .unwrap_or(bytes.len() - start);
                let end = start + length;
                // A lone `E` right before a quote, not the tail of a longer name.
                if rules.e_strings
                    && length == 1
                    && byte.eq_ignore_ascii_case(&b'e')
                    && bytes.get(end) == Some(&b'\'')
                {
                    (TokenKind::Quoted, end_of_quoted(bytes, end, true))
                } else {
                    (TokenKind::Word, end)
                }
            }
            _ => (TokenKind::Symbol, start + 1),
        })
    }
}

/// Up to, not past, the line break: it is still whitespace.
fn end_of_line(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |length| from + length)
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| from + position)
}

/// Where the block comment opened at `open` ends, just past its closing `*/`,
/// or the end of the input when it never closes.
///
/// The `*` of the opening is not the `*` of a close, so `/*/` is still open.
fn end_of_block_comment(bytes: &[u8], open: usize, nested: bool) -> usize {
    if !nested {
        return find(bytes, open + 2, b"*/").map_or(bytes.len(), |close| close + 2);
    }
    let mut depth = 1_usize;
    let mut index = open + 2;
    while let Some(pair) = bytes.get(index..index + 2) {
        match pair {
            b"/*" => {
                depth += 1;
                index += 2;
            }
            b"*/" => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

/// Where a string or quoted name opened at `open` ends, just past its closing
/// quote, or the end of the input when it never closes. A doubled quote is one
/// quote inside the string, which keeps whatever follows it -- a backslash
/// included -- inside the string too.
fn end_of_quoted(bytes: &[u8], open: usize, backslash_escapes: bool) -> usize {
    let quote = bytes[open];
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if backslash_escapes => index += 2,
            byte if byte == quote && bytes.get(index + 1) == Some(&quote) => index += 2,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// Where the dollar-quoted string opened at `open` ends, `None` if the `$` is
/// not the start of one -- `$1` is a parameter, and `a$b$` is a name.
///
/// A tag is made of what a name is made of, the bytes of a multi-byte
/// character included: `$é$ ... $é$` is a string.
fn dollar_quote_end(bytes: &[u8], open: usize) -> Option<usize> {
    let part_of_a_name = |byte: u8| is_word_byte(byte) || byte == b'$';
    if open > 0 && part_of_a_name(bytes[open - 1]) {
        return None;
    }
    let tag_end = open
        + 1
        + bytes[open + 1..]
            .iter()
            .position(|&byte| !is_word_byte(byte))?;
    if bytes[tag_end] != b'$' {
        return None;
    }
    // A tag cannot start with a digit, which is what keeps `$1$` from being one.
    if bytes.get(open + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let tag = &bytes[open..=tag_end];
    Some(find(bytes, tag_end + 1, tag).map_or(bytes.len(), |close| close + tag.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: SplitRules = SplitRules::STANDARD;
    const DOLLARS: SplitRules = SplitRules {
        dollar_quotes: true,
        ..SplitRules::STANDARD
    };
    const BACKSLASHES: SplitRules = SplitRules {
        backslash_escapes: true,
        ..SplitRules::STANDARD
    };
    const DOUBLE_QUOTED_STRINGS: SplitRules = SplitRules {
        backslash_escapes: true,
        double_quoted_strings: true,
        ..SplitRules::STANDARD
    };
    const HASHES: SplitRules = SplitRules {
        hash_comments: true,
        ..SplitRules::STANDARD
    };
    const DASH_NEEDS_SPACE: SplitRules = SplitRules {
        dash_comment_needs_space: true,
        ..SplitRules::STANDARD
    };
    const E_STRINGS: SplitRules = SplitRules {
        e_strings: true,
        ..SplitRules::STANDARD
    };
    const BRACKETS: SplitRules = SplitRules {
        bracket_names: true,
        ..SplitRules::STANDARD
    };
    const NESTED: SplitRules = SplitRules {
        nested_comments: true,
        dollar_quotes: true,
        ..SplitRules::STANDARD
    };

    fn statements(sql: &str, rules: SplitRules) -> Vec<&str> {
        statement_ranges(sql, rules)
            .into_iter()
            .map(|range| &sql[range])
            .collect()
    }

    /// The statement the cursor is at, where `|` marks the cursor.
    fn at(marked: &str, rules: SplitRules) -> Option<String> {
        let offset = marked.find('|').expect("a cursor marker");
        let sql = marked.replacen('|', "", 1);
        statement_at(&sql, offset, rules).map(|range| sql[range].to_string())
    }

    #[test]
    fn statements_split_on_semicolons_and_lose_the_terminator() {
        assert_eq!(
            statements("select 1; select 2;\nselect 3", PLAIN),
            ["select 1", "select 2", "select 3"]
        );
    }

    #[test]
    fn an_empty_buffer_has_no_statements() {
        assert!(statements("", PLAIN).is_empty());
        assert!(statements("  \n\t ", PLAIN).is_empty());
    }

    #[test]
    fn empty_statements_between_semicolons_are_skipped() {
        assert_eq!(statements(";;select 1;;  ;", PLAIN), ["select 1"]);
    }

    #[test]
    fn a_semicolon_in_a_single_quoted_string_does_not_split() {
        assert_eq!(
            statements("select 'a;b'; select 2", PLAIN),
            ["select 'a;b'", "select 2"]
        );
    }

    #[test]
    fn a_doubled_quote_stays_inside_its_string() {
        assert_eq!(
            statements("select 'it''s; fine'; select 2", PLAIN),
            ["select 'it''s; fine'", "select 2"]
        );
    }

    #[test]
    fn a_semicolon_in_a_quoted_name_does_not_split() {
        assert_eq!(
            statements(r#"select "a;b" from "t;u"; select `x;y`; select 3"#, PLAIN),
            [r#"select "a;b" from "t;u""#, "select `x;y`", "select 3"]
        );
    }

    #[test]
    fn a_semicolon_in_a_line_comment_does_not_split() {
        assert_eq!(
            statements("select 1 -- one; two\n, 2; select 3", PLAIN),
            ["select 1 -- one; two\n, 2", "select 3"]
        );
    }

    #[test]
    fn a_semicolon_in_a_block_comment_does_not_split() {
        assert_eq!(
            statements("select /* a; b */ 1; select 2", PLAIN),
            ["select /* a; b */ 1", "select 2"]
        );
    }

    #[test]
    fn a_quote_inside_a_comment_does_not_open_a_string() {
        assert_eq!(
            statements("select 1 -- it's\n; select 2", PLAIN),
            ["select 1", "select 2"]
        );
        assert_eq!(
            statements("select 1 /* it's */; select 2", PLAIN),
            ["select 1", "select 2"]
        );
    }

    #[test]
    fn comment_markers_inside_a_string_are_text() {
        assert_eq!(
            statements("select '-- not; a comment'; select '/* nor; this'", PLAIN),
            ["select '-- not; a comment'", "select '/* nor; this'"]
        );
    }

    #[test]
    fn a_stretch_of_only_comments_is_not_a_statement() {
        assert_eq!(
            statements("-- nothing\n/* here */;\nselect 1", PLAIN),
            ["select 1"]
        );
    }

    #[test]
    fn comments_around_a_statement_are_left_out_of_it() {
        assert_eq!(
            statements("-- all users\nselect 1 -- trailing\n;", PLAIN),
            ["select 1"]
        );
    }

    #[test]
    fn dollar_quoted_bodies_hold_their_semicolons_where_the_engine_has_them() {
        let sql = "create function f() returns int as $$ select 1; select 2; $$; select 3";
        assert_eq!(
            statements(sql, DOLLARS),
            [
                "create function f() returns int as $$ select 1; select 2; $$",
                "select 3"
            ]
        );
        // Without the rule `$$` is two stray characters.
        assert_eq!(statements(sql, PLAIN).len(), 4);
    }

    #[test]
    fn a_dollar_tag_closes_only_on_its_own_tag() {
        let sql = "do $body$ select '$$'; $x$; $body$; select 2";
        assert_eq!(
            statements(sql, DOLLARS),
            ["do $body$ select '$$'; $x$; $body$", "select 2"]
        );
    }

    #[test]
    fn a_numbered_parameter_is_not_a_dollar_quote() {
        assert_eq!(
            statements("select $1; select $2", DOLLARS),
            ["select $1", "select $2"]
        );
    }

    #[test]
    fn a_dollar_inside_a_name_does_not_open_a_quote() {
        assert_eq!(
            statements("select a$b$c; select 2", DOLLARS),
            ["select a$b$c", "select 2"]
        );
    }

    #[test]
    fn an_unterminated_dollar_quote_swallows_the_rest() {
        assert_eq!(statements("select $$ a; b", DOLLARS), ["select $$ a; b"]);
    }

    #[test]
    fn a_backslash_escapes_a_quote_only_where_the_engine_says_so() {
        let sql = r"select 'it\'s; fine'; select 2";
        assert_eq!(
            statements(sql, BACKSLASHES),
            [r"select 'it\'s; fine'", "select 2"]
        );
        // Standard strings end at the second quote, so the `;` is outside.
        assert_eq!(statements(sql, PLAIN)[0], r"select 'it\'s");
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end() {
        assert_eq!(statements("select 'a; b", PLAIN), ["select 'a; b"]);
    }

    /// Inside a backtick name a backslash is a backslash, even where it escapes
    /// a quote in a string.
    #[test]
    fn a_backslash_means_nothing_inside_a_backtick_name() {
        assert_eq!(
            statements(r"select `a\`; select 2", BACKSLASHES),
            [r"select `a\`", "select 2"]
        );
    }

    #[test]
    fn a_double_quote_holds_a_string_or_a_name_as_the_engine_says() {
        let sql = r#"select "a\"; select 2""#;
        assert_eq!(statements(sql, DOUBLE_QUOTED_STRINGS).len(), 1);
        // A name ends at the second quote, whatever precedes it.
        assert_eq!(
            statements(sql, BACKSLASHES),
            [r#"select "a\""#, r#"select 2""#]
        );
    }

    #[test]
    fn a_hash_starts_a_comment_only_where_the_engine_says_so() {
        assert_eq!(
            statements("select 1 # it's; here\n; select 2", HASHES),
            ["select 1", "select 2"]
        );
        assert_eq!(
            statements("select '#'; select 2", HASHES),
            ["select '#'", "select 2"]
        );
        assert_eq!(statements("select 1 # a; b", PLAIN), ["select 1 # a", "b"]);
    }

    #[test]
    fn a_double_dash_needs_a_blank_after_it_only_where_the_engine_says_so() {
        // Arithmetic, so the `;` ends the statement.
        assert_eq!(
            statements("select 1--1; select 2", DASH_NEEDS_SPACE),
            ["select 1--1", "select 2"]
        );
        assert_eq!(statements("select 1--1; select 2", PLAIN), ["select 1"]);
        for comment in [
            "select 1-- x; y\n; select 2",
            "select 1--\n; select 2",
            "select 1--",
        ] {
            assert_eq!(
                statements(comment, DASH_NEEDS_SPACE)[0],
                "select 1",
                "{comment:?}"
            );
        }
    }

    #[test]
    fn an_e_string_obeys_backslashes_but_a_name_ending_in_e_does_not_start_one() {
        assert_eq!(
            statements(r"select E'it\'s; fine'; select 2", E_STRINGS),
            [r"select E'it\'s; fine'", "select 2"]
        );
        assert_eq!(
            statements(r"select e'a\'; b'; select 2", E_STRINGS)[0],
            r"select e'a\'; b'"
        );
        // A plain string keeps its standard reading beside it.
        assert_eq!(
            statements(r"select 'a\'; select 2", E_STRINGS),
            [r"select 'a\'", "select 2"]
        );
        assert_eq!(
            statements(r"select name'a\'; select 2", E_STRINGS),
            [r"select name'a\'", "select 2"]
        );
        assert_eq!(
            statements(r"select E'a\'; select 2", PLAIN),
            [r"select E'a\'", "select 2"]
        );
    }

    /// A doubled quote is a quote in the string, so what follows it is still in
    /// the string -- including the `\'` that an `E` string honours.
    #[test]
    fn a_doubled_quote_does_not_end_an_e_string() {
        assert_eq!(
            statements(r"select E'a''b\''; select 2", E_STRINGS),
            [r"select E'a''b\''", "select 2"]
        );
    }

    /// Where the rule is on, a block comment ends only when every `/*` in it
    /// has been closed. The first `*/` is not the end of this one, so the quote
    /// after it is inside the comment and the `DELETE` is a statement.
    #[test]
    fn a_nested_block_comment_ends_at_its_outermost_close() {
        let sql = "SELECT 1 /* /* */ ' */ ; DELETE FROM t; --'";
        assert_eq!(
            statements(sql, NESTED),
            ["SELECT 1", "DELETE FROM t"],
            "the comment is one comment"
        );
        // Without the rule the first `*/` closes it and the quote swallows the
        // rest of the line.
        assert_eq!(statements(sql, PLAIN).len(), 1);

        assert_eq!(
            statements("select /* a /* b; */ c; */ 1; select 2", NESTED),
            ["select /* a /* b; */ c; */ 1", "select 2"]
        );
    }

    #[test]
    fn a_nested_block_comment_that_never_closes_runs_to_the_end() {
        assert_eq!(
            statements("select 1 /* a /* b */ ; select 2", NESTED),
            ["select 1"]
        );
        assert_eq!(statements("select 1 /*", NESTED), ["select 1"]);
    }

    /// `/*/` opens a comment and does not close it: the `*` is spent on the
    /// opening, so the `/` after it is nothing.
    #[test]
    fn a_slash_star_slash_opens_a_comment_without_closing_it() {
        assert_eq!(
            statements("select 1 /*/ ; */ ; select 2", NESTED),
            ["select 1", "select 2"]
        );
        assert_eq!(
            statements("select 1 /**/; select 2", NESTED),
            ["select 1", "select 2"]
        );
        assert_eq!(
            statements("select 1 /*/*/ ; select 2", NESTED),
            ["select 1"]
        );
    }

    #[test]
    fn a_dollar_tag_may_hold_any_byte_the_engine_allows_in_a_name() {
        // The tag is `$é$`: the quote inside is text, so the `DELETE` is a
        // statement of its own.
        let sql = "SELECT $é$ ' $é$; DELETE FROM t; --'";
        assert_eq!(
            statements(sql, DOLLARS),
            ["SELECT $é$ ' $é$", "DELETE FROM t"]
        );
        assert_eq!(
            statements("select $日本$ a; b $日本$; select 2", DOLLARS),
            ["select $日本$ a; b $日本$", "select 2"]
        );
        // It closes only on its own tag, and a tag cannot start with a digit.
        assert_eq!(
            statements("select $é$ a; $ü$ b; $é$; select 2", DOLLARS),
            ["select $é$ a; $ü$ b; $é$", "select 2"]
        );
        assert_eq!(
            statements("select $1é$ a; b", DOLLARS),
            ["select $1é$ a", "b"]
        );
    }

    #[test]
    fn a_bracketed_name_ends_at_the_bracket_and_has_no_escapes() {
        assert_eq!(
            statements("select [a;b] from [t'u]; select 2", BRACKETS),
            ["select [a;b] from [t'u]", "select 2"]
        );
        assert_eq!(statements("select [a; b", BRACKETS), ["select [a; b"]);
        assert_eq!(statements("select [a;b]", PLAIN), ["select [a", "b]"]);
    }

    #[test]
    fn tokens_cover_the_input_without_gaps_on_character_boundaries() {
        let sql = "select 'é;ü' /* ö */ , \"ß\" -- ñ\n; E'a\\'' [x] $$ y $$ # z\n`k` 1$2 é \
                   /* /* é */ ö */ $é$ ü ; $é$ $ß$ $";
        for rules in [
            PLAIN,
            DOLLARS,
            BACKSLASHES,
            DOUBLE_QUOTED_STRINGS,
            HASHES,
            DASH_NEEDS_SPACE,
            E_STRINGS,
            BRACKETS,
            NESTED,
        ] {
            let mut expected_start = 0;
            for token in tokens(sql, rules) {
                assert_eq!(token.range.start, expected_start, "{rules:?}");
                assert!(sql.is_char_boundary(token.range.start), "{rules:?}");
                assert!(sql.is_char_boundary(token.range.end), "{rules:?}");
                expected_start = token.range.end;
            }
            assert_eq!(expected_start, sql.len(), "{rules:?}");
        }
    }

    #[test]
    fn a_doubled_quote_and_a_word_with_a_dollar_are_each_one_token() {
        let kinds = |sql: &str, rules| {
            tokens(sql, rules)
                .map(|token| (token.kind, sql[token.range].to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds("'it''s'", PLAIN),
            [(TokenKind::Quoted, "'it''s'".to_string())]
        );
        assert_eq!(
            kinds("a$b 1", PLAIN),
            [
                (TokenKind::Word, "a$b".to_string()),
                (TokenKind::Blank, " ".to_string()),
                (TokenKind::Word, "1".to_string())
            ]
        );
    }

    #[test]
    fn multibyte_text_is_cut_on_character_boundaries() {
        assert_eq!(
            statements("select 'é;ü'; select 'ß'", PLAIN),
            ["select 'é;ü'", "select 'ß'"]
        );
    }

    #[test]
    fn the_cursor_inside_a_statement_picks_it() {
        assert_eq!(
            at("select 1; select |2; select 3", PLAIN).as_deref(),
            Some("select 2")
        );
    }

    #[test]
    fn the_cursor_at_either_end_of_a_statement_picks_it() {
        assert_eq!(
            at("|select 1; select 2", PLAIN).as_deref(),
            Some("select 1")
        );
        assert_eq!(
            at("select 1|; select 2", PLAIN).as_deref(),
            Some("select 1")
        );
    }

    #[test]
    fn the_cursor_just_after_a_semicolon_still_means_the_statement_before_it() {
        assert_eq!(
            at("select 1;| select 2", PLAIN).as_deref(),
            Some("select 1")
        );
    }

    #[test]
    fn the_cursor_after_the_semicolon_and_some_spaces_still_means_that_line() {
        assert_eq!(
            at("select 1;   |\nselect 2", PLAIN).as_deref(),
            Some("select 1")
        );
    }

    #[test]
    fn the_cursor_at_the_start_of_the_next_line_means_the_next_statement() {
        assert_eq!(
            at("select 1;\n|select 2", PLAIN).as_deref(),
            Some("select 2")
        );
    }

    #[test]
    fn the_cursor_in_trailing_blank_space_falls_back_to_the_last_statement() {
        assert_eq!(
            at("select 1;\nselect 2;\n\n|", PLAIN).as_deref(),
            Some("select 2")
        );
    }

    #[test]
    fn the_cursor_in_a_comment_between_statements_falls_back_to_the_one_before() {
        assert_eq!(
            at("select 1;\n-- the next|\n;select 2", PLAIN).as_deref(),
            Some("select 1")
        );
    }

    #[test]
    fn the_cursor_before_the_first_statement_falls_forward_to_it() {
        assert_eq!(
            at("-- header|\n;\nselect 1", PLAIN).as_deref(),
            Some("select 1")
        );
    }

    #[test]
    fn a_buffer_with_no_statement_has_nothing_at_the_cursor() {
        assert_eq!(at("|", PLAIN), None);
        assert_eq!(at("-- only| a comment", PLAIN), None);
    }

    #[test]
    fn an_offset_past_the_end_is_the_end() {
        assert_eq!(
            statement_at("select 1; select 2", 10_000, PLAIN).map(|range| range.end),
            Some(18)
        );
    }

    #[test]
    fn a_cursor_in_the_middle_of_a_string_with_a_semicolon_picks_the_whole_statement() {
        assert_eq!(
            at("select 'a;|b'; select 2", PLAIN).as_deref(),
            Some("select 'a;b'")
        );
    }
}
