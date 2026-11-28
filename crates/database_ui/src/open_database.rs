//! Adds SQLite connections by picking database files, or a folder holding them.
//!
//! SQLite is the one engine whose connection string is a path, so a file on
//! disk is already everything a connection needs: the name comes from the file
//! and there is no form to fill. A folder is scanned one level deep -- the
//! databases a project keeps beside its code, not every `.db` under `target/`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, PathPromptOptions,
    WeakEntity, Window,
};
use settings::Settings as _;
use ui::{Checkbox, ModalFooter, ModalHeader, ToggleState, prelude::*};
use util::ResultExt as _;
use workspace::Toast;
use workspace::notifications::NotificationId;
use workspace::{ModalView, Workspace};

use crate::connection_modal::Filled;
use crate::connection_store::DatabaseSettings;
use crate::driver_registry::{DATABASE_FILE_EXTENSIONS, driver_for_file, is_database_file};

/// One file to add, and the name its connection will carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedConnection {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    /// Already a connection under `name`: opened, not added a second time.
    pub(crate) existing: bool,
}

/// Names each file after itself, and keeps every name unique.
///
/// A file that is already a connection keeps that connection's name, so
/// opening it twice opens the one connection rather than adding a copy. Every
/// other name is the file's stem, with ` (2)`, ` (3)`... when that is taken --
/// by an existing connection or by another file in the same batch: two
/// `app.db` from different folders are both worth adding, and a project pins a
/// connection by name.
pub(crate) fn plan_connections(
    paths: &[PathBuf],
    existing: &[(String, String)],
) -> Vec<PlannedConnection> {
    let mut taken: HashSet<String> = existing.iter().map(|(name, _)| name.clone()).collect();
    let mut planned = Vec::with_capacity(paths.len());
    let mut seen_paths = HashSet::new();
    for path in paths {
        if !seen_paths.insert(path.clone()) {
            continue;
        }
        let url = path.to_string_lossy();
        if let Some((name, _)) = existing
            .iter()
            .find(|(_, existing_url)| *existing_url == url)
        {
            planned.push(PlannedConnection {
                path: path.clone(),
                name: name.clone(),
                existing: true,
            });
            continue;
        }
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "database".to_string());
        let mut name = stem.clone();
        let mut suffix = 2;
        while taken.contains(&name) {
            name = format!("{stem} ({suffix})");
            suffix += 1;
        }
        taken.insert(name.clone());
        planned.push(PlannedConnection {
            path: path.clone(),
            name,
            existing: false,
        });
    }
    planned
}

/// The database files directly inside `directory`, sorted by name.
pub(crate) async fn database_files_in(
    fs: &dyn fs::Fs,
    directory: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut entries = fs.read_dir(directory).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next().await {
        // One entry that cannot be read is no reason to hide every other one.
        let Some(path) = entry.log_err() else {
            continue;
        };
        if is_database_file(&path) && fs.is_file(&path).await {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Asks for one or more database files and adds each as a connection.
pub(crate) fn open_files(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if refuse_remote(workspace, cx) {
        return;
    }
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: true,
        prompt: Some("Open Database".into()),
        show_hidden: false,
    });
    let handle = cx.weak_entity();
    cx.spawn_in(window, async move |_, cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let (paths, skipped): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|path| is_database_file(path));
        if !skipped.is_empty() {
            toast(&handle, not_a_database_notice(&skipped), cx);
        }
        if paths.is_empty() {
            return;
        }
        cx.update(|window, cx| add_connections(&handle, &paths, window, cx))
            .ok();
    })
    .detach();
}

/// Asks for a folder and offers the database files found in it.
///
/// One file is added straight away; several are listed so the reader picks.
pub(crate) fn open_folder(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if refuse_remote(workspace, cx) {
        return;
    }
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open Folder".into()),
        show_hidden: false,
    });
    let fs = workspace.project().read(cx).fs().clone();
    let handle = cx.weak_entity();
    cx.spawn_in(window, async move |_, cx| {
        let Ok(Ok(Some(mut folders))) = picked.await else {
            return;
        };
        let Some(folder) = folders.pop() else {
            return;
        };
        let files = match database_files_in(fs.as_ref(), &folder).await {
            Ok(files) => files,
            Err(error) => {
                toast(
                    &handle,
                    format!("Could not read {}: {error}", folder.display()),
                    cx,
                );
                return;
            }
        };
        match files.as_slice() {
            [] => toast(
                &handle,
                format!(
                    "No database files in {} (looked for .{}).",
                    folder.display(),
                    DATABASE_FILE_EXTENSIONS.join(", .")
                ),
                cx,
            ),
            [_] => {
                cx.update(|window, cx| add_connections(&handle, &files, window, cx))
                    .ok();
            }
            _ => {
                handle
                    .update_in(cx, |workspace, window, cx| {
                        let workspace_handle = cx.weak_entity();
                        workspace.toggle_modal(window, cx, move |_window, cx| {
                            FolderDatabases::new(workspace_handle, folder, files, cx)
                        });
                    })
                    .ok();
            }
        }
    })
    .detach();
}

/// The picker is this machine's; a remote project's databases are on its host.
fn refuse_remote(workspace: &mut Workspace, cx: &mut Context<Workspace>) -> bool {
    if workspace.project().read(cx).is_local() {
        return false;
    }
    workspace.show_toast(
        Toast::new(
            NotificationId::unique::<OpenDatabaseRefused>(),
            "Opening a database file works in local projects. Add a remote one with Add Connection.",
        ),
        cx,
    );
    true
}

struct OpenDatabaseRefused;
struct OpenDatabaseFailed;

/// Saving one of these as a connection would only fail later, at connect,
/// further from the pick that caused it.
fn not_a_database_notice(skipped: &[PathBuf]) -> String {
    let names: Vec<String> = skipped
        .iter()
        .map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        })
        .collect();
    format!(
        "Skipped {} (not a database file; looked for .{}).",
        names.join(", "),
        DATABASE_FILE_EXTENSIONS.join(", .")
    )
}

fn toast(workspace: &WeakEntity<Workspace>, text: String, cx: &mut gpui::AsyncWindowContext) {
    workspace
        .update(cx, |workspace, cx| {
            workspace.show_toast(
                Toast::new(NotificationId::unique::<OpenDatabaseFailed>(), text),
                cx,
            );
        })
        .ok();
}

/// Saves every new connection in one settings write, then opens the first.
///
/// One write rather than one per file: each write reads the file it edits, and
/// several issued together could each miss the others' connection.
fn add_connections(
    workspace: &WeakEntity<Workspace>,
    paths: &[PathBuf],
    window: &mut Window,
    cx: &mut App,
) {
    let existing: Vec<(String, String)> = DatabaseSettings::get_global(cx)
        .connections
        .iter()
        .map(|connection| (connection.name.clone(), connection.url.clone()))
        .collect();
    let planned = plan_connections(paths, &existing);
    let Some(first) = planned.first().cloned() else {
        return;
    };
    let Some(driver) = driver_for_file(&first.path) else {
        workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(
                    Toast::new(
                        NotificationId::unique::<OpenDatabaseFailed>(),
                        format!("Could not open {} as a database.", first.path.display()),
                    ),
                    cx,
                );
            })
            .ok();
        return;
    };
    let entries = new_connection_entries(&planned, driver);

    let Ok(fs) = workspace.update(cx, |workspace, cx| {
        // The tab the connection opens in has to exist before `connect` looks
        // for it.
        crate::standalone::open(workspace, window, cx);
        workspace.project().read(cx).fs().clone()
    }) else {
        return;
    };
    if !entries.is_empty() {
        settings::update_settings_file(fs, cx, move |content, _cx| {
            content
                .database
                .get_or_insert_default()
                .connections
                .get_or_insert_default()
                .extend(entries);
        });
    }
    Filled::for_file(
        first.name,
        driver,
        first.path.to_string_lossy().into_owned(),
    )
    .connect(workspace, window, cx);
}

/// The settings entries for the planned files that are not connections yet.
fn new_connection_entries(
    planned: &[PlannedConnection],
    driver: &str,
) -> Vec<settings::DatabaseConnectionContent> {
    planned
        .iter()
        .filter(|connection| !connection.existing)
        .map(|connection| settings::DatabaseConnectionContent {
            name: Some(connection.name.clone()),
            driver: Some(driver.to_string()),
            url: Some(connection.path.to_string_lossy().into_owned()),
            safety: None,
        })
        .collect()
}

/// The database files a folder held, each with a box to tick.
pub(crate) struct FolderDatabases {
    workspace: WeakEntity<Workspace>,
    folder: PathBuf,
    files: Vec<(PathBuf, bool)>,
    focus_handle: FocusHandle,
}

impl FolderDatabases {
    fn new(
        workspace: WeakEntity<Workspace>,
        folder: PathBuf,
        files: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            workspace,
            folder,
            files: files.into_iter().map(|file| (file, true)).collect(),
            focus_handle: cx.focus_handle(),
        }
    }

    fn chosen(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|(_, chosen)| *chosen)
            .map(|(path, _)| path.clone())
            .collect()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = self.chosen();
        if chosen.is_empty() {
            return;
        }
        add_connections(&self.workspace, &chosen, window, cx);
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for FolderDatabases {}
impl ModalView for FolderDatabases {}

impl Focusable for FolderDatabases {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FolderDatabases {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.files.iter().filter(|(_, chosen)| *chosen).count();
        let add_label: SharedString = match chosen {
            1 => "Add 1 Connection".into(),
            count => format!("Add {count} Connections").into(),
        };
        v_flex()
            .key_context("FolderDatabases")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(32.))
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.confirm(window, cx)))
            .child(
                ModalHeader::new()
                    .headline("Databases in this folder")
                    .description(self.folder.display().to_string()),
            )
            .child(
                v_flex()
                    .px_3()
                    .pb_2()
                    .gap_1()
                    .max_h(rems(20.))
                    .id("folder-databases-list")
                    .overflow_y_scroll()
                    .children(
                        self.files
                            .iter()
                            .enumerate()
                            .map(|(index, (path, chosen))| {
                                let name = path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                Checkbox::new(("folder-database", index), (*chosen).into())
                                    .label(name)
                                    .on_click(cx.listener(
                                        move |this, state: &ToggleState, _window, cx| {
                                            if let Some(file) = this.files.get_mut(index) {
                                                file.1 = *state == ToggleState::Selected;
                                            }
                                            cx.notify();
                                        },
                                    ))
                            }),
                    ),
            )
            .child(
                ModalFooter::new().end_slot(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("folder-databases-cancel", "Cancel")
                                .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                        )
                        .child(
                            Button::new("folder-databases-add", add_label)
                                .style(ButtonStyle::Filled)
                                .disabled(chosen == 0)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.confirm(window, cx)),
                                ),
                        ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> PathBuf {
        PathBuf::from(raw)
    }

    #[test]
    fn only_sqlite_extensions_count_whatever_their_case() {
        assert!(is_database_file(&path("/a/app.sqlite")));
        assert!(is_database_file(&path("/a/app.DB")));
        assert!(is_database_file(&path("/a/data.sqlite3")));
        assert!(!is_database_file(&path("/a/app.sql")));
        assert!(!is_database_file(&path("/a/db")));
        assert!(!is_database_file(&path("/a/notes.md")));
    }

    #[test]
    fn a_file_is_named_after_itself() {
        let planned = plan_connections(&[path("/proj/app.sqlite")], &[]);
        assert_eq!(planned[0].name, "app");
        assert!(!planned[0].existing);
    }

    /// Taken by an existing connection, or by another file in the same pick.
    #[test]
    fn a_taken_name_gets_a_number() {
        let existing = vec![("app".to_string(), "/elsewhere/app.db".to_string())];
        let planned = plan_connections(&[path("/one/app.sqlite"), path("/two/app.db")], &existing);
        let names: Vec<_> = planned.iter().map(|plan| plan.name.as_str()).collect();
        assert_eq!(names, ["app (2)", "app (3)"]);
    }

    /// Opening a file that is already a connection opens that connection
    /// rather than adding a copy of it under a new name.
    #[test]
    fn a_file_already_connected_reuses_its_connection() {
        let existing = vec![("Orders".to_string(), "/proj/orders.db".to_string())];
        let planned = plan_connections(&[path("/proj/orders.db")], &existing);
        assert_eq!(
            planned,
            vec![PlannedConnection {
                path: path("/proj/orders.db"),
                name: "Orders".to_string(),
                existing: true,
            }]
        );
    }

    #[test]
    fn the_same_file_picked_twice_is_added_once() {
        let planned = plan_connections(&[path("/p/a.db"), path("/p/a.db")], &[]);
        assert_eq!(planned.len(), 1);
    }

    /// Everything new goes into the one settings write; a file that is already
    /// a connection adds nothing.
    #[test]
    fn only_new_files_become_settings_entries() {
        let existing = vec![("orders".to_string(), "/p/orders.db".to_string())];
        let planned = plan_connections(
            &[path("/p/orders.db"), path("/p/a.db"), path("/p/b.sqlite")],
            &existing,
        );
        let entries = new_connection_entries(&planned, "sqlite");
        let saved: Vec<_> = entries
            .iter()
            .map(|entry| (entry.name.as_deref(), entry.url.as_deref()))
            .collect();
        assert_eq!(
            saved,
            [
                (Some("a"), Some("/p/a.db")),
                (Some("b"), Some("/p/b.sqlite"))
            ]
        );
    }

    #[test]
    fn picked_files_that_are_not_databases_are_named_in_the_notice() {
        let notice = not_a_database_notice(&[path("/p/notes.txt"), path("/p/dump.sql")]);
        assert!(
            notice.starts_with("Skipped notes.txt, dump.sql"),
            "{notice}"
        );
    }

    /// Every file starts ticked; unticking one leaves it out of the add.
    #[gpui::test]
    fn the_folder_list_adds_only_ticked_files(cx: &mut gpui::TestAppContext) {
        let modal = cx.new(|cx| {
            FolderDatabases::new(
                WeakEntity::new_invalid(),
                path("/p"),
                vec![path("/p/a.db"), path("/p/b.db"), path("/p/c.db")],
                cx,
            )
        });
        modal.update(cx, |modal, _| {
            assert_eq!(modal.chosen().len(), 3);
            if let Some(file) = modal.files.get_mut(1) {
                file.1 = false;
            }
            assert_eq!(modal.chosen(), vec![path("/p/a.db"), path("/p/c.db")]);
        });
    }

    /// One level deep, database files only, in name order.
    #[gpui::test]
    async fn a_folder_offers_its_own_database_files(cx: &mut gpui::TestAppContext) {
        let fs = fs::FakeFs::new(cx.executor());
        fs.insert_tree(
            "/proj",
            serde_json::json!({
                "b.sqlite": "",
                "a.db": "",
                "notes.md": "",
                "dump.sql": "",
                "nested": { "deep.db": "" },
            }),
        )
        .await;
        let files = database_files_in(fs.as_ref(), Path::new("/proj"))
            .await
            .unwrap();
        assert_eq!(files, vec![path("/proj/a.db"), path("/proj/b.sqlite")]);
    }
}
