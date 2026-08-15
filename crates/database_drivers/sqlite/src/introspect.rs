use database::protocol::{ColumnDef, ResponseError, SchemaRef, TableKind, TableRef};
use rusqlite::Connection;

use crate::values;

/// SQLite's schemas are its attached databases.
///
/// Always at least `main`. `temp` is dropped: it exists on every connection,
/// holds nothing the user put there, and a tree that shows it makes every
/// database look like it has two.
pub fn schemas(connection: &Connection) -> Result<Vec<SchemaRef>, ResponseError> {
    let mut statement = connection
        .prepare("PRAGMA database_list")
        .map_err(values::error)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(values::error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(values::error)?;

    Ok(names
        .into_iter()
        .filter(|name| name != "temp")
        .map(|name| SchemaRef {
            is_default: name == "main",
            name,
        })
        .collect())
}

/// Tables and views in one schema.
///
/// `sqlite_master` is per-database, so the schema has to be part of the
/// identifier rather than a parameter -- which is why it is quoted here instead.
/// SQLite has no materialised views, so that arm never appears.
pub fn tables(connection: &Connection, schema: &str) -> Result<Vec<TableRef>, ResponseError> {
    let sql = format!(
        "SELECT name, type FROM {}.sqlite_master \
         WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
         ORDER BY name",
        quote_identifier(schema)
    );
    let mut statement = connection.prepare(&sql).map_err(values::error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(values::error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|(name, kind)| TableRef {
            name,
            kind: if kind == "view" {
                TableKind::View
            } else {
                TableKind::Table
            },
        })
        .collect())
}

/// Columns of one table.
///
/// `PRAGMA table_info` already answers the primary key -- its `pk` column is 0
/// for "not part of it" and otherwise the position within it -- so no second
/// round through `index_list`/`index_info` is needed. A table with no primary
/// key simply reports 0 everywhere, which is the honest answer rather than an
/// error.
pub fn columns(
    connection: &Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnDef>, ResponseError> {
    let sql = format!(
        "PRAGMA {}.table_info({})",
        quote_identifier(schema),
        quote_identifier(table)
    );
    let mut statement = connection.prepare(&sql).map_err(values::error)?;
    let columns = statement
        .query_map([], |row| {
            Ok(ColumnDef {
                name: row.get::<_, String>(1)?,
                // Blank for an expression column or an untyped one. Left as the
                // empty string rather than invented: SQLite really does not
                // know, and saying `TEXT` would be a guess shown as a fact.
                type_name: row.get::<_, String>(2).unwrap_or_default(),
                nullable: row.get::<_, i64>(3)? == 0,
                primary_key: row.get::<_, i64>(5)? > 0,
            })
        })
        .map_err(values::error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(values::error)?;
    Ok(columns)
}

/// Wraps an identifier in double quotes, doubling any it already contains.
///
/// Schema and table names reach us from the tree, which got them from the
/// database -- but a table really can be called `"; drop table users; --`, and
/// SQLite has no way to bind an identifier as a parameter.
pub fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_carrying_quotes_is_still_one_identifier() {
        assert_eq!(quote_identifier("users"), "\"users\"");
        assert_eq!(
            quote_identifier("odd\"name"),
            "\"odd\"\"name\"",
            "an embedded quote must be doubled, not dropped"
        );
        assert_eq!(
            quote_identifier("\"; DROP TABLE users; --"),
            "\"\"\"; DROP TABLE users; --\"",
            "a table may genuinely be named this, and it must stay one identifier"
        );
    }
}
