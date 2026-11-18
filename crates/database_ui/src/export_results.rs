//! Saving a result to a file: the page on screen, or every page of the
//! statement behind it.
//!
//! Every page is serialised, handed to the file writer and dropped before the
//! next is asked for, through a channel that holds one chunk. Memory stays at
//! about one page however long the export runs, and a file that cannot be
//! written stops the reading rather than filling a buffer.
//!
//! Nothing is written under the name the user chose until the export is over:
//! see `TempFile`. And only a statement that reads is exported by asking the
//! server for it again, page after page; one that changes data is exported
//! from the rows on screen (`offered_export_scopes`).

use crate::connection_store::DatabaseSettings;
use crate::database_panel::{DatabasePanel, NodeState};
use crate::query::{QueryState, TableBrowse};
use crate::row_serializers::{binary_cells, csv_header, csv_rows, insert_statements, json_objects};
use crate::safe_mode::StatementKind;
use crate::session::Session;
use crate::sql_dialect::{Dialect, Feature};
use crate::tree_menus::icon_entry;
use anyhow::{Context as _, Result, anyhow};
use database::protocol::ResultSet;
use fs::{Fs, RemoveOptions, RenameOptions};
use futures::channel::mpsc;
use futures::io::AsyncRead;
use futures::{SinkExt as _, Stream};
use gpui::{BackgroundExecutor, Context, Pixels, Point, SharedString, Task, Window};
use settings::Settings as _;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use ui::{ContextMenu, prelude::*};
use util::ResultExt as _;
use workspace::{Toast, notifications::NotificationId};

/// How many rows an export asks for at a time, when the pager shows fewer. A
/// page costs the server the rows before it as well, so reading a table in
/// pages of a few hundred spends time on skipping that grows with the square
/// of its length.
const EXPORT_PAGE_ROWS: u32 = 5_000;

/// The most rows an export may be told to write, whatever the setting says.
/// A driver that reports more rows to come than it has would otherwise keep
/// an export running for as long as the number lets it.
const MAX_EXPORT_ROWS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Csv,
    Json,
    Sql,
}

impl ExportFormat {
    fn icon(self) -> IconName {
        match self {
            ExportFormat::Csv => IconName::FileTextOutlined,
            ExportFormat::Json => IconName::Json,
            ExportFormat::Sql => IconName::Code,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            ExportFormat::Csv => "CSV",
            ExportFormat::Json => "JSON",
            ExportFormat::Sql => "SQL INSERT",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::Sql => "sql",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportScope {
    CurrentPage,
    AllRows,
}

/// The formats the menu offers. `INSERT` needs an engine that writes rows and
/// a table to write them into: without one the statements would name a
/// placeholder no engine parses.
pub(crate) fn offered_export_formats(
    dialect: Option<&Dialect>,
    table_known: bool,
) -> Vec<ExportFormat> {
    let mut formats = vec![ExportFormat::Csv, ExportFormat::Json];
    if table_known && dialect.is_some_and(|dialect| dialect.supports(Feature::WriteTemplates)) {
        formats.push(ExportFormat::Sql);
    }
    formats
}

/// What the menu offers to export. Every row means asking the server for the
/// statement again once per page, which is no more acceptable here than it is
/// for turning the page: a statement that changes data is only ever exported
/// from the rows already on screen.
pub(crate) fn offered_export_scopes(kind: StatementKind) -> Vec<ExportScope> {
    let mut scopes = vec![ExportScope::CurrentPage];
    if kind == StatementKind::Read {
        scopes.push(ExportScope::AllRows);
    }
    scopes
}

const WRITE_EXPORT_NOTE: &str = "This statement changes data, so exporting every row would run it again for each page. Export the current page instead.";

/// Whether reading a page after page of this result can repeat or skip rows,
/// because nothing fixes the order they come in.
pub(crate) fn paging_is_unordered(browse: Option<&TableBrowse>) -> bool {
    browse.is_none_or(|browse| browse.key_columns.is_empty() && browse.order.is_empty())
}

/// The page size an export reads with.
fn export_page_rows(page_limit: u32, minimum: u32) -> u32 {
    page_limit.max(minimum)
}

fn capped_row_limit(configured: u64) -> u64 {
    configured.min(MAX_EXPORT_ROWS)
}

/// Turns pages into the text of one file: a header once, rows per page, and for
/// JSON the brackets around them. Concatenated, the chunks equal what
/// `row_serializers` writes for all the rows at once.
pub(crate) struct ChunkWriter {
    format: ExportFormat,
    dialect: Option<Dialect>,
    table: Option<(String, String)>,
    started: bool,
    wrote_rows: bool,
    binary_values: u64,
}

impl ChunkWriter {
    pub(crate) fn new(
        format: ExportFormat,
        dialect: Option<Dialect>,
        table: Option<(String, String)>,
    ) -> Result<Self> {
        if format == ExportFormat::Sql
            && (table.is_none()
                || !dialect
                    .as_ref()
                    .is_some_and(|dialect| dialect.supports(Feature::WriteTemplates)))
        {
            return Err(anyhow!(
                "SQL INSERT needs a SQL table to write into; open one from the tree"
            ));
        }
        Ok(Self {
            format,
            dialect,
            table,
            started: false,
            wrote_rows: false,
            binary_values: 0,
        })
    }

    /// The first `take` rows of `result`, as the text to append.
    pub(crate) fn chunk(&mut self, result: &ResultSet, take: usize) -> Result<String> {
        let rows: Vec<usize> = (0..take.min(result.rows.len())).collect();
        self.binary_values += binary_cells(result, &rows);
        let mut text = String::new();
        match self.format {
            ExportFormat::Csv => {
                if !self.started {
                    text.push_str(&csv_header(result));
                }
                text.push_str(&csv_rows(result, &rows));
            }
            ExportFormat::Json => {
                let objects = json_objects(result, &rows);
                if !objects.is_empty() {
                    text.push_str(if self.wrote_rows { ",\n" } else { "[\n" });
                    text.push_str(&objects.join(",\n"));
                    self.wrote_rows = true;
                }
            }
            ExportFormat::Sql => {
                let table = self
                    .table
                    .as_ref()
                    .map(|(schema, name)| (schema.as_str(), name.as_str()));
                let statements = self
                    .dialect
                    .as_ref()
                    .and_then(|dialect| insert_statements(dialect, table, result, &rows))
                    .context("this result cannot be written as INSERT statements")?;
                for statement in statements {
                    text.push_str(&statement);
                    text.push('\n');
                }
            }
        }
        self.started = true;
        Ok(text)
    }

    /// How many cells written so far were binary, and so are notes.
    pub(crate) fn binary_values(&self) -> u64 {
        self.binary_values
    }

    /// What closes the file.
    pub(crate) fn finish(&self) -> &'static str {
        match (self.format, self.wrote_rows) {
            (ExportFormat::Json, true) => "\n]",
            (ExportFormat::Json, false) => "[]",
            _ => "",
        }
    }
}

/// What the pager says about the export.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExportStatus {
    #[default]
    Idle,
    Running {
        path: PathBuf,
        rows: u64,
        cancelling: bool,
    },
    Finished {
        path: PathBuf,
        rows: u64,
        /// Stopped at `export_row_limit` with rows still to come.
        capped: bool,
        /// Cells written as a note because their bytes never reached this
        /// crate.
        binary: u64,
    },
    Cancelled {
        path: PathBuf,
        rows: u64,
        binary: u64,
    },
    Failed {
        path: Option<PathBuf>,
        message: SharedString,
    },
}

/// The export in progress, held by the panel so that closing the panel drops
/// the task and with it the read and the write.
pub(crate) struct Export {
    pub(crate) status: ExportStatus,
    task: Option<Task<()>>,
    cancel: Arc<AtomicBool>,
    /// The fewest rows asked of the server at a time. A field rather than a
    /// constant so that a test can make three rows span several pages.
    pub(crate) min_page_rows: u32,
    /// The session the export reads from and the name of the request it is
    /// reading with, which is what stopping asks the server to interrupt.
    in_flight: Option<(Arc<Session>, String)>,
}

impl Default for Export {
    fn default() -> Self {
        Self {
            status: ExportStatus::default(),
            task: None,
            cancel: Arc::default(),
            min_page_rows: EXPORT_PAGE_ROWS,
            in_flight: None,
        }
    }
}

struct Job {
    session: Arc<Session>,
    sql: String,
    limit: u32,
    /// The page on screen, for `CurrentPage`: nothing is asked of the server,
    /// and nothing after it is read.
    on_screen: Option<ResultSet>,
    single_page: bool,
    writer: ChunkWriter,
    row_cap: u64,
    /// One name for every page: they are read one after another, and a driver
    /// interrupts the request it is running by name.
    request_id: String,
}

enum Ended {
    Complete { rows: u64, binary: u64 },
    Capped { rows: u64, binary: u64 },
    Cancelled { rows: u64, binary: u64 },
}

/// Feeds the file writer from the chunks the export task sends.
struct ChannelReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    current: Vec<u8>,
    position: usize,
}

impl AsyncRead for ChannelReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        loop {
            if self.position < self.current.len() {
                let count = buffer.len().min(self.current.len() - self.position);
                let start = self.position;
                buffer[..count].copy_from_slice(&self.current[start..start + count]);
                self.position += count;
                return Poll::Ready(Ok(count));
            }
            match Pin::new(&mut self.receiver).poll_next(cx) {
                Poll::Ready(Some(chunk)) => {
                    self.current = chunk;
                    self.position = 0;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

struct ExportFailed;

/// The file an export is written to until it is complete.
///
/// Dropped while armed, which is what happens to a task that is cancelled
/// along with its panel, it deletes the file: whatever the task was in the
/// middle of writing is not a file anybody asked for.
struct TempFile {
    fs: Arc<dyn Fs>,
    path: PathBuf,
    executor: BackgroundExecutor,
    armed: bool,
}

impl TempFile {
    fn new(fs: Arc<dyn Fs>, path: PathBuf, executor: BackgroundExecutor) -> Self {
        Self {
            fs,
            path,
            executor,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    /// Deletes the file now, for an export that failed or stopped before it
    /// wrote anything worth keeping.
    async fn discard(&mut self) {
        self.armed = false;
        remove_if_there(&*self.fs, &self.path).await;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (fs, path) = (self.fs.clone(), self.path.clone());
        self.executor
            .spawn(async move { remove_if_there(&*fs, &path).await })
            .detach();
    }
}

async fn remove_if_there(fs: &dyn Fs, path: &Path) {
    fs.remove_file(
        path,
        RemoveOptions {
            ignore_if_not_exists: true,
            ..Default::default()
        },
    )
    .await
    .log_err();
}

/// A name beside `path` for the export to be written under: the same folder,
/// so that moving it into place is a rename and not a copy.
fn temp_path(path: &Path, token: u64) -> Option<PathBuf> {
    let name = path.file_name()?.to_string_lossy();
    Some(path.with_file_name(format!(".{name}.{}-{token}.part", std::process::id())))
}

impl DatabasePanel {
    /// Asks where to save, then exports. Nothing happens if the dialog is
    /// dismissed.
    pub(crate) fn export_results(
        &mut self,
        format: ExportFormat,
        scope: ExportScope,
        cx: &mut Context<Self>,
    ) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if matches!(self.export.status, ExportStatus::Running { .. }) {
            return;
        }
        if !offered_export_scopes(page.kind).contains(&scope) {
            self.fail_export(None, WRITE_EXPORT_NOTE.into(), cx);
            return;
        }
        let stem = page
            .browse
            .as_ref()
            .map_or("results", |browse| browse.table.as_str());
        let name = format!("{stem}.{}", format.extension());
        let directory = std::env::home_dir().unwrap_or_default();
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        self.keep_task(cx.spawn(async move |this, cx| {
            let outcome = chosen.await;
            this.update(cx, |this, cx| match outcome {
                Ok(Ok(Some(path))) => this.export_to(path, format, scope, cx),
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(error)) => this.fail_export(None, format!("{error:#}"), cx),
            })
            .ok();
        }));
    }

    /// Starts writing `path`. The dialog has already had its say about an
    /// existing file.
    ///
    /// The rows go to a file beside `path` and are moved into its place when
    /// the export is over: until then `path` is whatever it was, so a read that
    /// fails, a panel closed or a disk that fills leaves a file that was there
    /// intact, and one that was not there absent.
    pub(crate) fn export_to(
        &mut self,
        path: PathBuf,
        format: ExportFormat,
        scope: ExportScope,
        cx: &mut Context<Self>,
    ) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        if matches!(self.export.status, ExportStatus::Running { .. }) {
            return;
        }
        if !offered_export_scopes(page.kind).contains(&scope) {
            self.fail_export(Some(path), WRITE_EXPORT_NOTE.into(), cx);
            return;
        }
        // Found by identity now, not by the place it had when the page was
        // read: the list may have been rebuilt since.
        let index = page
            .connection
            .as_ref()
            .and_then(|key| self.index_of_key(key));
        let session = index.and_then(|index| match &self.connections.get(index)?.state {
            NodeState::Connected(session) => Some(session.clone()),
            _ => None,
        });
        let Some(session) = session else {
            self.fail_export(
                Some(path),
                "The connection this result came from is closed".into(),
                cx,
            );
            return;
        };
        let table = page
            .browse
            .as_ref()
            .map(|browse| (browse.schema.clone(), browse.table.clone()));
        let writer = match ChunkWriter::new(format, index.and_then(|i| self.dialect(i)), table) {
            Ok(writer) => writer,
            Err(error) => {
                self.fail_export(Some(path), format!("{error:#}"), cx);
                return;
            }
        };
        let sql = page.sql.clone();
        let limit = export_page_rows(page.limit, self.export.min_page_rows);
        let on_screen = (scope == ExportScope::CurrentPage).then(|| page.result.clone());
        let Ok(fs) = self.workspace.read_with(cx, |workspace, cx| {
            workspace.project().read(cx).fs().clone()
        }) else {
            self.fail_export(Some(path), "The workspace is closed".into(), cx);
            return;
        };
        let request_number = self.next_request_id();
        let Some(temp_path) = temp_path(&path, request_number) else {
            self.fail_export(Some(path), "That is not the name of a file".into(), cx);
            return;
        };

        let request_id = format!("e{request_number}");
        let job = Job {
            session: session.clone(),
            sql,
            limit,
            single_page: on_screen.is_some(),
            on_screen,
            writer,
            row_cap: capped_row_limit(DatabaseSettings::get_global(cx).export_row_limit),
            request_id: request_id.clone(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.export.cancel = cancel.clone();
        self.export.in_flight = Some((session, request_id));
        self.export.status = ExportStatus::Running {
            path: path.clone(),
            rows: 0,
            cancelling: false,
        };
        self.export.task = Some(cx.spawn(async move |this, cx| {
            let (sender, receiver) = mpsc::channel::<Vec<u8>>(1);
            let mut temp = TempFile::new(fs.clone(), temp_path, cx.background_executor().clone());
            let file = cx.background_spawn({
                let fs = fs.clone();
                let temp_path = temp.path.clone();
                async move {
                    let reader = ChannelReader {
                        receiver,
                        current: Vec::new(),
                        position: 0,
                    };
                    fs.create_file_with(&temp_path, std::pin::pin!(reader))
                        .await
                }
            });

            let read = stream_pages(job, sender, &cancel, &this, &path, cx).await;
            let written = file.await;

            // The file's own error first: when it cannot be written the read
            // fails too, and that failure is only the symptom.
            let status = match (read, written) {
                (Ok(ended), Ok(())) => publish(&*fs, &mut temp, &path, ended).await,
                (read, written) => {
                    let message = match (written.err(), read.err()) {
                        (Some(error), _) | (None, Some(error)) => format!("{error:#}"),
                        (None, None) => String::new(),
                    };
                    temp.discard().await;
                    ExportStatus::Failed {
                        path: Some(path.clone()),
                        message: message.into(),
                    }
                }
            };
            this.update(cx, |this, cx| this.finish_export(status, cx))
                .ok();
        }));
        cx.notify();
    }

    /// Stops after the page in hand and closes the file, so what was written
    /// is still a valid file. The server is asked to stop the page being read
    /// as well, rather than leaving the user to wait it out.
    pub(crate) fn cancel_export(&mut self, cx: &mut Context<Self>) {
        let ExportStatus::Running { cancelling, .. } = &mut self.export.status else {
            return;
        };
        if std::mem::replace(cancelling, true) {
            return;
        }
        self.export.cancel.store(true, Ordering::SeqCst);
        if let Some((session, request_id)) = self.export.in_flight.clone() {
            self.keep_task(cx.spawn(async move |_this, _cx| {
                // Best effort, as for a query: an engine that cannot interrupt
                // finishes the page, and the export stops after it.
                session.cancel(request_id).await.log_err();
            }));
        }
        cx.notify();
    }

    /// Drops the export's task the way closing the panel does.
    #[cfg(test)]
    pub(crate) fn abandon_export(&mut self) {
        self.export.task = None;
    }

    pub(crate) fn dismiss_export_status(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.export.status, ExportStatus::Running { .. }) {
            self.export.status = ExportStatus::Idle;
            cx.notify();
        }
    }

    fn fail_export(&mut self, path: Option<PathBuf>, message: String, cx: &mut Context<Self>) {
        self.finish_export(
            ExportStatus::Failed {
                path,
                message: message.into(),
            },
            cx,
        );
    }

    fn finish_export(&mut self, status: ExportStatus, cx: &mut Context<Self>) {
        if let ExportStatus::Failed { path, message } = &status {
            let text = match path {
                Some(path) => format!("Export to {} failed: {message}", path.display()),
                None => format!("Export failed: {message}"),
            };
            self.workspace
                .update(cx, |workspace, cx| {
                    workspace.show_toast(
                        Toast::new(NotificationId::unique::<ExportFailed>(), text),
                        cx,
                    );
                })
                .ok();
        }
        self.export.in_flight = None;
        self.export.status = status;
        cx.notify();
    }

    fn note_export_progress(&mut self, written: u64, cx: &mut Context<Self>) {
        if let ExportStatus::Running { rows, .. } = &mut self.export.status {
            *rows = written;
            cx.notify();
        }
    }

    /// The export button's menu, over the pager.
    pub(crate) fn deploy_export_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let QueryState::Done(page) = &self.query else {
            return;
        };
        let table_known = page.browse.is_some();
        let unordered = paging_is_unordered(page.browse.as_ref());
        let scopes = offered_export_scopes(page.kind);
        let dialect = page
            .connection
            .as_ref()
            .and_then(|key| self.index_of_key(key))
            .and_then(|index| self.dialect(index));
        let formats = offered_export_formats(dialect.as_ref(), table_known);
        let panel = cx.weak_entity();
        self.open_context_menu(position, window, cx, move |menu, _window, _cx| {
            let submenu = |menu: ContextMenu,
                           title: &'static str,
                           icon: IconName,
                           scope,
                           unordered_note: bool| {
                let (panel, formats) = (panel.clone(), formats.clone());
                menu.submenu_with_icon(title, icon, move |mut submenu, _window, _cx| {
                    if unordered_note {
                        submenu = submenu.header(
                            "Without ORDER BY, rows may repeat or be skipped between pages",
                        );
                    }
                    for format in &formats {
                        let (panel, format) = (panel.clone(), *format);
                        submenu = icon_entry(
                            submenu,
                            format.label(),
                            format.icon(),
                            move |_window, cx| {
                                panel
                                    .update(cx, |panel, cx| panel.export_results(format, scope, cx))
                                    .ok();
                            },
                        );
                    }
                    submenu
                })
            };
            scopes.iter().fold(menu, |menu, scope| match scope {
                ExportScope::CurrentPage => {
                    submenu(menu, "Current Page as", IconName::File, *scope, false)
                }
                ExportScope::AllRows => {
                    submenu(menu, "All Rows as", IconName::Download, *scope, unordered)
                }
            })
        });
    }

    /// The line in the pager about the export, if there is anything to say.
    pub(crate) fn export_label(&self) -> Option<(String, Color)> {
        describe_export(&self.export.status)
    }
}

/// What to add to the line when cells could not be written as they are.
fn binary_note(count: u64) -> String {
    match count {
        0 => String::new(),
        1 => ". 1 binary value was not exported".to_string(),
        count => format!(". {count} binary values were not exported"),
    }
}

fn describe_export(status: &ExportStatus) -> Option<(String, Color)> {
    match status {
        ExportStatus::Idle => None,
        ExportStatus::Running {
            rows, cancelling, ..
        } => Some((
            if *cancelling {
                format!("Stopping after {rows} rows…")
            } else {
                format!("Exported {rows} rows…")
            },
            Color::Muted,
        )),
        ExportStatus::Finished {
            path,
            rows,
            capped,
            binary,
        } => Some((
            format!(
                "Exported {rows} rows to {}{}{}",
                path.display(),
                if *capped {
                    " (stopped at the export row limit)"
                } else {
                    ""
                },
                binary_note(*binary),
            ),
            if *binary > 0 {
                Color::Warning
            } else {
                Color::Success
            },
        )),
        ExportStatus::Cancelled { path, rows: 0, .. } => Some((
            format!(
                "Export cancelled; nothing was written to {}",
                path.display()
            ),
            Color::Warning,
        )),
        ExportStatus::Cancelled { path, rows, binary } => Some((
            format!(
                "Export cancelled after {rows} rows: {}{}",
                path.display(),
                binary_note(*binary)
            ),
            Color::Warning,
        )),
        ExportStatus::Failed { path, message } => Some((
            match path {
                Some(path) => format!("Export to {} failed: {message}", path.display()),
                None => format!("Export failed: {message}"),
            },
            Color::Error,
        )),
    }
}

/// Moves the finished export into the place the user chose.
async fn publish(fs: &dyn Fs, temp: &mut TempFile, path: &Path, ended: Ended) -> ExportStatus {
    // Stopped before a single row: there is nothing to put where the old file
    // is, and an empty file would be a poor trade for it.
    if let Ended::Cancelled { rows: 0, .. } = ended {
        temp.discard().await;
        return finished_status(path.to_path_buf(), ended);
    }
    let moved = fs
        .rename(
            &temp.path,
            path,
            RenameOptions {
                overwrite: true,
                ..Default::default()
            },
        )
        .await
        .with_context(|| format!("moving the finished export to {}", path.display()));
    match moved {
        Ok(()) => {
            temp.disarm();
            finished_status(path.to_path_buf(), ended)
        }
        Err(error) => {
            temp.discard().await;
            ExportStatus::Failed {
                path: Some(path.to_path_buf()),
                message: format!("{error:#}").into(),
            }
        }
    }
}

fn finished_status(path: PathBuf, ended: Ended) -> ExportStatus {
    match ended {
        Ended::Complete { rows, binary } => ExportStatus::Finished {
            path,
            rows,
            capped: false,
            binary,
        },
        Ended::Capped { rows, binary } => ExportStatus::Finished {
            path,
            rows,
            capped: true,
            binary,
        },
        Ended::Cancelled { rows, binary } => ExportStatus::Cancelled { path, rows, binary },
    }
}

/// Reads page after page, writing each before asking for the next, until the
/// statement runs out, the row cap is reached or the user cancels.
async fn stream_pages(
    mut job: Job,
    mut sender: mpsc::Sender<Vec<u8>>,
    cancel: &AtomicBool,
    panel: &gpui::WeakEntity<DatabasePanel>,
    path: &PathBuf,
    cx: &mut gpui::AsyncApp,
) -> Result<Ended> {
    let mut offset = 0u64;
    let mut rows_written = 0u64;

    let ended = loop {
        if cancel.load(Ordering::SeqCst) {
            break Ended::Cancelled {
                rows: rows_written,
                binary: job.writer.binary_values(),
            };
        }
        let result = match job.on_screen.take() {
            Some(result) => result,
            None => {
                let read = job
                    .session
                    .query(job.sql.clone(), job.limit, offset, job.request_id.clone())
                    .await;
                match read {
                    Ok(result) => result,
                    // The server was asked to stop this page, and an
                    // interrupted query answers with an error: that is the stop
                    // the user asked for, not a failure.
                    Err(_) if cancel.load(Ordering::SeqCst) => {
                        break Ended::Cancelled {
                            rows: rows_written,
                            binary: job.writer.binary_values(),
                        };
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("reading rows from {offset} for {}", path.display())
                        });
                    }
                }
            }
        };
        // Asked again after the read: the user may have cancelled while it ran.
        if cancel.load(Ordering::SeqCst) {
            break Ended::Cancelled {
                rows: rows_written,
                binary: job.writer.binary_values(),
            };
        }

        let room = job.row_cap.saturating_sub(rows_written);
        let take = result
            .rows
            .len()
            .min(usize::try_from(room).unwrap_or(usize::MAX));
        let chunk = job.writer.chunk(&result, take)?;
        send(&mut sender, chunk).await?;
        rows_written += take as u64;
        panel
            .update(cx, |panel, cx| panel.note_export_progress(rows_written, cx))
            .ok();

        let more_here = take < result.rows.len();
        if more_here || (result.truncated && rows_written >= job.row_cap) {
            break Ended::Capped {
                rows: rows_written,
                binary: job.writer.binary_values(),
            };
        }
        if job.single_page || !result.truncated || result.rows.is_empty() {
            break Ended::Complete {
                rows: rows_written,
                binary: job.writer.binary_values(),
            };
        }
        offset += result.rows.len() as u64;
    };

    send(&mut sender, job.writer.finish().to_string()).await?;
    Ok(ended)
}

async fn send(sender: &mut mpsc::Sender<Vec<u8>>, text: String) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    sender
        .send(text.into_bytes())
        .await
        .map_err(|_| anyhow!("the file stopped accepting data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_panel::NodeOrigin;
    use crate::query::SortColumn;
    use crate::row_serializers::{to_csv, to_insert, to_json};
    use database::protocol::{Capabilities, Cell, ResultColumn};

    fn page(first: u32, count: u32) -> ResultSet {
        ResultSet {
            columns: vec![
                ResultColumn {
                    name: "id".into(),
                    type_name: "integer".into(),
                },
                ResultColumn {
                    name: "note".into(),
                    type_name: "text".into(),
                },
            ],
            rows: (first..first + count)
                .map(|id| {
                    vec![
                        Cell::Number {
                            value: id.to_string(),
                        },
                        if id % 3 == 0 {
                            Cell::Null
                        } else {
                            Cell::Text {
                                value: format!("a,\"b\"\n{id}"),
                            }
                        },
                    ]
                })
                .collect(),
            truncated: false,
            elapsed_ms: 0,
        }
    }

    fn all_in_one(pages: &[ResultSet]) -> ResultSet {
        let mut all = pages[0].clone();
        for later in &pages[1..] {
            all.rows.extend(later.rows.iter().cloned());
        }
        all
    }

    fn written(mut writer: ChunkWriter, pages: &[ResultSet]) -> String {
        let mut text = String::new();
        for page in pages {
            text.push_str(&writer.chunk(page, page.rows.len()).expect("a chunk"));
        }
        text.push_str(writer.finish());
        text
    }

    fn sqlite() -> Dialect {
        Dialect::detect("sqlite", &Capabilities::default())
    }

    fn rows_of(count: usize) -> Vec<usize> {
        (0..count).collect()
    }

    #[test]
    fn csv_chunks_carry_the_header_once_and_equal_the_whole() {
        let pages = [page(1, 2), page(3, 2), page(5, 1)];
        let whole = all_in_one(&pages);
        let writer = ChunkWriter::new(ExportFormat::Csv, None, None).unwrap();
        assert_eq!(
            written(writer, &pages),
            to_csv(&whole, &rows_of(whole.rows.len()))
        );
    }

    #[test]
    fn json_chunks_join_into_one_valid_array_equal_to_the_whole() {
        let pages = [page(1, 2), page(3, 2), page(5, 1)];
        let whole = all_in_one(&pages);
        let writer = ChunkWriter::new(ExportFormat::Json, None, None).unwrap();
        let text = written(writer, &pages);
        assert_eq!(text, to_json(&whole, &rows_of(whole.rows.len())));
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed.as_array().map(Vec::len), Some(5));
    }

    #[test]
    fn json_with_no_rows_is_an_empty_array_whether_or_not_a_page_came() {
        let empty = page(1, 0);
        let writer = ChunkWriter::new(ExportFormat::Json, None, None).unwrap();
        assert_eq!(written(writer, &[empty]), "[]");
        let untouched = ChunkWriter::new(ExportFormat::Json, None, None).unwrap();
        assert_eq!(untouched.finish(), "[]");
    }

    #[test]
    fn json_stays_valid_when_an_empty_page_sits_between_full_ones() {
        let pages = [page(1, 1), page(2, 0), page(2, 1)];
        let writer = ChunkWriter::new(ExportFormat::Json, None, None).unwrap();
        let text = written(writer, &pages);
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(parsed.as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn csv_of_no_rows_is_the_header_alone() {
        let writer = ChunkWriter::new(ExportFormat::Csv, None, None).unwrap();
        assert_eq!(written(writer, &[page(1, 0)]), "id,note\r\n");
    }

    #[test]
    fn sql_chunks_write_one_statement_per_line_for_the_table() {
        let pages = [page(1, 2), page(3, 1)];
        let whole = all_in_one(&pages);
        let writer = ChunkWriter::new(
            ExportFormat::Sql,
            Some(sqlite()),
            Some(("main".into(), "notes".into())),
        )
        .unwrap();
        let expected = to_insert(
            &sqlite(),
            Some(("main", "notes")),
            &whole,
            &rows_of(whole.rows.len()),
        )
        .unwrap();
        assert_eq!(written(writer, &pages), format!("{expected}\n"));
    }

    #[test]
    fn a_chunk_takes_only_the_rows_it_is_given_room_for() {
        let mut writer = ChunkWriter::new(ExportFormat::Csv, None, None).unwrap();
        let text = writer.chunk(&page(1, 5), 2).unwrap();
        assert_eq!(text.matches("\r\n").count(), 3, "the header and two rows");
    }

    #[test]
    fn sql_is_refused_without_a_table_or_for_a_document_store() {
        assert!(ChunkWriter::new(ExportFormat::Sql, Some(sqlite()), None).is_err());
        assert!(ChunkWriter::new(ExportFormat::Sql, None, Some(("a".into(), "b".into()))).is_err());
        let documents = Dialect::detect(
            "an-extension",
            &Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        assert!(
            ChunkWriter::new(
                ExportFormat::Sql,
                Some(documents),
                Some(("a".into(), "b".into()))
            )
            .is_err()
        );
    }

    #[test]
    fn the_menu_offers_sql_only_for_a_known_table_on_an_engine_that_writes_rows() {
        use ExportFormat::*;
        assert_eq!(
            offered_export_formats(Some(&sqlite()), true),
            [Csv, Json, Sql]
        );
        assert_eq!(offered_export_formats(Some(&sqlite()), false), [Csv, Json]);
        assert_eq!(offered_export_formats(None, true), [Csv, Json]);
        let documents = Dialect::detect(
            "an-extension",
            &Capabilities {
                read_table_template: Some("{}".into()),
                ..Default::default()
            },
        );
        assert_eq!(offered_export_formats(Some(&documents), true), [Csv, Json]);
    }

    #[test]
    fn binary_cells_are_counted_as_they_are_written() {
        let mut writer = ChunkWriter::new(ExportFormat::Csv, None, None).unwrap();
        let mut result = page(1, 3);
        result.rows[0][1] = Cell::Binary { byte_len: 4 };
        result.rows[1][1] = Cell::Binary { byte_len: 5 };
        writer.chunk(&result, 3).unwrap();
        assert_eq!(writer.binary_values(), 2);
        writer.chunk(&result, 1).unwrap();
        assert_eq!(writer.binary_values(), 3, "only the rows taken are counted");
    }

    #[test]
    fn the_pager_says_how_many_binary_values_were_not_exported() {
        let finished = |binary| ExportStatus::Finished {
            path: "/x.csv".into(),
            rows: 5,
            capped: false,
            binary,
        };
        let (text, colour) = describe_export(&finished(2)).expect("a line");
        assert!(text.contains("2 binary values were not exported"), "{text}");
        assert_eq!(colour, Color::Warning);
        let (text, _) = describe_export(&finished(1)).expect("a line");
        assert!(text.contains("1 binary value was not exported"), "{text}");
        let (text, colour) = describe_export(&finished(0)).expect("a line");
        assert!(!text.contains("binary"), "{text}");
        assert_eq!(colour, Color::Success);

        let (text, _) = describe_export(&ExportStatus::Cancelled {
            path: "/x.csv".into(),
            rows: 5,
            binary: 3,
        })
        .expect("a line");
        assert!(text.contains("3 binary values were not exported"), "{text}");
    }

    #[test]
    fn a_cancel_before_any_row_says_nothing_was_written() {
        let (text, _) = describe_export(&ExportStatus::Cancelled {
            path: "/x.csv".into(),
            rows: 0,
            binary: 0,
        })
        .expect("a line");
        assert!(text.contains("nothing was written"), "{text}");
    }

    #[test]
    fn a_statement_that_changes_data_is_only_exported_from_the_screen() {
        use ExportScope::*;
        assert_eq!(
            offered_export_scopes(StatementKind::Read),
            [CurrentPage, AllRows]
        );
        assert_eq!(offered_export_scopes(StatementKind::Write), [CurrentPage]);
    }

    #[test]
    fn paging_is_unordered_without_a_sort_or_a_key() {
        let mut browse = TableBrowse::new(("a".to_string(), NodeOrigin::Personal), "main", "t");
        assert!(paging_is_unordered(None));
        assert!(paging_is_unordered(Some(&browse)));
        browse.key_columns = vec!["id".into()];
        assert!(!paging_is_unordered(Some(&browse)));
        browse.key_columns.clear();
        browse.order.push(SortColumn {
            column: "id".into(),
            descending: false,
        });
        assert!(!paging_is_unordered(Some(&browse)));
    }

    #[test]
    fn an_export_reads_in_pages_no_smaller_than_the_minimum() {
        assert_eq!(export_page_rows(200, 5_000), 5_000);
        assert_eq!(export_page_rows(20_000, 5_000), 20_000);
        assert_eq!(export_page_rows(1, 1), 1);
    }

    #[test]
    fn the_row_limit_has_a_ceiling() {
        assert_eq!(capped_row_limit(10), 10);
        assert_eq!(capped_row_limit(u64::MAX), MAX_EXPORT_ROWS);
    }

    #[gpui::test]
    async fn a_dropped_temp_file_deletes_itself_and_a_disarmed_one_stays(
        cx: &mut gpui::TestAppContext,
    ) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.insert_tree(
            "/exports",
            serde_json::json!({ ".a.part": "half", ".b.part": "whole" }),
        )
        .await;
        let fs: Arc<dyn Fs> = fs;
        let abandoned = TempFile::new(fs.clone(), "/exports/.a.part".into(), cx.executor());
        let mut finished = TempFile::new(fs.clone(), "/exports/.b.part".into(), cx.executor());
        finished.disarm();
        drop(abandoned);
        drop(finished);
        cx.run_until_parked();

        assert!(!fs.is_file(std::path::Path::new("/exports/.a.part")).await);
        assert!(fs.is_file(std::path::Path::new("/exports/.b.part")).await);
    }
}
