//! Storing the API key of the translation service in the system keychain.
//!
//! The key is never a setting: it is read from the masked field only while it
//! is being written, then the field is replaced by an empty one, so neither
//! the field nor its undo history holds it longer.

use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString, Task,
    Window,
};
use settings::Settings as _;
use translate::{
    ProviderKind, TranslateRequest, TranslateSettings, delete_api_key, provider_display_name,
    provider_requires_api_key, provider_uses_api_key, read_api_key, write_api_key,
};
use ui::{KeyBinding, Modal, ModalFooter, ModalHeader, Section, Tooltip, prelude::*};
use ui_input::InputField;
use workspace::{DismissDecision, ModalView};

const TEST_TEXT: &str = "Hello";
const TEST_FROM: &str = "en";
const TEST_TO: &str = "vi";
pub(crate) const TEST_BUTTON_LABEL: &str = "Test Saved Key";
const TEST_WITHOUT_KEY_LABEL: &str = "Test";

#[derive(Clone, Debug, PartialEq)]
enum StoredKey {
    Checking,
    Present,
    Absent,
    /// The keychain refused or failed to answer, which says nothing about
    /// whether a key is there.
    Unreadable(SharedString),
    /// The provider takes no key, so the keychain is never asked.
    NotUsed,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Notice {
    Saved,
    Removed,
    Failed(SharedString),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TestOutcome {
    Running,
    Translated(SharedString),
    Failed(SharedString),
}

pub struct ApiKeyModal {
    provider: ProviderKind,
    /// Focus for a provider without a key, where the key field is not shown.
    focus_handle: FocusHandle,
    placeholder: &'static str,
    pub(crate) key_input: Entity<InputField>,
    stored_key: StoredKey,
    notice: Option<Notice>,
    test_outcome: Option<TestOutcome>,
    _read_task: Task<()>,
    /// A keychain write or delete. While it runs, Save and Remove are off and
    /// the dialog does not close, since dropping the task would cancel it
    /// without a word.
    keychain_task: Option<Task<()>>,
    test_task: Option<Task<()>>,
}

impl ApiKeyModal {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let provider = TranslateSettings::get_global(cx).provider;
        let placeholder = if provider_requires_api_key(provider) {
            "Paste the API key"
        } else {
            "Paste the API key (optional for local servers)"
        };
        let key_input = new_key_input(placeholder, window, cx);

        if !provider_uses_api_key(provider) {
            return Self {
                provider,
                focus_handle: cx.focus_handle(),
                placeholder,
                key_input,
                stored_key: StoredKey::NotUsed,
                notice: None,
                test_outcome: None,
                _read_task: Task::ready(()),
                keychain_task: None,
                test_task: None,
            };
        }

        let credentials = zode_credentials_provider::global(cx);
        let read = read_api_key(credentials, provider, cx);
        let read_task = cx.spawn(async move |this, cx| {
            let stored_key = match read.await {
                Ok(Some(_)) => StoredKey::Present,
                Ok(None) => StoredKey::Absent,
                Err(error) => {
                    log::warn!("could not read the translation API key: {error:#}");
                    StoredKey::Unreadable(format!("{error:#}").into())
                }
            };
            this.update(cx, |this, cx| {
                if this.stored_key == StoredKey::Checking {
                    this.stored_key = stored_key;
                    cx.notify();
                }
            })
            .ok();
        });

        Self {
            provider,
            focus_handle: cx.focus_handle(),
            placeholder,
            key_input,
            stored_key: StoredKey::Checking,
            notice: None,
            test_outcome: None,
            _read_task: read_task,
            keychain_task: None,
            test_task: None,
        }
    }

    fn is_writing(&self) -> bool {
        self.keychain_task.is_some()
    }

    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_writing() || !self.shows_key_controls() {
            return;
        }
        let credentials = zode_credentials_provider::global(cx);
        let api_key = self.key_input.read(cx).text(cx);
        let write = write_api_key(credentials, self.provider, api_key, cx);
        // The field is the only copy outside the keychain. Clearing it would
        // leave the key in the field's undo history, so a new, empty field
        // takes its place; a failed write then asks for the key again rather
        // than keeping it.
        let had_focus = self.key_input.focus_handle(cx).is_focused(window);
        self.key_input = new_key_input(self.placeholder, window, cx);
        if had_focus {
            window.focus(&self.key_input.focus_handle(cx), cx);
        }
        self.notice = None;
        self.keychain_task = Some(cx.spawn(async move |this, cx| {
            let result = write.await;
            this.update(cx, |this, cx| {
                this.keychain_task = None;
                match result {
                    Ok(()) => {
                        this.stored_key = StoredKey::Present;
                        this.notice = Some(Notice::Saved);
                    }
                    Err(error) => {
                        this.notice = Some(Notice::Failed(format!("{error:#}").into()));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn remove(&mut self, cx: &mut Context<Self>) {
        if self.is_writing() || !self.shows_key_controls() {
            return;
        }
        let credentials = zode_credentials_provider::global(cx);
        let delete = delete_api_key(credentials, self.provider, cx);
        self.notice = None;
        self.keychain_task = Some(cx.spawn(async move |this, cx| {
            let result = delete.await;
            this.update(cx, |this, cx| {
                this.keychain_task = None;
                match result {
                    Ok(()) => {
                        this.stored_key = StoredKey::Absent;
                        this.notice = Some(Notice::Removed);
                    }
                    Err(error) => {
                        this.notice = Some(Notice::Failed(
                            format!("Could not remove the key: {error:#}").into(),
                        ));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Translates a fixed word with the saved settings and key, the same path
    /// a shortcut takes, so a pass here means the shortcuts will work.
    pub(crate) fn run_test(&mut self, cx: &mut Context<Self>) {
        let translation = translate::translate(test_request(), cx);
        self.test_outcome = Some(TestOutcome::Running);
        self.test_task = Some(cx.spawn(async move |this, cx| {
            let outcome = match translation.await {
                Ok(text) => TestOutcome::Translated(text.into()),
                Err(error) => TestOutcome::Failed(error.to_string().into()),
            };
            this.update(cx, |this, cx| {
                this.test_outcome = Some(outcome);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn shows_key_controls(&self) -> bool {
        self.stored_key != StoredKey::NotUsed
    }

    pub(crate) fn test_button_label(&self) -> &'static str {
        if self.shows_key_controls() {
            TEST_BUTTON_LABEL
        } else {
            TEST_WITHOUT_KEY_LABEL
        }
    }

    #[cfg(test)]
    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn test_outcome(&self) -> Option<&TestOutcome> {
        self.test_outcome.as_ref()
    }

    pub(crate) fn status_line(&self) -> (SharedString, Color) {
        match &self.notice {
            Some(Notice::Saved) => ("Key saved in the system keychain.".into(), Color::Success),
            Some(Notice::Removed) => ("Key removed from the system keychain.".into(), Color::Muted),
            Some(Notice::Failed(message)) => (message.clone(), Color::Error),
            None if self.is_writing() => ("Updating the system keychain…".into(), Color::Muted),
            None => match &self.stored_key {
                StoredKey::Checking => ("Checking the system keychain…".into(), Color::Muted),
                StoredKey::Unreadable(message) => (
                    format!("Could not read the system keychain: {message}").into(),
                    Color::Error,
                ),
                StoredKey::Present => (
                    "A key is saved in the system keychain. Paste a new one to replace it.".into(),
                    Color::Muted,
                ),
                StoredKey::Absent if provider_requires_api_key(self.provider) => {
                    ("No key saved yet.".into(), Color::Warning)
                }
                StoredKey::Absent => (
                    "No key saved. Local servers such as Ollama or LM Studio need none.".into(),
                    Color::Muted,
                ),
                StoredKey::NotUsed => (
                    format!(
                        "{} needs no API key. Use Test to check that it answers.",
                        provider_display_name(self.provider)
                    )
                    .into(),
                    Color::Muted,
                ),
            },
        }
    }

    fn render_test_outcome(&self) -> Option<AnyElement> {
        let outcome = self.test_outcome.as_ref()?;
        let (label, color) = match outcome {
            TestOutcome::Running => (
                SharedString::from(format!("Translating \"{TEST_TEXT}\"…")),
                Color::Muted,
            ),
            TestOutcome::Translated(text) => (
                SharedString::from(format!("\"{TEST_TEXT}\" → \"{text}\"")),
                Color::Success,
            ),
            TestOutcome::Failed(message) => (message.clone(), Color::Error),
        };
        Some(
            Label::new(label)
                .size(LabelSize::Small)
                .color(color)
                .into_any_element(),
        )
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl ModalView for ApiKeyModal {
    fn on_before_dismiss(&mut self, _: &mut Window, _: &mut Context<Self>) -> DismissDecision {
        DismissDecision::Dismiss(!self.is_writing())
    }
}

impl EventEmitter<DismissEvent> for ApiKeyModal {}

impl Focusable for ApiKeyModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.shows_key_controls() {
            self.key_input.focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl Render for ApiKeyModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let provider_name = provider_display_name(self.provider);
        let (status, status_color) = self.status_line();
        let writing = self.is_writing();
        let can_remove = self.stored_key == StoredKey::Present && !writing;
        let testing = self.test_outcome == Some(TestOutcome::Running);
        let shows_key_controls = self.shows_key_controls();
        let (headline, description, test_tooltip) = if shows_key_controls {
            (
                format!("{provider_name} API Key"),
                "Stored in the system keychain, never in settings.json. \
                 Change the provider in Settings → Translate.",
                "Translates \"Hello\" with the key already saved in the keychain. \
                 Save a new key first to test it.",
            )
        } else {
            (
                provider_name.to_string(),
                "No API key is needed for this provider. \
                 Change the provider in Settings → Translate.",
                "Translates \"Hello\" with this provider.",
            )
        };

        v_flex()
            .key_context("TranslateApiKeyModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(32.))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| this.cancel(cx)))
            .child(
                Modal::new("translate-api-key", None)
                    .header(
                        ModalHeader::new()
                            .child(Headline::new(headline).size(HeadlineSize::Small))
                            .description(description),
                    )
                    .section(
                        Section::new()
                            .when(shows_key_controls, |section| {
                                section.child(self.key_input.clone())
                            })
                            .child(
                                Label::new(status)
                                    .size(LabelSize::Small)
                                    .color(status_color),
                            )
                            .children(self.render_test_outcome()),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                Button::new("translate-api-key-test", self.test_button_label())
                                    .disabled(testing)
                                    .tooltip(Tooltip::text(test_tooltip))
                                    .on_click(cx.listener(|this, _, _, cx| this.run_test(cx))),
                            )
                            .when(shows_key_controls, |footer| {
                                footer.end_slot(
                                    h_flex()
                                        .gap_1()
                                        .child(
                                            Button::new("translate-api-key-remove", "Remove")
                                                .disabled(!can_remove)
                                                .on_click(
                                                    cx.listener(|this, _, _, cx| this.remove(cx)),
                                                ),
                                        )
                                        .child(
                                            Button::new("translate-api-key-save", "Save")
                                                .style(ButtonStyle::Filled)
                                                .disabled(writing)
                                                .key_binding(
                                                    KeyBinding::for_action_in(
                                                        &menu::Confirm,
                                                        &self.key_input.focus_handle(cx),
                                                        cx,
                                                    )
                                                    .size(rems_from_px(12.)),
                                                )
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.save(window, cx)
                                                })),
                                        ),
                                )
                            }),
                    ),
            )
    }
}

fn new_key_input(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<ApiKeyModal>,
) -> Entity<InputField> {
    cx.new(|cx| InputField::new(window, cx, placeholder).masked(true))
}

fn test_request() -> TranslateRequest {
    TranslateRequest {
        text: TEST_TEXT.to_string(),
        from: TEST_FROM.to_string(),
        to: TEST_TO.to_string(),
    }
}
