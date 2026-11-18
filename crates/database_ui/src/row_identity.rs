//! Which row of a table a row of the grid is.
//!
//! Editing a row means writing `WHERE <key> = <value>`, and that is only
//! correct if the key picks out exactly this row. A table without a primary
//! key, or a page that does not carry all of it, has no such answer, and the
//! honest outcome is that the row cannot be edited -- never a guess at one
//! column that happens to look unique.
use crate::query::Page;
use database::protocol::{Cell, ResultColumn};

/// The key of one row: its columns and what they hold.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowIdentity {
    pub(crate) columns: Vec<String>,
    pub(crate) values: Vec<Cell>,
}

/// Why a row has no identity, in words the grid can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NoRowIdentity {
    /// The page is a statement somebody typed, not a table.
    NotATable,
    /// The table has no primary key.
    NoKey,
    /// The page does not carry this key column, so the row cannot be named.
    KeyColumnMissing(String),
    /// A key holding `NULL` matches nothing with `=`, so `WHERE` would touch
    /// no row -- or, written as `IS NULL`, every row that shares it.
    NullInKey(String),
    /// A key the grid only holds a rendering of: a blob arrives as its size and
    /// a JSON document as the driver's text for it, and a `WHERE` written from
    /// either would match whatever rows share the rendering.
    UnnamableKey(String),
    NoSuchRow,
}

impl NoRowIdentity {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NotATable => "read-only: not a table".into(),
            Self::NoKey => "read-only: no primary key".into(),
            Self::KeyColumnMissing(column) => {
                format!("read-only: key column {column} is not in the result")
            }
            Self::NullInKey(column) => format!("read-only: key column {column} is NULL"),
            Self::UnnamableKey(column) => {
                format!("read-only: key column {column} cannot be written back exactly")
            }
            Self::NoSuchRow => "read-only: no such row".into(),
        }
    }
}

/// The identity of `row`, whose cells are laid out as `result_columns`.
///
/// What a key cell holds is the driver's rendering of the value -- text, a
/// number as digits, a timestamp as a string. The editing round writes it back
/// as a literal for the column's type, which is what makes that a name for the
/// row; this only refuses the keys that cannot be written back at all. A
/// document store's `_id` is kept as it arrives and is not usable for editing
/// until its type is known the same way.
pub(crate) fn identity_of(
    key_columns: &[String],
    result_columns: &[ResultColumn],
    row: &[Cell],
) -> Result<RowIdentity, NoRowIdentity> {
    if key_columns.is_empty() {
        return Err(NoRowIdentity::NoKey);
    }
    let mut values = Vec::with_capacity(key_columns.len());
    for key in key_columns {
        let cell = result_columns
            .iter()
            .position(|column| column.name == *key)
            .and_then(|position| row.get(position))
            .ok_or_else(|| NoRowIdentity::KeyColumnMissing(key.clone()))?;
        match cell {
            Cell::Null => return Err(NoRowIdentity::NullInKey(key.clone())),
            Cell::Binary { .. } | Cell::Json { .. } => {
                return Err(NoRowIdentity::UnnamableKey(key.clone()));
            }
            _ => {}
        }
        values.push(cell.clone());
    }
    Ok(RowIdentity {
        columns: key_columns.to_vec(),
        values,
    })
}

impl Page {
    /// The identity of row `index` of this page.
    pub(crate) fn row_identity(&self, index: usize) -> Result<RowIdentity, NoRowIdentity> {
        let browse = self.browse.as_ref().ok_or(NoRowIdentity::NotATable)?;
        let row = self
            .result
            .rows
            .get(index)
            .ok_or(NoRowIdentity::NoSuchRow)?;
        identity_of(&browse.key_columns, &self.result.columns, row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(names: &[&str]) -> Vec<ResultColumn> {
        names
            .iter()
            .map(|name| ResultColumn {
                name: (*name).into(),
                type_name: "text".into(),
            })
            .collect()
    }

    fn text(value: &str) -> Cell {
        Cell::Text {
            value: value.into(),
        }
    }

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn a_single_column_key_names_the_row_by_its_value() {
        let identity = identity_of(
            &keys(&["id"]),
            &columns(&["name", "id"]),
            &[text("ada"), text("7")],
        );
        assert_eq!(
            identity,
            Ok(RowIdentity {
                columns: keys(&["id"]),
                values: vec![text("7")],
            })
        );
    }

    /// Both halves, in the order the key declares them rather than the order
    /// the result happens to list them.
    #[test]
    fn a_composite_key_carries_every_column_in_key_order() {
        let identity = identity_of(
            &keys(&["tenant", "id"]),
            &columns(&["id", "note", "tenant"]),
            &[text("7"), text("x"), text("acme")],
        );
        assert_eq!(
            identity,
            Ok(RowIdentity {
                columns: keys(&["tenant", "id"]),
                values: vec![text("acme"), text("7")],
            })
        );
    }

    #[test]
    fn a_table_without_a_key_has_no_identity() {
        assert_eq!(
            identity_of(&[], &columns(&["a"]), &[text("1")]),
            Err(NoRowIdentity::NoKey)
        );
    }

    /// Half a composite key would match every row sharing that half.
    #[test]
    fn a_key_column_missing_from_the_result_is_no_identity() {
        assert_eq!(
            identity_of(&keys(&["tenant", "id"]), &columns(&["id"]), &[text("7")]),
            Err(NoRowIdentity::KeyColumnMissing("tenant".into()))
        );
    }

    #[test]
    fn a_row_shorter_than_its_columns_is_no_identity_rather_than_a_panic() {
        assert_eq!(
            identity_of(&keys(&["id"]), &columns(&["name", "id"]), &[text("ada")]),
            Err(NoRowIdentity::KeyColumnMissing("id".into()))
        );
    }

    #[test]
    fn a_null_in_the_key_is_no_identity() {
        assert_eq!(
            identity_of(
                &keys(&["tenant", "id"]),
                &columns(&["tenant", "id"]),
                &[text("acme"), Cell::Null]
            ),
            Err(NoRowIdentity::NullInKey("id".into()))
        );
    }

    /// A blob arrives as its size, not its bytes, and a JSON document as the
    /// driver's rendering of one: a `WHERE` written from either would match
    /// whatever rows happen to share the rendering.
    #[test]
    fn a_key_the_grid_only_shows_a_rendering_of_names_no_row() {
        for cell in [
            Cell::Binary { byte_len: 16 },
            Cell::Json {
                value: "{\"a\": 1}".into(),
            },
        ] {
            assert_eq!(
                identity_of(&keys(&["id"]), &columns(&["id"]), &[cell]),
                Err(NoRowIdentity::UnnamableKey("id".into()))
            );
        }
    }

    #[test]
    fn every_reason_reads_as_read_only() {
        for reason in [
            NoRowIdentity::NotATable,
            NoRowIdentity::NoKey,
            NoRowIdentity::KeyColumnMissing("id".into()),
            NoRowIdentity::NullInKey("id".into()),
            NoRowIdentity::UnnamableKey("id".into()),
            NoRowIdentity::NoSuchRow,
        ] {
            assert!(reason.message().starts_with("read-only: "), "{reason:?}");
        }
        assert_eq!(NoRowIdentity::NoKey.message(), "read-only: no primary key");
    }
}
