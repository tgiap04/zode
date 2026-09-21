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

    /// A machine that signed in on an earlier run.
    fn holding(user_id: &str, payload: serde_json::Value) -> Arc<dyn CredentialsProvider> {
        Arc::new(Self {
            stored: Mutex::new(Some((
                user_id.to_string(),
                serde_json::to_vec(&payload).expect("the fixture payload"),
            ))),
        })
    }
}

/// The keychain payload as it is actually written, spelled out rather than
/// built from the types.
///
/// Written by hand on purpose: this is an on-disk format that older builds
/// also read, so a change to it should break a test rather than a user's saved
/// session.
fn saved_session(with_user: bool) -> serde_json::Value {
    let expires_at = std::time::SystemTime::now() + Duration::from_secs(3_600);
    let since_epoch = expires_at
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("a clock after 1970");

    let mut payload = serde_json::json!({
        "access_token": "stored-access",
        "refresh_token": "stored-refresh",
        "expires_at": {
            "secs_since_epoch": since_epoch.as_secs(),
            "nanos_since_epoch": since_epoch.subsec_nanos(),
        },
    });

    if with_user {
        payload["user"] = serde_json::json!({
            "id": "1",
            "email": "ada@example.com",
            "name": null,
            "avatar_url": null,
        });
    }
    payload
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
            let mut account =
                Account::new(http_client, credentials, "https://zodekit.site/api".into());
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
            let mut account =
                Account::new(http_client, credentials, "https://zodekit.site/api".into());
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
                    "verification_uri": "https://zodekit.site/activate",
                    "verification_uri_complete": "https://zodekit.site/activate?code=A1B2-C3D4",
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
                "https://zodekit.site/api".into(),
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
                "https://zodekit.site/api".into(),
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

/// Invariant 5 — one refresh at a time, however many callers want a token.
///
/// The refresh token rotates on use, and the server reads a second use of a
/// spent one as theft and revokes the whole family. `zode_sync` and
/// `zode_env_sync` reach for a credential independently and know nothing about
/// each other, so two refreshes at once do not waste a request — they sign the
/// user out. Counted rather than reasoned about, because the counting is the
/// only thing that survives a refactor.
#[gpui::test]
async fn two_callers_wanting_a_fresh_token_cause_one_refresh(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::empty();

    let account = cx.update(|cx| {
        cx.new(|_| {
            let mut account = Account::for_test_with(
                AccountStatus::SignedIn(zode_account::AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }),
                http_client,
                credentials,
            );
            account.set_expired_tokens_for_test();
            account
        })
    });

    // Both asked before either could run: the shape two independent crates
    // produce when a token dies between them.
    let (first, second) = account.update(cx, |account: &mut Account, cx| {
        (account.api_credential(cx), account.api_credential(cx))
    });
    let (first, second) = futures::future::join(first, second).await;

    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "the second caller must join the refresh already running, not start another"
    );
    assert!(
        first.is_none() && second.is_none(),
        "the fake answers 500, so neither caller gets a credential"
    );
    account.read_with(cx, |account: &Account, _| {
        assert!(
            account.status().is_signed_in(),
            "a server error is not a dead credential — the user stays signed in"
        );
    });
}

/// Invariant 6 — a start with no network keeps the session it remembers.
///
/// The credential is good and the server merely unreachable. Appearing signed
/// out here is wrong twice over: the session still works, and nothing polls,
/// so there is no second chance to notice.
#[gpui::test]
async fn an_offline_start_still_names_the_account_it_saved(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::holding("1", saved_session(true));

    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account =
                Account::new(http_client, credentials, "https://zodekit.site/api".into());
            account.restore(cx).detach();
            account
        })
    });
    cx.run_until_parked();

    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "the token is still fresh, so only the identity call goes out"
    );
    account.read_with(cx, |account: &Account, _| {
        let user = account
            .status()
            .user()
            .expect("the remembered account must still be named");
        assert_eq!(user.email, "ada@example.com");
    });
}

/// The same entry written before it remembered anyone must still load.
///
/// An on-disk format older builds also wrote: a user who upgrades must not be
/// signed out by the upgrade itself.
#[gpui::test]
async fn an_entry_saved_before_the_user_was_remembered_still_loads(cx: &mut TestAppContext) {
    let (http_client, requests) = counting_client();
    let credentials = StubCredentials::holding("1", saved_session(false));

    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account =
                Account::new(http_client, credentials, "https://zodekit.site/api".into());
            account.restore(cx).detach();
            account
        })
    });
    cx.run_until_parked();

    // The request is the proof the entry parsed: a payload this build could
    // not read makes `storage::read` answer None, and `restore` then returns
    // before reaching the network at all. Asserting only the status would pass
    // in both cases and measure nothing.
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "the old entry must still be read and its token used"
    );
    // Nothing remembered, so nothing is claimed -- the behaviour this build
    // had before, which is the right one when there is no name to fall back on.
    account.read_with(cx, |account: &Account, _| {
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
}
