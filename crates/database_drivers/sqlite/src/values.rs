use database::protocol::{Cell, ErrorCode, ResponseError};
use rusqlite::types::ValueRef;

/// One SQLite value, rendered for display.
///
/// SQLite stores five things, but a column's *declared* type says how it is
/// meant to be read -- a `TIMESTAMP` column holds TEXT, and a grid that
/// right-aligns it as text has thrown away what the schema said. So the
/// declared type gets a say, and the storage class decides everything else.
pub fn cell(value: ValueRef<'_>, declared_type: &str) -> Cell {
    match value {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(number) => {
            // SQLite has no boolean: a `BOOLEAN` column holds 0 or 1, and
            // showing those as numbers loses what the schema meant.
            if is_boolean(declared_type) && (number == 0 || number == 1) {
                Cell::Bool { value: number == 1 }
            } else {
                Cell::Number {
                    value: number.to_string(),
                }
            }
        }
        ValueRef::Real(number) => Cell::Number {
            value: format_real(number),
        },
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            if is_timestamp(declared_type) {
                Cell::Timestamp { value: text }
            } else if is_json(declared_type) {
                Cell::Json { value: text }
            } else {
                Cell::Text { value: text }
            }
        }
        // Never the bytes. A blob through a line-delimited JSON pipe is how a
        // driver stalls the editor that is reading it.
        ValueRef::Blob(bytes) => Cell::Binary {
            byte_len: bytes.len() as u64,
        },
    }
}

/// `{:?}` rather than `{}`: the former is Rust's shortest round-trippable form,
/// so `0.1` stays `0.1` instead of becoming `0.1000000000000000055511151231257827`.
fn format_real(number: f64) -> String {
    if number.is_finite() {
        format!("{number:?}")
    } else {
        // JSON has no infinity or NaN, and a number that cannot be encoded
        // would fail the whole page rather than one cell.
        number.to_string()
    }
}

fn normalized(declared_type: &str) -> String {
    declared_type.trim().to_ascii_uppercase()
}

fn is_boolean(declared_type: &str) -> bool {
    let declared = normalized(declared_type);
    declared == "BOOLEAN" || declared == "BOOL"
}

fn is_timestamp(declared_type: &str) -> bool {
    let declared = normalized(declared_type);
    ["DATE", "DATETIME", "TIMESTAMP"]
        .iter()
        .any(|kind| declared.starts_with(kind))
}

fn is_json(declared_type: &str) -> bool {
    normalized(declared_type).starts_with("JSON")
}

/// SQLite's own errors, sorted into the few kinds the UI reacts to differently.
///
/// The read-only case is the one that matters: a user typing `delete` needs to
/// be told the column is read-only, not that their SQL is wrong.
pub fn error(error: rusqlite::Error) -> ResponseError {
    let detail = database::protocol::error_chain(&error);
    let code = match &error {
        rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
            rusqlite::ErrorCode::ReadOnly => ErrorCode::ReadOnly,
            rusqlite::ErrorCode::OperationInterrupted => ErrorCode::Cancelled,
            rusqlite::ErrorCode::CannotOpen | rusqlite::ErrorCode::DatabaseBusy => {
                ErrorCode::Connection
            }
            rusqlite::ErrorCode::Unknown => classify_by_message(&detail),
            _ => ErrorCode::Syntax,
        },
        _ => classify_by_message(&detail),
    };

    let message = match code {
        ErrorCode::ReadOnly => "this connection is read-only".to_string(),
        ErrorCode::Cancelled => "the query was cancelled".to_string(),
        _ => detail.clone(),
    };
    ResponseError::new(code, message).with_detail(detail)
}

/// SQLite reports a write to a read-only handle through the generic code as
/// well as the specific one, depending on where it was caught. Reading the
/// message is unpleasant but it is the difference between a useful answer and
/// "error: unknown".
fn classify_by_message(detail: &str) -> ErrorCode {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("readonly") || detail.contains("read-only") {
        ErrorCode::ReadOnly
    } else if detail.contains("interrupted") {
        ErrorCode::Cancelled
    } else {
        ErrorCode::Syntax
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blob_reports_its_size_and_not_its_bytes() {
        let cell = cell(ValueRef::Blob(&[1, 2, 3, 4]), "BLOB");
        assert_eq!(cell, Cell::Binary { byte_len: 4 });
    }

    #[test]
    fn a_declared_boolean_reads_as_one() {
        assert_eq!(
            cell(ValueRef::Integer(1), "BOOLEAN"),
            Cell::Bool { value: true }
        );
        assert_eq!(
            cell(ValueRef::Integer(1), "INTEGER"),
            Cell::Number {
                value: "1".to_string()
            },
            "an ordinary integer must stay a number"
        );
        assert_eq!(
            cell(ValueRef::Integer(7), "BOOLEAN"),
            Cell::Number {
                value: "7".to_string()
            },
            "a value outside 0/1 is not a boolean whatever the column claims"
        );
    }

    /// A float rendered through the shortest round-trippable form, not through
    /// `{}`, which turns 0.1 into a wall of digits.
    #[test]
    fn a_float_keeps_the_digits_it_was_written_with() {
        assert_eq!(
            cell(ValueRef::Real(0.1), "REAL"),
            Cell::Number {
                value: "0.1".to_string()
            }
        );
    }

    #[test]
    fn null_is_never_the_empty_string() {
        assert_eq!(cell(ValueRef::Null, "TEXT"), Cell::Null);
        assert_eq!(
            cell(ValueRef::Text(b""), "TEXT"),
            Cell::Text {
                value: String::new()
            }
        );
    }
}
