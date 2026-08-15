use database::protocol::{Cell, ErrorCode, ResponseError};
use postgres::types::Type;

/// One value, as PostgreSQL itself rendered it.
///
/// The text comes from the server rather than from a Rust decode, which is what
/// keeps `numeric(38,10)` exact -- every binary decode of it in Rust goes
/// through a type that rounds, and a database client that quietly rounds the
/// values it is asked to display is worse than useless.
///
/// The *kind* still comes from the column's OID, so the grid can tell a number
/// from a string without this crate parsing either.
pub fn cell(value: Option<&str>, kind: Option<&Type>) -> Cell {
    let Some(value) = value else {
        return Cell::Null;
    };

    let Some(kind) = kind else {
        return Cell::Text {
            value: value.to_string(),
        };
    };

    match *kind {
        Type::BOOL => match value {
            // PostgreSQL's text format for booleans, which is not `true`/`false`.
            "t" => Cell::Bool { value: true },
            "f" => Cell::Bool { value: false },
            other => Cell::Text {
                value: other.to_string(),
            },
        },
        Type::INT2
        | Type::INT4
        | Type::INT8
        | Type::FLOAT4
        | Type::FLOAT8
        | Type::NUMERIC
        | Type::OID => Cell::Number {
            value: value.to_string(),
        },
        Type::JSON | Type::JSONB => Cell::Json {
            value: value.to_string(),
        },
        Type::DATE | Type::TIME | Type::TIMETZ | Type::TIMESTAMP | Type::TIMESTAMPTZ => {
            Cell::Timestamp {
                value: value.to_string(),
            }
        }
        // `\x` followed by hex, so the length is half what is left. Reported as
        // a size rather than shipped: a large blob through a line-delimited
        // JSON pipe is how a driver stalls the editor reading it.
        Type::BYTEA => Cell::Binary {
            byte_len: value.strip_prefix("\\x").unwrap_or(value).len() as u64 / 2,
        },
        // Arrays, enums, ranges, composites and every extension type: the
        // server's own text is the honest answer, and inventing a kind for each
        // would be a second type system nobody can keep current.
        _ => Cell::Text {
            value: value.to_string(),
        },
    }
}

/// PostgreSQL's errors, sorted into the few kinds the UI reacts to differently.
pub fn error(error: postgres::Error) -> ResponseError {
    let detail = error.to_string();
    let code = error
        .as_db_error()
        .map(|db| match db.code() {
            // 25006 read_only_sql_transaction -- the one the whole read-only
            // guarantee produces, and the one the UI must not report as bad SQL.
            code if code.code() == "25006" => ErrorCode::ReadOnly,
            code if code.code().starts_with("28") => ErrorCode::Authentication,
            code if code.code() == "57014" => ErrorCode::Cancelled,
            code if code.code().starts_with("08") => ErrorCode::Connection,
            _ => ErrorCode::Syntax,
        })
        .unwrap_or(ErrorCode::Connection);

    let message = match code {
        ErrorCode::ReadOnly => "this connection is read-only".to_string(),
        ErrorCode::Cancelled => "the query was cancelled".to_string(),
        _ => error
            .as_db_error()
            .map(|db| db.message().to_string())
            .unwrap_or_else(|| detail.clone()),
    };
    ResponseError::new(code, message).with_detail(detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_is_never_the_empty_string() {
        assert_eq!(cell(None, Some(&Type::TEXT)), Cell::Null);
        assert_eq!(
            cell(Some(""), Some(&Type::TEXT)),
            Cell::Text {
                value: String::new()
            }
        );
    }

    /// PostgreSQL spells booleans `t` and `f`, which is exactly the sort of
    /// engine detail the UI must never have to know.
    #[test]
    fn a_boolean_arrives_as_a_boolean() {
        assert_eq!(
            cell(Some("t"), Some(&Type::BOOL)),
            Cell::Bool { value: true }
        );
        assert_eq!(
            cell(Some("f"), Some(&Type::BOOL)),
            Cell::Bool { value: false }
        );
    }

    /// The reason values cross as server-rendered text: every binary decode of
    /// `numeric` in Rust goes through a type that rounds.
    #[test]
    fn a_wide_numeric_keeps_every_digit() {
        let wide = "123456789012345678901234567890.0000000001";
        assert_eq!(
            cell(Some(wide), Some(&Type::NUMERIC)),
            Cell::Number {
                value: wide.to_string()
            }
        );
    }

    #[test]
    fn a_bytea_reports_its_size_and_not_its_bytes() {
        assert_eq!(
            cell(Some("\\x00010203"), Some(&Type::BYTEA)),
            Cell::Binary { byte_len: 4 }
        );
    }

    /// An array is not given a kind of its own: the server's text is the honest
    /// answer, and a kind per extension type is a second type system.
    #[test]
    fn an_unknown_type_falls_back_to_the_servers_own_text() {
        assert_eq!(
            cell(Some("{1,2,3}"), Some(&Type::INT4_ARRAY)),
            Cell::Text {
                value: "{1,2,3}".to_string()
            }
        );
    }
}
