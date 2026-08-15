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

        let mut engines = driver_registry::catalogue(cx);
        // Installed first, then alphabetically: an engine someone can actually
        // reach today should not sit below one they cannot.
        engines.sort_by(|a, b| b.installed.cmp(&a.installed).then(a.name.cmp(&b.name)));

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

    pub(crate) fn selected_engine(&self) -> Option<&CatalogueEntry> {
        self.engines.get(self.selected)
    }

    /// Moves to the second screen for whatever is selected.
    pub(crate) fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(engine) = self.selected_engine().cloned() else {
            return;
        };
        if !engine.installed {
            // Said rather than silently ignored: a Continue button that does
            // nothing is the worst answer available here.
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

    /// Starts the chosen engine's driver and asks it what to put on the form.
    fn open(&mut self, engine: CatalogueEntry, window: &mut Window, cx: &mut Context<Self>) {
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

        Ok(Filled {
            name,
            driver: engine.driver.to_string(),
            url,
            secret: values
                .iter()
                .find(|(field, value)| field.secret && !value.is_empty())
                .map(|(_, value)| value.clone()),
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
        let Some(engine) = self.engine().cloned() else {
            return;
        };
        let Some(descriptor) = self.descriptor_for(&engine, cx) else {
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
            Step::Asking { engine }
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
                let panel = workspace
                    .update(cx, |workspace, cx| {
                        workspace.panel::<crate::DatabasePanel>(cx)
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
        let (modal, cx) = modal_for_test(cx).await;

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
        let (modal, cx) = modal_for_test(cx).await;

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
        let (modal, cx) = modal_for_test(cx).await;

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
        let (modal, cx) = modal_for_test(cx).await;

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
    #[gpui::test]
    async fn the_picker_lists_engines_with_no_driver_and_marks_them(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx).await;

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

    /// Continue on an engine with no driver must say why rather than doing
    /// nothing, which is the worst answer a button can give.
    #[gpui::test]
    async fn continuing_on_an_uninstalled_engine_explains_itself(cx: &mut gpui::TestAppContext) {
        let (modal, cx) = modal_for_test(cx).await;

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
        let (modal, cx) = modal_for_test(cx).await;

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
