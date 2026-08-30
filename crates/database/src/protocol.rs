pub mod methods;
pub mod types;

pub use methods::*;
pub use types::*;

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
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
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
    Internal,
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
            self.1.as_deref().map(|layer| layer as &dyn std::error::Error)
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
                Some(Box::new(Layer("Network is unreachable (os error 51)", None))),
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
