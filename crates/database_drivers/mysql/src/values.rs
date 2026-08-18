use database::protocol::{Cell, ErrorCode, ResponseError};
use mysql::consts::ColumnType;

/// One MySQL value, rendered for display.
///
/// The bytes are the server's own text (the driver asks for text protocol), so
/// `DECIMAL(38,10)` arrives with every digit. The *kind* comes from the column
/// type, which is the only thing that can tell `DECIMAL` from `VARCHAR` once
/// both are text.
pub fn cell(value: Option<&[u8]>, kind: ColumnType) -> Cell {
    let Some(bytes) = value else {
        return Cell::Null;
    };

    // Never the bytes themselves, and decided before any UTF-8 attempt: a BLOB
    // is not text, and a large one through a line-delimited JSON pipe is how a
    // driver stalls the editor reading it.
    if is_binary(kind) {
        return Cell::Binary {
            byte_len: bytes.len() as u64,
        };
    }

    let text = String::from_utf8_lossy(bytes).into_owned();
    match kind {
        ColumnType::MYSQL_TYPE_TINY
        | ColumnType::MYSQL_TYPE_SHORT
        | ColumnType::MYSQL_TYPE_LONG
        | ColumnType::MYSQL_TYPE_LONGLONG
        | ColumnType::MYSQL_TYPE_INT24
        | ColumnType::MYSQL_TYPE_FLOAT
        | ColumnType::MYSQL_TYPE_DOUBLE
        | ColumnType::MYSQL_TYPE_DECIMAL
        | ColumnType::MYSQL_TYPE_NEWDECIMAL
        | ColumnType::MYSQL_TYPE_YEAR => Cell::Number { value: text },
        ColumnType::MYSQL_TYPE_JSON => Cell::Json { value: text },
        ColumnType::MYSQL_TYPE_DATE
        | ColumnType::MYSQL_TYPE_NEWDATE
        | ColumnType::MYSQL_TYPE_TIME
        | ColumnType::MYSQL_TYPE_TIME2
        | ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_DATETIME2
        | ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_TIMESTAMP2 => Cell::Timestamp { value: text },
        // MySQL has no boolean: `BOOLEAN` is `TINYINT(1)`, and the wire cannot
        // tell one from a genuine tiny integer. Reported as the number it is
        // rather than guessed at -- `Cell::Bool` would be an invention, and a
        // grid showing `true` for a column holding 7 is worse than showing 7.
        _ => Cell::Text { value: text },
    }
}

fn is_binary(kind: ColumnType) -> bool {
    matches!(
        kind,
        ColumnType::MYSQL_TYPE_BLOB
            | ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_GEOMETRY
    )
}

/// MySQL's errors, sorted into the few kinds the UI reacts to differently.
pub fn error(error: mysql::Error) -> ResponseError {
    let detail = error.to_string();
    let code = match &error {
        mysql::Error::MySqlError(server) => match server.code {
            // 1290 ER_OPTION_PREVENTS_STATEMENT -- what a read-only transaction
            // answers, and the one the UI must not report as bad SQL.
            // 1792 ER_CANT_EXECUTE_IN_READ_ONLY_TRANSACTION says it outright.
            1290 | 1792 => ErrorCode::ReadOnly,
            1045 | 1044 | 1698 => ErrorCode::Authentication,
            // 1317 ER_QUERY_INTERRUPTED -- what KILL QUERY produces.
            1317 => ErrorCode::Cancelled,
            2002..=2013 => ErrorCode::Connection,
            _ => ErrorCode::Syntax,
        },
        mysql::Error::IoError(_) | mysql::Error::DriverError(_) => ErrorCode::Connection,
        _ => ErrorCode::Internal,
    };

    let message = match code {
        ErrorCode::ReadOnly => "this connection is read-only".to_string(),
        ErrorCode::Cancelled => "the query was cancelled".to_string(),
        _ => detail.clone(),
    };
    ResponseError::new(code, message).with_detail(detail)
}

/// What this engine wraps identifiers in, reported through `initialize`.
///
/// Backticks rather than double quotes: MySQL only accepts the latter with
/// `ANSI_QUOTES` set, which is not the default and is not this driver's to
/// change on a session the user also types into.
pub const IDENTIFIER_QUOTE: &str = "`";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_is_never_the_empty_string() {
        assert_eq!(cell(None, ColumnType::MYSQL_TYPE_VAR_STRING), Cell::Null);
        assert_eq!(
            cell(Some(b""), ColumnType::MYSQL_TYPE_VAR_STRING),
            Cell::Text {
                value: String::new()
            }
        );
    }

    /// The whole reason values cross as the server's own text.
    #[test]
    fn a_wide_decimal_keeps_every_digit() {
        let wide = b"123456789012345678901234567890.0000000001";
        assert_eq!(
            cell(Some(wide), ColumnType::MYSQL_TYPE_NEWDECIMAL),
            Cell::Number {
                value: String::from_utf8_lossy(wide).into_owned()
            }
        );
    }

    /// Decided before any UTF-8 attempt: a BLOB is not text, and lossy-decoding
    /// one would put replacement characters where bytes were.
    #[test]
    fn a_blob_reports_its_size_and_not_its_bytes() {
        assert_eq!(
            cell(Some(&[0, 1, 2, 3]), ColumnType::MYSQL_TYPE_BLOB),
            Cell::Binary { byte_len: 4 }
        );
    }

    /// MySQL has no boolean. Reporting `TINYINT(1)` as one would be an
    /// invention, and would show `true` for a column holding 7.
    #[test]
    fn a_tinyint_stays_a_number_rather_than_becoming_a_boolean() {
        assert_eq!(
            cell(Some(b"1"), ColumnType::MYSQL_TYPE_TINY),
            Cell::Number {
                value: "1".to_string()
            }
        );
    }

    /// The one place the shipped drivers disagree. Quoting is done by the
    /// protocol's own helper from the character reported here, so what this
    /// asserts is that the character reaches it.
    #[test]
    fn an_identifier_is_quoted_the_way_mysql_expects() {
        let capabilities = database::protocol::Capabilities {
            identifier_quote: Some(IDENTIFIER_QUOTE.to_string()),
            ..Default::default()
        };
        assert_eq!(capabilities.quote_identifier("users"), "`users`");
        assert_eq!(
            capabilities.quote_identifier("odd`name"),
            "`odd``name`",
            "an embedded backtick must be doubled, not dropped"
        );
    }
}
