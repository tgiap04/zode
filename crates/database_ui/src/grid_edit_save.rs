//! Saving the edits of a page: what is sent, what the connection's safety level
//! says about sending it, and what an answer -- or a refusal -- does to the
//! cells.
//!
//! Nothing here writes SQL to run. The driver is handed typed changes (a row's
//! key and the values to set) and binds them itself; the statements shown in the
//! preview are for reading.

use crate::database_panel::{ConnectionKey, DatabasePanel};
use crate::grid_editing::{CellAddress, PageEdits, Report, changes_label};
use crate::query::{Page, QueryState};
use crate::safe_mode::{Gate, SafetyLevel, StatementKind, gate, preview};
use crate::sql_dialect::Dialect;
use database::client::DriverError;
use database::protocol::{Cell, ColumnValue, EditValue, ErrorCode, RowChange};
use gpui::{Context, PromptLevel, SharedString, Window};
use std::collections::BTreeMap;

/// What a failed save says about a row that matched nothing: a conflict, in the
/// words the cell shows.
const CONFLICT: &str = "changed or removed since it was read — reload the page";

/// Everything one save sends, and where each part came from on the page, so a
/// refusal that names change `n` can be put on the cell it was about.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SaveBatch {
    pub(crate) connection: ConnectionKey,
    pub(crate) schema: String,
    pub(crate) table: String,
    pub(crate) changes: Vec<RowChange>,
    /// For change `n`: the page row, and the page columns its `set` is made of,
    /// in the order `set` lists them.
    pub(crate) origins: Vec<(usize, Vec<usize>)>,
}

impl Page {
    /// The edits as changes, one per edited row, each addressed by its whole
    /// key. `Err` says why the page cannot be saved at all.
    pub(crate) fn save_batch(&self) -> Result<SaveBatch, String> {
        let browse = self
            .browse
            .as_ref()
            .ok_or_else(|| "this is not a table".to_string())?;
        // The level, the dialect and the session all come from the page's
        // connection. A batch for the table of another one would be judged by
        // the first and written by the second.
        if self.connection.as_ref() != Some(&browse.connection) {
            return Err("this page and its table come from different connections".into());
        }
        let mut by_row: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for address in self.edits.staged.keys() {
            by_row.entry(address.row).or_default().push(address.column);
        }
        let mut changes = Vec::with_capacity(by_row.len());
        let mut origins = Vec::with_capacity(by_row.len());
        for (row, columns) in by_row {
            let identity = self.row_identity(row).map_err(|reason| reason.message())?;
            let key = identity
                .columns
                .iter()
                .zip(&identity.values)
                .map(|(column, cell)| {
                    EditValue::from_cell(cell)
                        .map(|value| ColumnValue {
                            column: column.clone(),
                            value,
                        })
                        .ok_or_else(|| format!("key column {column} cannot be sent"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let set = columns
                .iter()
                .filter_map(|column| {
                    let staged = self.edits.staged.get(&CellAddress {
                        row,
                        column: *column,
                    })?;
                    Some(ColumnValue {
                        column: self.column_name(*column).to_string(),
                        value: match &staged.value {
                            Some(text) => EditValue::Text {
                                value: text.clone(),
                            },
                            None => EditValue::Null,
                        },
                    })
                })
                .collect();
            changes.push(RowChange { key, set });
            origins.push((row, columns));
        }
        Ok(SaveBatch {
            connection: browse.connection.clone(),
            schema: browse.schema.clone(),
            table: browse.table.clone(),
            changes,
            origins,
        })
    }

    fn is_number_at(&self, row: usize, column: usize) -> bool {
        matches!(
            self.result.rows.get(row).and_then(|row| row.get(column)),
            Some(Cell::Number { .. })
        )
    }

    /// The statements the batch stands for, to be read and not run.
    pub(crate) fn preview_sql(&self, dialect: &Dialect, batch: &SaveBatch) -> String {
        let literal = |value: &EditValue, numeric: bool| match value {
            EditValue::Null => dialect.preview_literal(None, numeric),
            EditValue::Text { value } => dialect.preview_literal(Some(value), numeric),
        };
        batch
            .changes
            .iter()
            .zip(&batch.origins)
            .map(|(change, (row, columns))| {
                let key: Vec<(&str, String)> = change
                    .key
                    .iter()
                    .map(|part| {
                        let numeric = self
                            .result
                            .columns
                            .iter()
                            .position(|column| column.name == part.column)
                            .is_some_and(|column| self.is_number_at(*row, column));
                        (part.column.as_str(), literal(&part.value, numeric))
                    })
                    .collect();
                let set: Vec<(&str, String)> = change
                    .set
                    .iter()
                    .zip(columns)
                    .map(|(part, column)| {
                        (
                            part.column.as_str(),
                            literal(&part.value, self.is_number_at(*row, *column)),
                        )
                    })
                    .collect();
                dialect.update_preview(&batch.schema, &batch.table, &set, &key)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Puts a refusal from the database where it belongs: on the cell it named,
    /// on the row it named, or in the bar when it named neither.
    fn apply_save_error(&mut self, batch: &SaveBatch, error: &anyhow::Error) {
        let driver = error.downcast_ref::<DriverError>();
        let data = driver.and_then(|error| error.0.data.as_ref());
        let origin = data
            .and_then(|data| data.change)
            .and_then(|change| usize::try_from(change).ok())
            .and_then(|change| batch.origins.get(change));
        match (driver.map(DriverError::code), origin) {
            (Some(ErrorCode::Conflict), Some((row, _))) => {
                self.edits.row_errors.insert(*row, CONFLICT.into());
                self.edits.failure = Some(
                    format!(
                        "Nothing was saved: row {} was {CONFLICT}.",
                        self.offset + *row as u64 + 1
                    )
                    .into(),
                );
            }
            (Some(ErrorCode::InvalidValue), Some((row, columns))) => {
                let named = data
                    .and_then(|data| data.column.as_deref())
                    .and_then(|name| {
                        columns
                            .iter()
                            .copied()
                            .find(|column| self.column_name(*column) == name)
                    });
                let address = CellAddress {
                    row: *row,
                    column: named.or(columns.first().copied()).unwrap_or_default(),
                };
                let message = driver.map(ToString::to_string).unwrap_or_default();
                if let Some(staged) = self.edits.staged.get_mut(&address) {
                    staged.error = Some(message);
                    staged.refused = true;
                }
                self.edits.failure =
                    Some("Nothing was saved: the database rejected a value.".into());
            }
            _ => {
                self.edits.failure = Some(format!("Could not save: {error:#}").into());
            }
        }
    }
}

impl DatabasePanel {
    /// Save: stages what is half typed, checks every value again, and sends the
    /// batch as far as the connection's safety level lets it.
    pub(crate) fn request_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor(cx);
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if page.edits.staged.is_empty() || page.edits.saving.is_some() {
            return;
        }
        let Some(environment) = self.environment_or_say(Report::Bar, cx) else {
            return;
        };
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        let invalid = page.revalidate(&environment.dialect);
        let batch = if invalid > 0 {
            Err(format!("Fix the {invalid} invalid values before saving."))
        } else {
            page.save_batch()
        };
        let batch = match batch {
            Ok(batch) => batch,
            Err(message) => {
                page.edits.failure = Some(message.into());
                cx.notify();
                return;
            }
        };
        page.edits.failure = None;
        match gate(environment.level, StatementKind::Write) {
            Gate::Run => self.send_save(batch, window, cx),
            Gate::Confirm => self.confirm_save(batch, window, cx),
            Gate::Refuse => self.refuse_save(&batch, cx),
        }
        cx.notify();
    }

    fn refuse_save(&mut self, batch: &SaveBatch, cx: &mut Context<Self>) {
        let name = &batch.connection.0;
        if let QueryState::Done(page) = &mut self.query {
            page.edits.failure = Some(
                format!("{name} is set to Read Only, so nothing was saved. Discard the edits or raise its safety level.")
                    .into(),
            );
        }
        cx.notify();
    }

    fn confirm_save(&mut self, batch: SaveBatch, window: &mut Window, cx: &mut Context<Self>) {
        let (QueryState::Done(page), Ok(environment)) = (&self.query, self.edit_environment(cx))
        else {
            return;
        };
        let detail = preview(
            Some(&environment.dialect),
            &page.preview_sql(&environment.dialect, &batch),
        );
        let count: usize = batch.changes.iter().map(|change| change.set.len()).sum();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Save {} to {} on {}?",
                saved_label(count),
                batch.table,
                batch.connection.0
            ),
            Some(&detail),
            // Cancel first, so that the key that confirms a dialog -- Return --
            // is not the one that writes.
            &["Cancel", "Save"],
            cx,
        );
        let task = cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() != Some(1) {
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.save_confirmed(batch, window, cx)
            })
            .ok();
        });
        self.keep_task(task);
    }

    /// Carries out a save the user said yes to.
    ///
    /// Checked again, because the prompt stays up while other things happen: the
    /// yes was for these changes on this connection, and either may have moved.
    fn save_confirmed(&mut self, batch: SaveBatch, window: &mut Window, cx: &mut Context<Self>) {
        let Some(environment) = self.environment_or_say(Report::Bar, cx) else {
            return;
        };
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        if page.edits.saving.is_some() {
            return;
        }
        if environment.level == SafetyLevel::ReadOnly {
            self.refuse_save(&batch, cx);
            return;
        }
        if page.save_batch().as_ref() != Ok(&batch) {
            page.edits.failure = Some(
                "The edits changed while the question was open, so nothing was saved. Save again."
                    .into(),
            );
            cx.notify();
            return;
        }
        self.send_save(batch, window, cx);
    }

    fn send_save(&mut self, batch: SaveBatch, window: &mut Window, cx: &mut Context<Self>) {
        // The session the page was read from, not whichever now answers to the
        // connection's name.
        let session = match self.page_session() {
            Ok((_, session)) => session,
            Err(message) => {
                if let QueryState::Done(page) = &mut self.query {
                    page.edits.failure = Some(format!("Could not save: {message}").into());
                }
                cx.notify();
                return;
            }
        };
        let save_id = self.next_request_id();
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        page.edits.saving = Some(save_id);
        page.edits.failure = None;
        page.edits.row_errors.clear();
        cx.notify();

        let task = cx.spawn_in(window, async move |this, cx| {
            let result = session
                .update_rows(&batch.schema, &batch.table, batch.changes.clone())
                .await;
            this.update_in(cx, |this, window, cx| {
                this.save_finished(batch, result, save_id, window, cx)
            })
            .ok();
        });
        self.keep_task(task);
    }

    fn save_finished(
        &mut self,
        batch: SaveBatch,
        result: anyhow::Result<u64>,
        save_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Whether the page can be read again by the session it was written
        // through, on the connection the user is on: otherwise reading it would
        // show another database, or swap the editor from under whatever is
        // being typed on the one they moved to.
        let readable = self
            .page_session()
            .is_ok_and(|(index, _)| self.active == Some(index));
        let QueryState::Done(page) = &mut self.query else {
            return;
        };
        // An answer is for the save it answers: the page it was sent from may
        // be gone, and its successor may be saving too.
        if page.edits.saving != Some(save_id) {
            return;
        }
        page.edits.saving = None;
        let cells: usize = batch.changes.iter().map(|change| change.set.len()).sum();
        match result {
            Ok(updated) => {
                let note = format!("Saved {} in {}", saved_label(cells), rows_label(updated));
                let (sql, offset, limit, browse) = (
                    page.sql.clone(),
                    page.offset,
                    page.limit,
                    page.browse.clone(),
                );
                if readable {
                    page.edits = PageEdits::default();
                    // The same page, in the same order: what was written is read
                    // back rather than assumed.
                    self.saved_note = Some(SharedString::from(note));
                    self.run_page(sql, offset, limit, browse, window, cx);
                } else {
                    page.edits = PageEdits::saved_unread(
                        format!("{note} — reload the page to see it").into(),
                    );
                }
            }
            Err(error) => page.apply_save_error(&batch, &error),
        }
        cx.notify();
    }

    /// Preview SQL: the statements a save would stand for, without saving.
    pub(crate) fn show_save_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor(cx);
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if page.edits.staged.is_empty() {
            return;
        }
        let Some(environment) = self.environment_or_say(Report::Bar, cx) else {
            return;
        };
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let (title, detail) = match page.save_batch() {
            Ok(batch) => (
                format!("SQL for {}", changes_label(page.edits.staged.len())),
                preview(
                    Some(&environment.dialect),
                    &page.preview_sql(&environment.dialect, &batch),
                ),
            ),
            Err(message) => ("These edits cannot be saved".to_string(), message),
        };
        let answer = window.prompt(PromptLevel::Info, &title, Some(&detail), &["Close"], cx);
        let task = cx.spawn(async move |_this, _cx| {
            answer.await.ok();
        });
        self.keep_task(task);
    }
}

fn saved_label(count: usize) -> String {
    match count {
        1 => "1 change".into(),
        count => format!("{count} changes"),
    }
}

fn rows_label(count: u64) -> String {
    match count {
        1 => "1 row".into(),
        count => format!("{count} rows"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid_editing::tests::{dialect, page};
    use database::protocol::{ErrorData, ResponseError};

    fn at(row: usize, column: usize) -> CellAddress {
        CellAddress { row, column }
    }

    fn value(column: &str, value: EditValue) -> ColumnValue {
        ColumnValue {
            column: column.into(),
            value,
        }
    }

    fn text_value(text: &str) -> EditValue {
        EditValue::Text { value: text.into() }
    }

    fn staged_page() -> Page {
        let mut page = page();
        let dialect = dialect("postgres");
        // Staged out of order on purpose: the batch is in reading order.
        page.stage_cell(&dialect, at(2, 2), Some("31".into()));
        page.stage_cell(&dialect, at(0, 2), None);
        page.stage_cell(&dialect, at(0, 1), Some("ada l.".into()));
        page
    }

    #[test]
    fn the_batch_has_one_change_per_row_keyed_by_the_whole_key_in_reading_order() {
        let batch = staged_page().save_batch().expect("a batch");
        assert_eq!(
            batch.connection,
            (
                "db".to_string(),
                crate::database_panel::NodeOrigin::Personal
            )
        );
        assert_eq!(
            (batch.schema.as_str(), batch.table.as_str()),
            ("public", "people")
        );
        assert_eq!(
            batch.changes,
            vec![
                RowChange {
                    key: vec![value("id", text_value("1"))],
                    set: vec![
                        value("name", text_value("ada l.")),
                        value("score", EditValue::Null),
                    ],
                },
                RowChange {
                    key: vec![value("id", text_value("3"))],
                    set: vec![value("score", text_value("31"))],
                },
            ]
        );
        assert_eq!(batch.origins, vec![(0, vec![1, 2]), (2, vec![2])]);
    }

    #[test]
    fn a_composite_key_travels_whole_in_the_order_the_table_declares_it() {
        let mut page = page();
        page.result.columns.swap(0, 1);
        for row in &mut page.result.rows {
            row.swap(0, 1);
        }
        let browse = page.browse.as_mut().unwrap();
        browse.key_columns = vec!["id".into(), "name".into()];
        browse.columns.iter_mut().for_each(|column| {
            column.primary_key = column.name == "id" || column.name == "name";
        });
        page.stage_cell(&dialect("postgres"), at(0, 2), Some("11".into()));

        let change = &page.save_batch().expect("a batch").changes[0];
        assert_eq!(
            change.key,
            vec![
                value("id", text_value("1")),
                value("name", text_value("ada"))
            ]
        );
    }

    #[test]
    fn a_page_that_cannot_name_its_rows_cannot_be_batched() {
        let mut page = staged_page();
        page.browse.as_mut().unwrap().key_columns.clear();
        assert_eq!(page.save_batch(), Err("read-only: no primary key".into()));
        page.browse = None;
        assert!(page.save_batch().is_err());
    }

    /// Safety, dialect and the session come from the page's connection; the
    /// batch must not name a different one.
    #[test]
    fn a_page_whose_table_names_another_connection_than_its_own_cannot_be_batched() {
        let mut page = staged_page();
        page.connection = Some((
            "elsewhere".to_string(),
            crate::database_panel::NodeOrigin::Personal,
        ));
        assert!(page.save_batch().is_err());
        page.connection = None;
        assert!(page.save_batch().is_err());
    }

    #[test]
    fn the_preview_writes_one_update_per_row_with_numbers_bare_and_text_quoted() {
        let page = staged_page();
        let dialect = dialect("postgres");
        let batch = page.save_batch().expect("a batch");
        assert_eq!(
            page.preview_sql(&dialect, &batch),
            "UPDATE \"public\".\"people\"\nSET \"name\" = 'ada l.', \"score\" = NULL\nWHERE \"id\" = 1;\n\n\
             UPDATE \"public\".\"people\"\nSET \"score\" = 31\nWHERE \"id\" = 3;"
        );
    }

    /// A number typed into a text column stays text in the preview, the way the
    /// driver will bind it.
    #[test]
    fn a_number_typed_into_a_text_column_is_quoted() {
        let mut page = page();
        page.stage_cell(&dialect("postgres"), at(0, 1), Some("42".into()));
        let batch = page.save_batch().expect("a batch");
        assert!(
            page.preview_sql(&dialect("postgres"), &batch)
                .contains("SET \"name\" = '42'")
        );
    }

    #[test]
    fn a_document_store_previews_an_update_one() {
        let page = staged_page();
        let batch = page.save_batch().expect("a batch");
        let preview = page.preview_sql(&dialect("mongodb"), &batch);
        assert!(
            preview.starts_with("{\"updateOne\": {\"database\": \"public\", \"collection\": \"people\", \"filter\": {\"id\": 1}"),
            "{preview}"
        );
        assert!(preview.contains("\"$set\": {\"name\": \"ada l.\", \"score\": null}"));
    }

    fn refusal(code: ErrorCode, change: Option<u64>, column: Option<&str>) -> anyhow::Error {
        DriverError(
            ResponseError::new(code, "the database said no").with_data(ErrorData {
                change,
                column: column.map(str::to_string),
            }),
        )
        .into()
    }

    #[test]
    fn a_conflict_marks_the_row_it_named_by_change_number() {
        let mut page = staged_page();
        let batch = page.save_batch().expect("a batch");
        page.apply_save_error(&batch, &refusal(ErrorCode::Conflict, Some(1), None));

        assert_eq!(
            page.edits.row_errors.keys().copied().collect::<Vec<_>>(),
            vec![2]
        );
        let failure = page.edits.failure.expect("said in the bar");
        assert!(failure.contains("row 43"), "{failure}");
        assert!(failure.contains("changed or removed since it was read"));
        assert_eq!(page.edits.staged.len(), 3, "every edit is kept");
    }

    #[test]
    fn a_rejected_value_marks_the_named_cell_of_the_named_change() {
        let mut page = staged_page();
        let batch = page.save_batch().expect("a batch");
        page.apply_save_error(
            &batch,
            &refusal(ErrorCode::InvalidValue, Some(0), Some("score")),
        );

        assert_eq!(page.edits.staged[&at(0, 1)].error, None);
        let marked = page.edits.staged[&at(0, 2)].error.clone().expect("marked");
        assert!(marked.contains("the database said no"), "{marked}");
        assert!(page.edits.failure.is_some());
    }

    #[test]
    fn a_rejected_value_with_no_column_marks_the_first_cell_of_the_change() {
        let mut page = staged_page();
        let batch = page.save_batch().expect("a batch");
        page.apply_save_error(&batch, &refusal(ErrorCode::InvalidValue, Some(0), None));
        assert!(page.edits.staged[&at(0, 1)].error.is_some());
        assert_eq!(page.edits.staged[&at(0, 2)].error, None);
    }

    /// The check run before a save would otherwise replace the database's word
    /// with its own, which has nothing to say against the value.
    #[test]
    fn a_value_the_database_refused_stays_refused_until_the_cell_is_edited_again() {
        let mut page = staged_page();
        let dialect = dialect("postgres");
        let batch = page.save_batch().expect("a batch");
        page.apply_save_error(
            &batch,
            &refusal(ErrorCode::InvalidValue, Some(0), Some("score")),
        );
        assert_eq!(page.edits.invalid_count(), 1);

        assert_eq!(page.revalidate(&dialect), 1, "the refusal still counts");
        let kept = page.edits.staged[&at(0, 2)].error.clone().expect("kept");
        assert!(kept.contains("the database said no"), "{kept}");

        page.stage_cell(&dialect, at(0, 2), Some("12".into()));
        assert_eq!(page.revalidate(&dialect), 0, "edited again, judged again");
    }

    #[test]
    fn a_refusal_that_names_nothing_is_said_in_the_bar_and_marks_no_cell() {
        for error in [
            refusal(ErrorCode::InvalidValue, None, None),
            refusal(ErrorCode::Conflict, Some(99), None),
            refusal(ErrorCode::Connection, Some(0), None),
            anyhow::anyhow!("the pipe broke"),
        ] {
            let mut page = staged_page();
            let batch = page.save_batch().expect("a batch");
            page.apply_save_error(&batch, &error);
            assert!(page.edits.row_errors.is_empty());
            assert_eq!(page.edits.invalid_count(), 0);
            let failure = page.edits.failure.expect("said in the bar");
            assert!(failure.starts_with("Could not save: "), "{failure}");
        }
    }
}
