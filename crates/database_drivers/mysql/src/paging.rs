//! Paging and the read-only guarantee, as one script.
//!
//! MySQL has no cursor a client can `FETCH` from outside a stored program, so
//! unlike the PostgreSQL driver this cannot page an arbitrary statement. It
//! wraps instead -- and says so plainly when it cannot.

/// Statements a derived table will not accept.
///
/// `EXPLAIN`, `SHOW`, `DESCRIBE` and the rest produce rows but cannot be
/// wrapped in `SELECT * FROM ( … )`. They are run unwrapped and their rows
/// truncated in the driver, which is safe only because none of them returns a
/// large result: MySQL has no cursor to page them with, and pretending
/// otherwise would be worse than saying so.
const UNWRAPPABLE: &[&str] = &[
    "explain", "show", "describe", "desc", "analyze", "check", "help", "with",
];

pub fn is_wrappable(sql: &str) -> bool {
    let head = sql
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !UNWRAPPABLE.contains(&head.as_str())
}

/// The user's statement, limited to one page.
///
/// `limit + 1` so `truncated` is answered without a `COUNT(*)`. The alias is
/// required by MySQL -- a derived table without one is a syntax error -- and is
/// named distinctively so it cannot collide with a table in the query.
pub fn paged(sql: &str, limit: u32, offset: u64) -> String {
    let fetch = u64::from(limit).saturating_add(1);
    if is_wrappable(sql) {
        format!("SELECT * FROM (\n{sql}\n) AS zode_page LIMIT {fetch} OFFSET {offset}")
    } else {
        sql.to_string()
    }
}

/// Every statement runs inside its own read-only transaction.
///
/// `SET SESSION TRANSACTION READ ONLY` would be undone by one
/// `SET SESSION TRANSACTION READ WRITE` typed into the scratch buffer. Nothing
/// inside a transaction can lift its read-only mode, so that is where the
/// guarantee lives -- the same conclusion the PostgreSQL driver reached.
pub const BEGIN_READ_ONLY: &str = "START TRANSACTION READ ONLY";
pub const END_READ_ONLY: &str = "ROLLBACK";

#[cfg(test)]
mod tests {
    use super::*;

    /// One row past the page, so `truncated` costs no `COUNT(*)`.
    #[test]
    fn a_page_asks_for_one_row_more_than_it_shows() {
        assert!(paged("SELECT 1", 200, 0).contains("LIMIT 201 OFFSET 0"));
        assert!(paged("SELECT 1", 1, 40).contains("LIMIT 2 OFFSET 40"));
    }

    /// A derived table needs an alias; without one MySQL rejects the statement
    /// outright, so every wrapped query would fail.
    #[test]
    fn the_derived_table_is_always_aliased() {
        assert!(paged("SELECT 1", 10, 0).contains(") AS zode_page "));
    }

    /// `EXPLAIN` and `SHOW` are exactly what someone reaches for when a query
    /// behaves strangely, and neither survives being wrapped.
    #[test]
    fn statements_that_cannot_be_wrapped_are_left_alone() {
        for sql in ["EXPLAIN SELECT 1", "SHOW TABLES", "DESCRIBE users"] {
            assert!(!is_wrappable(sql), "`{sql}` must not be wrapped");
            assert_eq!(paged(sql, 10, 0), sql, "`{sql}` must be run untouched");
        }
    }

    /// Case and leading whitespace are how this check gets fooled.
    #[test]
    fn the_check_is_not_fooled_by_case_or_indentation() {
        assert!(!is_wrappable("   ExPlAiN SELECT 1"));
        assert!(is_wrappable("\n  select 1"));
    }

    /// A CTE is not wrappable in every MySQL version that supports one, and
    /// guessing wrong turns a working query into a syntax error.
    #[test]
    fn a_cte_is_left_alone() {
        assert!(!is_wrappable("WITH t AS (SELECT 1) SELECT * FROM t"));
    }

    #[test]
    fn a_maximal_limit_does_not_wrap_around() {
        assert!(
            paged("SELECT 1", u32::MAX, 0).contains(&format!("LIMIT {}", u64::from(u32::MAX) + 1))
        );
    }
}
