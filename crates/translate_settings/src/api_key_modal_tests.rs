use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use credentials_provider::CredentialsProvider;
use futures::future::BoxFuture;
use gpui::{AsyncApp, Entity, TestAppContext, UpdateGlobal as _, VisualTestContext};
use http_client::HttpClient;
use settings::SettingsStore;
use translate::{
    ProviderKind, TranslateError, TranslateRequest, TranslateService, TranslationProvider,
    credentials_url,
};
use workspace::{DismissDecision, ModalView as _};

use crate::api_key_modal::{ApiKeyModal, Notice, TEST_BUTTON_LABEL, TestOutcome};

fn deepl_entry() -> &'static str {
    credentials_url(ProviderKind::Deepl).expect("DeepL keeps a key")
}

fn google_entry() -> &'static str {
    credentials_url(ProviderKind::Google).expect("Google keeps a key")
}

#[derive(Default)]
struct MemoryKeychain {
    entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
    read_error: Option<&'static str>,
}

type KeychainFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + 'a>>;

impl MemoryKeychain {
    fn password(&self, url: &str) -> Option<String> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(url)
            .map(|(_, password)| String::from_utf8_lossy(password).into_owned())
    }

    fn store(&self, url: &str, password: &str) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                url.to_string(),
                ("api-key".to_string(), password.as_bytes().to_vec()),
            );
    }
}

impl CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> KeychainFuture<'a, Option<(String, Vec<u8>)>> {
        Box::pin(async move {
            if let Some(error) = self.read_error {
                anyhow::bail!(error);
            }
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(url)
                .cloned())
        })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a AsyncApp,
    ) -> KeychainFuture<'a, ()> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(url.to_string(), (username.to_string(), password.to_vec()));
            Ok(())
        })
    }

    fn delete_credentials<'a>(&'a self, url: &'a str, _cx: &'a AsyncApp) -> KeychainFuture<'a, ()> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(url);
            Ok(())
        })
    }
}

/// Answers every request with a fixed result and records what it was asked.
struct FakeTranslator {
    answer: Result<String, TranslateError>,
    requests: Mutex<Vec<TranslateRequest>>,
}

impl TranslationProvider for FakeTranslator {
    fn name(&self) -> &'static str {
        "Fake"
    }

    fn translate(
        &self,
        request: &TranslateRequest,
        _http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request.clone());
        let answer = self.answer.clone();
        Box::pin(async move { answer })
    }
}

fn set_up(
    cx: &mut TestAppContext,
) -> (
    Arc<MemoryKeychain>,
    Entity<ApiKeyModal>,
    &mut VisualTestContext,
) {
    set_up_with(MemoryKeychain::default(), cx)
}

fn set_up_with(
    keychain: MemoryKeychain,
    cx: &mut TestAppContext,
) -> (
    Arc<MemoryKeychain>,
    Entity<ApiKeyModal>,
    &mut VisualTestContext,
) {
    set_up_for(None, keychain, cx)
}

fn set_up_for(
    provider: Option<ProviderKind>,
    keychain: MemoryKeychain,
    cx: &mut TestAppContext,
) -> (
    Arc<MemoryKeychain>,
    Entity<ApiKeyModal>,
    &mut VisualTestContext,
) {
    let keychain = Arc::new(keychain);
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        editor::init(cx);
        translate::init(cx);
        cx.set_global(zode_credentials_provider::ZodeCredentialsProvider(
            keychain.clone(),
        ));
        if let Some(provider) = provider {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.translate.get_or_insert_default().provider = Some(provider);
                });
            });
        }
    });
    let (modal, cx) = cx.add_window_view(ApiKeyModal::new);
    cx.run_until_parked();
    (keychain, modal, cx)
}

fn type_key(modal: &Entity<ApiKeyModal>, key: &str, cx: &mut VisualTestContext) {
    modal.update_in(cx, |modal, window, cx| {
        let editor = modal.key_input.read(cx).editor().clone();
        editor.set_text(key, window, cx);
    });
}

/// The editor behind the masked field as it is now; saving may replace it.
fn field_editor(modal: &Entity<ApiKeyModal>, cx: &mut VisualTestContext) -> Entity<editor::Editor> {
    modal.update(cx, |modal, cx| {
        modal
            .key_input
            .read(cx)
            .editor()
            .as_any()
            .downcast_ref::<Entity<editor::Editor>>()
            .cloned()
            .expect("the field is an editor")
    })
}

#[gpui::test]
async fn saving_a_key_writes_the_keychain_and_nothing_else(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    let key = "deepl-secret-key:fx";
    type_key(&modal, &format!("  {key}\n"), cx);

    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    cx.run_until_parked();

    assert_eq!(keychain.password(deepl_entry()).as_deref(), Some(key));
    modal.update(cx, |modal, cx| {
        assert_eq!(modal.notice(), Some(&Notice::Saved));
        assert_eq!(
            modal.key_input.read(cx).text(cx),
            "",
            "the field is cleared so the key is not kept on screen or in memory"
        );
    });
    let settings = cx.update(|_, cx| {
        let store = cx.global::<SettingsStore>();
        serde_json::to_string(store.merged_settings()).unwrap_or_default()
    });
    assert!(!settings.is_empty());
    assert!(
        !settings.contains(key),
        "the key must never reach the settings"
    );
}

#[gpui::test]
async fn saving_an_empty_key_reports_it_and_writes_nothing(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    type_key(&modal, "   ", cx);

    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    cx.run_until_parked();

    assert_eq!(keychain.password(deepl_entry()), None);
    modal.update(cx, |modal, _| {
        assert!(
            matches!(modal.notice(), Some(Notice::Failed(message)) if message.contains("empty")),
            "{:?}",
            modal.notice()
        );
    });
}

#[gpui::test]
async fn removing_deletes_the_key_of_the_current_provider(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    keychain.store(deepl_entry(), "old-key");
    keychain.store(google_entry(), "google-key");

    modal.update(cx, |modal, cx| modal.remove(cx));
    cx.run_until_parked();

    assert_eq!(keychain.password(deepl_entry()), None);
    assert_eq!(
        keychain.password(google_entry()).as_deref(),
        Some("google-key")
    );
    modal.update(cx, |modal, _| {
        assert_eq!(modal.notice(), Some(&Notice::Removed));
    });
}

fn use_translator(
    cx: &mut VisualTestContext,
    answer: Result<String, TranslateError>,
) -> Arc<FakeTranslator> {
    let translator = Arc::new(FakeTranslator {
        answer,
        requests: Mutex::default(),
    });
    cx.update(|_, cx| cx.set_global(TranslateService::with_provider(translator.clone())));
    translator
}

#[gpui::test]
async fn test_button_shows_the_translation(cx: &mut TestAppContext) {
    let (_, modal, cx) = set_up(cx);
    let translator = use_translator(cx, Ok("Xin chào".to_string()));

    modal.update(cx, |modal, cx| modal.run_test(cx));
    cx.run_until_parked();

    modal.update(cx, |modal, _| {
        assert_eq!(
            modal.test_outcome(),
            Some(&TestOutcome::Translated("Xin chào".into()))
        );
    });
    let requests = translator
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(
        requests,
        vec![TranslateRequest {
            text: "Hello".into(),
            from: "en".into(),
            to: "vi".into(),
        }]
    );
}

#[gpui::test]
async fn test_button_shows_a_readable_error(cx: &mut TestAppContext) {
    let (_, modal, cx) = set_up(cx);
    use_translator(
        cx,
        Err(TranslateError::InvalidKey {
            provider: "DeepL",
            status: 403,
        }),
    );

    modal.update(cx, |modal, cx| modal.run_test(cx));
    cx.run_until_parked();

    modal.update(cx, |modal, _| match modal.test_outcome() {
        Some(TestOutcome::Failed(message)) => {
            assert!(message.contains("rejected the API key"), "{message}")
        }
        other => panic!("expected a failed test, got {other:?}"),
    });
}

#[gpui::test]
async fn saving_leaves_no_key_in_the_field_undo_history(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    let key = "deepl-secret-key:fx";
    type_key(&modal, key, cx);
    let editor = field_editor(&modal, cx);
    editor.update(cx, |editor, cx| {
        editor
            .buffer()
            .update(cx, |buffer, cx| buffer.finalize_last_transaction(cx))
    });

    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    cx.run_until_parked();
    assert_eq!(keychain.password(deepl_entry()).as_deref(), Some(key));

    let editor = field_editor(&modal, cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.undo(&editor::actions::Undo, window, cx)
    });
    modal.update(cx, |modal, cx| {
        assert_eq!(
            modal.key_input.read(cx).text(cx),
            "",
            "undo in the field must not bring the saved key back"
        );
    });
}

#[gpui::test]
async fn remove_right_after_save_does_not_cancel_the_write(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    type_key(&modal, "new-key", cx);

    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    modal.update(cx, |modal, cx| modal.remove(cx));
    cx.run_until_parked();

    assert_eq!(
        keychain.password(deepl_entry()).as_deref(),
        Some("new-key"),
        "the save finishes; a Remove pressed while it runs is ignored"
    );
    modal.update(cx, |modal, _| {
        assert_eq!(modal.notice(), Some(&Notice::Saved));
    });
}

#[gpui::test]
async fn the_dialog_stays_open_until_a_save_finishes(cx: &mut TestAppContext) {
    let (keychain, modal, cx) = set_up(cx);
    type_key(&modal, "new-key", cx);

    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    let decision = modal.update_in(cx, |modal, window, cx| modal.on_before_dismiss(window, cx));
    assert!(
        matches!(decision, DismissDecision::Dismiss(false)),
        "Escape while saving would cancel the write"
    );

    cx.run_until_parked();
    assert_eq!(keychain.password(deepl_entry()).as_deref(), Some("new-key"));
    let decision = modal.update_in(cx, |modal, window, cx| modal.on_before_dismiss(window, cx));
    assert!(matches!(decision, DismissDecision::Dismiss(true)));
}

#[gpui::test]
async fn an_unreadable_keychain_is_reported_not_shown_as_missing(cx: &mut TestAppContext) {
    let keychain = MemoryKeychain {
        read_error: Some("User interaction is not allowed."),
        ..MemoryKeychain::default()
    };
    let (_, modal, cx) = set_up_with(keychain, cx);

    let (status, _) = modal.update(cx, |modal, _| modal.status_line());
    assert!(
        status.contains("User interaction is not allowed."),
        "{status}"
    );
    assert!(!status.contains("No key saved"), "{status}");
}

#[gpui::test]
async fn the_test_button_says_it_uses_the_saved_key(_cx: &mut TestAppContext) {
    assert!(
        TEST_BUTTON_LABEL.contains("Saved Key"),
        "a key typed but not saved is not what Test checks: {TEST_BUTTON_LABEL}"
    );
}

#[gpui::test]
async fn the_free_google_endpoint_says_no_key_is_needed(cx: &mut TestAppContext) {
    let keychain = MemoryKeychain {
        read_error: Some("the keychain must not be asked"),
        ..MemoryKeychain::default()
    };
    let (keychain, modal, cx) = set_up_for(Some(ProviderKind::GoogleFree), keychain, cx);

    let (status, _) = modal.update(cx, |modal, _| modal.status_line());
    assert!(status.contains("needs no API key"), "{status}");
    assert!(!status.contains("keychain"), "{status}");
    modal.update(cx, |modal, _| {
        assert!(
            !modal.shows_key_controls(),
            "no field, Save or Remove for a provider without a key"
        );
        assert!(!modal.test_button_label().contains("Saved Key"));
    });

    type_key(&modal, "pasted-anyway", cx);
    modal.update_in(cx, |modal, window, cx| modal.save(window, cx));
    modal.update(cx, |modal, cx| modal.remove(cx));
    cx.run_until_parked();
    assert!(
        keychain
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
    modal.update(cx, |modal, _| assert_eq!(modal.notice(), None));
}

#[gpui::test]
async fn the_free_google_endpoint_can_still_be_tested(cx: &mut TestAppContext) {
    let (_, modal, cx) = set_up_for(
        Some(ProviderKind::GoogleFree),
        MemoryKeychain::default(),
        cx,
    );
    use_translator(cx, Ok("Xin chào".to_string()));

    modal.update(cx, |modal, cx| modal.run_test(cx));
    cx.run_until_parked();

    modal.update(cx, |modal, _| {
        assert_eq!(
            modal.test_outcome(),
            Some(&TestOutcome::Translated("Xin chào".into()))
        );
    });
}

#[gpui::test]
async fn providers_with_a_key_keep_the_key_controls(cx: &mut TestAppContext) {
    let (_, modal, cx) = set_up(cx);
    modal.update(cx, |modal, _| {
        assert!(modal.shows_key_controls());
        assert_eq!(modal.test_button_label(), TEST_BUTTON_LABEL);
    });
}
