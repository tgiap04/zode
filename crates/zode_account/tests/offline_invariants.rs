//! The four promises that make signing in optional rather than nominal.
//!
//! Each is asserted by observation — counting requests, advancing the clock —
//! rather than by reading the code, because the code is exactly what changes.
//! Documentation of an invariant does not hold it; this file does.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use credentials_provider::CredentialsProvider;
use futures::AsyncReadExt as _;
use futures::channel::oneshot;
use gpui::{AppContext as _, AsyncApp, Entity, TestAppContext};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Request, Response};
use zode_account::{Account, AccountStatus};

/// A keychain that is whatever the test says it is.
///
/// `None` stands for the machine that has never signed in — the case the
/// no-network promise is about.
struct StubCredentials {
    stored: Mutex<Option<(String, Vec<u8>)>>,
    /// How many more writes are refused; `usize::MAX` means every one.
    writes_to_refuse: AtomicUsize,
    deletes_fail: AtomicBool,
    writes_attempted: AtomicUsize,
    deletes_attempted: AtomicUsize,
    /// Holds the next write until the test lets it through, so a sign-out can
    /// happen while a save is in flight.
    write_gate: Mutex<Option<oneshot::Receiver<()>>>,
}

impl StubCredentials {
    fn with_entry(entry: Option<(String, Vec<u8>)>) -> Self {
        Self {
            stored: Mutex::new(entry),
            writes_to_refuse: AtomicUsize::new(0),
            deletes_fail: AtomicBool::new(false),
            writes_attempted: AtomicUsize::new(0),
            deletes_attempted: AtomicUsize::new(0),
            write_gate: Mutex::new(None),
        }
    }

    fn empty() -> Arc<dyn CredentialsProvider> {
        Arc::new(Self::with_entry(None))
    }

    /// A keychain that refuses to be written, as a locked one does.
    fn refuse_writes(&self, refuse: bool) {
        self.writes_to_refuse
            .store(if refuse { usize::MAX } else { 0 }, Ordering::SeqCst);
    }

    /// A keychain that recovers: it refuses this many writes, then accepts.
    fn refuse_next_writes(&self, count: usize) {
        self.writes_to_refuse.store(count, Ordering::SeqCst);
    }

    fn refuse_deletes(&self, refuse: bool) {
        self.deletes_fail.store(refuse, Ordering::SeqCst);
    }

    fn writes_attempted(&self) -> usize {
        self.writes_attempted.load(Ordering::SeqCst)
    }

    fn deletes_attempted(&self) -> usize {
        self.deletes_attempted.load(Ordering::SeqCst)
    }

    fn hold_next_write(&self) -> oneshot::Sender<()> {
        let (release, gate) = oneshot::channel();
        *self.write_gate.lock().unwrap() = Some(gate);
        release
    }

    /// A machine that signed in on an earlier run.
    ///
    /// Typed rather than boxed so a test can read back what a later run left
    /// behind; it still coerces to the trait object wherever one is wanted.
    fn holding(user_id: &str, payload: serde_json::Value) -> Arc<Self> {
        Arc::new(Self::with_entry(Some((
            user_id.to_string(),
            serde_json::to_vec(&payload).expect("the fixture payload"),
        ))))
    }

    /// The entry as the keychain holds it right now: its username and payload.
    fn held(&self) -> Option<(String, serde_json::Value)> {
        let (username, payload) = self.stored.lock().unwrap().clone()?;
        let payload = serde_json::from_slice(&payload).expect("the stored payload is JSON");
        Some((username, payload))
    }
}

/// The keychain payload as it is actually written, spelled out rather than
/// built from the types.
///
/// Written by hand on purpose: this is an on-disk format that older builds
/// also read, so a change to it should break a test rather than a user's saved
/// session.
fn saved_session(with_user: bool) -> serde_json::Value {
    session_expiring_at(
        with_user,
        std::time::SystemTime::now() + Duration::from_secs(3_600),
    )
}

/// The same entry once its access token has died, so the next start has to
/// refresh it before it can do anything.
fn expired_session(with_user: bool) -> serde_json::Value {
    session_expiring_at(
        with_user,
        std::time::SystemTime::now() - Duration::from_secs(3_600),
    )
}

fn session_expiring_at(with_user: bool, expires_at: std::time::SystemTime) -> serde_json::Value {
    session_holding_refresh_token("stored-refresh", with_user, expires_at)
}

fn session_holding_refresh_token(
    refresh_token: &str,
    with_user: bool,
    expires_at: std::time::SystemTime,
) -> serde_json::Value {
    let since_epoch = expires_at
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("a clock after 1970");

    let mut payload = serde_json::json!({
        "access_token": "stored-access",
        "refresh_token": refresh_token,
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
            self.writes_attempted.fetch_add(1, Ordering::SeqCst);
            let gate = self.write_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.await.ok();
            }
            let refusing = self.writes_to_refuse.load(Ordering::SeqCst);
            if refusing > 0 {
                if refusing != usize::MAX {
                    self.writes_to_refuse.fetch_sub(1, Ordering::SeqCst);
                }
                anyhow::bail!("the keychain refused the write");
            }
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
            self.deletes_attempted.fetch_add(1, Ordering::SeqCst);
            if self.deletes_fail.load(Ordering::SeqCst) {
                anyhow::bail!("the keychain refused the delete");
            }
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

/// An account service whose refresh token rotates on use, and which treats a
/// second use of a spent one as theft.
///
/// That rule is what makes an unsaved rotation fatal rather than wasteful, so
/// the fake enforces it instead of answering every refresh with a fresh pair.
/// Every pair it issues expires at once (`expires_in: 0`), which makes each
/// later start refresh again and so exercises the stored token every time.
struct RotatingServer {
    live_refresh_token: Mutex<String>,
    rotations: AtomicUsize,
    reuse_seen: AtomicBool,
    identity_hangs: AtomicBool,
    identity_errors: AtomicBool,
    long_lived_tokens: AtomicBool,
    /// Replaces the answer to every refresh, as a gateway in front of the
    /// service does.
    refresh_override: Mutex<Option<(u16, String)>>,
    /// Holds the next refresh answer back after the rotation has happened, as
    /// a slow network does.
    refresh_response_gate: Mutex<Option<oneshot::Receiver<()>>>,
    revoked_tokens: Mutex<Vec<String>>,
    /// Records a logout and then never answers it.
    revokes_hang: AtomicBool,
}

impl RotatingServer {
    fn new(live_refresh_token: &str) -> Arc<Self> {
        Arc::new(Self {
            live_refresh_token: Mutex::new(live_refresh_token.to_string()),
            rotations: AtomicUsize::new(0),
            reuse_seen: AtomicBool::new(false),
            identity_hangs: AtomicBool::new(false),
            identity_errors: AtomicBool::new(false),
            long_lived_tokens: AtomicBool::new(false),
            refresh_override: Mutex::new(None),
            refresh_response_gate: Mutex::new(None),
            revoked_tokens: Mutex::new(Vec::new()),
            revokes_hang: AtomicBool::new(false),
        })
    }

    fn hang_revokes(&self) {
        self.revokes_hang.store(true, Ordering::SeqCst);
    }

    /// Issues access tokens that stay valid, so a later credential request
    /// does not rotate again.
    fn issue_long_lived_tokens(&self) {
        self.long_lived_tokens.store(true, Ordering::SeqCst);
    }

    fn answer_refreshes_with(&self, status: u16, body: &str) {
        *self.refresh_override.lock().unwrap() = Some((status, body.to_string()));
    }

    fn hold_next_refresh_response(&self) -> oneshot::Sender<()> {
        let (release, gate) = oneshot::channel();
        *self.refresh_response_gate.lock().unwrap() = Some(gate);
        release
    }

    fn rotations(&self) -> usize {
        self.rotations.load(Ordering::SeqCst)
    }

    fn revoked(&self, refresh_token: &str) -> bool {
        self.revoked_tokens
            .lock()
            .unwrap()
            .iter()
            .any(|revoked| revoked == refresh_token)
    }

    fn client(self: &Arc<Self>) -> Arc<dyn HttpClient> {
        let server = self.clone();
        FakeHttpClient::create(move |request| {
            let server = server.clone();
            async move { server.answer(request).await }
        })
    }

    fn reuse_seen(&self) -> bool {
        self.reuse_seen.load(Ordering::SeqCst)
    }

    /// Makes `/auth/me` never answer, as a request cut off mid-flight would.
    fn hang_identity(&self) {
        self.identity_hangs.store(true, Ordering::SeqCst);
    }

    /// Makes `/auth/me` answer 503, as a gateway in front of a down service does.
    fn fail_identity(&self) {
        self.identity_errors.store(true, Ordering::SeqCst);
    }

    /// Rotates the token if it is the live one, and reads anything else as the
    /// reuse the real service answers with a 401.
    fn rotate(&self, presented: &str) -> Response<AsyncBody> {
        let mut live = self.live_refresh_token.lock().unwrap();
        if presented != *live {
            self.reuse_seen.store(true, Ordering::SeqCst);
            return json_response(
                401,
                serde_json::json!({
                    "statusCode": 401,
                    "message": "Unauthorized",
                    "error": "Unauthorized",
                }),
            );
        }

        let rotation = self.rotations.fetch_add(1, Ordering::SeqCst) + 1;
        *live = format!("rt-{rotation}");
        json_response(
            200,
            serde_json::json!({
                "access_token": format!("at-{rotation}"),
                "refresh_token": *live,
                "expires_in": if self.long_lived_tokens.load(Ordering::SeqCst) { 3_600 } else { 0 },
            }),
        )
    }

    async fn answer(&self, request: Request<AsyncBody>) -> anyhow::Result<Response<AsyncBody>> {
        let path = request.uri().path().to_string();

        if path.ends_with("/auth/device/refresh") {
            let overridden = self.refresh_override.lock().unwrap().clone();
            if let Some((status, body)) = overridden {
                return Ok(Response::builder()
                    .status(status)
                    .body(body.into())
                    .expect("a well-formed response"));
            }

            let mut raw = String::new();
            request.into_body().read_to_string(&mut raw).await?;
            let presented: serde_json::Value = serde_json::from_str(&raw)?;
            let presented = presented["refresh_token"].as_str().unwrap_or_default();

            let answer = self.rotate(presented);

            let gate = self.refresh_response_gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.await.ok();
            }
            return Ok(answer);
        }

        if path.ends_with("/auth/device/logout") {
            let mut raw = String::new();
            request.into_body().read_to_string(&mut raw).await?;
            let presented: serde_json::Value = serde_json::from_str(&raw)?;
            self.revoked_tokens.lock().unwrap().push(
                presented["refresh_token"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
            if self.revokes_hang.load(Ordering::SeqCst) {
                futures::future::pending::<()>().await;
                unreachable!()
            }
            return Ok(json_response(201, serde_json::json!({ "ok": true })));
        }

        // A sign-in the user approves at once: the first poll gets tokens.
        if path.ends_with("/auth/device/code") {
            return Ok(json_response(
                201,
                serde_json::json!({
                    "device_code": "dc",
                    "user_code": "A1B2-C3D4",
                    "verification_uri": "https://zodekit.site/activate",
                    "verification_uri_complete": "https://zodekit.site/activate?code=A1B2-C3D4",
                    "expires_in": 600,
                    "interval": 5,
                }),
            ));
        }
        if path.ends_with("/auth/device/token") {
            return Ok(json_response(
                201,
                serde_json::json!({
                    "access_token": "at-signed-in",
                    "refresh_token": "rt-signed-in",
                    "expires_in": 3_600,
                }),
            ));
        }

        if path.ends_with("/auth/me") {
            if self.identity_hangs.load(Ordering::SeqCst) {
                // Never resolves in any bounded amount of scheduler time.
                futures::future::pending::<()>().await;
                unreachable!()
            }
            if self.identity_errors.load(Ordering::SeqCst) {
                return Ok(json_response(
                    503,
                    serde_json::json!({ "error": "unavailable" }),
                ));
            }
            return Ok(json_response(
                200,
                serde_json::json!({
                    "id": "1",
                    "email": "ada@example.com",
                    "name": null,
                    "avatarUrl": null,
                }),
            ));
        }

        Ok(json_response(
            404,
            serde_json::json!({ "error": "not_found" }),
        ))
    }
}

fn json_response(status: u16, body: serde_json::Value) -> Response<AsyncBody> {
    Response::builder()
        .status(status)
        .body(body.to_string().into())
        .expect("a well-formed response")
}

fn restored_account(
    cx: &mut TestAppContext,
    http_client: Arc<dyn HttpClient>,
    credentials: &Arc<StubCredentials>,
) -> Entity<Account> {
    cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(
                http_client,
                credentials.clone(),
                "https://zodekit.site/api".into(),
            );
            account.restore(cx).detach();
            account
        })
    })
}

/// Invariant 7 — a refresh during restore survives a restart.
///
/// The refresh token rotates on use, so the pair a restore obtains is the only
/// live one the moment the server hands it over. If it is not written back, the
/// keychain holds a spent token, the next start presents it, and the server
/// reads that as theft and signs the user out everywhere. Two starts in a row
/// against a server that enforces the rule is the whole scenario.
#[gpui::test]
async fn a_refresh_during_restore_survives_a_restart(cx: &mut TestAppContext) {
    let server = RotatingServer::new("stored-refresh");
    let credentials = StubCredentials::holding("1", expired_session(true));

    let first = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    first.read_with(cx, |account: &Account, _| {
        assert!(
            account.status().is_signed_in(),
            "the first start refreshes a valid session and stays signed in"
        );
    });
    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(
        saved["refresh_token"], "rt-1",
        "the rotated refresh token must be on disk before the process can exit"
    );

    let second = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    assert!(
        !server.reuse_seen(),
        "the second start presented a refresh token the server had already spent"
    );
    second.read_with(cx, |account: &Account, _| {
        assert!(
            account.status().is_signed_in(),
            "the second start must still be signed in"
        );
    });
    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(saved["refresh_token"], "rt-2");
}

/// Invariant 7, for the entry that does not remember who it belongs to.
///
/// The rotation is saved under the id the entry was already filed under, and
/// once `/auth/me` names the owner the entry is rewritten to remember them.
#[gpui::test]
async fn a_refresh_during_restore_of_a_legacy_entry_is_saved_with_its_owner(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("stored-refresh");
    let credentials = StubCredentials::holding("1", expired_session(false));

    let account = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    account.read_with(cx, |account: &Account, _| {
        assert!(account.status().is_signed_in());
    });
    let (username, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(username, "1");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(
        saved["user"]["id"], "1",
        "the entry now remembers its owner"
    );
}

/// Invariant 7, before the identity call answers.
///
/// The identity call is the slow step that follows a rotation, and quitting
/// during it is the window the write is ordered ahead of. Here that call never
/// answers, so the rotated pair must already be on disk when the run stops.
#[gpui::test]
async fn a_rotation_is_saved_before_the_identity_call_answers(cx: &mut TestAppContext) {
    let server = RotatingServer::new("stored-refresh");
    server.hang_identity();
    let credentials = StubCredentials::holding("1", expired_session(true));

    let _account = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(
        saved["refresh_token"], "rt-1",
        "the rotated refresh token must be on disk while the identity call is pending"
    );
}

/// Invariant 7, for an entry that does not remember who it belongs to, when the
/// identity call cannot name them.
///
/// The keychain username is the user id, so the entry can be rewritten under the
/// id it was already filed under without waiting for `/auth/me`. Skipping that
/// leaves a spent refresh token on disk, and the next start presenting it is read
/// as theft and revokes every session of the user. The stored id is deliberately
/// not the one the server would answer with, so it can only have come from the
/// keychain.
async fn legacy_entry_keeps_its_rotation_while_identity_fails(
    cx: &mut TestAppContext,
    server: Arc<RotatingServer>,
) {
    let credentials = StubCredentials::holding("filed-under-id", expired_session(false));

    let _first = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    let (username, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(
        saved["refresh_token"], "rt-1",
        "the rotated refresh token must be on disk although the owner is still unknown"
    );
    assert_eq!(
        username, "filed-under-id",
        "the entry stays filed under the id it already had"
    );
    assert!(
        saved["user"].is_null(),
        "an owner nobody has named must not be invented"
    );

    let _second = restored_account(cx, server.client(), &credentials);
    cx.run_until_parked();

    assert!(
        !server.reuse_seen(),
        "the second start presented a refresh token the server had already spent"
    );
    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(saved["refresh_token"], "rt-2");
}

#[gpui::test]
async fn a_legacy_entry_keeps_its_rotation_while_the_identity_call_hangs(cx: &mut TestAppContext) {
    let server = RotatingServer::new("stored-refresh");
    server.hang_identity();
    legacy_entry_keeps_its_rotation_while_identity_fails(cx, server).await;
}

#[gpui::test]
async fn a_legacy_entry_keeps_its_rotation_while_the_identity_call_errors(cx: &mut TestAppContext) {
    let server = RotatingServer::new("stored-refresh");
    server.fail_identity();
    legacy_entry_keeps_its_rotation_while_identity_fails(cx, server).await;
}

fn ada() -> zode_account::AccountUser {
    zode_account::AccountUser {
        id: "1".into(),
        email: "ada@example.com".into(),
        name: None,
        avatar_url: None,
    }
}

/// A signed-in account whose access token has already died and whose keychain
/// holds the refresh token the account itself holds, which is the state every
/// credential request below starts from.
fn signed_in_with_expired_tokens(
    cx: &mut TestAppContext,
    server: &Arc<RotatingServer>,
    credentials: &Arc<StubCredentials>,
) -> Entity<Account> {
    cx.update(|cx| {
        cx.new(|_| {
            let mut account = Account::for_test_with(
                AccountStatus::SignedIn(ada()),
                server.client(),
                credentials.clone(),
            );
            account.set_expired_tokens_for_test();
            account
        })
    })
}

fn keychain_holding_the_account_tokens() -> Arc<StubCredentials> {
    StubCredentials::holding(
        "1",
        session_holding_refresh_token(
            "test-refresh",
            true,
            std::time::SystemTime::now() - Duration::from_secs(3_600),
        ),
    )
}

/// Invariant 8 -- a rotation the keychain refused must not leave the spent
/// token behind.
///
/// The server spent the token the moment it rotated it, so whatever the entry
/// still holds is one the next start presents, and presenting a spent token is
/// read as theft and revokes every session the user has. A locked keychain that
/// stays locked is enough to get there. No entry at all is a clean sign-out.
#[gpui::test]
async fn a_rotation_the_keychain_refuses_does_not_leave_the_spent_token_behind(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("test-refresh");
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    let credential = account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;

    assert!(
        credential.is_some(),
        "the session must keep working for this run"
    );
    assert_eq!(server.rotations(), 1);
    assert!(
        credentials.held().is_none(),
        "the keychain still holds the token the server has already spent: {:?}",
        credentials.held()
    );
}

/// The write that failed is tried again, and the entry comes back once the
/// keychain takes it, so the session still survives a restart.
#[gpui::test]
async fn an_unsaved_rotation_is_written_by_the_next_credential_request(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;
    assert!(credentials.held().is_none());

    credentials.refuse_writes(false);
    cx.executor().advance_clock(Duration::from_secs(31));
    let credential = account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;

    assert!(credential.is_some());
    assert_eq!(
        server.rotations(),
        1,
        "the token was still fresh, so the retry must not rotate it again"
    );
    let (username, saved) = credentials
        .held()
        .expect("the keychain accepted the write, so the session is saved");
    assert_eq!(username, "1");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(saved["user"]["id"], "1");
}

/// A delete that failed is retried too. The entry is the thing that must not
/// outlive the rotation, so a keychain that refuses both gets another chance
/// at the delete when the next request arrives.
#[gpui::test]
async fn a_spent_entry_that_could_not_be_deleted_is_deleted_by_the_next_request(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    credentials.refuse_deletes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;
    let (_, stale) = credentials
        .held()
        .expect("the keychain refused the delete, so the old entry is all it has");
    assert_eq!(stale["refresh_token"], "test-refresh");

    credentials.refuse_deletes(false);
    cx.executor().advance_clock(Duration::from_secs(31));
    account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;

    assert!(
        credentials.held().is_none(),
        "the write still fails, so the entry holding the spent token must go: {:?}",
        credentials.held()
    );
}

/// Invariant 9 -- signing out wins over a refresh that is already on the wire.
///
/// The server has rotated the token and the answer is still travelling. If it
/// lands after the user signed out it must neither be written to the keychain,
/// which would sign them back in at the next start, nor be left alive on the
/// server, which holds the only live token of a session nobody can reach.
#[gpui::test]
async fn a_refresh_that_answers_after_sign_out_does_not_sign_the_user_back_in(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("test-refresh");
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    let release = server.hold_next_refresh_response();

    let credential = account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    cx.run_until_parked();
    assert_eq!(server.rotations(), 1, "the refresh must be in flight");

    let signing_out = account.update(cx, |account: &mut Account, cx| account.sign_out(cx));
    cx.run_until_parked();
    signing_out.await;
    assert!(credentials.held().is_none(), "sign-out clears the keychain");

    release.send(()).expect("the server is still waiting");
    cx.run_until_parked();

    assert!(
        credentials.held().is_none(),
        "the late answer was written back: {:?}",
        credentials.held()
    );
    assert!(
        credential.await.is_none(),
        "a signed-out account hands out no credential"
    );
    account.read_with(cx, |account: &Account, _| {
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
    assert_eq!(
        credentials.writes_attempted(),
        0,
        "a session that is already over must not be written even briefly"
    );
    assert!(
        server.revoked("rt-1"),
        "the token the server issued after sign-out must be revoked"
    );
}

/// The same race one step later: the answer is in, the keychain write has
/// begun, and the sign-out finishes its delete before the write does.
#[gpui::test]
async fn a_save_that_lands_after_sign_out_is_taken_back(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    let release = credentials.hold_next_write();

    let credential = account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    cx.run_until_parked();
    assert_eq!(server.rotations(), 1);

    let signing_out = account.update(cx, |account: &mut Account, cx| account.sign_out(cx));
    cx.run_until_parked();
    signing_out.await;
    assert!(credentials.held().is_none());

    release.send(()).expect("the write is still waiting");
    cx.run_until_parked();

    assert!(
        credentials.held().is_none(),
        "the write that was in flight resurrected the session: {:?}",
        credentials.held()
    );
    assert!(credential.await.is_none());
    assert!(server.revoked("rt-1"));
}

/// A caller that stops waiting must not cost the rotation. The server has
/// already spent the token, so a rotation that is neither applied nor saved is
/// the theft signal at the next start.
#[gpui::test]
async fn a_caller_that_gives_up_does_not_lose_the_rotation(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    let abandoned = account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    drop(abandoned);
    cx.run_until_parked();

    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(
        saved["refresh_token"], "rt-1",
        "the rotation the server performed must reach the keychain"
    );
    let credential = account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;
    assert_eq!(
        server.rotations(),
        1,
        "the rotated tokens were applied, so the next request has nothing to refresh"
    );
    assert!(credential.is_some());
}

/// Invariant 10 -- only the server saying the credential is dead signs the user
/// out. Everything else is a reason to try again later.
#[gpui::test]
async fn a_refresh_the_gateway_throttles_keeps_the_session(cx: &mut TestAppContext) {
    for (status, body) in [
        (
            429,
            r#"{"statusCode":429,"message":"ThrottlerException: Too Many Requests"}"#,
        ),
        (408, ""),
        (401, "<html><body>Sign in to the network</body></html>"),
        (403, "<!doctype html><title>Blocked</title>"),
        (
            400,
            r#"{"statusCode":400,"message":["refresh_token must be a string"],"error":"Bad Request"}"#,
        ),
    ] {
        let server = RotatingServer::new("test-refresh");
        server.answer_refreshes_with(status, body);
        let credentials = keychain_holding_the_account_tokens();
        let account = signed_in_with_expired_tokens(cx, &server, &credentials);

        let credential = account
            .update(cx, |account: &mut Account, cx| account.api_credential(cx))
            .await;

        assert!(credential.is_none(), "{status}: nothing to hand out");
        account.read_with(cx, |account: &Account, _| {
            assert!(
                account.status().is_signed_in(),
                "{status} signed the user out"
            );
        });
        let (_, saved) = credentials
            .held()
            .unwrap_or_else(|| panic!("{status} wiped the keychain"));
        assert_eq!(saved["refresh_token"], "test-refresh");
    }
}

/// The other side of the same rule: a refresh token the server refuses is dead,
/// and keeping it would only retry it against a server that will never accept it.
#[gpui::test]
async fn a_refresh_the_server_refuses_signs_the_user_out_and_clears_the_keychain(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("some-other-token");
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    let credential = account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await;
    cx.run_until_parked();

    assert!(credential.is_none());
    assert!(server.reuse_seen());
    account.read_with(cx, |account: &Account, _| {
        assert_eq!(*account.status(), AccountStatus::SignedOut);
    });
    assert!(credentials.held().is_none());
}

async fn credential_of(account: &Entity<Account>, cx: &mut TestAppContext) -> bool {
    account
        .update(cx, |account: &mut Account, cx| account.api_credential(cx))
        .await
        .is_some()
}

/// A write that fails twice and then goes through is a blip, not a broken
/// keychain, so the entry is never deleted and the next request has nothing
/// left to retry.
#[gpui::test]
async fn a_rotation_the_keychain_refuses_twice_is_saved_on_the_third_attempt(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_next_writes(2);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    assert!(credential_of(&account, cx).await);

    let (_, saved) = credentials.held().expect("the third attempt saved it");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(credentials.writes_attempted(), 3);
    assert_eq!(
        credentials.deletes_attempted(),
        0,
        "the entry was removed although a later attempt would have worked"
    );

    cx.executor().advance_clock(Duration::from_secs(3_600));
    assert!(credential_of(&account, cx).await);
    assert_eq!(
        credentials.writes_attempted(),
        3,
        "a saved rotation has nothing left to retry"
    );
}

/// A keychain that stays broken, as one without a secret service does, must not
/// be written to, deleted from and complained about on every credential
/// request. The retries back off instead, and each failure doubles the wait.
#[gpui::test]
async fn a_keychain_that_stays_broken_is_retried_with_a_growing_pause(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);

    assert!(credential_of(&account, cx).await);
    let after_the_rotation = (
        credentials.writes_attempted(),
        credentials.deletes_attempted(),
    );
    assert_eq!(after_the_rotation, (3, 1));

    for _ in 0..5 {
        assert!(credential_of(&account, cx).await);
    }
    assert_eq!(
        (
            credentials.writes_attempted(),
            credentials.deletes_attempted()
        ),
        after_the_rotation,
        "requests made straight after a failure hammered the keychain"
    );

    cx.executor().advance_clock(Duration::from_secs(31));
    assert!(credential_of(&account, cx).await);
    let after_the_first_retry = (
        credentials.writes_attempted(),
        credentials.deletes_attempted(),
    );
    assert_eq!(after_the_first_retry, (6, 2), "the first retry is due");

    cx.executor().advance_clock(Duration::from_secs(31));
    assert!(credential_of(&account, cx).await);
    assert_eq!(
        (
            credentials.writes_attempted(),
            credentials.deletes_attempted()
        ),
        after_the_first_retry,
        "a second failure doubles the wait, so 31 seconds is too soon"
    );

    cx.executor().advance_clock(Duration::from_secs(31));
    assert!(credential_of(&account, cx).await);
    assert_eq!(
        (
            credentials.writes_attempted(),
            credentials.deletes_attempted()
        ),
        (9, 3),
        "after a full minute the second retry is due"
    );
}

fn quit(cx: &mut TestAppContext) {
    cx.update(|cx| cx.shutdown());
}

/// Quitting is the last chance to keep a rotation the keychain refused: the
/// spent token would otherwise be presented by the next start. A keychain that
/// has recovered by then is written.
#[gpui::test]
async fn quitting_writes_a_rotation_the_keychain_had_refused(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    assert!(credential_of(&account, cx).await);
    assert!(credentials.held().is_none());

    credentials.refuse_writes(false);
    quit(cx);

    let (username, saved) = credentials
        .held()
        .expect("the keychain recovered, so quitting saves the session");
    assert_eq!(username, "1");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(saved["user"]["id"], "1");
}

/// And when it still cannot be written, quitting removes whatever is there,
/// because a clean sign-out at the next start beats a theft signal.
#[gpui::test]
async fn quitting_deletes_the_stale_entry_when_the_write_still_fails(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    credentials.refuse_deletes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    assert!(credential_of(&account, cx).await);
    assert!(credentials.held().is_some(), "the delete was refused too");

    credentials.refuse_deletes(false);
    quit(cx);

    assert!(
        credentials.held().is_none(),
        "the spent token survived the quit: {:?}",
        credentials.held()
    );
}

/// A session whose keychain entry is current has nothing to flush, and quitting
/// must not touch it.
#[gpui::test]
async fn quitting_leaves_a_saved_session_alone(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    assert!(credential_of(&account, cx).await);
    let (writes, deletes) = (
        credentials.writes_attempted(),
        credentials.deletes_attempted(),
    );

    quit(cx);

    assert_eq!(
        (
            credentials.writes_attempted(),
            credentials.deletes_attempted()
        ),
        (writes, deletes)
    );
    let (_, saved) = credentials.held().expect("the session is still saved");
    assert_eq!(saved["refresh_token"], "rt-1");
}

/// Signing out ends the obligation: nothing may be written back at quit.
#[gpui::test]
async fn quitting_after_a_sign_out_writes_nothing(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.issue_long_lived_tokens();
    let credentials = keychain_holding_the_account_tokens();
    credentials.refuse_writes(true);
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    assert!(credential_of(&account, cx).await);

    account
        .update(cx, |account: &mut Account, cx| account.sign_out(cx))
        .await;
    credentials.refuse_writes(false);
    let writes = credentials.writes_attempted();
    quit(cx);

    assert_eq!(credentials.writes_attempted(), writes);
    assert!(credentials.held().is_none());
}

/// Invariant 11 -- a sign-out that is followed by a sign-in starts a new
/// session, and a credential request in it must not be handed the old
/// session's pending refresh, which is certain to answer nothing.
#[gpui::test]
async fn a_new_session_does_not_join_the_old_sessions_pending_refresh(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    let release = server.hold_next_refresh_response();

    let old_session_request =
        account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    cx.run_until_parked();
    assert_eq!(
        server.rotations(),
        1,
        "the old session's refresh is in flight"
    );

    account
        .update(cx, |account: &mut Account, cx| account.sign_out(cx))
        .await;
    account.update(cx, |account: &mut Account, cx| account.sign_in(cx));
    cx.run_until_parked();
    account.read_with(cx, |account: &Account, _| {
        assert!(account.status().is_signed_in(), "the new session is up");
    });

    let new_session_request =
        account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    release.send(()).expect("the server is still waiting");
    cx.run_until_parked();

    assert!(old_session_request.await.is_none());
    let credential = new_session_request
        .await
        .expect("the new session's token is fresh, so it needs no refresh");
    assert_eq!(credential.access_token, "at-signed-in");
}

/// A late token is revoked in the background. The callers that asked for a
/// credential are not waiting on a network round trip that tells them nothing.
#[gpui::test]
async fn revoking_a_late_token_does_not_hold_the_caller_up(cx: &mut TestAppContext) {
    let server = RotatingServer::new("test-refresh");
    server.hang_revokes();
    let credentials = keychain_holding_the_account_tokens();
    let account = signed_in_with_expired_tokens(cx, &server, &credentials);
    let release = server.hold_next_refresh_response();

    let credential = account.update(cx, |account: &mut Account, cx| account.api_credential(cx));
    cx.run_until_parked();
    let _signing_out = account.update(cx, |account: &mut Account, cx| account.sign_out(cx));
    cx.run_until_parked();
    release.send(()).expect("the server is still waiting");

    assert!(
        credential.await.is_none(),
        "a signed-out account hands out no credential"
    );
    assert!(
        server.revoked("rt-1"),
        "the late token must still be revoked"
    );
}

/// Restore, with a keychain that refuses the rotated pair.
///
/// The server spent the stored token, so the entry must not keep it: the next
/// start would present it. The session still works, and the pair is written
/// at the next request once the keychain takes it.
#[gpui::test]
async fn a_restore_whose_save_is_refused_leaves_no_spent_token_and_saves_later(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("stored-refresh");
    server.issue_long_lived_tokens();
    let credentials = StubCredentials::holding("1", expired_session(true));
    credentials.refuse_writes(true);

    let account = restored_account(cx, server.client(), &credentials);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    account.read_with(cx, |account: &Account, _| {
        assert!(
            account.status().is_signed_in(),
            "a locked keychain is not a sign-out"
        );
    });
    assert_eq!(credentials.writes_attempted(), 3);
    assert!(
        credentials.held().is_none(),
        "the entry still holds the token the server spent: {:?}",
        credentials.held()
    );

    credentials.refuse_writes(false);
    cx.executor().advance_clock(Duration::from_secs(31));
    assert!(credential_of(&account, cx).await);

    let (username, saved) = credentials.held().expect("the retry saved the session");
    assert_eq!(username, "1");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(saved["user"]["id"], "1");
    assert_eq!(server.rotations(), 1, "the retry did not rotate again");
}

/// An entry that never recorded its owner is saved again once `/auth/me` names
/// them, and that second write is also the retry of the one that failed.
#[gpui::test]
async fn a_legacy_restore_whose_first_save_is_refused_is_saved_once_the_owner_is_named(
    cx: &mut TestAppContext,
) {
    let server = RotatingServer::new("stored-refresh");
    server.issue_long_lived_tokens();
    let credentials = StubCredentials::holding("filed-under-id", expired_session(false));
    credentials.refuse_next_writes(3);

    let account = restored_account(cx, server.client(), &credentials);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    let (username, saved) = credentials
        .held()
        .expect("the write that names the owner went through");
    assert_eq!(username, "1");
    assert_eq!(saved["refresh_token"], "rt-1");
    assert_eq!(saved["user"]["id"], "1");
    assert_eq!(credentials.writes_attempted(), 4);

    cx.executor().advance_clock(Duration::from_secs(3_600));
    assert!(credential_of(&account, cx).await);
    assert_eq!(
        credentials.writes_attempted(),
        4,
        "the session is saved, so nothing is left to retry"
    );
}

/// The same entry when nothing can name its owner and the keychain refuses
/// every write: there is nothing safe to leave behind.
#[gpui::test]
async fn a_legacy_restore_that_cannot_be_saved_leaves_no_entry(cx: &mut TestAppContext) {
    let server = RotatingServer::new("stored-refresh");
    server.fail_identity();
    let credentials = StubCredentials::holding("filed-under-id", expired_session(false));
    credentials.refuse_writes(true);

    let _account = restored_account(cx, server.client(), &credentials);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();

    assert!(
        credentials.held().is_none(),
        "the entry still holds the token the server spent: {:?}",
        credentials.held()
    );
}
