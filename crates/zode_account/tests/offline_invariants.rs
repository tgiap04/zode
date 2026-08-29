//! The four promises that make signing in optional rather than nominal.
//!
//! Each is asserted by observation — counting requests, advancing the clock —
//! rather than by reading the code, because the code is exactly what changes.
//! Documentation of an invariant does not hold it; this file does.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, TestAppContext};
use http_client::{FakeHttpClient, HttpClient, Response};
use zode_account::{Account, AccountStatus};

/// A keychain that is whatever the test says it is.
///
/// `None` stands for the machine that has never signed in — the case the
/// no-network promise is about.
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

/// An HTTP client that answers nothing and counts everything.
///
/// Answering 500 rather than a plausible body is deliberate: any test here
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

/// Invariant 1 — signed out means no network, at startup or ever.
#[gpui::test]
async fn a_machine_that_has_never_signed_in_issues_no_request(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::empty();

    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(http_client, credentials, "https://zode.dev/api".into());
            account.restore(cx).detach();
            account
        })
    });

    cx.run_until_parked();

    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "restoring an absent session must not reach the network"
    );
    account.read_with(cx, |account: &Account, _| {
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
}

/// Invariant 2 — nothing polls in the background.
///
/// An hour of scheduler time with nobody asking for anything must produce
/// nothing. This is what catches a well-meaning "refresh the session every N
/// minutes" being added later.
#[gpui::test]
async fn an_idle_signed_out_account_never_wakes_up(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::empty();

    let _account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(http_client, credentials, "https://zode.dev/api".into());
            account.restore(cx).detach();
            account
        })
    });

    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(3_600));
    cx.run_until_parked();

    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "an idle account must not poll anything"
    );
}

/// Invariant 2, the other half — signing out stops the polling for good.
#[gpui::test]
async fn cancelling_a_sign_in_stops_the_polling(cx: &mut TestAppContext) {
    // Always pending, so the flow would poll forever if nothing stopped it.
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let http_client = FakeHttpClient::create(move |request| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            let body = if request.uri().path().ends_with("/auth/device/code") {
                serde_json::json!({
                    "device_code": "dc",
                    "user_code": "A1B2-C3D4",
                    "verification_uri": "https://zode.dev/activate",
                    "verification_uri_complete": "https://zode.dev/activate?code=A1B2-C3D4",
                    "expires_in": 600,
                    "interval": 5
                })
                .to_string()
            } else {
                serde_json::json!({ "error": "authorization_pending" }).to_string()
            };
            let status = if request.uri().path().ends_with("/auth/device/code") {
                201
            } else {
                400
            };
            Ok(Response::builder()
                .status(status)
                .body(body.into())
                .unwrap())
        }
    }) as Arc<dyn HttpClient>;

    let account = cx.update(|cx| {
        cx.new(|_| {
            Account::new(
                http_client,
                StubCredentials::empty(),
                "https://zode.dev/api".into(),
            )
        })
    });

    account.update(cx, |account: &mut Account, cx| account.sign_in(cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(20));
    cx.run_until_parked();

    let while_waiting = count.load(Ordering::SeqCst);
    assert!(
        while_waiting > 1,
        "the flow should have polled at least once"
    );
    account.read_with(cx, |account: &Account, _| {
        assert!(matches!(
            account.status(),
            AccountStatus::WaitingForApproval { .. }
        ));
    });

    account.update(cx, |account: &mut Account, cx| account.cancel_sign_in(cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(600));
    cx.run_until_parked();

    assert_eq!(
        count.load(Ordering::SeqCst),
        while_waiting,
        "cancelling must drop the poll task, not merely hide it"
    );
    account.read_with(cx, |account: &Account, _| {
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
}

/// Invariant 3 — a hung account service never blocks anything else.
///
/// The account entity is left waiting on a request that will never answer;
/// unrelated work scheduled afterwards must still complete. If the sign-in
/// were ever made blocking, this is the test that stops compiling shortcuts.
#[gpui::test]
async fn a_hung_account_service_does_not_block_the_rest_of_the_app(cx: &mut TestAppContext) {
    let http_client = FakeHttpClient::create(move |_request| async move {
        // Never resolves in any bounded amount of scheduler time.
        futures::future::pending::<()>().await;
        unreachable!()
    }) as Arc<dyn HttpClient>;

    let account = cx.update(|cx| {
        cx.new(|_| {
            Account::new(
                http_client,
                StubCredentials::empty(),
                "https://zode.dev/api".into(),
            )
        })
    });
    account.update(cx, |account: &mut Account, cx| account.sign_in(cx));

    let unrelated_work_finished = Arc::new(AtomicUsize::new(0));
    let flag = unrelated_work_finished.clone();
    cx.background_executor
        .spawn(async move {
            flag.fetch_add(1, Ordering::SeqCst);
        })
        .detach();

    cx.run_until_parked();

    assert_eq!(
        unrelated_work_finished.load(Ordering::SeqCst),
        1,
        "unrelated work must complete while the account service hangs"
    );
    account.read_with(cx, |account: &Account, _| {
        // Still signed out — the hung request produced no state, and no panic.
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
}
