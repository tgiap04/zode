//! The promises that make syncing safe to turn on, asserted by observation.
//!
//! Companion to `zode_account/tests/offline_invariants.rs`, and written the
//! same way: count requests, look at files. Documentation of an invariant does
//! not hold it.

mod fake_store;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, TestAppContext};
use http_client::{FakeHttpClient, HttpClient, Response};
use zode_account::{Account, AccountStatus, AccountUser};
use zode_sync::{Kind, SyncSession, extensions};

/// A keychain that starts empty, like a machine that has never synced.
struct StubCredentials {
    stored: Mutex<Option<(String, Vec<u8>)>>,
}

impl StubCredentials {
    fn empty() -> Arc<dyn CredentialsProvider> {
        Arc::new(Self {
            stored: Mutex::new(None),
        })
    }
}

impl CredentialsProvider for StubCredentials {
    fn read_credentials<'a>(
        &'a self,
        _url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<(String, Vec<u8>)>>> + 'a>> {
        Box::pin(async move { Ok(self.stored.lock().unwrap().clone()) })
    }

    fn write_credentials<'a>(
        &'a self,
        _url: &'a str,
        username: &'a str,
        password: &'a [u8],
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async move {
            *self.stored.lock().unwrap() = Some((username.to_string(), password.to_vec()));
            Ok(())
        })
    }

    fn delete_credentials<'a>(
        &'a self,
        _url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
        Box::pin(async move {
            *self.stored.lock().unwrap() = None;
            Ok(())
        })
    }
}

/// Answers 500 to everything and counts every attempt.
///
/// Answering 500 rather than something plausible is deliberate: a test here
/// that starts passing because a request *succeeded* has stopped measuring
/// what it claims to measure.
fn counting_client() -> (Arc<dyn HttpClient>, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let client = FakeHttpClient::create(move |_request| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Response::builder()
                .status(500)
                .body(Default::default())
                .unwrap())
        }
    });
    (client as Arc<dyn HttpClient>, count)
}

/// A signed-in account wired to the CALLER's client and keychain.
///
/// `Account::for_test` blocks the network outright, which would make "nothing
/// was sent" true by construction — an assertion that cannot fail is not an
/// assertion.
fn signed_in_account(
    http_client: Arc<dyn HttpClient>,
    credentials: Arc<dyn CredentialsProvider>,
    cx: &mut TestAppContext,
) -> gpui::Entity<Account> {
    cx.update(|cx| {
        cx.new(|_| {
            Account::for_test_with(
                AccountStatus::SignedIn(AccountUser {
                    id: "68b1f0c2a4d3e5f60718293a".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }),
                http_client,
                credentials,
            )
        })
    })
}

/// Invariant 2, extended to sync: being signed in is not consent to transfer
/// anything. Opening the sync window reads the keychain; it must not reach the
/// network, and neither must simply sitting there.
#[gpui::test]
async fn a_signed_in_session_transfers_nothing_until_asked(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::empty();
    let account = signed_in_account(http_client, credentials, cx);

    let session = cx.update(|cx| cx.new(|_| SyncSession::new(account)));

    // What opening the sync window does.
    let loading = session.update(cx, |session, cx| session.load_key(cx));
    loading.await;

    // And a long wait afterwards.
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3600));
    cx.run_until_parked();

    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "opening sync and then waiting must not send anything",
    );
    session.update(cx, |session, _| {
        assert!(
            !session.has_key(),
            "an empty keychain means no key, not an error"
        );
    });
}

/// Invariant 7: a pulled extension list is a list. There is no path from
/// receiving one to installing anything — a sync payload that installed code
/// on arrival would be a supply-chain hole with the user's own account as the
/// key.
#[gpui::test]
async fn a_pulled_extension_list_installs_nothing(cx: &mut TestAppContext) {
    let (http_client, _) = counting_client();
    let credentials = StubCredentials::empty();
    let account = signed_in_account(http_client, credentials, cx);
    let session = cx.update(|cx| cx.new(|_| SyncSession::new(account)));

    // The whole surface `zode_sync` exposes for extensions: render, compare,
    // and report. `install` does not appear in this crate at all.
    let installed = vec!["a/one".to_string()];
    let stored = extensions::parse(&extensions::render(vec![
        "a/one".to_string(),
        "b/two".to_string(),
    ]))
    .unwrap();
    let comparison = extensions::compare(&installed, &stored);
    assert_eq!(comparison.missing, vec!["b/two".to_string()]);

    // And applying a decision about extensions writes no file, unlike the
    // settings path — `Kind::Extensions` has no file to write.
    session.update(cx, |session, cx| {
        session.apply_pending(cx);
        assert!(session.missing_extensions().is_empty());
    });

    let _ = Kind::Extensions;
}

/// Proves the counter above is actually wired to the account.
///
/// Without this, `a_signed_in_session_transfers_nothing_until_asked` would
/// pass just as happily against a client nothing uses — which is precisely the
/// mistake it was written with the first time.
#[gpui::test]
async fn the_request_counter_is_wired_to_the_account(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::empty();
    let account = signed_in_account(http_client, credentials.clone(), cx);
    let session = cx.update(|cx| cx.new(|_| SyncSession::new(account.clone())));

    // Put a key in the keychain and a token on the account, so a push has
    // everything it needs and the only thing left to stop it would be a client
    // that goes nowhere.
    let dek = zode_sync::Dek::from_bytes([0x33; 32]);
    let async_cx = cx.update(|cx| cx.to_async());
    zode_sync::keystore::write(&credentials, "user", &dek, &async_cx)
        .await
        .unwrap();
    account.update(cx, |account, _| account.set_tokens_for_test());

    session.update(cx, |session, cx| session.load_key(cx)).await;
    session.update(cx, |session, cx| {
        assert!(
            session.has_key(),
            "the key must have been loaded from the keychain"
        );
        session.push_extensions(vec!["a/one".into()], cx);
    });
    cx.run_until_parked();

    assert!(
        requests.load(Ordering::SeqCst) > 0,
        "a push must reach the counted client, or the no-request test proves nothing",
    );
}
