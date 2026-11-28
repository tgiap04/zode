use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Result, anyhow};
use credentials_provider::CredentialsProvider;
use futures::FutureExt as _;
use futures::future::BoxFuture;
use gpui::{AsyncApp, TestAppContext};

use crate::error::GithubError;
use crate::token::{
    GH_ARGUMENTS, GH_TIMEOUT, GhHost, GhOutput, GhRunResult, GithubToken, KEYCHAIN_URL,
    TokenSource, TokenVariable, resolve_token, token_from_gh, write_keychain_token,
};

#[derive(Default)]
pub(crate) struct MemoryKeychain {
    pub entries: Mutex<HashMap<String, (String, Vec<u8>)>>,
    pub writes: Mutex<usize>,
    pub fail_reads: bool,
}

impl MemoryKeychain {
    pub fn holding(token: &str) -> Self {
        let keychain = Self::default();
        keychain
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                KEYCHAIN_URL.to_string(),
                ("github-token".to_string(), token.as_bytes().to_vec()),
            );
        keychain
    }

    fn write_count(&self) -> usize {
        *self.writes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CredentialsProvider for MemoryKeychain {
    fn read_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<Option<(String, Vec<u8>)>>> + 'a>> {
        Box::pin(async move {
            if self.fail_reads {
                return Err(anyhow!("the keychain is locked"));
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
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move {
            *self.writes.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            self.entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(url.to_string(), (username.to_string(), password.to_vec()));
            Ok(())
        })
    }

    fn delete_credentials<'a>(
        &'a self,
        url: &'a str,
        _cx: &'a AsyncApp,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(url);
            Ok(())
        })
    }
}

#[derive(Clone)]
enum GhBehaviour {
    Prints { success: bool, stdout: String },
    Hangs,
    CannotRun,
}

struct FakeGhHost {
    environment: HashMap<String, String>,
    gh_path: Option<PathBuf>,
    behaviour: GhBehaviour,
    runs: Mutex<Vec<(PathBuf, Vec<String>)>>,
}

impl FakeGhHost {
    fn printing(stdout: &str) -> Self {
        Self {
            environment: HashMap::new(),
            gh_path: Some(fake_gh_path()),
            behaviour: GhBehaviour::Prints {
                success: true,
                stdout: stdout.to_string(),
            },
            runs: Mutex::new(Vec::new()),
        }
    }

    fn with_behaviour(mut self, behaviour: GhBehaviour) -> Self {
        self.behaviour = behaviour;
        self
    }

    fn with_variable(mut self, name: &str, value: &str) -> Self {
        self.environment.insert(name.to_string(), value.to_string());
        self
    }

    fn runs(&self) -> Vec<(PathBuf, Vec<String>)> {
        self.runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl GhHost for FakeGhHost {
    fn environment_variable(&self, name: &str) -> Option<String> {
        self.environment.get(name).cloned()
    }

    fn find_on_path(&self, program: &str) -> Option<PathBuf> {
        (program == "gh").then(|| self.gh_path.clone()).flatten()
    }

    fn run(&self, program: PathBuf, arguments: Vec<String>) -> BoxFuture<'static, GhRunResult> {
        self.runs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((program, arguments));
        match self.behaviour.clone() {
            GhBehaviour::Prints { success, stdout } => {
                futures::future::ready(Ok(GhOutput { success, stdout })).boxed()
            }
            GhBehaviour::Hangs => futures::future::pending().boxed(),
            GhBehaviour::CannotRun => {
                futures::future::ready(Err("permission denied".to_string())).boxed()
            }
        }
    }
}

async fn resolve(
    keychain: Arc<MemoryKeychain>,
    host: Arc<FakeGhHost>,
    cx: &mut TestAppContext,
) -> Result<GithubToken, GithubError> {
    cx.spawn(async move |cx| resolve_token(keychain.as_ref(), host.as_ref(), &cx).await)
        .await
}

#[gpui::test]
async fn the_keychain_token_comes_first_and_gh_never_runs(cx: &mut TestAppContext) {
    let keychain = Arc::new(MemoryKeychain::holding("ghu_keychain\n"));
    let host = Arc::new(FakeGhHost::printing("gho_cli\n"));
    let token = resolve(keychain, host.clone(), cx).await.expect("a token");
    assert_eq!(token.source(), TokenSource::Keychain);
    assert_eq!(token.secret(), "ghu_keychain");
    assert!(host.runs().is_empty());
}

#[gpui::test]
async fn an_unreadable_keychain_is_an_error_not_a_switch_to_gh(cx: &mut TestAppContext) {
    let keychain = Arc::new(MemoryKeychain {
        fail_reads: true,
        ..MemoryKeychain::default()
    });
    let host = Arc::new(FakeGhHost::printing("gho_cli\n"));
    let error = resolve(keychain, host.clone(), cx).await.err();
    assert!(
        matches!(error, Some(GithubError::Keychain { .. })),
        "{error:?}"
    );
    assert!(host.runs().is_empty());
}

#[gpui::test]
async fn a_stored_value_that_is_not_a_token_is_an_error_not_a_switch_to_gh(
    cx: &mut TestAppContext,
) {
    let keychain = Arc::new(MemoryKeychain::holding("not a token\n"));
    let host = Arc::new(FakeGhHost::printing("gho_cli\n"));
    let error = resolve(keychain, host.clone(), cx).await.err();
    assert_eq!(
        error,
        Some(GithubError::Keychain {
            message: "the stored token is unreadable".to_string()
        })
    );
    assert!(host.runs().is_empty());
}

#[gpui::test]
async fn without_a_keychain_token_gh_runs_by_absolute_path(cx: &mut TestAppContext) {
    let keychain = Arc::new(MemoryKeychain::default());
    let host = Arc::new(FakeGhHost::printing("gho_cli\n"));
    let token = resolve(keychain.clone(), host.clone(), cx)
        .await
        .expect("a token");
    assert_eq!(token.source(), TokenSource::GhCli);
    assert_eq!(token.secret(), "gho_cli");
    assert_eq!(
        host.runs(),
        vec![(
            fake_gh_path(),
            GH_ARGUMENTS
                .iter()
                .map(|argument| argument.to_string())
                .collect()
        )]
    );
    assert_eq!(keychain.write_count(), 0, "gh's token is never stored");
}

#[gpui::test]
async fn a_token_from_zodes_environment_is_reported_as_such(cx: &mut TestAppContext) {
    let host =
        Arc::new(FakeGhHost::printing("github_pat_x\n").with_variable("GH_TOKEN", "github_pat_x"));
    let token = resolve(Arc::default(), host, cx).await.expect("a token");
    assert_eq!(
        token.source(),
        TokenSource::GhEnvironment(TokenVariable::GhToken)
    );

    let host = Arc::new(
        FakeGhHost::printing("ghp_ci\n")
            .with_variable("GH_TOKEN", "something-else")
            .with_variable("GITHUB_TOKEN", "ghp_ci"),
    );
    let token = resolve(Arc::default(), host, cx).await.expect("a token");
    assert_eq!(
        token.source(),
        TokenSource::GhEnvironment(TokenVariable::GithubToken)
    );
}

#[gpui::test]
async fn no_usable_gh_means_not_signed_in(cx: &mut TestAppContext) {
    let missing = FakeGhHost {
        gh_path: None,
        ..FakeGhHost::printing("gho_cli")
    };
    let missing = Arc::new(missing);
    assert_eq!(
        resolve(Arc::default(), missing.clone(), cx).await.err(),
        Some(GithubError::NotSignedIn)
    );
    assert!(missing.runs().is_empty());

    let relative = Arc::new(FakeGhHost {
        gh_path: Some(PathBuf::from("bin/gh")),
        ..FakeGhHost::printing("gho_cli")
    });
    assert_eq!(
        resolve(Arc::default(), relative.clone(), cx).await.err(),
        Some(GithubError::NotSignedIn)
    );
    assert!(relative.runs().is_empty(), "a relative path is never run");

    let logged_out = Arc::new(
        FakeGhHost::printing("").with_behaviour(GhBehaviour::Prints {
            success: false,
            stdout: String::new(),
        }),
    );
    assert_eq!(
        resolve(Arc::default(), logged_out, cx).await.err(),
        Some(GithubError::NotSignedIn)
    );

    let blank = Arc::new(FakeGhHost::printing("  \n"));
    assert_eq!(
        resolve(Arc::default(), blank, cx).await.err(),
        Some(GithubError::NotSignedIn)
    );

    let garbage = Arc::new(FakeGhHost::printing("not a token at all\n"));
    assert_eq!(
        resolve(Arc::default(), garbage, cx).await.err(),
        Some(GithubError::NotSignedIn)
    );
}

#[gpui::test]
async fn gh_that_cannot_run_is_reported(cx: &mut TestAppContext) {
    let host = Arc::new(FakeGhHost::printing("").with_behaviour(GhBehaviour::CannotRun));
    let error = resolve(Arc::default(), host, cx).await.err();
    assert_eq!(
        error,
        Some(GithubError::GhCli {
            message: "permission denied".to_string()
        })
    );
}

#[gpui::test]
async fn gh_that_hangs_is_given_up_after_the_timeout(cx: &mut TestAppContext) {
    let host = Arc::new(FakeGhHost::printing("").with_behaviour(GhBehaviour::Hangs));
    let task = cx.spawn({
        let host = host.clone();
        async move |cx| token_from_gh(host.as_ref(), cx.background_executor()).await
    });
    cx.executor()
        .advance_clock(GH_TIMEOUT - std::time::Duration::from_millis(1));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1));
    cx.run_until_parked();
    let error = task.await.err();
    assert_eq!(
        error,
        Some(GithubError::GhCli {
            message: "`gh auth token` did not answer within 8 seconds.".to_string()
        })
    );
}

#[test]
fn a_token_never_shows_in_debug_output() {
    let token = GithubToken::new("ghp_supersecret", TokenSource::GhCli);
    let debug = format!("{token:?}");
    assert!(!debug.contains("supersecret"), "{debug}");
    assert!(debug.contains("GhCli"), "{debug}");
    let same = GithubToken::new("ghp_supersecret\n", TokenSource::Keychain);
    let other = GithubToken::new("ghp_other", TokenSource::GhCli);
    assert_eq!(token.identity(), same.identity());
    assert_ne!(token.identity(), other.identity());
    assert!(!format!("{:?}", token.identity()).contains("supersecret"));
}

#[gpui::test]
async fn a_device_flow_token_is_stored_trimmed(cx: &mut TestAppContext) {
    let keychain = Arc::new(MemoryKeychain::default());
    cx.update(|cx| write_keychain_token(keychain.clone(), "  ghu_new \n".to_string(), cx))
        .await
        .expect("stored");
    let stored = keychain
        .entries
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(KEYCHAIN_URL)
        .cloned();
    assert_eq!(
        stored.map(|(_, password)| password),
        Some(b"ghu_new".to_vec())
    );
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

/// Zode reads the login shell's `PATH` in the background after starting, so a
/// tab restored at launch can look for `gh` before that `PATH` has arrived --
/// under the bare one a Dock launch starts with, where Homebrew's directory is
/// not. `gh` found nowhere then read as "not signed in", with nothing to say
/// why. It is looked for where it is installed as well as on `PATH`.
#[cfg(unix)]
#[test]
fn gh_is_found_where_it_is_installed_even_off_path() {
    use std::os::unix::fs::PermissionsExt as _;

    let installed = tempfile::tempdir().expect("a temporary directory");
    let gh = installed.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\n").expect("the stand-in gh is written");
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in gh is executable");
    let elsewhere = tempfile::tempdir().expect("a temporary directory");

    let found = crate::token::find_program(
        "gh",
        Some(elsewhere.path().as_os_str().to_owned()),
        &[installed.path().to_path_buf()],
    );
    assert_eq!(
        found.map(|path| path.canonicalize().unwrap_or(path)),
        Some(gh.canonicalize().unwrap_or(gh)),
    );

    assert_eq!(
        crate::token::find_program("gh", Some(elsewhere.path().as_os_str().to_owned()), &[]),
        None,
        "and nowhere is still nowhere"
    );
}
