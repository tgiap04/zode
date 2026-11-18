//! `update_rows`: the one write this driver makes.
//!
//! A change is `updateOne({_id}, {$set: {...}})`. Everything that can be
//! decided without writing -- which document each key names, what each value
//! becomes -- is decided first for the whole batch, so a bad key or a value of
//! the wrong type is refused before any document is touched, on every server.
//! Only then are the writes sent: in one transaction where the deployment has
//! them, one after another where it does not.

use crate::{Live, edit_values, values};
use database::protocol::{
    ColumnValue, EditValue, ErrorCode, ErrorData, ResponseError, RowChange, UpdateRowsParams,
    UpdateRowsResult,
};
use mongodb::bson::{Bson, Document, doc};
use mongodb::error::{ErrorKind, WriteFailure};
use mongodb::sync::{ClientSession, Collection};
use std::cell::RefCell;
use std::collections::HashMap;

/// MongoDB's `NoSuchTransaction`: the server already ended it, so there is
/// nothing left for an abort to do.
const NO_SUCH_TRANSACTION: i32 = 251;
/// MongoDB's `CommandNotFound`: what a server that predates `hello` answers it
/// with.
const COMMAND_NOT_FOUND: i32 = 59;
/// How many documents are read to learn what type a field the edited document
/// lacks usually has. A scan of the first few, not of the collection: the cost
/// of an edit must not grow with the size of what it edits.
const EDIT_SAMPLE_SIZE: i64 = 20;

struct PlannedUpdate {
    set: Document,
    /// `_id` and the type each edited field had when it was read: see
    /// [`guard_filter`].
    filter: Document,
}

pub fn apply(live: &Live, params: UpdateRowsParams) -> Result<UpdateRowsResult, ResponseError> {
    if params.changes.is_empty() {
        return Ok(UpdateRowsResult { updated: 0 });
    }
    let collection = live
        .client
        .database(&params.schema)
        .collection::<Document>(&params.table);

    // One sample per field, however many documents of the batch lack it.
    let dominant_types = RefCell::new(HashMap::<String, Option<Bson>>::new());
    let plans = params
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            plan_change(
                index,
                change,
                |candidate, projection| {
                    collection
                        .find_one(doc! { "_id": candidate.clone() })
                        .projection(projection.clone())
                        .run()
                        .map_err(values::error)
                },
                |field| {
                    if let Some(known) = dominant_types.borrow().get(field) {
                        return Ok(known.clone());
                    }
                    let found = sample_field(&collection, field)?;
                    dominant_types
                        .borrow_mut()
                        .insert(field.to_string(), found.clone());
                    Ok(found)
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    if supports_transactions(live)? {
        write_in_transaction(live, &collection, &plans)?;
    } else {
        write_in_order(&collection, &plans)?;
    }
    Ok(UpdateRowsResult {
        updated: plans.len() as u64,
    })
}

/// What a server error code says about a write that was refused.
///
/// A whitelist for `invalid_value`, because "the engine said no" is not the same
/// as "the value was wrong": a failover, a timeout or a missing privilege are
/// all refusals, and calling them a bad value sends the person to fix text that
/// was fine.
fn classify_write(code: i32) -> ErrorCode {
    match code {
        // BadValue, TypeMismatch, PathNotViable, ConflictingUpdateOperators,
        // DollarPrefixedFieldName, InvalidIdField, EmptyFieldName,
        // ImmutableField, DocumentValidationFailure, the three spellings of
        // DuplicateKey, a size change in a capped collection, and
        // CommandNotSupportedOnView.
        2 | 14 | 28 | 40 | 52 | 53 | 56 | 66 | 121 | 166 | 10003 | 11000 | 11001 | 12582 => {
            ErrorCode::InvalidValue
        }
        // AuthenticationFailed.
        18 => ErrorCode::Authentication,
        // Unauthorized: the user has no right to write here.
        13 => ErrorCode::ReadOnly,
        // WriteConflict: another operation changed the same document, which is
        // what `conflict` means. (A lock wait in the SQL engines is not that.)
        112 => ErrorCode::Conflict,
        // ShutdownInProgress, PrimarySteppedDown, InterruptedAtShutdown,
        // InterruptedDueToReplStateChange, NotWritablePrimary and the two
        // not-primary codes: the server is changing hands, so the call failed
        // on the way rather than on its content.
        91 | 189 | 10107 | 11600 | 11602 | 13435 | 13436 => ErrorCode::Connection,
        _ => ErrorCode::Internal,
    }
}

/// The server's own numeric code, when the failure came from a server at all.
fn server_code(error: &mongodb::error::Error) -> Option<i32> {
    match error.kind.as_ref() {
        ErrorKind::Command(command) => Some(command.code),
        ErrorKind::Write(WriteFailure::WriteError(write)) => Some(write.code),
        ErrorKind::Write(WriteFailure::WriteConcernError(concern)) => Some(concern.code),
        _ => None,
    }
}

/// Whether `hello` failing with this code means the server is too old to know
/// it. Only that: any other failure of the question is a real one.
fn hello_is_unknown(code: Option<i32>) -> bool {
    code == Some(COMMAND_NOT_FOUND)
}

/// What each edited field held when it was read, as a filter on the write.
///
/// A field the other client changed in between -- to another type, or into
/// existence -- then matches nothing, and the save is a conflict instead of a
/// number overwritten by text. `probes` pairs a field with the value read, or
/// `None` for a field the document did not have.
fn guard_filter(id: &Bson, probes: &[(&str, Option<&Bson>)]) -> Document {
    let mut filter = doc! { "_id": id.clone() };
    for (field, held) in probes {
        let guard = match held {
            Some(value) => doc! { "$type": edit_values::type_alias(value) },
            None => doc! { "$exists": false },
        };
        filter.insert(*field, guard);
    }
    filter
}

fn rejected(index: usize, column: Option<&str>, message: impl Into<String>) -> ResponseError {
    ResponseError::new(ErrorCode::InvalidValue, message).with_data(ErrorData {
        change: Some(index as u64),
        column: column.map(str::to_string),
    })
}

fn conflict(index: usize, message: String, matched: usize) -> ResponseError {
    ResponseError::new(ErrorCode::Conflict, message)
        .with_detail(format!("row {} matched {matched} rows", index + 1))
        .with_data(ErrorData {
            change: Some(index as u64),
            column: None,
        })
}

/// Works out one change without writing anything. `lookup` fetches the
/// document whose `_id` is the candidate, with only the projected fields.
fn plan_change(
    index: usize,
    change: &RowChange,
    mut lookup: impl FnMut(&Bson, &Document) -> Result<Option<Document>, ResponseError>,
    mut dominant: impl FnMut(&str) -> Result<Option<Bson>, ResponseError>,
) -> Result<PlannedUpdate, ResponseError> {
    let [
        ColumnValue {
            column,
            value: EditValue::Text { value: id_text },
        },
    ] = change.key.as_slice()
    else {
        return Err(rejected(
            index,
            None,
            "a MongoDB document is addressed by its _id alone",
        ));
    };
    if column != "_id" {
        return Err(rejected(
            index,
            Some(column),
            format!("a MongoDB document is addressed by _id, not `{column}`"),
        ));
    }
    if change.set.is_empty() {
        return Err(rejected(index, None, "there is nothing to set"));
    }

    let mut projection = Document::new();
    for ColumnValue { column, .. } in &change.set {
        if let Some(problem) = edit_values::field_name_problem(column) {
            return Err(rejected(index, Some(column), problem));
        }
        if column == "_id" {
            return Err(rejected(
                index,
                Some(column),
                "the _id of a document cannot be changed",
            ));
        }
        if projection.contains_key(column) {
            return Err(rejected(
                index,
                Some(column),
                format!("`{column}` is set twice in one change"),
            ));
        }
        projection.insert(column.clone(), 1);
    }

    let mut found = Vec::new();
    for candidate in edit_values::id_candidates(id_text) {
        if let Some(document) = lookup(&candidate, &projection)? {
            found.push(document);
        }
    }
    let matched = found.len();
    let document = match (found.pop(), found.is_empty()) {
        (Some(document), true) => document,
        (None, _) => {
            return Err(conflict(
                index,
                format!("no document has the _id {id_text}"),
                0,
            ));
        }
        (Some(_), false) => {
            return Err(conflict(
                index,
                format!("more than one document has the _id {id_text}, stored as different types"),
                matched,
            ));
        }
    };
    let id = document.get("_id").cloned().ok_or_else(|| {
        ResponseError::new(
            ErrorCode::Internal,
            "the server returned a document with no _id",
        )
    })?;

    let mut set = Document::new();
    for ColumnValue { column, value } in &change.set {
        let converted = match value {
            EditValue::Null => Bson::Null,
            EditValue::Text { value } => {
                let held = document.get(column);
                // Its own value says what type it is, unless it has none to
                // say: then the rest of the collection does.
                let exemplar = match held {
                    None | Some(Bson::Null | Bson::Undefined) => dominant(column)?,
                    Some(value) => Some(value.clone()),
                };
                edit_values::convert(value, exemplar.as_ref())
                    .map_err(|message| rejected(index, Some(column), message))?
            }
        };
        set.insert(column.clone(), converted);
    }
    let probes: Vec<(&str, Option<&Bson>)> = change
        .set
        .iter()
        .map(|set| (set.column.as_str(), document.get(&set.column)))
        .collect();
    let filter = guard_filter(&id, &probes);
    Ok(PlannedUpdate { set, filter })
}

/// Transactions need a replica set or a sharded cluster; a standalone server
/// answers the first write of one with an error. Asked of the server up front
/// so the choice is a fact and not a failure to interpret.
fn supports_transactions(live: &Live) -> Result<bool, ResponseError> {
    let admin = live.client.database("admin");
    let reply = match admin.run_command(doc! { "hello": 1 }).run() {
        Ok(reply) => reply,
        // `hello` arrived in 4.4.2 (and 4.5); before it the same question is
        // `isMaster`, and the reply has the same fields.
        Err(error) if hello_is_unknown(server_code(&error)) => admin
            .run_command(doc! { "isMaster": 1 })
            .run()
            .map_err(values::error)?,
        Err(error) => return Err(values::error(error)),
    };
    Ok(has_transactions(&reply))
}

/// Replica set members name their set and a sharded cluster's router says
/// `isdbgrid`; a standalone server says neither. `hello` and `isMaster` answer
/// in the same shape for both fields.
fn has_transactions(reply: &Document) -> bool {
    reply.contains_key("setName")
        || matches!(reply.get("msg"), Some(Bson::String(message)) if message == "isdbgrid")
}

/// The type a field usually has in the first few documents of the collection.
fn sample_field(
    collection: &Collection<Document>,
    field: &str,
) -> Result<Option<Bson>, ResponseError> {
    let sample = collection
        .find(Document::new())
        .projection(doc! { field: 1 })
        .limit(EDIT_SAMPLE_SIZE)
        .run()
        .map_err(values::error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(values::error)?;
    Ok(edit_values::dominant_type(field, &sample).cloned())
}

fn write_one(
    collection: &Collection<Document>,
    index: usize,
    plan: &PlannedUpdate,
    session: Option<&mut ClientSession>,
) -> Result<(), ResponseError> {
    let mut action = collection.update_one(plan.filter.clone(), doc! { "$set": plan.set.clone() });
    if let Some(session) = session {
        action = action.session(session);
    }
    let result = action.run().map_err(|error| write_failed(index, error))?;
    if result.matched_count != 1 {
        return Err(conflict(
            index,
            "the document was changed or deleted since it was read, so nothing was changed"
                .to_string(),
            result.matched_count as usize,
        ));
    }
    Ok(())
}

fn write_failed(index: usize, error: mongodb::error::Error) -> ResponseError {
    let code = server_code(&error);
    let mut mapped = values::error(error);
    // Only a server's own answer is sorted by code; a failure on the way to one
    // (a lost connection, no server selected) keeps the mapping it already has.
    if let Some(code) = code {
        mapped.code = classify_write(code);
        match mapped.code {
            ErrorCode::ReadOnly => {
                mapped.message = "this user is not allowed to write to the collection".to_string();
            }
            ErrorCode::Conflict => {
                mapped.message = "another operation changed the document while it was being \
                                  saved, so nothing was saved -- try again"
                    .to_string();
            }
            _ => {}
        }
    }
    mapped.with_data(ErrorData {
        change: Some(index as u64),
        column: None,
    })
}

fn write_in_transaction(
    live: &Live,
    collection: &Collection<Document>,
    plans: &[PlannedUpdate],
) -> Result<(), ResponseError> {
    let mut session = live.client.start_session().run().map_err(values::error)?;
    session.start_transaction().run().map_err(values::error)?;
    for (index, plan) in plans.iter().enumerate() {
        if let Err(mut error) = write_one(collection, index, plan, Some(&mut session)) {
            if let Err(abort_error) = session.abort_transaction().run()
                && !matches!(
                    abort_error.kind.as_ref(),
                    ErrorKind::Command(command) if command.code == NO_SUCH_TRANSACTION
                )
            {
                let detail = error.detail.take().unwrap_or_default();
                error.detail = Some(format!(
                    "{detail}; rolling the transaction back also failed: {}",
                    values::error(abort_error).message
                ));
            }
            return Err(error);
        }
    }
    session.commit_transaction().run().map_err(values::error)
}

fn write_in_order(
    collection: &Collection<Document>,
    plans: &[PlannedUpdate],
) -> Result<(), ResponseError> {
    for (index, plan) in plans.iter().enumerate() {
        if let Err(mut error) = write_one(collection, index, plan, None) {
            if index > 0 {
                let verb = if index == 1 {
                    "change was"
                } else {
                    "changes were"
                };
                error.message = format!(
                    "{}; {index} earlier {verb} already saved, because this server has no \
                     transactions to undo them",
                    error.message
                );
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::oid::ObjectId;
    use std::cell::RefCell;

    fn text(value: &str) -> EditValue {
        EditValue::Text {
            value: value.to_string(),
        }
    }

    fn change(key: &str, set: Vec<(&str, EditValue)>) -> RowChange {
        RowChange {
            key: vec![ColumnValue {
                column: "_id".into(),
                value: text(key),
            }],
            set: set
                .into_iter()
                .map(|(column, value)| ColumnValue {
                    column: column.into(),
                    value,
                })
                .collect(),
        }
    }

    /// A collection that answers by `_id` and remembers what it was asked.
    struct Fake {
        documents: Vec<Document>,
        asked: RefCell<Vec<Bson>>,
        projections: RefCell<Vec<Document>>,
    }

    impl Fake {
        fn new(documents: Vec<Document>) -> Self {
            Self {
                documents,
                asked: RefCell::default(),
                projections: RefCell::default(),
            }
        }

        fn plan(&self, change: &RowChange) -> Result<PlannedUpdate, ResponseError> {
            plan_change(
                0,
                change,
                |candidate, projection| {
                    self.asked.borrow_mut().push(candidate.clone());
                    self.projections.borrow_mut().push(projection.clone());
                    Ok(self
                        .documents
                        .iter()
                        .find(|document| document.get("_id") == Some(candidate))
                        .cloned())
                },
                |field| Ok(edit_values::dominant_type(field, &self.documents).cloned()),
            )
        }
    }

    fn data(error: &ResponseError) -> (Option<u64>, Option<&str>) {
        let data = error.data.as_ref().expect("the error names its place");
        (data.change, data.column.as_deref())
    }

    #[test]
    fn values_become_the_type_each_field_already_holds() {
        let id = ObjectId::new();
        let fake = Fake::new(vec![
            doc! { "_id": id, "age": 30, "name": "a", "paid": false },
        ]);
        let plan = fake
            .plan(&change(
                &id.to_hex(),
                vec![
                    ("age", text("31")),
                    ("name", EditValue::Null),
                    ("paid", text("true")),
                ],
            ))
            .expect("a valid change");
        assert_eq!(plan.filter.get("_id"), Some(&Bson::ObjectId(id)));
        assert_eq!(
            plan.set,
            doc! { "age": 31, "name": Bson::Null, "paid": true }
        );
    }

    #[test]
    fn only_the_edited_fields_are_fetched() {
        let fake = Fake::new(vec![doc! { "_id": "k", "big": "x" }]);
        fake.plan(&change("k", vec![("big", text("y"))]))
            .expect("plans");
        assert_eq!(fake.projections.borrow()[0], doc! { "big": 1 });
    }

    #[test]
    fn a_key_is_probed_as_object_id_then_integer_then_string() {
        let id = ObjectId::new();
        let fake = Fake::new(vec![]);
        let _ = fake.plan(&change(&id.to_hex(), vec![("a", text("1"))]));
        assert_eq!(
            *fake.asked.borrow(),
            vec![Bson::ObjectId(id), Bson::String(id.to_hex())]
        );

        let fake = Fake::new(vec![]);
        let _ = fake.plan(&change("5", vec![("a", text("1"))]));
        assert_eq!(
            *fake.asked.borrow(),
            vec![Bson::Int64(5), Bson::String("5".into())]
        );
    }

    #[test]
    fn an_integer_key_finds_a_document_stored_with_that_integer() {
        let fake = Fake::new(vec![doc! { "_id": 5_i64, "n": 1 }]);
        let plan = fake
            .plan(&change("5", vec![("n", text("2"))]))
            .expect("found");
        assert_eq!(plan.filter.get("_id"), Some(&Bson::Int64(5)));
        assert_eq!(plan.set, doc! { "n": 2 });
    }

    #[test]
    fn a_key_matching_nothing_is_a_conflict_naming_the_change() {
        let fake = Fake::new(vec![]);
        let error = fake
            .plan(&change("ghost", vec![("a", text("1"))]))
            .err()
            .expect("no such document");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(data(&error), (Some(0), None));
        assert_eq!(error.detail.as_deref(), Some("row 1 matched 0 rows"));
    }

    #[test]
    fn a_key_two_documents_answer_to_is_a_conflict_not_a_guess() {
        let fake = Fake::new(vec![
            doc! { "_id": 5_i64, "n": 1 },
            doc! { "_id": "5", "n": 1 },
        ]);
        let error = fake
            .plan(&change("5", vec![("n", text("2"))]))
            .err()
            .expect("ambiguous");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.detail.as_deref(), Some("row 1 matched 2 rows"));
    }

    #[test]
    fn a_value_of_the_wrong_type_names_the_field_and_the_type_expected() {
        let fake = Fake::new(vec![doc! { "_id": "k", "age": 30 }]);
        let error = fake
            .plan(&change("k", vec![("age", text("old"))]))
            .err()
            .expect("not a number");
        assert_eq!(error.code, ErrorCode::InvalidValue);
        assert_eq!(data(&error), (Some(0), Some("age")));
        assert!(error.message.contains("whole number"), "{}", error.message);
    }

    #[test]
    fn a_field_name_that_is_an_operator_or_a_path_never_reaches_the_server() {
        for name in ["$set", "a.b", "$where", ""] {
            let fake = Fake::new(vec![doc! { "_id": "k" }]);
            let error = fake
                .plan(&change("k", vec![(name, text("x"))]))
                .err()
                .expect(name);
            assert_eq!(error.code, ErrorCode::InvalidValue, "{name}");
            assert_eq!(data(&error), (Some(0), Some(name)));
            assert!(fake.asked.borrow().is_empty(), "{name} was looked up");
        }
    }

    #[test]
    fn the_id_cannot_be_edited_and_a_field_cannot_be_set_twice() {
        let fake = Fake::new(vec![doc! { "_id": "k" }]);
        let id = fake
            .plan(&change("k", vec![("_id", text("other"))]))
            .err()
            .expect("immutable");
        assert_eq!(id.code, ErrorCode::InvalidValue);
        let twice = fake
            .plan(&change("k", vec![("a", text("1")), ("a", text("2"))]))
            .err()
            .expect("duplicate");
        assert_eq!(data(&twice), (Some(0), Some("a")));
    }

    #[test]
    fn a_key_that_is_not_the_id_alone_is_refused() {
        let fake = Fake::new(vec![doc! { "_id": "k" }]);
        let mut other_column = change("k", vec![("a", text("1"))]);
        other_column.key[0].column = "email".into();
        let mut two_columns = change("k", vec![("a", text("1"))]);
        two_columns.key.push(two_columns.key[0].clone());
        let mut null_key = change("k", vec![("a", text("1"))]);
        null_key.key[0].value = EditValue::Null;
        for bad in [other_column, two_columns, null_key] {
            let error = fake.plan(&bad).err().expect("not an _id key");
            assert_eq!(error.code, ErrorCode::InvalidValue);
        }
        let none = RowChange {
            key: vec![],
            set: vec![],
        };
        assert!(fake.plan(&none).is_err());
        assert!(fake.asked.borrow().is_empty());
    }

    #[test]
    fn an_empty_set_is_refused_rather_than_sent_as_an_empty_update() {
        let fake = Fake::new(vec![doc! { "_id": "k" }]);
        let error = fake
            .plan(&change("k", vec![]))
            .err()
            .expect("nothing to set");
        assert_eq!(error.code, ErrorCode::InvalidValue);
    }

    /// The text of a string field is data. A value that looks like an operator
    /// or a JSON document stays a string and is never parsed.
    #[test]
    fn typed_text_is_never_read_as_an_operator() {
        let fake = Fake::new(vec![doc! { "_id": "k", "note": "x" }]);
        let plan = fake
            .plan(&change(
                "k",
                vec![("note", text(r#"{"$set": {"admin": true}}"#))],
            ))
            .expect("plans");
        assert_eq!(plan.set, doc! { "note": r#"{"$set": {"admin": true}}"# });
    }

    #[test]
    fn a_write_the_server_refused_is_sorted_by_what_it_was_refused_for() {
        for validation in [
            2, 14, 28, 40, 52, 53, 56, 66, 121, 11000, 11001, 12582, 10003, 166,
        ] {
            assert_eq!(
                classify_write(validation),
                ErrorCode::InvalidValue,
                "{validation}"
            );
        }
        assert_eq!(classify_write(13), ErrorCode::ReadOnly, "Unauthorized");
        assert_eq!(classify_write(18), ErrorCode::Authentication);
        assert_eq!(classify_write(112), ErrorCode::Conflict, "WriteConflict");
        for topology in [10107, 13435, 13436, 11600, 11602, 91, 189] {
            assert_eq!(
                classify_write(topology),
                ErrorCode::Connection,
                "{topology}"
            );
        }
        // A failure that is not about the value is never reported as one.
        for other in [50, 251, 9001, 1, 8] {
            assert_eq!(classify_write(other), ErrorCode::Internal, "{other}");
        }
    }

    #[test]
    fn a_server_that_does_not_know_hello_is_asked_the_old_way() {
        assert!(hello_is_unknown(Some(59)), "CommandNotFound");
        assert!(!hello_is_unknown(Some(13)), "Unauthorized is not that");
        assert!(!hello_is_unknown(None), "a network failure is not that");
    }

    #[test]
    fn a_replica_set_or_a_cluster_has_transactions_and_a_standalone_does_not() {
        assert!(has_transactions(
            &doc! { "setName": "rs0", "ismaster": true }
        ));
        assert!(has_transactions(&doc! { "msg": "isdbgrid" }));
        assert!(!has_transactions(
            &doc! { "ismaster": true, "msg": "other" }
        ));
        assert!(!has_transactions(&doc! { "isWritablePrimary": true }));
    }

    /// The type each edited field had when it was read goes into the filter of
    /// the write, so a field another client changed in between matches nothing
    /// and the save is a conflict instead of a value of the wrong type.
    #[test]
    fn the_write_is_filtered_on_the_type_each_field_had_when_it_was_read() {
        let id = ObjectId::new();
        let fake = Fake::new(vec![
            doc! { "_id": id, "age": 30, "name": "a", "gone": Bson::Null, "big": 5_i64 },
        ]);
        let plan = fake
            .plan(&change(
                &id.to_hex(),
                vec![
                    ("age", text("31")),
                    ("name", text("b")),
                    ("gone", text("c")),
                    ("big", text("6")),
                    ("fresh", text("d")),
                ],
            ))
            .expect("a valid change");
        assert_eq!(
            plan.filter,
            doc! {
                "_id": id,
                "age": { "$type": "int" },
                "name": { "$type": "string" },
                "gone": { "$type": "null" },
                "big": { "$type": "long" },
                "fresh": { "$exists": false },
            }
        );
    }

    #[test]
    fn every_type_has_the_alias_the_server_reads_in_a_type_filter() {
        use mongodb::bson::{Binary, DateTime, Decimal128, Regex, Timestamp, spec::BinarySubtype};
        let cases: Vec<(Bson, &str)> = vec![
            (Bson::Double(1.0), "double"),
            (Bson::String(String::new()), "string"),
            (Bson::Document(doc! {}), "object"),
            (Bson::Array(vec![]), "array"),
            (
                Bson::Binary(Binary {
                    subtype: BinarySubtype::Generic,
                    bytes: vec![],
                }),
                "binData",
            ),
            (Bson::Undefined, "undefined"),
            (Bson::ObjectId(ObjectId::new()), "objectId"),
            (Bson::Boolean(true), "bool"),
            (Bson::DateTime(DateTime::now()), "date"),
            (Bson::Null, "null"),
            (
                Bson::RegularExpression(Regex {
                    pattern: String::new(),
                    options: String::new(),
                }),
                "regex",
            ),
            (Bson::Int32(1), "int"),
            (
                Bson::Timestamp(Timestamp {
                    time: 1,
                    increment: 1,
                }),
                "timestamp",
            ),
            (Bson::Int64(1), "long"),
            (Bson::Decimal128(Decimal128::from_bytes([0; 16])), "decimal"),
            (Bson::MinKey, "minKey"),
            (Bson::MaxKey, "maxKey"),
        ];
        for (value, alias) in cases {
            assert_eq!(edit_values::type_alias(&value), alias, "{value:?}");
        }
    }

    /// A field the document being edited does not hold takes the type the rest
    /// of the collection gives it, where it used to take a string whatever the
    /// collection said.
    #[test]
    fn a_field_this_document_lacks_takes_the_type_its_neighbours_hold() {
        let fake = Fake::new(vec![
            doc! { "_id": "a", "age": 30 },
            doc! { "_id": "b", "age": 41 },
            doc! { "_id": "c", "name": "no age yet", "gone": Bson::Null },
        ]);
        let plan = fake
            .plan(&change(
                "c",
                vec![
                    ("age", text("52")),
                    ("gone", text("x")),
                    ("nick", text("7")),
                ],
            ))
            .expect("a valid change");
        assert_eq!(
            plan.set,
            doc! { "age": 52, "gone": "x", "nick": "7" },
            "age follows its neighbours; a field nobody holds stays a string"
        );
        let refused = fake
            .plan(&change("c", vec![("age", text("old"))]))
            .err()
            .expect("not a number");
        assert_eq!(refused.code, ErrorCode::InvalidValue);
        assert_eq!(data(&refused), (Some(0), Some("age")));
    }
}
