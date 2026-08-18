use database::protocol::{ColumnDef, ResponseError, SchemaRef, TableRef};
use postgres::Client;

use crate::values;

/// Schemas the user put there.
///
/// `pg_catalog`, `information_schema` and the `pg_toast*` families are filtered
/// out: they exist on every database, nobody browsing their own data wants
/// them, and listing them makes every database look like it has forty schemas.
pub fn schemas(client: &mut Client, current: &str) -> Result<Vec<SchemaRef>, ResponseError> {
    let rows = client
        .query(
            "SELECT nspname FROM pg_namespace \
             WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' \
             ORDER BY nspname",
            &[],
        )
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let name: String = row.get(0);
            SchemaRef {
                is_default: name == current,
                name,
            }
        })
        .collect())
}

/// Tables, views and materialised views in one schema.
///
/// Materialised views are told apart because reading one costs nothing while
/// reading a view can cost a great deal -- that is worth knowing before
/// clicking.
pub fn tables(client: &mut Client, schema: &str) -> Result<Vec<TableRef>, ResponseError> {
    let rows = client
        .query(
            "SELECT c.relname, c.relkind \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'v', 'm') \
             ORDER BY c.relname",
            &[&schema],
        )
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let name: String = row.get(0);
            let kind: i8 = row.get(1);
            TableRef {
                name,
                kind: match kind as u8 as char {
                    'v' => database::protocol::TableKind::View,
                    'm' => database::protocol::TableKind::MaterializedView,
                    _ => database::protocol::TableKind::Table,
                },
            }
        })
        .collect())
}

/// Columns of one table, with the primary key marked.
///
/// `pg_attribute` rather than `information_schema.columns`: the catalog answers
/// for materialised views too, which `information_schema` does not list at all.
pub fn columns(
    client: &mut Client,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnDef>, ResponseError> {
    let rows = client
        .query(
            "SELECT a.attname, \
                    format_type(a.atttypid, a.atttypmod), \
                    NOT a.attnotnull, \
                    COALESCE(i.indisprimary, false) \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_index i ON i.indrelid = c.oid \
                                 AND a.attnum = ANY(i.indkey) \
                                 AND i.indisprimary \
             WHERE n.nspname = $1 AND c.relname = $2 \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[&schema, &table],
        )
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|row| ColumnDef {
            name: row.get(0),
            type_name: row.get(1),
            nullable: row.get(2),
            primary_key: row.get(3),
        })
        .collect())
}

/// The schema a fresh connection is already pointed at.
///
/// `current_schema()` rather than the first entry of `search_path`: the two
/// disagree when the first entry does not exist, and it is the former that
/// decides where an unqualified name resolves.
pub fn current_schema(client: &mut Client) -> Result<String, ResponseError> {
    let row = client
        .query_one("SELECT current_schema()", &[])
        .map_err(values::error)?;
    Ok(row
        .try_get::<_, Option<String>>(0)
        .unwrap_or(None)
        .unwrap_or_else(|| "public".into()))
}
