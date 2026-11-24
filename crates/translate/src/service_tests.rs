use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use credentials_provider::CredentialsProvider;
use futures::future::BoxFuture;
use gpui::{AsyncApp, TestAppContext, UpdateGlobal as _};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Request, Response};
use settings::{Settings as _, SettingsStore};

use crate::{
    ProviderKind, TranslateError, TranslateRequest, TranslateService, TranslateSettings,
    TranslationProvider, credentials_url, delete_api_key, read_api_key, translate, write_api_key,
};

#[derive(Default)]
struct MemoryKeychain {
    entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
    /// A keychain that is locked and never answers a read.
    hangs_on_read: AtomicBool,
}

type KeychainFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + 'a>>;

impl CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> KeychainFuture<'a, Option<(String, Vec<u8>)>> {
        Box::pin(async move {
            if self.hangs_on_read.load(Ordering::SeqCst) {
                futures::future::pending::<()>().await;
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

struct Harness {
    keychain: Arc<MemoryKeychain>,
    requests: Arc<Mutex<Vec<Request<AsyncBody>>>>,
}

impl Harness {
    fn request_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn header_of_first(&self, name: &str) -> Option<String> {
        let requests = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
        let value = requests.first()?.headers().get(name)?;
        value.to_str().ok().map(str::to_string)
    }

    fn uri_of_first(&self) -> Option<String> {
        let requests = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
        requests.first().map(|request| request.uri().to_string())
    }
}

/// Settings from `default.json`, an empty keychain, and a service that
/// answers every request with `body` unless `hang` keeps it waiting forever.
fn set_up(cx: &mut TestAppContext, body: &'static str, hang: bool) -> Harness {
    let keychain = Arc::new(MemoryKeychain::default());
    let requests: Arc<Mutex<Vec<Request<AsyncBody>>>> = Arc::default();
    let client = FakeHttpClient::create({
        let requests = requests.clone();
        move |request| {
            requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request);
            async move {
                if hang {
                    futures::future::pending::<()>().await;
                }
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(body))?)
            }
        }
    });
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        cx.set_http_client(client);
        cx.set_global(zode_credentials_provider::ZodeCredentialsProvider(
            keychain.clone(),
        ));
        crate::init(cx);
    });
    Harness { keychain, requests }
}

fn store_key(harness: &Harness, kind: ProviderKind, key: &str) {
    harness
        .keychain
        .entries
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            credentials_url(kind)
                .expect("the provider keeps a key")
                .to_string(),
            ("api-key".to_string(), key.as_bytes().to_vec()),
        );
}

fn update_settings(
    cx: &mut TestAppContext,
    update: impl FnOnce(&mut settings::TranslateSettingsContent),
) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                update(settings.translate.get_or_insert_default());
            });
        });
    });
}

fn request(text: &str) -> TranslateRequest {
    TranslateRequest {
        text: text.into(),
        from: "vi".into(),
        to: "en".into(),
    }
}

async fn translate_now(
    cx: &mut TestAppContext,
    request: TranslateRequest,
) -> Result<String, TranslateError> {
    let task = cx.update(|cx| translate(request, cx));
    cx.run_until_parked();
    task.await
}

const DEEPL_OK: &str = r#"{"translations":[{"text":"Hello"}]}"#;

#[gpui::test]
async fn defaults_come_from_default_json(cx: &mut TestAppContext) {
    set_up(cx, DEEPL_OK, false);
    let settings = cx.update(|cx| TranslateSettings::get_global(cx).clone());
    assert_eq!(settings.provider, ProviderKind::Deepl);
    assert_eq!(settings.max_characters, 2000);
    assert_eq!(settings.timeout, Duration::from_secs(20));
    assert_eq!(
        settings.openai_compatible_base_url,
        "http://localhost:11434/v1"
    );
    assert_eq!(settings.openai_compatible_model, "");
}

#[gpui::test]
async fn timeout_and_limit_are_kept_within_sane_bounds(cx: &mut TestAppContext) {
    set_up(cx, DEEPL_OK, false);
    update_settings(cx, |settings| {
        settings.timeout_seconds = Some(0);
        settings.max_characters = Some(0);
    });
    let settings = cx.update(|cx| TranslateSettings::get_global(cx).clone());
    assert_eq!(settings.timeout, Duration::from_secs(1));
    assert_eq!(settings.max_characters, 1);

    update_settings(cx, |settings| {
        settings.timeout_seconds = Some(u64::MAX);
        settings.max_characters = Some(usize::MAX);
    });
    let settings = cx.update(|cx| TranslateSettings::get_global(cx).clone());
    assert_eq!(settings.timeout, Duration::from_secs(120));
    assert_eq!(settings.max_characters, 30_000);
}

#[gpui::test]
async fn the_key_comes_from_the_keychain_entry_of_the_chosen_provider(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    store_key(&harness, ProviderKind::Deepl, "deepl-key:fx");
    store_key(&harness, ProviderKind::Google, "google-key");

    let result = translate_now(cx, request("Xin chào")).await;
    assert_eq!(result, Ok("Hello".to_string()));
    assert_eq!(
        harness.header_of_first("authorization").as_deref(),
        Some("DeepL-Auth-Key deepl-key:fx")
    );
}

#[gpui::test]
async fn the_provider_setting_switches_the_service(cx: &mut TestAppContext) {
    let harness = set_up(
        cx,
        r#"{"data":{"translations":[{"translatedText":"Hello"}]}}"#,
        false,
    );
    store_key(&harness, ProviderKind::Google, "google-key");
    update_settings(cx, |settings| {
        settings.provider = Some(ProviderKind::Google)
    });

    let result = translate_now(cx, request("Xin chào")).await;
    assert_eq!(result, Ok("Hello".to_string()));
    assert_eq!(
        harness.uri_of_first().as_deref(),
        Some("https://translation.googleapis.com/language/translate/v2")
    );
}

#[gpui::test]
async fn a_missing_key_is_reported_before_any_request(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    let result = translate_now(cx, request("Xin chào")).await;
    assert_eq!(
        result,
        Err(TranslateError::MissingKey { provider: "DeepL" })
    );
    assert_eq!(harness.request_count(), 0);
    let message = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(message.contains("Settings"), "{message}");
}

#[gpui::test]
async fn text_over_the_limit_is_refused_without_a_request(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    store_key(&harness, ProviderKind::Deepl, "key");

    // Characters, not bytes: 2000 Vietnamese letters are far more bytes.
    let at_limit = "ệ".repeat(2000);
    assert_eq!(
        translate_now(cx, request(&at_limit)).await,
        Ok("Hello".to_string())
    );
    assert_eq!(harness.request_count(), 1);

    let over_limit = "ệ".repeat(2001);
    assert_eq!(
        translate_now(cx, request(&over_limit)).await,
        Err(TranslateError::TooLong {
            length: 2001,
            limit: 2000
        })
    );
    assert_eq!(harness.request_count(), 1);
}

#[gpui::test]
async fn blank_text_and_bad_codes_are_refused_without_a_request(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    store_key(&harness, ProviderKind::Deepl, "key");

    assert_eq!(
        translate_now(cx, request(" \n\t")).await,
        Err(TranslateError::EmptyText)
    );
    let bad_code = TranslateRequest {
        text: "Xin chào".into(),
        from: "vietnamese".into(),
        to: "en".into(),
    };
    assert_eq!(
        translate_now(cx, bad_code).await,
        Err(TranslateError::InvalidLanguage {
            code: "vietnamese".into()
        })
    );
    assert_eq!(harness.request_count(), 0);
}

#[gpui::test]
async fn a_service_that_never_answers_times_out(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, true);
    store_key(&harness, ProviderKind::Deepl, "key");
    update_settings(cx, |settings| settings.timeout_seconds = Some(3));

    let task = cx.update(|cx| translate(request("Xin chào"), cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(
        task.await,
        Err(TranslateError::Timeout {
            provider: "DeepL",
            seconds: 3
        })
    );
}

#[gpui::test]
async fn an_openai_compatible_server_on_this_machine_needs_no_key(cx: &mut TestAppContext) {
    let harness = set_up(
        cx,
        r#"{"choices":[{"message":{"content":"Hello"}}]}"#,
        false,
    );
    update_settings(cx, |settings| {
        settings.provider = Some(ProviderKind::OpenaiCompatible);
        settings.openai_compatible.get_or_insert_default().model = Some("qwen2.5".into());
    });
    assert_eq!(
        translate_now(cx, request("Xin chào")).await,
        Ok("Hello".to_string())
    );
    assert_eq!(
        harness.uri_of_first().as_deref(),
        Some("http://localhost:11434/v1/chat/completions")
    );
    assert_eq!(harness.header_of_first("authorization"), None);
}

#[gpui::test]
async fn the_free_google_endpoint_needs_no_key_and_never_touches_the_keychain(
    cx: &mut TestAppContext,
) {
    let harness = set_up(
        cx,
        r#"[[["Hello","Xin chào",null,null,10]],null,"vi"]"#,
        false,
    );
    store_key(&harness, ProviderKind::Google, "google-key");
    update_settings(cx, |settings| {
        settings.provider = Some(ProviderKind::GoogleFree)
    });

    assert_eq!(
        translate_now(cx, request("Xin chào")).await,
        Ok("Hello".to_string())
    );
    assert_eq!(
        harness.uri_of_first().as_deref(),
        Some("https://translate.googleapis.com/translate_a/single?client=gtx&sl=vi&tl=en&dt=t")
    );
    assert_eq!(harness.header_of_first("x-goog-api-key"), None);
    assert_eq!(harness.header_of_first("authorization"), None);
    assert_eq!(credentials_url(ProviderKind::GoogleFree), None);
    assert!(!crate::provider_requires_api_key(ProviderKind::GoogleFree));
    assert_eq!(
        crate::provider_display_name(ProviderKind::GoogleFree),
        "Google Translate (free, no key)"
    );
}

#[gpui::test]
async fn no_key_is_stored_for_the_free_google_endpoint(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    let keychain: Arc<dyn CredentialsProvider> = harness.keychain.clone();

    let write =
        cx.update(|cx| write_api_key(keychain.clone(), ProviderKind::GoogleFree, "key".into(), cx));
    let message = write
        .await
        .expect_err("there is nowhere to keep it")
        .to_string();
    assert!(message.contains("needs no API key"), "{message}");
    let read = cx.update(|cx| read_api_key(keychain.clone(), ProviderKind::GoogleFree, cx));
    assert_eq!(read.await.ok().flatten(), None);
    let delete = cx.update(|cx| delete_api_key(keychain.clone(), ProviderKind::GoogleFree, cx));
    delete.await.expect("nothing to delete is not an error");
    assert!(
        harness
            .keychain
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
}

struct EchoProvider;

impl TranslationProvider for EchoProvider {
    fn name(&self) -> &'static str {
        "Echo"
    }

    fn translate(
        &self,
        request: &TranslateRequest,
        _http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>> {
        let echoed = format!("{}→{}: {}", request.from, request.to, request.text);
        Box::pin(async move { Ok(echoed) })
    }
}

#[gpui::test]
async fn a_provider_set_on_the_service_replaces_the_configured_one(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    cx.update(|cx| cx.set_global(TranslateService::with_provider(Arc::new(EchoProvider))));
    let result = translate_now(
        cx,
        TranslateRequest {
            text: "Hi".into(),
            from: "EN".into(),
            to: "pt_br".into(),
        },
    )
    .await;
    assert_eq!(result, Ok("en→pt-BR: Hi".to_string()));
    assert_eq!(harness.request_count(), 0);
}

#[gpui::test]
async fn keys_are_written_read_and_deleted_per_provider(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    let keychain: Arc<dyn CredentialsProvider> = harness.keychain.clone();

    let urls = [
        credentials_url(ProviderKind::Deepl),
        credentials_url(ProviderKind::Google),
        credentials_url(ProviderKind::OpenaiCompatible),
    ];
    assert_eq!(urls[0], Some("zode-translate://deepl"));
    assert!(urls.iter().all(Option::is_some));
    assert!(urls[0] != urls[1] && urls[1] != urls[2] && urls[0] != urls[2]);

    let write = cx.update(|cx| {
        write_api_key(
            keychain.clone(),
            ProviderKind::Google,
            " key-1 \n".into(),
            cx,
        )
    });
    write.await.expect("the key is stored");
    let read = cx.update(|cx| read_api_key(keychain.clone(), ProviderKind::Google, cx));
    assert_eq!(read.await.ok().flatten().as_deref(), Some("key-1"));
    let other = cx.update(|cx| read_api_key(keychain.clone(), ProviderKind::Deepl, cx));
    assert_eq!(other.await.ok().flatten(), None);

    let blank =
        cx.update(|cx| write_api_key(keychain.clone(), ProviderKind::Deepl, "  ".into(), cx));
    assert!(blank.await.is_err(), "a blank key is refused");

    let delete = cx.update(|cx| delete_api_key(keychain.clone(), ProviderKind::Google, cx));
    delete.await.expect("the key is deleted");
    let read = cx.update(|cx| read_api_key(keychain.clone(), ProviderKind::Google, cx));
    assert_eq!(read.await.ok().flatten(), None);
}

fn use_openai_compatible_at(cx: &mut TestAppContext, base_url: &str) {
    update_settings(cx, |settings| {
        settings.provider = Some(ProviderKind::OpenaiCompatible);
        let openai_compatible = settings.openai_compatible.get_or_insert_default();
        openai_compatible.model = Some("gpt".into());
        openai_compatible.base_url = Some(base_url.into());
    });
}

const OPENAI_ANSWER: &str = r#"{"choices":[{"message":{"content":"Hello"}}]}"#;

#[gpui::test]
async fn a_saved_openai_compatible_key_goes_only_to_the_server_it_was_saved_for(
    cx: &mut TestAppContext,
) {
    let harness = set_up(cx, OPENAI_ANSWER, false);
    use_openai_compatible_at(cx, "https://api.openai.com/v1");
    cx.update(|cx| {
        write_api_key(
            harness.keychain.clone(),
            ProviderKind::OpenaiCompatible,
            "sk-secret".into(),
            cx,
        )
    })
    .await
    .expect("the key is saved");

    assert_eq!(
        translate_now(cx, request("Xin chào")).await,
        Ok("Hello".to_string())
    );
    assert_eq!(
        harness.header_of_first("authorization").as_deref(),
        Some("Bearer sk-secret")
    );

    // Another path on the same server is the same server.
    use_openai_compatible_at(cx, "https://api.openai.com/other/v2");
    assert_eq!(
        translate_now(cx, request("Xin chào")).await,
        Ok("Hello".to_string())
    );
    assert_eq!(harness.request_count(), 2);

    use_openai_compatible_at(cx, "https://proxy.example.net/v1");
    let error = translate_now(cx, request("Xin chào"))
        .await
        .expect_err("the key stays with its server");
    assert!(
        matches!(error, TranslateError::KeyForOtherServer { .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("https://api.openai.com"), "{message}");
    assert!(message.contains("https://proxy.example.net"), "{message}");
    assert!(!message.contains("sk-secret"), "{message}");
    assert_eq!(harness.request_count(), 2, "nothing was sent");
}

#[gpui::test]
async fn an_openai_compatible_key_without_a_recorded_server_is_not_sent_anywhere(
    cx: &mut TestAppContext,
) {
    let harness = set_up(cx, OPENAI_ANSWER, false);
    use_openai_compatible_at(cx, "https://api.openai.com/v1");
    // Saved before keys were tied to a server: the entry names none.
    store_key(&harness, ProviderKind::OpenaiCompatible, "sk-old");

    let error = translate_now(cx, request("Xin chào"))
        .await
        .expect_err("an unbound key is not used");
    assert!(
        matches!(error, TranslateError::KeyForOtherServer { .. }),
        "{error:?}"
    );
    assert_eq!(harness.request_count(), 0);
}

#[gpui::test]
async fn a_locked_keychain_counts_against_the_timeout(cx: &mut TestAppContext) {
    let harness = set_up(cx, DEEPL_OK, false);
    store_key(&harness, ProviderKind::Deepl, "key");
    harness.keychain.hangs_on_read.store(true, Ordering::SeqCst);
    update_settings(cx, |settings| settings.timeout_seconds = Some(3));

    let task = cx.update(|cx| translate(request("Xin chào"), cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(
        task.await,
        Err(TranslateError::Timeout {
            provider: "DeepL",
            seconds: 3
        })
    );
    assert_eq!(harness.request_count(), 0);
}
