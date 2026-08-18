use postgres::SimpleQueryMessage;

/// The script that fetches one page, read-only, server-side.
///
/// Every part of it earns its place:
///
/// - `BEGIN TRANSACTION READ ONLY` is the read-only guarantee. Session-level
///   settings are undone by one `SET SESSION …` in the user's own SQL; nothing
///   inside a transaction can lift its read-only mode.
/// - `NO SCROLL` says the cursor only ever moves forward, which lets PostgreSQL
///   avoid materialising what it has already passed.
/// - `MOVE FORWARD` skips to the offset in the server, so a page 400 rows in
///   still sends 200 rows over the wire.
/// - `FETCH FORWARD limit + 1` asks for one row past the page, which answers
///   "is there more" without a `COUNT(*)`.
/// - `ROLLBACK` rather than `COMMIT`: the transaction did nothing to keep, and
///   rolling back is the honest end for one that was never allowed to write.
pub fn cursor_script(sql: &str, limit: u32, offset: u64) -> String {
    let fetch = u64::from(limit).saturating_add(1);
    format!(
        "BEGIN TRANSACTION READ ONLY;\n\
         DECLARE zode_page NO SCROLL CURSOR FOR {sql};\n\
         MOVE FORWARD {offset} IN zode_page;\n\
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
/// The extra row asked for by `cursor_script` is dropped here rather than
/// shown -- it exists only to answer `truncated`.
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
        let script = cursor_script("SELECT 1", 200, 0);
        assert!(
            script.starts_with("BEGIN TRANSACTION READ ONLY;"),
            "the transaction must open read-only before the user's SQL is named: {script}"
        );
        assert!(
            script.trim_end().ends_with("ROLLBACK;"),
            "and must be rolled back, never committed: {script}"
        );
        assert!(
            script.find("BEGIN TRANSACTION READ ONLY").unwrap() < script.find("SELECT 1").unwrap(),
            "the user's SQL must never run before the transaction is read-only"
        );
    }

    /// One row past the page, so `truncated` costs no `COUNT(*)`.
    #[test]
    fn the_script_asks_for_one_row_more_than_the_page() {
        assert!(cursor_script("SELECT 1", 200, 0).contains("FETCH FORWARD 201 "));
        assert!(cursor_script("SELECT 1", 1, 0).contains("FETCH FORWARD 2 "));
    }

    /// Paging happens in the server. Skipping in the client would send every
    /// skipped row over the wire -- which is the thing this design exists to
    /// avoid.
    #[test]
    fn the_offset_is_skipped_in_the_server() {
        assert!(cursor_script("SELECT 1", 200, 400).contains("MOVE FORWARD 400 IN zode_page"));
    }

    /// A cursor takes any statement that produces rows, including the two
    /// people reach for when a query behaves strangely.
    #[test]
    fn a_statement_that_is_not_a_subquery_still_pages() {
        for sql in ["EXPLAIN SELECT 1", "SHOW search_path", "VALUES (1), (2)"] {
            let script = cursor_script(sql, 10, 0);
            assert!(
                script.contains(&format!("CURSOR FOR {sql};")),
                "`{sql}` must be handed to the cursor untouched"
            );
        }
    }

    /// `limit + 1` never overflows into a script that asks for nothing.
    #[test]
    fn a_maximal_limit_does_not_wrap() {
        let script = cursor_script("SELECT 1", u32::MAX, 0);
        assert!(script.contains(&format!("FETCH FORWARD {} ", u64::from(u32::MAX) + 1)));
    }
}
