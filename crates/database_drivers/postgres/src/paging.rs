use postgres::SimpleQueryMessage;

/// Opens the transaction every page runs in, as a statement of its own.
///
/// `BEGIN TRANSACTION READ ONLY` is the read-only guarantee. Session-level
/// settings are undone by one `SET SESSION …` in the user's own SQL; nothing
/// inside a transaction can lift its read-only mode.
pub const BEGIN_READ_ONLY: &str = "BEGIN TRANSACTION READ ONLY";

/// The user's statement as the body of a cursor, and nothing else.
///
/// Sent over the extended protocol, which carries exactly one statement: a
/// string holding two is refused by the server before anything runs. That is
/// what keeps `SELECT 1; COMMIT; UPDATE …` from ending the read-only
/// transaction and writing -- the simple protocol would have run all of it, and
/// no amount of care in building a script around the user's text can make it
/// safe to hand that text to a protocol that accepts several statements.
///
/// Nothing is appended: a trailing `-- comment` of the user's would swallow
/// whatever followed it.
///
/// `NO SCROLL` says the cursor only ever moves forward, which lets PostgreSQL
/// avoid materialising what it has already passed.
pub fn declare_cursor(sql: &str) -> String {
    format!("DECLARE zode_page NO SCROLL CURSOR FOR {sql}")
}

/// Everything after the cursor exists, and nothing the user typed.
///
/// - `MOVE FORWARD` skips to the offset in the server, so a page 400 rows in
///   still sends 200 rows over the wire.
/// - `FETCH FORWARD limit + 1` asks for one row past the page, which answers
///   "is there more" without a `COUNT(*)`.
/// - `ROLLBACK` rather than `COMMIT`: the transaction did nothing to keep, and
///   rolling back is the honest end for one that was never allowed to write.
pub fn page_script(limit: u32, offset: u64) -> String {
    let fetch = u64::from(limit).saturating_add(1);
    format!(
        "MOVE FORWARD {offset} IN zode_page;\n\
         FETCH FORWARD {fetch} FROM zode_page;\n\
         CLOSE zode_page;\n\
         ROLLBACK;"
    )
}

/// Column names, rows and whether there is more, from the script's replies.
///
/// A `simple_query` of a multi-statement script answers for every statement in
/// it; only the `FETCH` produces rows, so everything before it is skipped by
/// looking for the first row description that arrives.
///
/// The extra row asked for by `page_script` is dropped here rather than shown
/// -- it exists only to answer `truncated`.
pub fn collect(
    messages: &[SimpleQueryMessage],
    limit: u32,
) -> (Vec<String>, Vec<Vec<Option<String>>>, bool) {
    let mut columns = Vec::new();
    let mut rows = Vec::new();

    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(description) => {
                columns = description
                    .iter()
                    .map(|column| column.name().to_string())
                    .collect();
            }
            SimpleQueryMessage::Row(row) => {
                rows.push(
                    (0..row.len())
                        .map(|index| row.get(index).map(str::to_string))
                        .collect(),
                );
            }
            _ => {}
        }
    }

    let truncated = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    (columns, rows, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read-only guarantee is a property of this string. If `BEGIN
    /// TRANSACTION READ ONLY` ever leaves it, every write the user types
    /// succeeds -- so it is asserted rather than assumed.
    #[test]
    fn every_page_runs_inside_a_read_only_transaction() {
        assert_eq!(BEGIN_READ_ONLY, "BEGIN TRANSACTION READ ONLY");
        assert!(
            page_script(200, 0).trim_end().ends_with("ROLLBACK;"),
            "and must be rolled back, never committed"
        );
        assert!(
            !declare_cursor("SELECT 1").contains("READ ONLY"),
            "the transaction is opened by a statement of its own, before the user's SQL is named"
        );
    }

    /// What makes the extended protocol's one-statement rule bite: the user's
    /// text is the end of the statement, with nothing of ours after it that a
    /// comment or a stray quote could swallow or a `;` could be hidden behind.
    #[test]
    fn nothing_follows_the_users_statement() {
        for sql in [
            "SELECT 1",
            "SELECT 1 -- note",
            "SELECT 1;",
            "VALUES (1), (2)",
        ] {
            let declared = declare_cursor(sql);
            assert!(
                declared.ends_with(sql),
                "`{sql}` must be handed to the cursor untouched and last: {declared}"
            );
            assert!(declared.starts_with("DECLARE zode_page NO SCROLL CURSOR FOR "));
        }
    }

    /// The text after the cursor is built from two numbers and fixed words.
    #[test]
    fn the_page_script_holds_no_text_the_user_typed() {
        let script = page_script(10, 5);
        assert_eq!(
            script,
            "MOVE FORWARD 5 IN zode_page;\nFETCH FORWARD 11 FROM zode_page;\nCLOSE zode_page;\nROLLBACK;"
        );
    }

    /// One row past the page, so `truncated` costs no `COUNT(*)`.
    #[test]
    fn the_script_asks_for_one_row_more_than_the_page() {
        assert!(page_script(200, 0).contains("FETCH FORWARD 201 "));
        assert!(page_script(1, 0).contains("FETCH FORWARD 2 "));
    }

    /// Paging happens in the server. Skipping in the client would send every
    /// skipped row over the wire -- which is the thing this design exists to
    /// avoid.
    #[test]
    fn the_offset_is_skipped_in_the_server() {
        assert!(page_script(200, 400).contains("MOVE FORWARD 400 IN zode_page"));
    }

    /// `limit + 1` never overflows into a script that asks for nothing.
    #[test]
    fn a_maximal_limit_does_not_wrap() {
        let script = page_script(u32::MAX, 0);
        assert!(script.contains(&format!("FETCH FORWARD {} ", u64::from(u32::MAX) + 1)));
    }
}
