//! What the tree shows: databases, collections, and the fields a collection's
//! documents actually carry.

use crate::values;
use database::protocol::{ColumnDef, ResponseError, SchemaRef, TableKind, TableRef};
use mongodb::bson::Document;
use mongodb::sync::Client;

/// The databases MongoDB's own tooling hides, and for the same reason: they are
/// the server's bookkeeping, not anybody's data.
const INTERNAL_DATABASES: [&str; 3] = ["admin", "config", "local"];

pub fn databases(client: &Client, default: &str) -> Result<Vec<SchemaRef>, ResponseError> {
    let mut names = client.list_database_names().run().map_err(values::error)?;
    names.retain(|name| !INTERNAL_DATABASES.contains(&name.as_str()));
    names.sort();
    Ok(names
        .into_iter()
        .map(|name| SchemaRef {
            is_default: name == default,
            name,
        })
        .collect())
}

pub fn collections(client: &Client, database: &str) -> Result<Vec<TableRef>, ResponseError> {
    let mut tables: Vec<TableRef> = client
        .database(database)
        .list_collections()
        .run()
        .map_err(values::error)?
        .filter_map(|collection| collection.ok())
        .map(|collection| TableRef {
            // A MongoDB view is defined by a pipeline over another collection
            // and is read-only at the server, which is exactly the distinction
            // `TableKind` exists to carry.
            kind: match collection.collection_type {
                mongodb::results::CollectionType::View => TableKind::View,
                _ => TableKind::Table,
            },
            name: collection.name,
        })
        .collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(tables)
}

/// How many documents a field list is inferred from.
///
/// A sample, because a collection has no declared schema and reading all of it
/// to describe it would be the most expensive thing this driver ever did. Small
/// enough to stay instant on a collection of millions.
pub const SAMPLE_SIZE: i64 = 50;

/// The fields a collection's documents carry, inferred from a sample.
///
/// Every other engine here answers `describe_table` from a catalogue. MongoDB
/// has none, so this is an inference and is labelled as one: `nullable` is true
/// for every field that did not appear in every sampled document, which is the
/// honest reading of "this field is sometimes absent".
pub fn fields(
    client: &Client,
    database: &str,
    collection: &str,
) -> Result<Vec<ColumnDef>, ResponseError> {
    let sample: Vec<Document> = client
        .database(database)
        .collection::<Document>(collection)
        .find(Document::new())
        .limit(SAMPLE_SIZE)
        .run()
        .map_err(values::error)?
        .filter_map(|document| document.ok())
        .collect();

    Ok(describe_sample(&sample))
}

/// The column list a sample of documents implies.
///
/// Split out from the query so the inference itself can be tested without a
/// server -- it is the part with a decision in it.
pub fn describe_sample(sample: &[Document]) -> Vec<ColumnDef> {
    values::columns_across(sample)
        .into_iter()
        .map(|name| {
            let present = sample
                .iter()
                .filter(|document| document.contains_key(&name))
                .count();
            let type_name = sample
                .iter()
                .find_map(|document| document.get(&name))
                .map(values::type_name)
                .unwrap_or("")
                .to_string();
            ColumnDef {
                // `_id` is the one field MongoDB itself guarantees and indexes.
                primary_key: name == "_id",
                // Inferred, not declared: a field missing from any sampled
                // document is one that can be absent.
                nullable: present < sample.len(),
                name,
                type_name,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    /// The inference this whole module is: no catalogue exists, so the field
    /// list is what the documents happen to carry.
    #[test]
    fn a_field_missing_from_one_document_is_reported_as_nullable() {
        let sample = vec![doc! { "_id": 1, "name": "a" }, doc! { "_id": 2 }];
        let columns = describe_sample(&sample);

        let id = &columns[0];
        assert_eq!(id.name, "_id");
        assert!(id.primary_key, "_id is the one field MongoDB guarantees");
        assert!(!id.nullable, "every sampled document carried it");

        let name = &columns[1];
        assert_eq!(name.name, "name");
        assert!(
            name.nullable,
            "a field absent from a sampled document can be absent"
        );
    }

    #[test]
    fn the_engines_own_type_names_reach_the_column() {
        let sample = vec![doc! { "_id": 1, "when": mongodb::bson::DateTime::MAX }];
        let columns = describe_sample(&sample);
        assert_eq!(columns[1].type_name, "date");
    }

    /// An empty collection describes to nothing rather than to a guess.
    #[test]
    fn an_empty_sample_describes_no_fields() {
        assert!(describe_sample(&[]).is_empty());
    }
}
