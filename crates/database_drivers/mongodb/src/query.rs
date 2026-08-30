//! What a statement typed into the scratch buffer is allowed to be.
//!
//! MongoDB has no SQL, so a statement here is a JSON command document -- the
//! same shape the server's own command reference uses:
//!
//! ```json
//! { "find": "users", "filter": { "active": true }, "sort": { "name": 1 } }
//! { "aggregate": "orders", "pipeline": [ { "$group": { "_id": "$user" } } ] }
//! ```
//!
//! Read-only is enforced *here*, by what this module will agree to parse, and
//! that is the whole reason it exists as its own module with its own tests.
//! MongoDB offers no read-only connection to open -- the other two drivers get
//! their guarantee from the engine (`SQLITE_OPEN_READ_ONLY`, a read-only
//! transaction), and this one cannot. So the guarantee is a whitelist: two
//! verbs, and an aggregation pipeline with every writing stage refused.

use database::protocol::{ErrorCode, ResponseError};
use mongodb::bson::{Bson, Document};

/// The stages that make an aggregation a write.
///
/// Named rather than detected: `$out` and `$merge` are the only two, they are
/// the whole difference between reading and writing in a pipeline, and a
/// blacklist that guesses would be a read-only guarantee that guesses.
const WRITING_STAGES: [&str; 2] = ["$out", "$merge"];

#[derive(Debug, PartialEq)]
pub struct Command {
    pub collection: String,
    /// Which database to run against, from the command document's `$db`.
    ///
    /// `None` means the one the connection opened on. It matters because the
    /// tree lists every database on the server while a connection opens on one:
    /// without this, clicking a collection in any *other* database ran the
    /// statement against the default and answered about the wrong collection --
    /// or, worse, about a collection of the same name that happens to exist
    /// there. `$db` is MongoDB's own spelling for this, carried in the command
    /// document rather than in a new protocol field.
    pub database: Option<String>,
    pub operation: Operation,
}

#[derive(Debug, PartialEq)]
pub enum Operation {
    Find {
        filter: Document,
        sort: Option<Document>,
        projection: Option<Document>,
    },
    Aggregate {
        pipeline: Vec<Document>,
    },
}

fn refused(message: impl Into<String>) -> ResponseError {
    ResponseError::new(ErrorCode::Syntax, message)
}

fn read_only(message: impl Into<String>) -> ResponseError {
    ResponseError::new(ErrorCode::ReadOnly, message)
}

fn sub_document(command: &Document, key: &str) -> Result<Option<Document>, ResponseError> {
    match command.get(key) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::Document(document)) => Ok(Some(document.clone())),
        Some(_) => Err(refused(format!("`{key}` has to be an object"))),
    }
}

/// Reads one statement, refusing everything that is not a read.
pub fn parse(statement: &str) -> Result<Command, ResponseError> {
    let statement = statement.trim();
    if statement.is_empty() {
        return Err(refused("there is no statement to run"));
    }

    let value: serde_json::Value = serde_json::from_str(statement).map_err(|error| {
        refused(
            "a MongoDB statement is a JSON command document, such as \
             {\"find\": \"users\", \"filter\": {}}",
        )
        .with_detail(database::protocol::error_chain(&error))
    })?;
    let command: Document = mongodb::bson::to_document(&value)
        .map_err(|error| refused("that statement is not a command document").with_detail(error.to_string()))?;

    let database = match command.get("$db") {
        None | Some(Bson::Null) => None,
        Some(Bson::String(name)) => Some(name.clone()),
        Some(_) => return Err(refused("`$db` has to be a database name")),
    };

    if let Ok(collection) = command.get_str("find") {
        return Ok(Command {
            collection: collection.to_string(),
            database,
            operation: Operation::Find {
                filter: sub_document(&command, "filter")?.unwrap_or_default(),
                sort: sub_document(&command, "sort")?,
                projection: sub_document(&command, "projection")?,
            },
        });
    }

    if let Ok(collection) = command.get_str("aggregate") {
        let pipeline = match command.get("pipeline") {
            Some(Bson::Array(stages)) => stages
                .iter()
                .map(|stage| match stage {
                    Bson::Document(stage) => Ok(stage.clone()),
                    _ => Err(refused("every stage of a pipeline has to be an object")),
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
            Some(_) => return Err(refused("`pipeline` has to be an array of stages")),
        };

        if let Some(stage) = writing_stage(&pipeline) {
            return Err(read_only(format!(
                "`{stage}` writes to a collection, and this connection only reads"
            )));
        }

        return Ok(Command {
            collection: collection.to_string(),
            database,
            operation: Operation::Aggregate { pipeline },
        });
    }

    Err(read_only(
        "this connection only reads: a statement has to be a `find` or an `aggregate`",
    ))
}

/// The first stage of `pipeline` that would write, if any.
fn writing_stage(pipeline: &[Document]) -> Option<&'static str> {
    pipeline.iter().find_map(|stage| {
        WRITING_STAGES
            .into_iter()
            .find(|writing| stage.contains_key(writing))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::protocol::ErrorCode;
    use mongodb::bson::doc;

    #[test]
    fn a_find_carries_its_filter_and_sort() {
        let command = parse(r#"{"find": "users", "filter": {"active": true}, "sort": {"name": 1}}"#)
            .expect("a well-formed find");
        assert_eq!(
            command,
            Command {
                collection: "users".into(),
                database: None,
                operation: Operation::Find {
                    filter: doc! { "active": true },
                    // `1i64`, not `1`: a statement arrives as JSON, whose
                    // integers become BSON 64-bit ones. MongoDB reads either as
                    // a sort direction, so this is the test matching the wire
                    // rather than the wire needing to change.
                    sort: Some(doc! { "name": 1i64 }),
                    projection: None,
                },
            }
        );
    }

    #[test]
    fn a_find_without_a_filter_reads_everything() {
        let command = parse(r#"{"find": "users"}"#).expect("a bare find is a whole-collection read");
        assert_eq!(
            command,
            Command {
                collection: "users".into(),
                database: None,
                operation: Operation::Find {
                    filter: Document::new(),
                    sort: None,
                    projection: None,
                },
            }
        );
    }

    /// The read-only guarantee, and the reason this module exists. MongoDB
    /// offers no read-only connection to open, so nothing but this whitelist
    /// stands between a scratch buffer and a dropped collection.
    #[test]
    fn a_write_command_is_refused_by_name() {
        for statement in [
            r#"{"insert": "users", "documents": [{"x": 1}]}"#,
            r#"{"delete": "users", "deletes": [{"q": {}, "limit": 0}]}"#,
            r#"{"update": "users", "updates": []}"#,
            r#"{"drop": "users"}"#,
            r#"{"dropDatabase": 1}"#,
            r#"{"createIndexes": "users", "indexes": []}"#,
        ] {
            let error = parse(statement).expect_err("{statement} must be refused");
            assert_eq!(
                error.code,
                ErrorCode::ReadOnly,
                "{statement} was refused for the wrong reason: {}",
                error.message
            );
        }
    }

    /// The subtle one. An aggregation reads -- until its last stage writes the
    /// whole result into a collection, which `$out` and `$merge` do.
    #[test]
    fn an_aggregation_that_ends_in_a_write_is_refused() {
        for stage in [r#"{"$out": "copy"}"#, r#"{"$merge": {"into": "copy"}}"#] {
            let statement =
                format!(r#"{{"aggregate": "orders", "pipeline": [{{"$match": {{}}}}, {stage}]}}"#);
            let error = parse(&statement).expect_err("a writing pipeline must be refused");
            assert_eq!(error.code, ErrorCode::ReadOnly, "{}", error.message);
        }
    }

    #[test]
    fn a_reading_pipeline_is_allowed_through() {
        let command = parse(r#"{"aggregate": "orders", "pipeline": [{"$match": {"paid": true}}]}"#)
            .expect("a reading pipeline");
        assert_eq!(
            command,
            Command {
                collection: "orders".into(),
                database: None,
                operation: Operation::Aggregate {
                    pipeline: vec![doc! { "$match": { "paid": true } }],
                },
            }
        );
    }

    /// The message has to teach the shape, because there is no SQL here to fall
    /// back on and someone typing their first statement has nothing to copy.
    #[test]
    fn a_statement_that_is_not_json_says_what_one_looks_like() {
        let error = parse("SELECT * FROM users").expect_err("SQL is not a MongoDB statement");
        assert_eq!(error.code, ErrorCode::Syntax);
        assert!(error.message.contains("{\"find\""), "{}", error.message);
    }

    /// The tree lists every database on the server while a connection opens on
    /// one, so a statement built from a click on another database has to say
    /// which -- or it reads the wrong collection and looks like it worked.
    #[test]
    fn a_statement_may_name_the_database_it_runs_against() {
        let command =
            parse(r#"{"find": "users", "$db": "other_app"}"#).expect("a well-formed find");
        assert_eq!(command.database.as_deref(), Some("other_app"));
        assert_eq!(command.collection, "users");
    }

    #[test]
    fn an_empty_statement_says_so_rather_than_running_nothing() {
        let error = parse("   ").expect_err("nothing to run");
        assert_eq!(error.code, ErrorCode::Syntax);
    }
}
