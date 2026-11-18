pub mod methods;
pub mod types;
pub mod url_import;

pub use methods::*;
pub use types::*;
pub use url_import::ImportedUrl;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which shape of this protocol Zode speaks.
///
/// A driver reports its own at `initialize`, and a mismatch in the major part
/// is refused outright rather than half-worked-around -- see
/// `version_mismatch`. Third-party drivers pin to this number, so bumping it is
/// a promise to everyone who wrote one.
///
/// Frozen at `1`: a second engine (PostgreSQL) was written against it without
/// a line changing in `database_ui`, which is what a protocol has to survive
/// to be one. From here a breaking change means a new number, and every
/// third-party driver pinned to this one stops working -- so it costs something.
pub const PROTOCOL_VERSION: u32 = 1;

/// Whether a driver reporting `theirs` can be talked to at all.
///
/// Below 1.0 every difference is breaking -- there is nothing yet promised to
/// stay put. Afterwards the major part must match and the driver may be behind
/// on the minor.
pub fn version_is_compatible(ours: u32, theirs: u32) -> bool {
    ours == theirs
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(u64),
    Text(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: RequestId, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    pub fn ok(id: RequestId, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: RequestId, error: ResponseError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: ErrorCode,
    pub message: String,
    /// The engine's own words, kept apart from `message` so the UI can show a
    /// short line and keep the rest for whoever wants it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Where in the request the fault was, when the driver can tell. Lets a UI
    /// mark the cell instead of printing the engine's sentence.
    ///
    /// Read leniently: it is a hint, and a shape this build does not expect
    /// costs the caller the hint, not the answer.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_data"
    )]
    pub data: Option<ErrorData>,
}

/// Whatever of `data` can be read, and nothing when none of it can.
///
/// Field by field rather than through `ErrorData`'s derive, which refuses the
/// whole value over one field of the wrong type -- and with it the whole
/// response, which the caller would then wait out the request timeout for.
fn lenient_data<'de, D>(deserializer: D) -> Result<Option<ErrorData>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let data = ErrorData {
        change: object.get("change").and_then(Value::as_u64),
        column: object
            .get("column")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    Ok((data != ErrorData::default()).then_some(data))
}

/// Which part of an `update_rows` call an error is about.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorData {
    /// Index into the request's `changes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

/// Every link of an error's `source()` chain, joined into one line.
///
/// `to_string()` alone is not enough and the gap is not academic: a
/// `tokio_postgres` connect failure Displays as the fixed sentence
/// "error connecting to server" and carries the `io::Error` that actually
/// explains it -- refused, unreachable, unresolved -- as its `source`. Reported
/// through `to_string()`, the one fact worth having never reached the user, who
/// saw a connection fail for no stated reason.
///
/// A link the line already carries is skipped, wherever it already sits. A
/// wrapper that restates its cause is common, and an engine whose own `Display`
/// prints its source in full (MongoDB does) would otherwise have that source
/// appended to it a second time.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut chain = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        let text = error.to_string();
        if !text.is_empty() && !chain.contains(&text) {
            chain.push_str(": ");
            chain.push_str(&text);
        }
        cause = error.source();
    }
    chain
}

impl ResponseError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            data: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_data(mut self, data: ErrorData) -> Self {
        self.data = Some(data);
        self
    }
}

/// Why a call failed, in the few kinds a UI actually reacts to differently.
///
/// Deliberately coarse. A code per engine error would be a second type system
/// nobody can keep current; these are the distinctions that change what the
/// user is told or offered next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The connection refused a write. Told apart from `Syntax` because the
    /// answer is "this column is read-only", not "check your SQL".
    ReadOnly,
    /// Credentials rejected -- the one error worth offering a password prompt for.
    Authentication,
    /// Could not reach the server, or lost it mid-call.
    Connection,
    /// The engine rejected the statement.
    Syntax,
    /// The call named a connection the driver does not have.
    UnknownConnection,
    /// The driver does not implement this method.
    Unsupported,
    /// Cancelled at the caller's request -- not a failure to report as one.
    Cancelled,
    /// An `update_rows` change matched a number of rows other than one -- the
    /// row was changed or deleted since the grid read it. The whole batch was
    /// rolled back.
    Conflict,
    /// The engine rejected a value in an `update_rows` change: a failed cast, a
    /// constraint. The whole batch was rolled back.
    InvalidValue,
    Internal,
    /// A code this build does not know, from a newer driver.
    ///
    /// Without it one new code makes the whole response undecodable, and the
    /// caller sits out the request timeout instead of reading an error. Treat
    /// it as `Internal`: a failure, with the driver's `message` to show.
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-tripping the envelope is the one thing every other test rests on.
    #[test]
    fn envelopes_survive_a_round_trip() {
        let request = Request::new(
            RequestId::Number(7),
            Query::NAME,
            serde_json::json!({ "sql": "select 1" }),
        );
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: Request = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, RequestId::Number(7));
        assert_eq!(decoded.method, "query");
        assert_eq!(decoded.jsonrpc, "2.0");

        let error = Response::err(
            RequestId::Text("a".into()),
            ResponseError::new(ErrorCode::ReadOnly, "read-only").with_detail("PRAGMA query_only"),
        );
        let decoded: Response =
            serde_json::from_str(&serde_json::to_string(&error).unwrap()).unwrap();
        let error = decoded.error.expect("an error response must carry one");
        assert_eq!(error.code, ErrorCode::ReadOnly);
        assert_eq!(error.detail.as_deref(), Some("PRAGMA query_only"));
        assert!(
            decoded.result.is_none(),
            "an error response must not also claim a result"
        );
    }

    /// A null and an empty string are different answers, and a database client
    /// that renders them alike is lying about the data.
    #[test]
    fn null_does_not_collapse_into_the_empty_string() {
        let null = serde_json::to_string(&Cell::Null).unwrap();
        let empty = serde_json::to_string(&Cell::Text {
            value: String::new(),
        })
        .unwrap();
        assert_ne!(null, empty);
        assert_eq!(
            serde_json::from_str::<Cell>(&null).unwrap(),
            Cell::Null,
            "and the difference must survive the wire, not just the type"
        );
    }

    /// Numbers cross as text on purpose: `numeric(38,10)` and `u64` both lose
    /// digits through an f64.
    #[test]
    fn wide_numbers_keep_every_digit() {
        let wide = "123456789012345678901234567890.0000000001";
        let cell = Cell::Number {
            value: wide.to_string(),
        };
        let decoded: Cell = serde_json::from_str(&serde_json::to_string(&cell).unwrap()).unwrap();
        assert_eq!(decoded, cell, "a wide number must survive byte for byte");
    }

    /// The direction that bites in practice: a driver newer than this build
    /// answers with a code this build has never heard of. If that failed to
    /// decode, the client would log "undecodable line", drop the answer and the
    /// caller would wait out the whole request timeout for an error that had
    /// already arrived.
    #[test]
    fn a_code_from_a_newer_driver_still_decodes_as_an_error() {
        let line = r#"{"jsonrpc":"2.0","id":3,"error":{"code":"quota_exceeded","message":"too many rows"}}"#;
        let decoded: Response = serde_json::from_str(line).expect("an unknown code must parse");
        let error = decoded.error.expect("the error must survive");
        assert_eq!(error.code, ErrorCode::Other);
        assert_eq!(error.message, "too many rows");
    }

    #[test]
    fn the_new_codes_use_their_documented_wire_names() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::Conflict).unwrap(),
            "\"conflict\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidValue).unwrap(),
            "\"invalid_value\""
        );
        assert_eq!(
            serde_json::from_str::<ErrorCode>("\"invalid_value\"").unwrap(),
            ErrorCode::InvalidValue
        );
    }

    /// An error from a driver that predates `data` has no such key, and one
    /// that carries none must not put one on the wire for a client that
    /// predates it.
    #[test]
    fn error_data_is_optional_in_both_directions() {
        let old = r#"{"code":"syntax","message":"bad"}"#;
        let decoded: ResponseError = serde_json::from_str(old).unwrap();
        assert_eq!(decoded.data, None);

        let encoded = serde_json::to_value(ResponseError::new(ErrorCode::Syntax, "bad")).unwrap();
        assert!(encoded.get("data").is_none(), "{encoded}");

        let with_data = ResponseError::new(ErrorCode::InvalidValue, "bad").with_data(ErrorData {
            change: Some(2),
            column: Some("age".into()),
        });
        let round: ResponseError =
            serde_json::from_str(&serde_json::to_string(&with_data).unwrap()).unwrap();
        assert_eq!(round.data, with_data.data);
    }

    /// `data` is a hint for marking a cell. A driver that sends it in a shape
    /// this build does not expect -- a string where a number was promised, an
    /// object that is not one -- must cost the caller the hint and nothing else:
    /// strict decoding made the whole answer undecodable, and the caller sat out
    /// the request timeout for an error that had already arrived.
    #[test]
    fn an_error_whose_data_is_malformed_still_arrives_with_its_code_and_message() {
        for data in [
            serde_json::json!({ "change": "two", "column": 7 }),
            serde_json::json!({ "change": -1 }),
            serde_json::json!({ "change": 1.5, "column": ["a"] }),
            serde_json::json!("oops"),
            serde_json::json!([1, 2]),
            serde_json::json!(7),
            serde_json::json!(null),
            serde_json::json!({}),
        ] {
            let line = serde_json::json!({
                "jsonrpc": "2.0", "id": 4,
                "error": { "code": "invalid_value", "message": "bad", "data": data },
            });
            let decoded: Response = serde_json::from_value(line)
                .unwrap_or_else(|error| panic!("{data}: the answer must decode: {error}"));
            let error = decoded.error.expect("the error survives");
            assert_eq!(error.code, ErrorCode::InvalidValue, "{data}");
            assert_eq!(error.message, "bad", "{data}");
            assert_eq!(error.data, None, "{data}: nothing usable in it");
        }
    }

    /// Whatever part of `data` can be read is kept, and a field this build has
    /// never heard of is ignored.
    #[test]
    fn the_readable_part_of_error_data_survives_the_unreadable_part() {
        let decoded: ResponseError = serde_json::from_value(serde_json::json!({
            "code": "invalid_value", "message": "bad",
            "data": { "change": "two", "column": "age", "row": 9 },
        }))
        .expect("decodes");
        assert_eq!(
            decoded.data,
            Some(ErrorData {
                change: None,
                column: Some("age".into())
            })
        );
        let decoded: ResponseError = serde_json::from_value(serde_json::json!({
            "code": "conflict", "message": "gone", "data": { "change": 3, "column": null },
        }))
        .expect("decodes");
        assert_eq!(
            decoded.data,
            Some(ErrorData {
                change: Some(3),
                column: None
            })
        );
    }

    /// A UI built before `row_updates` existed asks nothing of it, and a
    /// driver that never heard of it reports nothing: both must read as "no".
    #[test]
    fn a_driver_that_never_heard_of_row_updates_cannot_update_rows() {
        let old: InitializeResult = serde_json::from_str(
            r#"{"protocol_version":1,"driver_name":"x","capabilities":{"cancellation":true}}"#,
        )
        .unwrap();
        assert!(!old.capabilities.row_updates);
        let bare: InitializeResult =
            serde_json::from_str(r#"{"protocol_version":1,"driver_name":"x"}"#).unwrap();
        assert!(!bare.capabilities.row_updates);
    }

    /// The wire shape is the contract third-party drivers code against.
    #[test]
    fn update_rows_params_have_the_documented_shape() {
        let params = UpdateRowsParams {
            connection_id: ConnectionId("c1".into()),
            schema: "public".into(),
            table: "users".into(),
            changes: vec![RowChange {
                key: vec![ColumnValue {
                    column: "id".into(),
                    value: EditValue::Text { value: "7".into() },
                }],
                set: vec![ColumnValue {
                    column: "email".into(),
                    value: EditValue::Null,
                }],
            }],
        };
        assert_eq!(
            serde_json::to_value(&params).unwrap(),
            serde_json::json!({
                "connection_id": "c1",
                "schema": "public",
                "table": "users",
                "changes": [{
                    "key": [{"column": "id", "value": {"kind": "text", "value": "7"}}],
                    "set": [{"column": "email", "value": {"kind": "null"}}],
                }],
            })
        );
        assert_eq!(UpdateRows::NAME, "update_rows");
        assert!(METHOD_NAMES.contains(&"update_rows"));
    }

    /// Typing nothing into a text column is a value; clearing it to NULL is a
    /// different instruction -- through JSON, not just through the type.
    #[test]
    fn an_empty_edit_is_not_a_null_edit() {
        let null = serde_json::to_string(&EditValue::Null).unwrap();
        let empty = serde_json::to_string(&EditValue::Text {
            value: String::new(),
        })
        .unwrap();
        assert_ne!(null, empty);
        assert_eq!(
            serde_json::from_str::<EditValue>(&null).unwrap(),
            EditValue::Null
        );
        assert_eq!(
            serde_json::from_str::<EditValue>(&empty).unwrap(),
            EditValue::Text {
                value: String::new()
            }
        );
    }

    #[test]
    fn a_cell_becomes_an_edit_value_unless_only_its_size_is_known() {
        let text = |value: &str| {
            Some(EditValue::Text {
                value: value.into(),
            })
        };
        assert_eq!(EditValue::from_cell(&Cell::Null), Some(EditValue::Null));
        assert_eq!(
            EditValue::from_cell(&Cell::Number {
                value: "1.50".into()
            }),
            text("1.50")
        );
        assert_eq!(
            EditValue::from_cell(&Cell::Bool { value: true }),
            text("true")
        );
        assert_eq!(
            EditValue::from_cell(&Cell::Timestamp {
                value: "2026-08-15 10:00:00".into()
            }),
            text("2026-08-15 10:00:00")
        );
        assert_eq!(EditValue::from_cell(&Cell::Binary { byte_len: 4 }), None);
    }

    #[test]
    fn every_method_name_is_listed_once() {
        let mut names = METHOD_NAMES.to_vec();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(count, names.len(), "two methods share a wire name");
    }

    #[derive(Debug)]
    struct Layer(&'static str, Option<Box<Layer>>);

    impl std::fmt::Display for Layer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1
                .as_deref()
                .map(|layer| layer as &dyn std::error::Error)
        }
    }

    /// The whole reason this exists: the outermost error of a connect failure
    /// is a fixed sentence, and the one that says *why* is two links down.
    #[test]
    fn a_cause_two_links_down_still_reaches_the_line() {
        let error = Layer(
            "error connecting to server",
            Some(Box::new(Layer(
                "tcp connect error",
                Some(Box::new(Layer(
                    "Network is unreachable (os error 51)",
                    None,
                ))),
            ))),
        );
        assert_eq!(
            error_chain(&error),
            "error connecting to server: tcp connect error: Network is unreachable (os error 51)"
        );
    }

    /// A wrapper that restates its cause is common, and reads as a stutter.
    #[test]
    fn a_link_that_merely_repeats_its_parent_is_left_out() {
        let error = Layer(
            "could not reach db.example:5432",
            Some(Box::new(Layer("could not reach db.example:5432", None))),
        );
        assert_eq!(error_chain(&error), "could not reach db.example:5432");
    }

    /// An engine whose own `Display` already prints its source -- MongoDB does,
    /// in full -- would otherwise have that source appended a second time, and
    /// the reader gets the same failure three times over in one line.
    #[test]
    fn a_cause_the_outer_message_already_quotes_is_not_appended_again() {
        let error = Layer(
            "SCRAM failure: Authentication failed., source: Authentication failed.",
            Some(Box::new(Layer("Authentication failed.", None))),
        );
        assert_eq!(
            error_chain(&error),
            "SCRAM failure: Authentication failed., source: Authentication failed."
        );
    }
}
