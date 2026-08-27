use anyhow::{Context as _, Result};
use db::kvp::KeyValueStore;
use gpui::{
    App, AppContext as _, AsyncApp, BackgroundExecutor, Context, Entity, Global, Task, Window,
    actions,
};
use http_client::{HttpClient, HttpRequestExt as _, RedirectPolicy, Request};
use release_channel::ReleaseChannel;
use semver::Version;
use serde::{Deserialize, Serialize};
use smol::fs::File;
use smol::{fs, io::AsyncReadExt};
use std::mem;
use std::{
    env::{
        self,
        consts::{ARCH, OS},
    },
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
use util::command::new_command;
use workspace::Workspace;

const SHOULD_SHOW_UPDATE_NOTIFICATION_KEY: &str = "auto-updater-should-show-updated-notification";

/// The repository releases are read from. `github.repository` is not available to a
/// running app the way it is to a workflow, so this is the one place the fork's identity
/// is written down.
const RELEASE_REPO: &str = "tgiap04/zode";

// Only Linux reports a missing dependency this way -- it is the one platform where the
// installer shells out to a tool (`rsync`) that may simply not be present.
#[cfg(target_os = "linux")]
#[derive(Debug)]
struct MissingDependencyError(String);

#[cfg(target_os = "linux")]
impl std::fmt::Display for MissingDependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(target_os = "linux")]
impl std::error::Error for MissingDependencyError {}

#[cfg(target_os = "linux")]
fn linux_rsync_install_hint() -> &'static str {
    let os_release = match std::fs::read_to_string("/etc/os-release") {
        Ok(os_release) => os_release,
        Err(_) => return "Please install rsync using your package manager",
    };

    let mut distribution_ids = Vec::new();
    for line in os_release.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("ID=") {
            distribution_ids.push(value.trim_matches('"').to_ascii_lowercase());
        } else if let Some(value) = trimmed.strip_prefix("ID_LIKE=") {
            for id in value.trim_matches('"').split_whitespace() {
                distribution_ids.push(id.to_ascii_lowercase());
            }
        }
    }

    let package_manager_hint = if distribution_ids
        .iter()
        .any(|distribution_id| distribution_id == "arch")
    {
        Some("Install it with: sudo pacman -S rsync")
    } else if distribution_ids
        .iter()
        .any(|distribution_id| distribution_id == "debian" || distribution_id == "ubuntu")
    {
        Some("Install it with: sudo apt install rsync")
    } else if distribution_ids.iter().any(|distribution_id| {
        distribution_id == "fedora"
            || distribution_id == "rhel"
            || distribution_id == "centos"
            || distribution_id == "rocky"
            || distribution_id == "almalinux"
    }) {
        Some("Install it with: sudo dnf install rsync")
    } else {
        None
    };

    package_manager_hint.unwrap_or("Please install rsync using your package manager")
}

actions!(
    auto_update,
    [
        /// Checks for available updates.
        Check,
        /// Dismisses the update error message.
        DismissMessage,
        /// Opens the release notes for the current version in a browser.
        ViewReleaseNotes,
    ]
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionCheckType {
    Semantic(Version),
}

#[derive(Clone, Debug)]
pub enum AutoUpdateStatus {
    Idle,
    Checking,
    Downloading { version: VersionCheckType },
    Installing { version: VersionCheckType },
    Updated { version: VersionCheckType },
    Errored { error: Arc<anyhow::Error> },
}

impl PartialEq for AutoUpdateStatus {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AutoUpdateStatus::Idle, AutoUpdateStatus::Idle) => true,
            (AutoUpdateStatus::Checking, AutoUpdateStatus::Checking) => true,
            (
                AutoUpdateStatus::Downloading { version: v1 },
                AutoUpdateStatus::Downloading { version: v2 },
            ) => v1 == v2,
            (
                AutoUpdateStatus::Installing { version: v1 },
                AutoUpdateStatus::Installing { version: v2 },
            ) => v1 == v2,
            (
                AutoUpdateStatus::Updated { version: v1 },
                AutoUpdateStatus::Updated { version: v2 },
            ) => v1 == v2,
            (AutoUpdateStatus::Errored { error: e1 }, AutoUpdateStatus::Errored { error: e2 }) => {
                e1.to_string() == e2.to_string()
            }
            _ => false,
        }
    }
}

impl AutoUpdateStatus {
    pub fn is_updated(&self) -> bool {
        matches!(self, Self::Updated { .. })
    }
}

pub struct AutoUpdater {
    status: AutoUpdateStatus,
    current_version: Version,
    http_client: Arc<dyn HttpClient>,
    pending_poll: Option<Task<Option<()>>>,
    quit_subscription: Option<gpui::Subscription>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReleaseAsset {
    pub version: String,
    pub url: String,
    /// The release body, rendered as the release notes. `None` when the release has none.
    pub notes: Option<String>,
}

/// The shape of `GET /repos/{owner}/{repo}/releases/latest` -- only the parts used here.
#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<LatestReleaseAsset>,
}

#[derive(Deserialize)]
struct LatestReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// Reads a path under this repository's releases API.
///
/// No credential and no identifier is ever attached: the repository is public, and an
/// ambient `GITHUB_TOKEN` would make unpublished drafts visible to whoever happens to have
/// one exported.
async fn github_get(http_client: &Arc<dyn HttpClient>, path: &str) -> Result<Vec<u8>> {
    let url = format!("https://api.github.com/repos/{RELEASE_REPO}/{path}");
    let request = Request::get(&url)
        .header("Accept", "application/vnd.github+json")
        // GitHub refuses API requests that arrive with no User-Agent, and answers with a
        // 403 that reads like an authorization problem rather than a missing header.
        .header("User-Agent", "Zode")
        .follow_redirects(RedirectPolicy::FollowAll)
        .body(Default::default())?;

    let mut response = http_client
        .send(request)
        .await
        .with_context(|| format!("could not reach the GitHub API for {path}"))?;
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;

    anyhow::ensure!(
        response.status().is_success(),
        "GitHub API returned {} for {path}: {:?}",
        response.status().as_u16(),
        String::from_utf8_lossy(&body),
    );

    Ok(body)
}

/// The asset each platform installs from. These names come out of the bundling scripts
/// and are asserted by the `validate_release_assets` job in the release workflow; if that
/// list ever changes, this one has to change with it or updates stop resolving.
fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("macos", "aarch64") => "Zode-aarch64.dmg",
        ("macos", "x86_64") => "Zode-x86_64.dmg",
        ("linux", "aarch64") => "zode-linux-aarch64.tar.gz",
        ("linux", "x86_64") => "zode-linux-x86_64.tar.gz",
        ("windows", "aarch64") => "Zode-aarch64.exe",
        ("windows", "x86_64") => "Zode-x86_64.exe",
        _ => return None,
    })
}

struct MacOsUnmounter<'a> {
    mount_path: PathBuf,
    background_executor: &'a BackgroundExecutor,
}

impl Drop for MacOsUnmounter<'_> {
    fn drop(&mut self) {
        let mount_path = mem::take(&mut self.mount_path);
        self.background_executor
            .spawn(async move {
                let unmount_output = new_command("hdiutil")
                    .args(["detach", "-force"])
                    .arg(&mount_path)
                    .output()
                    .await;
                match unmount_output {
                    Ok(output) if output.status.success() => {
                        log::info!("Successfully unmounted the disk image");
                    }
                    Ok(output) => {
                        log::error!(
                            "Failed to unmount disk image: {:?}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                    Err(error) => {
                        log::error!("Error while trying to unmount disk image: {:?}", error);
                    }
                }
            })
            .detach();
    }
}

#[derive(Default)]
struct GlobalAutoUpdate(Option<Entity<AutoUpdater>>);

impl Global for GlobalAutoUpdate {}

pub fn init(http_client: Arc<dyn HttpClient>, cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|_, action, window, cx| check(action, window, cx));

        workspace.register_action(|_, action, _, cx| {
            view_release_notes(action, cx);
        });
    })
    .detach();

    // Leftovers from a previous update on Windows: the helper cannot delete the directories
    // it is running out of, so they are cleared on the next start instead. Upstream did this
    // at the head of its hourly polling loop; there is no loop here to hang it off.
    #[cfg(target_os = "windows")]
    cx.spawn(async move |_| {
        use util::ResultExt as _;
        cleanup_windows()
            .await
            .context("failed to clean up old update directories")
            .log_err();
    })
    .detach();

    let version = release_channel::AppVersion::global(cx);
    let auto_updater = cx.new(|cx| AutoUpdater::new(version, http_client, cx));
    cx.set_global(GlobalAutoUpdate(Some(auto_updater)));
}

pub fn check(_: &Check, window: &mut Window, cx: &mut App) {
    if let Some(message) = option_env!("ZED_UPDATE_EXPLANATION")
        .map(ToOwned::to_owned)
        .or_else(|| env::var("ZED_UPDATE_EXPLANATION").ok())
    {
        drop(window.prompt(
            gpui::PromptLevel::Info,
            "Zode was installed via a package manager.",
            Some(&message),
            &["Ok"],
            cx,
        ));
        return;
    }

    if !ReleaseChannel::try_global(cx)
        .map(|channel| channel.poll_for_updates())
        .unwrap_or(false)
    {
        return;
    }

    if let Some(updater) = AutoUpdater::get(cx) {
        updater.update(cx, |updater, cx| updater.poll(cx));
    } else {
        drop(window.prompt(
            gpui::PromptLevel::Info,
            "Could not check for updates",
            Some("Auto-updates disabled for non-bundled app."),
            &["Ok"],
            cx,
        ));
    }
}

/// The web page for the running version's release. Used as the fallback when the notes
/// cannot be shown inside the app.
pub fn release_notes_url(cx: &mut App) -> Option<String> {
    let auto_updater = AutoUpdater::get(cx)?;
    let mut version = auto_updater.read(cx).current_version.clone();
    // A built binary carries channel and commit in the build metadata; the release it came
    // from is tagged with the bare version.
    version.pre = semver::Prerelease::EMPTY;
    version.build = semver::BuildMetadata::EMPTY;
    Some(format!(
        "https://github.com/{RELEASE_REPO}/releases/tag/v{version}"
    ))
}

pub fn view_release_notes(_: &ViewReleaseNotes, cx: &mut App) -> Option<()> {
    let url = release_notes_url(cx)?;
    cx.open_url(&url);
    None
}

#[cfg(not(target_os = "windows"))]
struct InstallerDir(tempfile::TempDir);

#[cfg(not(target_os = "windows"))]
impl InstallerDir {
    async fn new() -> Result<Self> {
        Ok(Self(
            tempfile::Builder::new()
                .prefix("zed-auto-update")
                .tempdir()?,
        ))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(target_os = "windows")]
struct InstallerDir(PathBuf);

#[cfg(target_os = "windows")]
impl InstallerDir {
    async fn new() -> Result<Self> {
        let installer_dir = std::env::current_exe()?
            .parent()
            .context("No parent dir for Zode.exe")?
            .join("updates");
        if smol::fs::metadata(&installer_dir).await.is_ok() {
            smol::fs::remove_dir_all(&installer_dir).await?;
        }
        smol::fs::create_dir(&installer_dir).await?;
        Ok(Self(installer_dir))
    }

    fn path(&self) -> &Path {
        self.0.as_path()
    }
}

impl AutoUpdater {
    pub fn get(cx: &mut App) -> Option<Entity<Self>> {
        cx.default_global::<GlobalAutoUpdate>().0.clone()
    }

    fn new(
        current_version: Version,
        http_client: Arc<dyn HttpClient>,
        cx: &mut Context<Self>,
    ) -> Self {
        // On windows, executable files cannot be overwritten while they are
        // running, so we must wait to overwrite the application until quitting
        // or restarting. When quitting the app, we spawn the auto update helper
        // to finish the auto update process after Zed exits. When restarting
        // the app after an update, we use `set_restart_path` to run the auto
        // update helper instead of the app, so that it can overwrite the app
        // and then spawn the new binary.
        #[cfg(target_os = "windows")]
        let quit_subscription = Some(cx.on_app_quit(|_, _| finalize_auto_update_on_quit()));
        #[cfg(not(target_os = "windows"))]
        let quit_subscription = None;

        cx.on_app_restart(|this, _| {
            this.quit_subscription.take();
        })
        .detach();

        Self {
            status: AutoUpdateStatus::Idle,
            current_version,
            http_client,
            pending_poll: None,
            quit_subscription,
        }
    }

    pub fn poll(&mut self, cx: &mut Context<Self>) {
        if self.pending_poll.is_some() {
            return;
        }

        cx.notify();

        self.pending_poll = Some(cx.spawn(async move |this, cx| {
            let result = Self::update(this.upgrade()?, cx).await;
            this.update(cx, |this, cx| {
                this.pending_poll = None;
                if let Err(error) = result {
                    // Every check here is a deliberate press of a button, so every failure
                    // is something the person is standing there waiting to hear. Upstream
                    // also had a quiet path, for the hourly background poll that would
                    // otherwise shout at anyone who happened to be offline; there is no
                    // background poll in this fork, so there is nothing to be quiet for.
                    log::error!("update failed: error:{:?}", error);
                    this.status = AutoUpdateStatus::Errored {
                        error: Arc::new(error),
                    };
                    cx.notify();
                }
            })
            .ok()
        }));
    }

    pub fn current_version(&self) -> Version {
        self.current_version.clone()
    }

    pub fn status(&self) -> AutoUpdateStatus {
        self.status.clone()
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        if let AutoUpdateStatus::Idle = self.status {
            return false;
        }
        self.status = AutoUpdateStatus::Idle;
        cx.notify();
        true
    }

    /// Reads the newest published release and resolves the asset this platform installs.
    ///
    /// `/releases/latest` is what makes the release flow safe: it skips drafts *and*
    /// prereleases. A tagged release stays invisible to the updater until a person
    /// publishes it, and the rolling `nightly` prerelease is never offered as an update.
    ///
    /// Deliberately not `http_client::github::latest_github_release`: that helper reads the
    /// `/releases` list instead, and attaches an ambient `GITHUB_TOKEN` when one happens to
    /// be exported -- which would make unpublished drafts visible to whoever has a token in
    /// their shell. Nothing here sends a credential or an identifier.
    async fn fetch_latest_release(
        this: &Entity<Self>,
        os: &str,
        arch: &str,
        cx: &mut AsyncApp,
    ) -> Result<ReleaseAsset> {
        let wanted_asset = asset_name(os, arch)
            .with_context(|| format!("no release asset is published for {os} {arch}"))?;
        let http_client = this.read_with(cx, |this, _| this.http_client.clone());

        let body = github_get(&http_client, "releases/latest").await?;

        let release: LatestRelease = serde_json::from_slice(&body).with_context(|| {
            format!(
                "could not read the release payload: {:?}",
                String::from_utf8_lossy(&body)
            )
        })?;

        // Name both sides on a miss. This is the failure mode when the bundling scripts
        // rename an asset and this table is not updated with them, and knowing only one
        // half of the mismatch makes that a guessing game.
        let asset_url = release
            .assets
            .iter()
            .find(|asset| asset.name == wanted_asset)
            .map(|asset| asset.browser_download_url.clone())
            .with_context(|| {
                let published = release
                    .assets
                    .iter()
                    .map(|asset| asset.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "release {} publishes no {wanted_asset}; it has: [{published}]",
                    release.tag_name
                )
            })?;

        Ok(ReleaseAsset {
            version: release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name)
                .to_string(),
            url: asset_url,
            notes: release.body,
        })
    }

    /// The release notes for a specific version, as published on its release.
    ///
    /// Separate from the notes carried by [`ReleaseAsset`]: those belong to the newest
    /// release, while this answers "what changed in the version I am running".
    pub async fn fetch_release_notes(
        this: &Entity<Self>,
        version: &Version,
        cx: &mut AsyncApp,
    ) -> Result<Option<String>> {
        let http_client = this.read_with(cx, |this, _| this.http_client.clone());
        let body = github_get(&http_client, &format!("releases/tags/v{version}")).await?;
        let release: LatestRelease = serde_json::from_slice(&body).with_context(|| {
            format!(
                "could not read the release payload for v{version}: {:?}",
                String::from_utf8_lossy(&body)
            )
        })?;
        Ok(release.body)
    }

    async fn update(this: Entity<Self>, cx: &mut AsyncApp) -> Result<()> {
        let (client, installed_version, previous_status) = this.read_with(cx, |this, _| {
            (
                this.http_client.clone(),
                this.current_version.clone(),
                this.status.clone(),
            )
        });

        Self::check_dependencies()?;

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Checking;
            log::info!("Auto Update: checking for updates");
            cx.notify();
        });

        let fetched_release_data = Self::fetch_latest_release(&this, OS, ARCH, cx).await?;
        let fetched_version = fetched_release_data.clone().version;
        let newer_version = Self::check_if_fetched_version_is_newer(
            installed_version,
            fetched_version,
            previous_status.clone(),
        )?;

        let Some(newer_version) = newer_version else {
            this.update(cx, |this, cx| {
                let status = match previous_status {
                    AutoUpdateStatus::Updated { .. } => previous_status,
                    _ => AutoUpdateStatus::Idle,
                };
                this.status = status;
                cx.notify();
            });
            return Ok(());
        };

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Downloading {
                version: newer_version.clone(),
            };
            cx.notify();
        });

        let installer_dir = InstallerDir::new()
            .await
            .context("Failed to create installer dir")?;
        let target_path = Self::target_path(&installer_dir).await?;
        download_release(&target_path, fetched_release_data, client)
            .await
            .with_context(|| format!("Failed to download update to {}", target_path.display()))?;

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Installing {
                version: newer_version.clone(),
            };
            cx.notify();
        });

        let new_binary_path = Self::install_release(installer_dir, &target_path, cx)
            .await
            .with_context(|| format!("Failed to install update at: {}", target_path.display()))?;
        if let Some(new_binary_path) = new_binary_path {
            cx.update(|cx| cx.set_restart_path(new_binary_path));
        }

        this.update(cx, |this, cx| {
            this.set_should_show_update_notification(true, cx)
                .detach_and_log_err(cx);
            this.status = AutoUpdateStatus::Updated {
                version: newer_version,
            };
            cx.notify();
        });
        Ok(())
    }

    fn check_if_fetched_version_is_newer(
        installed_version: Version,
        fetched_version: String,
        status: AutoUpdateStatus,
    ) -> Result<Option<VersionCheckType>> {
        let parsed_fetched_version = fetched_version.parse::<Version>()?;

        // A finished update records the version it installed. Compare against that rather
        // than the running binary: the new build is already on disk while this process is
        // still the old one, so comparing against `installed_version` would keep offering
        // the same update until someone restarts.
        if let AutoUpdateStatus::Updated {
            version: VersionCheckType::Semantic(cached_version),
        } = status
        {
            return Self::check_if_fetched_version_is_newer_non_nightly(
                cached_version,
                parsed_fetched_version,
            );
        }

        Self::check_if_fetched_version_is_newer_non_nightly(
            installed_version,
            parsed_fetched_version,
        )
    }

    fn check_dependencies() -> Result<()> {
        #[cfg(target_os = "linux")]
        if which::which("rsync").is_err() {
            let install_hint = linux_rsync_install_hint();
            return Err(MissingDependencyError(format!(
                "rsync is required for auto-updates but is not installed. {install_hint}"
            ))
            .into());
        }

        #[cfg(target_os = "macos")]
        anyhow::ensure!(
            which::which("rsync").is_ok(),
            "Could not auto-update because the required rsync utility was not found."
        );

        Ok(())
    }

    async fn target_path(installer_dir: &InstallerDir) -> Result<PathBuf> {
        let filename = match OS {
            "macos" => anyhow::Ok("Zed.dmg"),
            "linux" => Ok("zed.tar.gz"),
            "windows" => Ok("Zed.exe"),
            unsupported_os => anyhow::bail!("not supported: {unsupported_os}"),
        }?;

        Ok(installer_dir.path().join(filename))
    }

    async fn install_release(
        installer_dir: InstallerDir,
        target_path: &Path,
        cx: &AsyncApp,
    ) -> Result<Option<PathBuf>> {
        #[cfg(test)]
        if let Some(test_install) =
            cx.try_read_global::<tests::InstallOverride, _>(|g, _| g.0.clone())
        {
            return test_install(target_path, cx);
        }
        match OS {
            "macos" => install_release_macos(&installer_dir, target_path, cx).await,
            "linux" => install_release_linux(&installer_dir, target_path, cx).await,
            "windows" => install_release_windows(target_path).await,
            unsupported_os => anyhow::bail!("not supported: {unsupported_os}"),
        }
    }

    fn check_if_fetched_version_is_newer_non_nightly(
        mut installed_version: Version,
        fetched_version: Version,
    ) -> Result<Option<VersionCheckType>> {
        // For non-nightly releases, ignore build and pre-release fields as they're not provided by our endpoints right now.
        installed_version.pre = semver::Prerelease::EMPTY;
        installed_version.build = semver::BuildMetadata::EMPTY;
        let should_download = fetched_version > installed_version;
        let newer_version = should_download.then(|| VersionCheckType::Semantic(fetched_version));
        Ok(newer_version)
    }

    pub fn set_should_show_update_notification(
        &self,
        should_show: bool,
        cx: &App,
    ) -> Task<Result<()>> {
        let kvp = KeyValueStore::global(cx);
        cx.background_spawn(async move {
            if should_show {
                kvp.write_kvp(
                    SHOULD_SHOW_UPDATE_NOTIFICATION_KEY.to_string(),
                    "".to_string(),
                )
                .await?;
            } else {
                kvp.delete_kvp(SHOULD_SHOW_UPDATE_NOTIFICATION_KEY.to_string())
                    .await?;
            }
            Ok(())
        })
    }

    pub fn should_show_update_notification(&self, cx: &App) -> Task<Result<bool>> {
        let kvp = KeyValueStore::global(cx);
        cx.background_spawn(async move {
            Ok(kvp.read_kvp(SHOULD_SHOW_UPDATE_NOTIFICATION_KEY)?.is_some())
        })
    }
}

async fn download_release(
    target_path: &Path,
    release: ReleaseAsset,
    client: Arc<dyn HttpClient>,
) -> Result<()> {
    let mut target_file = File::create(&target_path).await?;

    let mut response = client.get(&release.url, Default::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to download update: {:?}",
        response.status()
    );
    smol::io::copy(response.body_mut(), &mut target_file).await?;
    log::info!("downloaded update. path:{:?}", target_path);

    Ok(())
}

async fn install_release_linux(
    temp_dir: &InstallerDir,
    downloaded_tar_gz: &Path,
    cx: &AsyncApp,
) -> Result<Option<PathBuf>> {
    let channel = cx.update(|cx| ReleaseChannel::global(cx).dev_name());
    let home_dir = PathBuf::from(env::var("HOME").context("no HOME env var set")?);
    let running_app_path = cx.update(|cx| cx.app_path())?;

    let extracted = temp_dir.path().join("zed");
    fs::create_dir_all(&extracted)
        .await
        .context("failed to create directory into which to extract update")?;

    let mut cmd = new_command("tar");
    cmd.arg("-xzf")
        .arg(&downloaded_tar_gz)
        .arg("-C")
        .arg(&extracted);
    let output = cmd
        .output()
        .await
        .with_context(|| "failed to extract: {cmd}")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to extract {:?} to {:?}: {:?}",
        downloaded_tar_gz,
        extracted,
        String::from_utf8_lossy(&output.stderr)
    );

    let suffix = if channel != "stable" {
        format!("-{}", channel)
    } else {
        String::default()
    };
    let app_folder_name = format!("zed{}.app", suffix);

    let from = extracted.join(&app_folder_name);
    let mut to = home_dir.join(".local");

    let expected_suffix = format!("{}/libexec/zed-editor", app_folder_name);

    if let Some(prefix) = running_app_path
        .to_str()
        .and_then(|str| str.strip_suffix(&expected_suffix))
    {
        to = PathBuf::from(prefix);
    }

    let mut cmd = new_command("rsync");
    cmd.args(["-av", "--delete"]).arg(&from).arg(&to);
    let output = cmd
        .output()
        .await
        .with_context(|| "failed to rsync: {cmd}")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to copy Zed update from {:?} to {:?}: {:?}",
        from,
        to,
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(Some(to.join(expected_suffix)))
}

async fn install_release_macos(
    temp_dir: &InstallerDir,
    downloaded_dmg: &Path,
    cx: &AsyncApp,
) -> Result<Option<PathBuf>> {
    let running_app_path = cx.update(|cx| cx.app_path())?;
    let running_app_filename = running_app_path
        .file_name()
        .with_context(|| format!("invalid running app path {running_app_path:?}"))?;

    let mount_path = temp_dir.path().join("Zed");
    let mut mounted_app_path: OsString = mount_path.join(running_app_filename).into();

    mounted_app_path.push("/");
    let mut cmd = new_command("hdiutil");
    cmd.args(["attach", "-nobrowse"])
        .arg(&downloaded_dmg)
        .arg("-mountroot")
        .arg(temp_dir.path());
    let output = cmd
        .output()
        .await
        .with_context(|| "failed to mount: {cmd}")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to mount: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Create an MacOsUnmounter that will be dropped (and thus unmount the disk) when this function exits
    let _unmounter = MacOsUnmounter {
        mount_path: mount_path.clone(),
        background_executor: cx.background_executor(),
    };

    let mut cmd = new_command("rsync");
    cmd.args(["-av", "--delete", "--exclude", "Icon?"])
        .arg(&mounted_app_path)
        .arg(&running_app_path);
    let output = cmd
        .output()
        .await
        .with_context(|| "failed to rsync: {cmd}")?;

    anyhow::ensure!(
        output.status.success(),
        "failed to copy app: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(None)
}

#[cfg(target_os = "windows")]
async fn cleanup_windows() -> Result<()> {
    let parent = std::env::current_exe()?
        .parent()
        .context("No parent dir for Zed.exe")?
        .to_owned();

    // keep in sync with crates/auto_update_helper/src/updater.rs
    _ = smol::fs::remove_dir(parent.join("updates")).await;
    _ = smol::fs::remove_dir(parent.join("install")).await;
    _ = smol::fs::remove_dir(parent.join("old")).await;

    Ok(())
}

async fn install_release_windows(downloaded_installer: &Path) -> Result<Option<PathBuf>> {
    let mut cmd = new_command(downloaded_installer);
    cmd.arg("/verysilent")
        .arg("/update=true")
        .arg("!desktopicon")
        .arg("!quicklaunchicon");
    let output = cmd.output().await?;
    anyhow::ensure!(
        output.status.success(),
        "failed to start installer: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    // We return the path to the update helper program, because it will
    // perform the final steps of the update process, copying the new binary,
    // deleting the old one, and launching the new binary.
    let helper_path = std::env::current_exe()?
        .parent()
        .context("No parent dir for Zed.exe")?
        .join("tools")
        .join("auto_update_helper.exe");
    Ok(Some(helper_path))
}

pub async fn finalize_auto_update_on_quit() {
    let Some(installer_path) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("updates")))
    else {
        return;
    };

    // The installer will create a flag file after it finishes updating
    let flag_file = installer_path.join("versions.txt");
    if flag_file.exists()
        && let Some(helper) = installer_path
            .parent()
            .map(|p| p.join("tools").join("auto_update_helper.exe"))
    {
        let mut command = util::command::new_command(helper);
        command.arg("--launch");
        command.arg("false");
        if let Ok(mut cmd) = command.spawn() {
            _ = cmd.status().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::channel::oneshot;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};
    use std::{
        rc::Rc,
        sync::{
            Arc,
            atomic::{self, AtomicBool},
        },
        time::Duration,
    };
    use tempfile::tempdir;

    #[ctor::ctor]
    fn init_logger() {
        zlog::init_test();
    }

    use super::*;

    pub(super) struct InstallOverride(pub Rc<dyn Fn(&Path, &AsyncApp) -> Result<Option<PathBuf>>>);
    impl Global for InstallOverride {}

    #[gpui::test]
    async fn test_auto_update_downloads(cx: &mut TestAppContext) {
        cx.background_executor.allow_parking();
        zlog::init_test();
        let release_available = Arc::new(AtomicBool::new(false));

        let (dmg_tx, dmg_rx) = oneshot::channel::<String>();

        // Whichever platform the suite runs on, the updater asks for that platform's asset.
        // Deriving the fixture from the same table the code reads keeps this test honest on
        // macOS and on both Linux architectures in CI.
        let asset = asset_name(OS, ARCH).expect("this platform must have a published asset");
        let latest_path = format!("/repos/{RELEASE_REPO}/releases/latest");

        cx.update(|cx| {
            settings::init(cx);

            let current_version = semver::Version::new(0, 100, 0);
            release_channel::init_test(current_version, ReleaseChannel::Stable, cx);

            let release_available = Arc::clone(&release_available);
            let dmg_rx = Arc::new(parking_lot::Mutex::new(Some(dmg_rx)));
            let fake_client_http = FakeHttpClient::create(move |req| {
                let release_available = release_available.load(atomic::Ordering::Relaxed);
                let dmg_rx = dmg_rx.clone();
                let latest_path = latest_path.clone();
                async move {
                    if latest_path == req.uri().path() {
                        let (version, download) = if release_available {
                            ("0.100.1", "https://test.example/new-download")
                        } else {
                            ("0.100.0", "https://test.example/old-download")
                        };
                        let body = format!(
                            r#"{{"tag_name":"v{version}","body":"notes for {version}","assets":[{{"name":"{asset}","browser_download_url":"{download}"}}]}}"#
                        );
                        return Ok(Response::builder().status(200).body(body.into()).unwrap());
                    } else if req.uri().path() == "/new-download" {
                        return Ok(Response::builder()
                            .status(200)
                            .body({
                                let dmg_rx = dmg_rx.lock().take().unwrap();
                                dmg_rx.await.unwrap().into()
                            })
                            .unwrap());
                    }
                    Ok(Response::builder().status(404).body("".into()).unwrap())
                }
            });
            crate::init(fake_client_http, cx);
        });

        let auto_updater = cx.update(|cx| AutoUpdater::get(cx).expect("auto updater should exist"));

        cx.background_executor.run_until_parked();

        // Nothing happens until it is asked to: there is no background poll in this fork.
        auto_updater.read_with(cx, |updater, _| {
            assert_eq!(updater.status(), AutoUpdateStatus::Idle);
            assert_eq!(updater.current_version(), semver::Version::new(0, 100, 0));
        });

        // Asked while the newest release is the one already installed: it must come back to
        // Idle rather than offering an update, and must not have downloaded anything.
        auto_updater.update(cx, |updater, cx| updater.poll(cx));
        loop {
            cx.background_executor.timer(Duration::from_millis(0)).await;
            cx.run_until_parked();
            if auto_updater.read_with(cx, |updater, _| updater.pending_poll.is_none()) {
                break;
            }
        }
        auto_updater.read_with(cx, |updater, _| {
            assert_eq!(updater.status(), AutoUpdateStatus::Idle);
        });

        release_available.store(true, atomic::Ordering::SeqCst);
        auto_updater.update(cx, |updater, cx| updater.poll(cx));
        cx.background_executor.run_until_parked();

        loop {
            cx.background_executor.timer(Duration::from_millis(0)).await;
            cx.run_until_parked();
            let status = auto_updater.read_with(cx, |updater, _| updater.status());
            if !matches!(status, AutoUpdateStatus::Idle) {
                break;
            }
        }
        let status = auto_updater.read_with(cx, |updater, _| updater.status());
        assert_eq!(
            status,
            AutoUpdateStatus::Downloading {
                version: VersionCheckType::Semantic(semver::Version::new(0, 100, 1))
            }
        );

        dmg_tx.send("<fake-zed-update>".to_owned()).unwrap();

        let tmp_dir = Arc::new(tempdir().unwrap());

        cx.update(|cx| {
            let tmp_dir = tmp_dir.clone();
            cx.set_global(InstallOverride(Rc::new(move |target_path, _cx| {
                let tmp_dir = tmp_dir.clone();
                let dest_path = tmp_dir.path().join("zed");
                std::fs::copy(&target_path, &dest_path)?;
                Ok(Some(dest_path))
            })));
        });

        loop {
            cx.background_executor.timer(Duration::from_millis(0)).await;
            cx.run_until_parked();
            let status = auto_updater.read_with(cx, |updater, _| updater.status());
            if !matches!(status, AutoUpdateStatus::Downloading { .. }) {
                break;
            }
        }
        let status = auto_updater.read_with(cx, |updater, _| updater.status());
        assert_eq!(
            status,
            AutoUpdateStatus::Updated {
                version: VersionCheckType::Semantic(semver::Version::new(0, 100, 1))
            }
        );
        let will_restart = cx.expect_restart();
        cx.update(|cx| cx.restart());
        let path = will_restart.await.unwrap().unwrap();
        assert_eq!(path, tmp_dir.path().join("zed"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "<fake-zed-update>");
    }

    #[test]
    fn asset_name_covers_every_published_platform() {
        // The six names the release workflow's validate_release_assets job asserts.
        for (os, arch, expected) in [
            ("macos", "aarch64", "Zode-aarch64.dmg"),
            ("macos", "x86_64", "Zode-x86_64.dmg"),
            ("linux", "aarch64", "zode-linux-aarch64.tar.gz"),
            ("linux", "x86_64", "zode-linux-x86_64.tar.gz"),
            ("windows", "aarch64", "Zode-aarch64.exe"),
            ("windows", "x86_64", "Zode-x86_64.exe"),
        ] {
            assert_eq!(asset_name(os, arch), Some(expected), "{os} {arch}");
        }
    }

    #[test]
    fn asset_name_is_none_for_a_platform_with_no_release() {
        assert_eq!(asset_name("freebsd", "x86_64"), None);
        assert_eq!(asset_name("macos", "riscv64"), None);
    }

    #[test]
    fn test_stable_does_not_update_when_fetched_version_is_not_higher() {
        let installed_version = semver::Version::new(1, 0, 0);
        let status = AutoUpdateStatus::Idle;
        let fetched_version = semver::Version::new(1, 0, 0);

        let newer_version = AutoUpdater::check_if_fetched_version_is_newer(
            installed_version,
            fetched_version.to_string(),
            status,
        );

        assert_eq!(newer_version.unwrap(), None);
    }

    #[test]
    fn test_stable_does_update_when_fetched_version_is_higher() {
        let installed_version = semver::Version::new(1, 0, 0);
        let status = AutoUpdateStatus::Idle;
        let fetched_version = semver::Version::new(1, 0, 1);

        let newer_version = AutoUpdater::check_if_fetched_version_is_newer(
            installed_version,
            fetched_version.to_string(),
            status,
        );

        assert_eq!(
            newer_version.unwrap(),
            Some(VersionCheckType::Semantic(fetched_version))
        );
    }

    #[test]
    fn test_stable_does_not_update_when_fetched_version_is_not_higher_than_cached() {
        let installed_version = semver::Version::new(1, 0, 0);
        let status = AutoUpdateStatus::Updated {
            version: VersionCheckType::Semantic(semver::Version::new(1, 0, 1)),
        };
        let fetched_version = semver::Version::new(1, 0, 1);

        let newer_version = AutoUpdater::check_if_fetched_version_is_newer(
            installed_version,
            fetched_version.to_string(),
            status,
        );

        assert_eq!(newer_version.unwrap(), None);
    }

    #[test]
    fn test_stable_does_update_when_fetched_version_is_higher_than_cached() {
        let installed_version = semver::Version::new(1, 0, 0);
        let status = AutoUpdateStatus::Updated {
            version: VersionCheckType::Semantic(semver::Version::new(1, 0, 1)),
        };
        let fetched_version = semver::Version::new(1, 0, 2);

        let newer_version = AutoUpdater::check_if_fetched_version_is_newer(
            installed_version,
            fetched_version.to_string(),
            status,
        );

        assert_eq!(
            newer_version.unwrap(),
            Some(VersionCheckType::Semantic(fetched_version))
        );
    }
}
