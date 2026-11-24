//! Stand-ins for the keychain, `gh` and the network, so the GitHub tab can be
//! opened in a test without touching any of them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use credentials_provider::CredentialsProvider;
use futures::AsyncReadExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use github::{FakeGithubBackend, GhHost, GhOutput, GhRunResult, KEYCHAIN_URL};
use gpui::{App, AsyncApp};
use http_client::{AsyncBody, FakeHttpClient, HttpClient, Response};

use crate::sign_in::{GithubEnvironment, TokenStorage};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct MemoryKeychain {
    entries: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Decides the next write: `true` lets it land, `false` fails it.
    held_write: Mutex<Option<oneshot::Receiver<bool>>>,
}

impl MemoryKeychain {
    pub fn holding(token: &str) -> Self {
        let keychain = Self::default();
        lock(&keychain.entries).insert(KEYCHAIN_URL.to_string(), token.as_bytes().to_vec());
        keychain
    }

    /// Holds the next write until the returned sender decides it. The write
    /// goes on in the background, as an operating system's keychain call
    /// does, so it still lands when whoever asked for it gave up waiting.
    pub fn hold_next_write(&self) -> oneshot::Sender<bool> {
        let (decide, decided) = oneshot::channel();
        *lock(&self.held_write) = Some(decided);
        decide
    }

    pub fn github_token(&self) -> Option<String> {
        lock(&self.entries)
            .get(KEYCHAIN_URL)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

impl CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
        let entry = lock(&self.entries).get(url).cloned();
        Box::pin(async move { Ok(entry.map(|bytes| ("github-token".to_string(), bytes))) })
    }

    fn write_credentials<'a>(
        &'a self,
        url: &'a str,
        _username: &'a str,
        password: &'a [u8],
        cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        let held = lock(&self.held_write).take();
        let Some(decided) = held else {
            lock(&self.entries).insert(url.to_string(), password.to_vec());
            return Box::pin(async { Ok(()) });
        };
        let entries = self.entries.clone();
        let (url, password) = (url.to_string(), password.to_vec());
        let (finished, wait) = oneshot::channel();
        cx.background_executor()
            .spawn(async move {
                let lands = decided.await.unwrap_or(false);
                if lands {
                    lock(&entries).insert(url, password);
                }
                finished.send(lands).ok();
            })
            .detach();
        Box::pin(async move {
            match wait.await {
                Ok(true) => Ok(()),
                _ => anyhow::bail!("the keychain refused the write"),
            }
        })
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        lock(&self.entries).remove(url);
        Box::pin(async { Ok(()) })
    }
}

/// `gh` signed in with `token`, or not installed at all.
pub struct FakeGhHost {
    token: Option<String>,
    runs: Mutex<usize>,
}

impl FakeGhHost {
    pub fn signed_in(token: &str) -> Self {
        Self {
            token: Some(token.to_string()),
            runs: Mutex::new(0),
        }
    }

    pub fn missing() -> Self {
        Self {
            token: None,
            runs: Mutex::new(0),
        }
    }

    pub fn runs(&self) -> usize {
        *lock(&self.runs)
    }
}

impl GhHost for FakeGhHost {
    fn environment_variable(&self, _name: &str) -> Option<String> {
        None
    }

    fn find_on_path(&self, _program: &str) -> Option<PathBuf> {
        self.token.as_ref().map(|_| fake_gh_path())
    }

    fn run(&self, _program: PathBuf, _arguments: Vec<String>) -> BoxFuture<'static, GhRunResult> {
        *lock(&self.runs) += 1;
        let stdout = self.token.clone().unwrap_or_default();
        Box::pin(async move {
            Ok(GhOutput {
                success: true,
                stdout,
            })
        })
    }
}

/// One request as it was sent.
#[derive(Clone, Debug)]
pub struct SentRequest {
    pub url: String,
    pub accept: Option<String>,
    pub body: String,
}

/// Answers each URL from its own queue of `(status, body)`; the last answer
/// repeats. An unscripted URL fails as if the network were down.
#[derive(Clone, Default)]
pub struct ScriptedHttp {
    answers: Arc<Mutex<HashMap<String, VecDeque<(u16, String)>>>>,
    hanging: Arc<Mutex<HashSet<String>>>,
    sent: Arc<Mutex<Vec<SentRequest>>>,
}

impl ScriptedHttp {
    pub fn answer(&self, url: &str, status: u16, body: impl Into<String>) {
        lock(&self.answers)
            .entry(url.to_string())
            .or_default()
            .push_back((status, body.into()));
    }

    /// Requests to `url` are taken and never answered.
    pub fn hang(&self, url: &str) {
        lock(&self.hanging).insert(url.to_string());
    }

    pub fn sent(&self) -> Vec<SentRequest> {
        lock(&self.sent).clone()
    }

    pub fn client(&self) -> Arc<dyn HttpClient> {
        let answers = self.answers.clone();
        let hanging = self.hanging.clone();
        let sent = self.sent.clone();
        FakeHttpClient::create(move |request| {
            let answers = answers.clone();
            let hanging = hanging.clone();
            let sent = sent.clone();
            async move {
                let url = request.uri().to_string();
                let accept = request
                    .headers()
                    .get("Accept")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let mut body = String::new();
                request.into_body().read_to_string(&mut body).await?;
                lock(&sent).push(SentRequest {
                    url: url.clone(),
                    accept,
                    body,
                });
                if lock(&hanging).contains(&url) {
                    futures::future::pending::<()>().await;
                }
                let answer = {
                    let mut answers = lock(&answers);
                    let queue = answers.get_mut(&url);
                    match queue {
                        Some(queue) if queue.len() > 1 => queue.pop_front(),
                        Some(queue) => queue.front().cloned(),
                        None => None,
                    }
                };
                let Some((status, body)) = answer else {
                    anyhow::bail!("no answer scripted for {url}");
                };
                Ok(Response::builder()
                    .status(status)
                    .body(AsyncBody::from(body))?)
            }
        })
    }
}

/// No `client_id`, an empty keychain, no `gh`, no network, and a fake
/// backend: opening the tab in a test reaches nothing outside the process.
pub fn use_test_environment(cx: &mut App) {
    cx.set_global(github::GithubClientId(None));
    cx.set_global(GithubEnvironment {
        token_storage: TokenStorage::Keychain,
        credentials: Arc::new(MemoryKeychain::default()),
        gh_host: Arc::new(FakeGhHost::missing()),
        http_client: ScriptedHttp::default().client(),
        make_backend: Arc::new(|_token, _cx| Arc::new(FakeGithubBackend::new())),
    });
}

/// Where the fake finds `gh`: an absolute path on every OS, as a real
/// lookup on `PATH` gives. `/opt/...` is not absolute on Windows.
fn fake_gh_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\GitHub CLI\gh.exe")
    } else {
        PathBuf::from("/opt/homebrew/bin/gh")
    }
}
