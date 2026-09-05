//! Adding a connection without opening a settings file.
//!
//! Two screens: pick an engine, then fill in what that engine asks for. The
//! second is the interesting one -- **the driver says what the fields are**.
//! Zode starts it, reads `connection_form` out of `initialize`, and builds the
//! form from that, so nothing in this crate learns what a host or a file path
//! means to any particular engine.
//!
//! A driver that declares no form is asked for a URL and nothing else, which is
//! what every driver written before `connection_form` existed still gets, and
//! what "Import from URL" switches to deliberately.

use crate::connection_store::{DatabaseSettings, write_secret};
use crate::driver_registry::{self, CatalogueEntry};
use crate::session::driver_capabilities;
use database::install::InstallProgress;
use database::protocol::{ConnectionField, ConnectionForm};
use database::registry::DriverDescriptor;
use editor::Editor;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task,
    WeakEntity, Window,
};
use settings::Settings as _;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

/// What a driver that declares nothing is asked for.
fn fallback_form() -> ConnectionForm {
    ConnectionForm {
        fields: vec![ConnectionField {
            key: "url".into(),
            label: "URL".into(),
            group: Some("Connection".into()),
            placeholder: Some("engine://user@host:5432/database".into()),
            ..Default::default()
        }],
        url_template: "{url}".into(),
    }
}

/// A stand-in engine for the URL-only path, so the second screen has a name and
/// a driver like any other.
/// Every engine, installed ones first.
///
/// Installed first, then alphabetically: an engine someone can actually reach
/// today should not sit below one they cannot.
fn sorted_catalogue(cx: &mut App) -> Vec<CatalogueEntry> {
    let mut engines = driver_registry::catalogue(cx);
    engines.sort_by(|a, b| b.installed.cmp(&a.installed).then(a.name.cmp(&b.name)));
    engines
}

fn imported_entry() -> CatalogueEntry {
    CatalogueEntry {
        driver: SharedString::new_static(""),
        name: SharedString::new_static("Connection"),
        description: SharedString::new_static("From a URL"),
        group: SharedString::new_static("Relational"),
        installed: true,
    }
}

pub(crate) enum Step {
    /// Choosing which engine.
    PickEngine,
    /// Fetching a driver that is not on this machine yet.
    ///
    /// Ahead of [`Step::Asking`] because it has to be: there is nothing to ask
    /// until there is a driver to ask. Unlike `Asking` it has a position worth
    /// drawing and can be given up on, which is why it is a step of its own
    /// rather than a flag on that one.
    Downloading {
        engine: CatalogueEntry,
        progress: InstallProgress,
    },
    /// Waiting for the chosen driver to say what it wants asked.
    Asking { engine: CatalogueEntry },
    Filling {
        engine: CatalogueEntry,
        form: ConnectionForm,
        /// One editor per field, in the driver's order.
        inputs: Vec<Entity<Editor>>,
    },
    /// The driver could not be started, or would not answer.
    Unreachable {
        engine: CatalogueEntry,
        message: SharedString,
    },
}

/// What the Status row is saying.
pub(crate) enum TestState {
    Untested,
    Running,
    Reached(SharedString),
    Refused(SharedString),
}

pub(crate) struct ConnectionModal {
    pub(crate) focus_handle: FocusHandle,
    /// Held for the project's `Fs`, which is what writes the settings file, and
    /// for reaching the panel after "Save & Connect".
    workspace: WeakEntity<Workspace>,
    pub(crate) engines: Vec<CatalogueEntry>,
    /// Which row the picker has highlighted. `Continue` acts on it, so a click
    /// selects rather than commits -- an engine chosen by a stray click is one
    /// nobody meant to choose.
    pub(crate) selected: usize,
    pub(crate) search_editor: Entity<Editor>,
    pub(crate) step: Step,
    /// What the connection is called. Separate from the driver's fields because
    /// it is Zode's, not the engine's: it is how a project pins a connection.
    pub(crate) name_editor: Entity<Editor>,
    pub(crate) test: TestState,
    pub(crate) error: Option<SharedString>,
    _task: Option<Task<()>>,
    _test_task: Option<Task<()>>,
}

impl ConnectionModal {
    pub(crate) fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Connection name", window, cx);
            editor
        });
        let search_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Search", window, cx);
            editor
        });
        // Redrawn on every keystroke so the list narrows as it is typed.
        cx.subscribe(&search_editor, |_this, _editor, event, cx| {
            if matches!(event, editor::EditorEvent::BufferEdited) {
                cx.notify();
            }
        })
        .detach();

        let engines = sorted_catalogue(cx);

        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            engines,
            selected: 0,
            search_editor,
            step: Step::PickEngine,
            name_editor,
            test: TestState::Untested,
            error: None,
            _task: None,
            _test_task: None,
        }
    }

    /// The rows the picker is showing, with their index into `engines`.
    ///
    /// Matched against the description as well as the name, because half of
    /// what someone types is what the engine *is* -- "postgres" finds
    /// CockroachDB, which is exactly the point of writing the descriptions.
    pub(crate) fn matches(&self, cx: &App) -> Vec<(usize, &CatalogueEntry)> {
        let query = self.search_editor.read(cx).text(cx).trim().to_lowercase();
        self.engines
            .iter()
            .enumerate()
            .filter(|(_, engine)| {
                query.is_empty()
                    || engine.name.to_lowercase().contains(&query)
                    || engine.description.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub(crate) fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index;
        self.error = None;
        cx.notify();
    }

    /// Moves the highlight through what is on screen, not through the whole
    /// list -- arrowing into a row the search has hidden reads as a bug.
    pub(crate) fn select_next(&mut self, forward: bool, cx: &mut Context<Self>) {
        let visible: Vec<usize> = self
            .matches(cx)
            .into_iter()
            .map(|(index, _)| index)
            .collect();
        let Some(position) = visible.iter().position(|index| *index == self.selected) else {
            if let Some(first) = visible.first() {
                self.select(*first, cx);
            }
            return;
        };
        let next = if forward {
            (position + 1).min(visible.len().saturating_sub(1))
        } else {
            position.saturating_sub(1)
        };
        if let Some(index) = visible.get(next) {
            self.select(*index, cx);
        }
    }

    /// Re-reads which engines are reachable.
    ///
    /// Which of them are installed changes underneath an open dialog now: a
    /// driver downloaded for one engine lands while the list is on screen.
    pub(crate) fn reload_engines(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_engine().map(|engine| engine.name.clone());
        self.engines = sorted_catalogue(cx);
        // Kept on the same engine rather than the same index: installing a
        // driver moves it up the list, and following the index would silently
        // select whatever slid into its place.
        if let Some(selected) = selected
            && let Some(index) = self
                .engines
                .iter()
                .position(|engine| engine.name == selected)
        {
            self.selected = index;
        }
        cx.notify();
    }

    pub(crate) fn selected_engine(&self) -> Option<&CatalogueEntry> {
        self.engines.get(self.selected)
    }

    /// Moves to the second screen for whatever is selected.
    pub(crate) fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(engine) = self.selected_engine().cloned() else {
            return;
        };
        if !engine.installed {
            if driver_registry::is_publishable(&engine.driver) {
                // Zode publishes this one, so the honest answer is to go and
                // get it rather than to explain why it is missing.
                self.download_driver(engine, window, cx);
                return;
            }
            // Still true for the engines Zode ships no driver for -- Oracle and
            // SQL Server share a protocol with nothing here. Said rather than
            // silently ignored: a Continue button that does nothing is the
            // worst answer available.
            self.error = Some(
                format!(
                    "No driver for {} is installed. An extension can provide one.",
                    engine.name
                )
                .into(),
            );
            cx.notify();
            return;
        }
        self.open(engine, window, cx);
    }

    /// Skips the engine list and asks for a URL.
    pub(crate) fn import_from_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let engine = imported_entry();
        self.name_editor.update(cx, |editor, cx| {
            if editor.text(cx).is_empty() {
                editor.set_text("Imported", window, cx);
            }
        });
        self.show_form(engine, Some(fallback_form()), window, cx);
    }

    /// Holds the one background task this dialog runs at a time.
    ///
    /// Dropping the previous one cancels it, which is what makes leaving a step
    /// actually stop the work that step started.
    pub(crate) fn set_task(&mut self, task: impl Into<Option<Task<()>>>) {
        self._task = task.into();
    }

    /// Starts the chosen engine's driver and asks it what to put on the form.
    pub(crate) fn open(
        &mut self,
        engine: CatalogueEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(descriptor) = self.descriptor_for(&engine, cx) else {
            self.step = Step::Unreachable {
                message: format!("No driver called `{}` is registered.", engine.driver).into(),
                engine,
            };
            cx.notify();
            return;
        };

        self.step = Step::Asking {
            engine: engine.clone(),
        };
        self.error = None;
        cx.notify();

        let asked = driver_capabilities(descriptor, cx);
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let capabilities = asked.await;
            this.update_in(cx, |this, window, cx| match capabilities {
                Ok(capabilities) => {
                    this.show_form(engine, capabilities.connection_form, window, cx)
                }
                Err(error) => {
                    this.step = Step::Unreachable {
                        engine,
                        message: format!("{error:#}").into(),
                    };
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn descriptor_for(&self, engine: &CatalogueEntry, cx: &mut App) -> Option<DriverDescriptor> {
        driver_registry::global(cx)
            .read(cx)
            .get(&engine.driver)
            .cloned()
    }

    /// Builds the form the driver described, or the URL-only one for a driver
    /// that described none.
    ///
    /// Separate from [`Self::open`] so it can be reached without starting a
    /// process: what is worth testing here is the form, not the spawn.
    pub(crate) fn show_form(
        &mut self,
        engine: CatalogueEntry,
        form: Option<ConnectionForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let form = form.unwrap_or_else(fallback_form);
        let inputs = form
            .fields
            .iter()
            .map(|field| Self::input_for(field, window, cx))
            .collect();
        // Named after the engine, so the common case is one edit rather than
        // one more thing to invent.
        if self.name_editor.read(cx).text(cx).is_empty() {
            self.name_editor.update(cx, |editor, cx| {
                editor.set_text(engine.name.as_ref(), window, cx);
            });
        }
        self.test = TestState::Untested;
        self.step = Step::Filling {
            engine,
            form,
            inputs,
        };
        cx.notify();
    }

    fn input_for(
        field: &ConnectionField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        let field = field.clone();
        cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            if field.secret {
                editor.set_masked(true, cx);
            }
            if let Some(placeholder) = &field.placeholder {
                editor.set_placeholder_text(placeholder.as_str(), window, cx);
            }
            if let Some(default) = &field.default {
                editor.set_text(default.as_str(), window, cx);
            }
            editor
        })
    }

    /// Reads the form, and says what is missing rather than saving half of it.
    fn filled(&self, cx: &App) -> Result<Filled, SharedString> {
        let Step::Filling {
            engine,
            form,
            inputs,
        } = &self.step
        else {
            return Err("Choose an engine first.".into());
        };

        let name = self.name_editor.read(cx).text(cx).trim().to_string();
        if name.is_empty() {
            return Err("Give the connection a name.".into());
        }

        let mut values: Vec<(&ConnectionField, String)> = Vec::with_capacity(form.fields.len());
        for (field, input) in form.fields.iter().zip(inputs) {
            let value = input.read(cx).text(cx).trim().to_string();
            // A blank password is a server that wants none. A blank anything
            // else is an address with a hole in it.
            if value.is_empty() && !field.secret {
                return Err(format!("{} is needed.", field.label).into());
            }
            values.push((field, value));
        }

        let url = form.build_url(|key| {
            values
                .iter()
                .find(|(field, _)| field.key == key)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        });
        // A URL that arrived already built may carry a password in its
        // userinfo, and the import path is exactly that: one plain field, no
        // `secret` on it, so `build_url` had nothing to blank and the password
        // went into the settings file in the clear. Taken out here rather than
        // at the import path alone, so no future form can reintroduce it.
        let (url, url_password) = database::protocol::split_password(&url);

        // A URL pasted into the import path arrived under a stand-in engine
        // carrying no driver at all (see `imported_entry`), and it was saved
        // that way: a connection naming no driver, which nothing can ever open,
        // and which made `Test Connection` return without a word. The scheme is
        // the only thing that says what a URL is for, so it is read here.
        let driver = if engine.driver.is_empty() {
            driver_registry::driver_for_url(&url)
                .ok_or_else(|| {
                    SharedString::from(
                        "That URL's scheme names no engine this build knows. \
                         Pick the engine from the list instead.",
                    )
                })?
                .to_string()
        } else {
            engine.driver.to_string()
        };

        Ok(Filled {
            name,
            driver,
            url,
            // A field marked secret wins: it is what the person typed into
            // this form, while the one in the URL may be left over from
            // whatever they pasted.
            secret: values
                .iter()
                .find(|(field, value)| field.secret && !value.is_empty())
                .map(|(_, value)| value.clone())
                .or(url_password),
        })
    }

    /// Opens the connection, reports, and closes it again.
    ///
    /// Worth having as its own button: everything else on this form is guesses
    /// until something on the other end agrees, and finding out after saving
    /// means editing a settings file to correct it.
    pub(crate) fn test_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filled = match self.filled(cx) {
            Ok(filled) => filled,
            Err(message) => {
                self.error = Some(message);
                cx.notify();
                return;
            }
        };
        // Looked up by the driver the form resolved rather than by the engine
        // on screen: the import path's engine is a stand-in with no driver, so
        // this lookup failed for it -- and failed by returning, which is a
        // button that does nothing at all when pressed.
        let Some(descriptor) = driver_registry::global(cx)
            .read(cx)
            .get(&filled.driver)
            .cloned()
        else {
            self.error =
                Some(format!("No driver called `{}` is registered.", filled.driver).into());
            cx.notify();
            return;
        };

        self.error = None;
        self.test = TestState::Running;
        cx.notify();

        let config = crate::connection_store::ConnectionConfig {
            name: filled.name.clone(),
            driver: filled.driver.clone(),
            url: filled.url.clone(),
        };
        let secret = filled.secret;
        self._test_task = Some(cx.spawn_in(window, async move |this, cx| {
            let opened = crate::session::Session::open(descriptor, config, secret, cx).await;
            this.update(cx, |this, cx| {
                this.test = match opened {
                    // Dropped right here, which kills the driver: this was a
                    // question, not a session.
                    Ok(session) => TestState::Reached(session.driver_name.into()),
                    Err(error) => TestState::Refused(format!("{error:#}").into()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn engine(&self) -> Option<&CatalogueEntry> {
        match &self.step {
            Step::Downloading { engine, .. }
            | Step::Asking { engine }
            | Step::Filling { engine, .. }
            | Step::Unreachable { engine, .. } => Some(engine),
            Step::PickEngine => None,
        }
    }

    /// Writes the connection down, and optionally opens it.
    pub(crate) fn save(&mut self, connect: bool, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let filled = match self.filled(cx) {
            Ok(filled) => filled,
            Err(message) => {
                self.error = Some(message);
                cx.notify();
                return;
            }
        };
        if let Err(message) = filled.save(&workspace, cx) {
            self.error = Some(message);
            cx.notify();
            return;
        }

        if connect {
            filled.connect(&workspace, window, cx);
        }
        cx.emit(DismissEvent);
    }

    /// Back to the engine list, keeping whatever name was typed.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        self.step = Step::PickEngine;
        self.error = None;
        self.test = TestState::Untested;
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

/// A form that has been read and found complete.
struct Filled {
    name: String,
    driver: String,
    url: String,
    /// Kept apart from `url` the whole way down: this goes to the keychain and
    /// nowhere else.
    secret: Option<String>,
}

impl Filled {
    /// Writes the connection into the user's settings, and its password into
    /// the keychain.
    ///
    /// Settings rather than a store belonging to this dialog: a connection
    /// added here and one typed by hand must be the same thing, or the two
    /// would drift and only one of them would be in anyone's backup.
    fn save(&self, workspace: &WeakEntity<Workspace>, cx: &mut App) -> Result<(), SharedString> {
        if let Some(clash) = name_clash(&self.name, cx) {
            return Err(clash);
        }

        if let Some(secret) = &self.secret {
            write_secret(
                zed_credentials_provider::global(cx),
                self.url.clone(),
                self.name.clone(),
                secret.clone(),
                cx,
            )
            // Detached: the settings write below is what makes the connection
            // appear, and a keychain slow to answer should not hold the dialog.
            .detach();
        }

        let entry = settings::DatabaseConnectionContent {
            name: Some(self.name.clone()),
            driver: Some(self.driver.clone()),
            url: Some(self.url.clone()),
        };
        // Taken from the project rather than from a global, which is also what
        // keeps this crate off a direct dependency on `fs`.
        let fs = workspace
            .update(cx, |workspace, cx| {
                workspace.project().read(cx).fs().clone()
            })
            .map_err(|_| SharedString::from("This window is closing."))?;
        settings::update_settings_file(fs, cx, move |content, _cx| {
            content
                .database
                .get_or_insert_default()
                .connections
                .get_or_insert_default()
                .push(entry);
        });
        Ok(())
    }

    /// Opens the connection that was just saved.
    ///
    /// Deferred, because the node it opens does not exist yet: the settings
    /// write above is what creates it, and the panel only hears about that on
    /// the next settings notification.
    fn connect(&self, workspace: &WeakEntity<Workspace>, window: &mut Window, cx: &mut App) {
        let (workspace, name) = (workspace.clone(), self.name.clone());
        window
            .spawn(cx, async move |cx| {
                // A pane item now, not a dock panel: the database lives in a tab.
                let panel = workspace
                    .update(cx, |workspace, cx| {
                        workspace.items_of_type::<crate::DatabasePanel>(cx).next()
                    })
                    .ok()
                    .flatten()?;
                panel
                    .update_in(cx, |panel, window, cx| {
                        let index = panel.index_of(&name)?;
                        panel.toggle_connection(index, window, cx);
                        panel.set_active(index, window, cx);
                        Some(())
                    })
                    .ok()
            })
            .detach();
    }
}

/// Refuses a name another connection already has.
///
/// A project pins a connection by name, so two called the same thing would pin
/// each other's -- and the keychain entry is found through the connection, not
/// through the row.
pub(crate) fn name_clash(name: &str, cx: &App) -> Option<SharedString> {
    DatabaseSettings::get_global(cx)
        .connections
        .iter()
        .any(|connection| connection.name == name)
        .then(|| format!("There is already a connection called \"{name}\".").into())
}

impl ModalView for ConnectionModal {}

impl EventEmitter<DismissEvent> for ConnectionModal {}

impl Focusable for ConnectionModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.step {
            Step::PickEngine => self.search_editor.focus_handle(cx),
            Step::Filling { .. } => self.name_editor.focus_handle(cx),
            _ => self.focus_handle.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database_panel_tests::modal_for_test;
    use database::install::test_support::{driver_archive, manifest_for, sha256_of};
    use http_client::{FakeHttpClient, HttpClient};
    use tempfile::TempDir;

    fn engine(installed: bool) -> CatalogueEntry {
        CatalogueEntry {
            driver: "example".into(),
            name: "Example".into(),
            description: "An engine".into(),
            group: "Relational".into(),
            installed,
        }
    }

    fn server_form() -> ConnectionForm {
        ConnectionForm {
            fields: vec![
                ConnectionField {
                    key: "host".into(),
                    label: "Host".into(),
                    group: Some("Connection".into()),
                    default: Some("localhost".into()),
                    url_encoded: true,
                    ..Default::default()
                },
                ConnectionField {
                    key: "password".into(),
                    label: "Password".into(),
                    group: Some("Authentication".into()),
                    secret: true,
                    ..Default::default()
                },
            ],
            url_template: "example://{host}".into(),
        }
    }

    /// A driver that predates `connection_form` -- or one whose DSN does not
    /// decompose -- must still be addable, not left out of the dialog.
    #[gpui::test]
    async fn a_driver_that_declares_no_form_is_asked_for_a_url(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(engine(true), None, window, cx);
            let Step::Filling { form, .. } = &modal.step else {
                panic!("the form should be showing");
            };
            assert_eq!(form.fields.len(), 1);
            assert_eq!(form.fields[0].key, "url");
        });
    }

    /// A default the driver supplied is a field already answered. Making the
    /// user retype `localhost` would be asking them to do the driver's work.
    #[gpui::test]
    async fn a_declared_default_arrives_already_typed(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(engine(true), Some(server_form()), window, cx);
            let Step::Filling { inputs, .. } = &modal.step else {
                panic!("the form should be showing");
            };
            assert_eq!(inputs[0].read(cx).text(cx), "localhost");
            assert_eq!(
                modal.name_editor.read(cx).text(cx),
                "Example",
                "the engine's name is a better first guess than an empty box"
            );
        });
    }

    /// A blank password is a server that wants none; a blank anything else is
    /// an address with a hole in it.
    #[gpui::test]
    async fn a_blank_required_field_is_refused_and_a_blank_password_is_not(
        cx: &mut gpui::TestAppContext,
    ) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(engine(true), Some(server_form()), window, cx);
            let Step::Filling { inputs, .. } = &modal.step else {
                panic!("the form should be showing");
            };
            let inputs = inputs.clone();

            inputs[0].update(cx, |editor, cx| editor.set_text("", window, cx));
            // Matched rather than `expect_err`, which would need `Debug` on a
            // struct holding a password.
            let refused = match modal.filled(cx) {
                Ok(_) => panic!("a blank host is not an address"),
                Err(message) => message,
            };
            assert!(refused.contains("Host"), "{refused}");

            inputs[0].update(cx, |editor, cx| editor.set_text("db.example", window, cx));
            let Ok(filled) = modal.filled(cx) else {
                panic!("a blank password is allowed");
            };
            assert_eq!(filled.url, "example://db.example");
            assert!(filled.secret.is_none());
        });
    }

    /// The password must reach the keychain and nothing else. This is the check
    /// that it never travels in the field written to a settings file.
    #[gpui::test]
    async fn a_typed_password_is_kept_out_of_the_url(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            modal.show_form(engine(true), Some(server_form()), window, cx);
            let Step::Filling { inputs, .. } = &modal.step else {
                panic!("the form should be showing");
            };
            inputs[1].update(cx, |editor, cx| editor.set_text("hunter2", window, cx));

            let Ok(filled) = modal.filled(cx) else {
                panic!("the form is complete");
            };
            assert!(!filled.url.contains("hunter2"), "{}", filled.url);
            assert_eq!(filled.secret.as_deref(), Some("hunter2"));
        });
    }

    /// Every engine Zode knows the name of is listed, including the ones it
    /// cannot reach. An engine missing from the picker is one nobody can
    /// discover is missing.
    ///
    /// Zode bundles no drivers, so "reachable" is now a thing a machine either
    /// has done or has not. One is installed here deliberately: the ordering
    /// this asserts only means anything when both kinds are present.
    #[gpui::test]
    async fn the_picker_lists_engines_with_no_driver_and_marks_them(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres"]).await;

        modal.read_with(cx, |modal, _| {
            assert!(
                modal.engines.iter().any(|engine| !engine.installed),
                "an engine with no driver must still be listed"
            );
            assert!(
                modal.engines.iter().any(|engine| engine.installed),
                "and the shipped ones must be listed as reachable"
            );
            let first_uninstalled = modal
                .engines
                .iter()
                .position(|engine| !engine.installed)
                .unwrap_or_default();
            let last_installed = modal
                .engines
                .iter()
                .rposition(|engine| engine.installed)
                .unwrap_or_default();
            assert!(
                last_installed < first_uninstalled,
                "what someone can reach today must not sit below what they cannot"
            );
        });
    }

    // ---- Downloading a driver ---------------------------------------------

    const TEST_VERSION: &str = "0.0.0";

    /// A release serving one driver, and nothing else.
    fn release_serving(manifest: Vec<u8>, archive: Vec<u8>) -> std::sync::Arc<dyn HttpClient> {
        FakeHttpClient::create(move |request| {
            let manifest = manifest.clone();
            let archive = archive.clone();
            async move {
                let path = request.uri().path().to_string();
                let body = if path.ends_with("zode-db-drivers-manifest.json") {
                    manifest
                } else if path.ends_with("zode-db-postgres.tar.gz") {
                    archive
                } else {
                    return Ok(http_client::Response::builder()
                        .status(404)
                        .body(Default::default())?);
                };
                Ok(http_client::Response::builder()
                    .status(200)
                    .body(body.into())?)
            }
        })
    }

    /// Points the dialog at a release a test serves, into a store it owns.
    async fn release_with(cx: &mut gpui::VisualTestContext, checksum: Checksum) -> TempDir {
        let store = tempfile::tempdir().expect("a temporary directory");
        let archive = driver_archive("postgres").await;
        let published = match checksum {
            Checksum::Correct => sha256_of(&archive),
            Checksum::Wrong => "a".repeat(64),
        };
        let manifest = manifest_for("postgres", &archive, published, TEST_VERSION);
        cx.update(|_window, cx| {
            cx.set_http_client(release_serving(manifest, archive));
            driver_registry::set_installer_for_test(store.path().to_path_buf(), TEST_VERSION, cx);
        });
        store
    }

    /// Waits for the download to finish, whichever way it ends.
    ///
    /// `run_until_parked` alone is not enough: the install reads and writes
    /// real files, so the task parks on the IO reactor and the test would look
    /// at a bar that has not moved yet. Bounded rather than unbounded so a
    /// download that never finishes fails the test instead of hanging CI.
    async fn until_the_download_ends(
        modal: &Entity<ConnectionModal>,
        cx: &mut gpui::VisualTestContext,
    ) {
        for _ in 0..500 {
            cx.run_until_parked();
            let still_going = modal.read_with(cx, |modal, _| {
                matches!(modal.step, Step::Downloading { .. })
            });
            if !still_going {
                return;
            }
            cx.executor()
                .timer(std::time::Duration::from_millis(10))
                .await;
        }
        panic!("the download never finished");
    }

    enum Checksum {
        Correct,
        Wrong,
    }

    fn select_engine(modal: &mut ConnectionModal, driver: &str, cx: &mut Context<ConnectionModal>) {
        let index = modal
            .engines
            .iter()
            .position(|engine| engine.driver == driver)
            .unwrap_or_else(|| panic!("`{driver}` must be listed"));
        modal.select(index, cx);
    }

    /// The defect this whole path exists for: Zode bundles no drivers, so the
    /// ordinary state of an engine nobody has used is "absent" -- and Continue
    /// used to answer that by suggesting an extension that was never the
    /// answer for an engine Zode publishes a driver for itself.
    #[gpui::test]
    async fn downloading_a_driver_makes_it_one_the_dialog_can_open(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        let (modal, cx) = modal_for_test(cx, &[]).await;
        let _store = release_with(cx, Checksum::Correct).await;

        modal.update_in(cx, |modal, window, cx| {
            select_engine(modal, "postgres", cx);
            modal.advance(window, cx);
            assert!(
                matches!(modal.step, Step::Downloading { .. }),
                "Continue on an absent driver must fetch it, not explain itself"
            );
        });
        until_the_download_ends(&modal, cx).await;

        cx.update(|_window, cx| {
            let registry = driver_registry::global(cx);
            let registry = registry.read(cx);
            assert!(
                registry
                    .get("postgres")
                    .is_some_and(|driver| driver.is_installed()),
                "the registry every other path reads must be updated before anything acts on it"
            );
        });
        modal.read_with(cx, |modal, _| {
            assert!(
                !matches!(modal.step, Step::Downloading { .. }),
                "a finished download must carry on rather than sit on the bar"
            );
            assert!(
                modal
                    .engines
                    .iter()
                    .any(|engine| engine.driver == "postgres" && engine.installed),
                "the list must stop calling it absent"
            );
        });
    }

    /// An unverifiable binary is one that gets executed, so the download is
    /// refused -- and refusing has to leave a way forward, not a dead dialog.
    #[gpui::test]
    async fn a_download_that_fails_its_checksum_says_so_and_offers_another_go(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let (modal, cx) = modal_for_test(cx, &[]).await;
        let _store = release_with(cx, Checksum::Wrong).await;

        modal.update_in(cx, |modal, window, cx| {
            select_engine(modal, "postgres", cx);
            modal.advance(window, cx);
        });
        until_the_download_ends(&modal, cx).await;

        modal.read_with(cx, |modal, _| {
            let Step::Unreachable { engine, message } = &modal.step else {
                panic!("a refused download must say so");
            };
            assert_eq!(engine.driver, "postgres");
            assert!(message.contains("checksum"), "{message}");
            assert!(
                !engine.installed,
                "nothing may be installed from a download that failed its checksum"
            );
        });

        // And the failure must not be cached: Retry has to try, not replay.
        modal.update_in(cx, |modal, window, cx| {
            modal.retry_download(window, cx);
            assert!(matches!(modal.step, Step::Downloading { .. }));
        });
    }

    #[gpui::test]
    async fn cancelling_a_download_returns_to_the_engine_list(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        let (modal, cx) = modal_for_test(cx, &[]).await;
        let store = release_with(cx, Checksum::Correct).await;

        modal.update_in(cx, |modal, window, cx| {
            select_engine(modal, "postgres", cx);
            modal.advance(window, cx);
            modal.cancel_download(cx);
            assert!(matches!(modal.step, Step::PickEngine));
        });
        cx.run_until_parked();

        assert_eq!(
            database::install::store::installed_path_in(store.path(), "postgres", TEST_VERSION),
            None,
            "giving up must leave the store as it was"
        );
    }

    /// Oracle and SQL Server share a wire protocol with nothing Zode ships, so
    /// there is no driver to go and get. Offering to download one would be a
    /// button that can only fail.
    #[gpui::test]
    async fn an_engine_zode_publishes_no_driver_for_is_not_offered_a_download(
        cx: &mut gpui::TestAppContext,
    ) {
        let (modal, cx) = modal_for_test(cx, &[]).await;

        modal.update_in(cx, |modal, window, cx| {
            select_engine(modal, "oracle", cx);
            modal.advance(window, cx);

            assert!(
                matches!(modal.step, Step::PickEngine),
                "there is nothing to download, so nothing may be started"
            );
            let error = modal.error.clone().expect("it must still say why");
            assert!(error.contains("extension"), "{error}");
        });
    }

    /// Continue on an engine with no driver must say why rather than doing
    /// nothing, which is the worst answer a button can give.
    #[gpui::test]
    async fn continuing_on_an_uninstalled_engine_explains_itself(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            let index = modal
                .engines
                .iter()
                .position(|engine| !engine.installed)
                .expect("one engine has no driver");
            modal.select(index, cx);
            modal.advance(window, cx);

            assert!(
                matches!(modal.step, Step::PickEngine),
                "it must not move on to a form it cannot fill"
            );
            let error = modal.error.clone().expect("it must say why");
            assert!(error.contains("extension"), "{error}");
        });
    }

    /// Searching matches the description too: half of what someone types is
    /// what the engine *is*, which is the whole reason the descriptions exist.
    #[gpui::test]
    async fn the_search_matches_descriptions_as_well_as_names(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx, &["postgres", "sqlite", "mysql", "mongodb"]).await;

        modal.update_in(cx, |modal, window, cx| {
            modal
                .search_editor
                .update(cx, |editor, cx| editor.set_text("wire", window, cx));
            assert!(
                modal.matches(cx).is_empty(),
                "a search matching nothing must narrow to nothing"
            );

            modal
                .search_editor
                .update(cx, |editor, cx| editor.set_text("single file", window, cx));
            let found = modal.matches(cx);
            assert_eq!(
                found.len(),
                1,
                "a phrase from one description must find exactly that engine"
            );
        });
    }
}
