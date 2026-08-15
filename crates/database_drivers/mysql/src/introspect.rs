use crate::values;
use database::protocol::{ColumnDef, ResponseError, SchemaRef, TableKind, TableRef};
use mysql::Conn;
use mysql::prelude::Queryable as _;

/// MySQL's databases are what the protocol calls schemas.
///
/// The mapping is the driver's job, not the protocol's -- this is the one place
/// MySQL genuinely disagrees with the other two engines, and the answer is to
/// translate here rather than to give the protocol a second word for the same
/// idea. The server's own four (`mysql`, `information_schema`,
/// `performance_schema`, `sys`) are filtered out: they are on every server and
/// nobody browsing their own data wants them.
pub fn schemas(conn: &mut Conn, current: Option<&str>) -> Result<Vec<SchemaRef>, ResponseError> {
    let names: Vec<String> = conn
        .query(
            "SELECT schema_name FROM information_schema.schemata \
             WHERE schema_name NOT IN \
                 ('mysql', 'information_schema', 'performance_schema', 'sys') \
             ORDER BY schema_name",
        )
        .map_err(values::error)?;

    Ok(names
        .into_iter()
        .map(|name| SchemaRef {
            is_default: Some(name.as_str()) == current,
            name,
        })
        .collect())
}

pub fn tables(conn: &mut Conn, schema: &str) -> Result<Vec<TableRef>, ResponseError> {
    let rows: Vec<(String, String)> = conn
        .exec(
            "SELECT table_name, table_type FROM information_schema.tables \
             WHERE table_schema = ? ORDER BY table_name",
            (schema,),
        )
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|(name, kind)| TableRef {
            name,
            // MySQL has no materialised views, so that arm never appears --
            // which is exactly the sort of thing a driver answers for itself
            // rather than the protocol pretending every engine is the same.
            kind: if kind == "VIEW" {
                TableKind::View
            } else {
                TableKind::Table
            },
        })
        .collect())
}

pub fn columns(
    conn: &mut Conn,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnDef>, ResponseError> {
    let rows: Vec<(String, String, String, String)> = conn
        .exec(
            "SELECT column_name, column_type, is_nullable, column_key \
             FROM information_schema.columns \
             WHERE table_schema = ? AND table_name = ? \
             ORDER BY ordinal_position",
            (schema, table),
        )
        .map_err(values::error)?;

    Ok(rows
        .into_iter()
        .map(|(name, type_name, nullable, key)| ColumnDef {
            name,
            // `column_type` rather than `data_type`: the former carries the
            // width and signedness (`tinyint(1) unsigned`), which is most of
            // what makes a MySQL column definition mean anything.
            type_name,
            nullable: nullable == "YES",
            primary_key: key == "PRI",
        })
        .collect())
}

/// The database a fresh connection is pointed at, if the URL named one.
///
/// `None` is a legitimate answer -- a MySQL URL need not name a database, and
/// inventing one would point the tree somewhere the user did not ask for.
pub fn current_schema(conn: &mut Conn) -> Result<Option<String>, ResponseError> {
    let current: Option<Option<String>> = conn
        .query_first("SELECT DATABASE()")
        .map_err(values::error)?;
    Ok(current.flatten())
}
