use database::protocol::{Cell, ErrorCode, ResponseError};
use mongodb::bson::{Bson, Document};

/// One BSON value, rendered for display.
///
/// The tag says how to *present* the value, not what MongoDB called it -- the
/// same contract every other driver here follows. Two choices are worth saying
/// out loud:
///
/// - A nested document or array becomes [`Cell::Json`] rather than being
///   flattened into columns. A collection is not a table and its documents do
///   not agree on a shape, so flattening would invent a schema and then hide
///   whatever did not fit it.
/// - Numbers cross as text, like everywhere else here. `Decimal128` and `i64`
///   both lose digits through an `f64`, and a database client that quietly
///   rounds what it was asked to show is worse than useless.
pub fn cell(value: &Bson) -> Cell {
    match value {
        Bson::Null | Bson::Undefined => Cell::Null,
        Bson::String(value) => Cell::Text {
            value: value.clone(),
        },
        Bson::Boolean(value) => Cell::Bool { value: *value },
        Bson::Int32(value) => Cell::Number {
            value: value.to_string(),
        },
        Bson::Int64(value) => Cell::Number {
            value: value.to_string(),
        },
        Bson::Double(value) => Cell::Number {
            value: value.to_string(),
        },
        Bson::Decimal128(value) => Cell::Number {
            value: value.to_string(),
        },
        Bson::DateTime(value) => Cell::Timestamp {
            value: value.to_string(),
        },
        Bson::Timestamp(value) => Cell::Timestamp {
            value: format!("{}:{}", value.time, value.increment),
        },
        // Never the bytes themselves: a large binary through a line-delimited
        // JSON pipe is how a driver stalls the editor reading it.
        Bson::Binary(binary) => Cell::Binary {
            byte_len: binary.bytes.len() as u64,
        },
        // An ObjectId is the identity of the document, and it is what someone
        // copies out to go and find it again -- so it crosses as its own hex
        // text rather than as the `{"$oid": ...}` wrapper.
        Bson::ObjectId(id) => Cell::Text { value: id.to_hex() },
        Bson::Document(_) | Bson::Array(_) => Cell::Json {
            value: value.clone().into_relaxed_extjson().to_string(),
        },
        // Everything left is rare and has no shorter honest rendering than the
        // engine's own extended JSON: regexes, code, min/max keys.
        other => Cell::Json {
            value: other.clone().into_relaxed_extjson().to_string(),
        },
    }
}

/// MongoDB's type names, for the column headings a grid shows.
pub fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Null | Bson::Undefined => "null",
        Bson::String(_) => "string",
        Bson::Boolean(_) => "bool",
        Bson::Int32(_) => "int",
        Bson::Int64(_) => "long",
        Bson::Double(_) => "double",
        Bson::Decimal128(_) => "decimal",
        Bson::DateTime(_) => "date",
        Bson::Timestamp(_) => "timestamp",
        Bson::Binary(_) => "binData",
        Bson::ObjectId(_) => "objectId",
        Bson::Document(_) => "object",
        Bson::Array(_) => "array",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => "javascript",
        Bson::MinKey => "minKey",
        Bson::MaxKey => "maxKey",
        Bson::Symbol(_) => "symbol",
        Bson::DbPointer(_) => "dbPointer",
    }
}

/// MongoDB's errors, sorted into the few kinds the UI reacts to differently.
pub fn error(error: mongodb::error::Error) -> ResponseError {
    // `error_chain` rather than `to_string()` for the reason every driver here
    // uses it -- though this engine needs it least: its own `Display` already
    // spells out the topology and the command failure underneath, so the chain
    // usually adds nothing, and `error_chain` drops a link the line already
    // carries rather than saying it twice.
    let detail = database::protocol::error_chain(&error);
    let code = match error.kind.as_ref() {
        mongodb::error::ErrorKind::Authentication { .. } => ErrorCode::Authentication,
        mongodb::error::ErrorKind::ServerSelection { .. }
        | mongodb::error::ErrorKind::ConnectionPoolCleared { .. }
        | mongodb::error::ErrorKind::DnsResolve { .. }
        | mongodb::error::ErrorKind::Io(_) => ErrorCode::Connection,
        mongodb::error::ErrorKind::InvalidArgument { .. }
        | mongodb::error::ErrorKind::InvalidResponse { .. } => ErrorCode::Syntax,
        mongodb::error::ErrorKind::Command(command) => match command.code {
            // 13 Unauthorized, 18 AuthenticationFailed.
            13 | 18 => ErrorCode::Authentication,
            _ => ErrorCode::Syntax,
        },
        _ => ErrorCode::Internal,
    };

    let message = match code {
        ErrorCode::Cancelled => "the query was cancelled".to_string(),
        // Said in a sentence rather than by handing over the engine's own dump,
        // and it names `authSource` on purpose: MongoDB authenticates against
        // the database in the URI unless told otherwise, so the same user and
        // password that work in another client fail here for a reason the
        // server's own answer -- "Authentication failed." -- never mentions.
        ErrorCode::Authentication => {
            "the server refused these credentials -- check the user, the password, \
             and the auth database they were created in"
                .to_string()
        }
        _ => detail.clone(),
    };
    ResponseError::new(code, message).with_detail(detail)
}

/// What this engine wraps identifiers in.
///
/// Nothing: MongoDB has no identifier quoting because it has no SQL to quote
/// them in. Reported as absent so the protocol's helper falls back to the
/// standard double quote and nothing above this layer has to special-case it.
pub const IDENTIFIER_QUOTE: Option<String> = None;

/// Top-level field names across a page of documents, in first-seen order, with
/// `_id` first.
///
/// A page rather than the whole collection, and a union rather than the first
/// document's keys: documents in one collection need not agree, and showing
/// only the first one's fields would silently hide every other shape.
pub fn columns_across(documents: &[Document]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for document in documents {
        for key in document.keys() {
            if !names.iter().any(|name| name == key) {
                names.push(key.clone());
            }
        }
    }
    if let Some(at) = names.iter().position(|name| name == "_id")
        && at != 0
    {
        let id = names.remove(at);
        names.insert(0, id);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId};

    /// A collection is not a table: two documents in one need not carry the
    /// same fields, and a grid built from the first alone would hide the rest.
    #[test]
    fn columns_are_the_union_of_every_document_on_the_page() {
        let documents = vec![
            doc! { "name": "a", "_id": 1 },
            doc! { "_id": 2, "email": "b@example" },
        ];
        assert_eq!(columns_across(&documents), vec!["_id", "name", "email"]);
    }

    /// `_id` is the one field every document has and the one a reader looks for
    /// first, so it leads regardless of where it sat in the document.
    #[test]
    fn the_identifier_leads_even_when_it_was_written_last() {
        let documents = vec![doc! { "name": "a", "_id": 1 }];
        assert_eq!(columns_across(&documents), vec!["_id", "name"]);
    }

    /// Flattening a nested document into columns would invent a schema the
    /// collection does not have.
    #[test]
    fn a_nested_document_stays_one_value() {
        let cell = cell(&Bson::Document(doc! { "street": "1 Main", "zip": "01000" }));
        let Cell::Json { value } = cell else {
            panic!("a nested document must arrive as json, got {cell:?}");
        };
        assert!(value.contains("street"), "{value}");
    }

    /// The same reason every driver here sends numbers as text.
    #[test]
    fn a_long_keeps_every_digit() {
        assert_eq!(
            cell(&Bson::Int64(9_007_199_254_740_993)),
            Cell::Number {
                value: "9007199254740993".to_string()
            },
            "a value past f64's integers must survive byte for byte"
        );
    }

    /// What someone copies out of the grid to go and find the document again.
    #[test]
    fn an_object_id_arrives_as_the_hex_a_person_would_paste() {
        let id = ObjectId::parse_str("65a1f2c3d4e5f60718293a4b").unwrap();
        assert_eq!(
            cell(&Bson::ObjectId(id)),
            Cell::Text {
                value: "65a1f2c3d4e5f60718293a4b".to_string()
            }
        );
    }

    /// Null and the empty string are different answers, and in a database
    /// client the difference matters more than almost anywhere else.
    #[test]
    fn null_is_never_the_empty_string() {
        assert_eq!(cell(&Bson::Null), Cell::Null);
        assert_eq!(
            cell(&Bson::String(String::new())),
            Cell::Text {
                value: String::new()
            }
        );
    }
}
