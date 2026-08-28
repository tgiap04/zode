use gh_workflow::*;
use serde_json::Value;

use crate::tasks::workflows::{
    runners::Platform,
    vars::{self, StepOutput},
};

pub(crate) fn use_clang(job: Job) -> Job {
    job.add_env(Env::new("CC", "clang"))
        .add_env(Env::new("CXX", "clang++"))
}

const SCCACHE_R2_BUCKET: &str = "sccache-zed";

pub(crate) const BASH_SHELL: &str = "bash -euxo pipefail {0}";
// https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepsshell
pub const PWSH_SHELL: &str = "pwsh";

pub(crate) struct Nextest(Step<Run>);

pub(crate) fn cargo_nextest(platform: Platform) -> Nextest {
    Nextest(named::run!(
        platform,
        "cargo nextest run --workspace --no-fail-fast --no-tests=warn",
    ))
}

impl Nextest {
    #[allow(dead_code)]
    pub(crate) fn with_filter_expr(mut self, filter_expr: &str) -> Self {
        if let Some(nextest_command) = self.0.value.run.as_mut() {
            nextest_command.push_str(&format!(r#" -E "{filter_expr}""#));
        }
        self
    }

    pub(crate) fn with_changed_packages_filter(mut self, orchestrate_job: &str) -> Self {
        if let Some(nextest_command) = self.0.value.run.as_mut() {
            nextest_command.push_str(&format!(
                r#"${{{{ needs.{orchestrate_job}.outputs.changed_packages && format(' -E "{{0}}"', needs.{orchestrate_job}.outputs.changed_packages) || '' }}}}"#
            ));
        }
        self
    }
}

impl From<Nextest> for Step<Run> {
    fn from(value: Nextest) -> Self {
        value.0
    }
}

#[derive(Default)]
enum FetchDepth {
    #[default]
    Shallow,
    Full,
    Custom(serde_json::Value),
}

#[derive(Default)]
pub(crate) struct CheckoutStep {
    fetch_depth: FetchDepth,
    name: Option<String>,
    token: Option<String>,
    path: Option<String>,
    repository: Option<String>,
    ref_: Option<String>,
}

impl CheckoutStep {
    pub fn with_full_history(mut self) -> Self {
        self.fetch_depth = FetchDepth::Full;
        self
    }

    pub fn with_custom_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self
    }

    pub fn with_custom_fetch_depth(mut self, fetch_depth: impl Into<Value>) -> Self {
        self.fetch_depth = FetchDepth::Custom(fetch_depth.into());
        self
    }

    /// Sets `fetch-depth` to `2` on the main branch and `350` on all other branches.
    pub fn with_deep_history_on_non_main(self) -> Self {
        self.with_custom_fetch_depth("${{ github.ref == 'refs/heads/main' && 2 || 350 }}")
    }

    pub fn with_token(mut self, token: &StepOutput) -> Self {
        self.token = Some(token.to_string());
        self
    }

    /// For a token that is not a step output -- a secret read straight from the
    /// workflow context, rather than one minted by an earlier step.
    pub fn with_token_expression(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.path = Some(path.to_string());
        self
    }

    pub fn with_repository(mut self, repository: &str) -> Self {
        self.repository = Some(repository.to_string());
        self
    }

    pub fn with_ref(mut self, ref_: impl ToString) -> Self {
        self.ref_ = Some(ref_.to_string());
        self
    }
}

impl From<CheckoutStep> for Step<Use> {
    fn from(value: CheckoutStep) -> Self {
        Step::new(value.name.unwrap_or("steps::checkout_repo".to_string()))
            .uses(
                "actions",
                "checkout",
                "93cb6efe18208431cddfb8368fd83d5badbf9bfd", // v5.0.1
            )
            // prevent checkout action from running `git clean -ffdx` which
            // would delete the target directory
            .add_with(("clean", false))
            .map(|step| match value.fetch_depth {
                FetchDepth::Shallow => step,
                FetchDepth::Full => step.add_with(("fetch-depth", 0)),
                FetchDepth::Custom(depth) => step.add_with(("fetch-depth", depth)),
            })
            .when_some(value.path, |step, path| step.add_with(("path", path)))
            .when_some(value.repository, |step, repository| {
                step.add_with(("repository", repository))
            })
            .when_some(value.ref_, |step, ref_| step.add_with(("ref", ref_)))
            .when_some(value.token, |step, token| step.add_with(("token", token)))
    }
}

pub fn checkout_repo() -> CheckoutStep {
    CheckoutStep::default()
}

pub fn setup_pnpm() -> Step<Use> {
    named::uses!(
        "pnpm",
        "action-setup",
        "fe02b34f77f8bc703788d5817da081398fad5dd2", // v4.0.0
    )
    .add_with(("version", "9"))
}

pub fn setup_node() -> Step<Use> {
    named::uses!(
        "actions",
        "setup-node",
        "49933ea5288caeca8642d1e84afbd3f7d6820020", // v4
    )
    .add_with(("node-version", "20"))
}

pub fn prettier() -> Step<Run> {
    named::bash!("./script/prettier")
}

pub fn cargo_fmt() -> Step<Run> {
    named::bash!("cargo fmt --all -- --check")
}

pub fn cargo_install_nextest() -> Step<Use> {
    named::uses!(
        "taiki-e",
        "install-action",
        "921e2c9f7148d7ba14cd819f417db338f63e733c", // nextest
    )
}

pub fn setup_cargo_config(platform: Platform) -> Step<Run> {
    match platform {
        Platform::Windows => named::pwsh!(indoc::indoc! {r#"
            New-Item -ItemType Directory -Path "./../.cargo" -Force
            Copy-Item -Path "./.cargo/ci-config.toml" -Destination "./../.cargo/config.toml"
        "#}),

        Platform::Linux | Platform::Mac => named::bash!(indoc::indoc! {r#"
            mkdir -p ./../.cargo
            cp ./.cargo/ci-config.toml ./../.cargo/config.toml
        "#}),
    }
}

pub fn cleanup_cargo_config(platform: Platform) -> Step<Run> {
    let step = match platform {
        Platform::Windows => named::pwsh!(indoc::indoc! {r#"
            Remove-Item -Recurse -Path "./../.cargo" -Force -ErrorAction SilentlyContinue
        "#}),
        Platform::Linux | Platform::Mac => named::bash!(indoc::indoc! {r#"
            rm -rf ./../.cargo
        "#}),
    };

    step.if_condition(Expression::new("always()"))
}

pub fn clear_target_dir_if_large(platform: Platform) -> Step<Run> {
    match platform {
        Platform::Windows => named::pwsh!("./script/clear-target-dir-if-larger-than.ps1 350 200"),
        Platform::Linux => named::bash!("./script/clear-target-dir-if-larger-than 350 200"),
        Platform::Mac => named::bash!("./script/clear-target-dir-if-larger-than 350 200"),
    }
}

/// Brings a bare distro image up to the minimum a job needs before `actions/checkout`
/// runs. Only for container jobs — a hosted runner image already has all of this.
///
/// `git` is not optional: without it `actions/checkout` silently falls back to
/// downloading a source tarball and leaves no `.git` behind, and
/// `script/bundle-linux` runs `git rev-parse HEAD` — so the job would die at packaging
/// time, after paying for the whole build. `ca-certificates` because a bare `ubuntu`
/// image ships none, and both the rustup bootstrap and every crates.io fetch are HTTPS.
///
/// The `$GITHUB_PATH` line is what makes `cargo` reachable: `script/linux` installs
/// rustup into `$HOME/.cargo/bin` but only edits shell rc files, and the `bash -e` that
/// GitHub runs each step with is non-login, so it never sources them. Written before the
/// directory exists, which is harmless — `$GITHUB_PATH` applies to later steps only.
///
/// `safe.directory` is ours to set because `actions/checkout` will not do it for us: it
/// writes that entry under a `HOME` it overrides for the duration of the action and then
/// throws away, so nothing survives into the steps that follow. The workspace is owned by
/// the runner's uid on the host while this container is root, so the first `git` call in
/// `script/bundle-linux` -- `git rev-parse HEAD`, at packaging time, after the whole build
/// is paid for — dies with "detected dubious ownership". A hosted runner never hits this,
/// because there the job runs as the uid that owns the checkout.
///
/// `--system` rather than `--global` deliberately: `/etc/gitconfig` is read whatever `HOME`
/// happens to be, and `HOME` is demonstrably not stable here — rustup warns in this very
/// job that it differs from the euid-obtained one.
pub fn bootstrap_container() -> Step<Run> {
    named::bash!(indoc::indoc! {r#"
        apt-get update
        apt-get install -y --no-install-recommends ca-certificates curl git
        git config --system --add safe.directory "$GITHUB_WORKSPACE"
        echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
    "#})
}

/// Reclaims space on the runner before a bundle build. Hosted runners document 14 GB of
/// free disk, against a release `target/` dir that runs to tens of GB.
///
/// The trailing `df`/`Get-PSDrive` is load-bearing for tuning: it puts the real number in
/// the job log so the next adjustment is made against a measurement, not a guess.
///
/// The Linux arm reclaims far less than the other two, because the Linux bundle jobs run
/// inside a container (see `run_bundling::bundle_linux`): there is no `sudo` and no
/// `docker` in the image, and the host directories the other platforms delete
/// (`/usr/share/dotnet`, `/opt/ghc`, the android SDK) are not mounted into it. `/__t` is,
/// being the host tool cache, so CodeQL is still worth removing. If a run ever fails on
/// ENOSPC the fix is to mount the host root into the container (`volumes: ["/:/host"]`)
/// and delete through that, not to move the job back out of the container.
pub fn free_disk_space(platform: Platform) -> Step<Run> {
    match platform {
        Platform::Linux => named::bash!(indoc::indoc! {r#"
            rm -rf /__t/CodeQL || true
            df -h /
        "#}),
        // No equivalent windfall exists here, which makes the 7 GB / 14 GB arm64 mac
        // runner the likeliest of the six targets to fail first.
        Platform::Mac => named::bash!(indoc::indoc! {r#"
            sudo rm -rf /Applications/Xcode_15*.app
            df -h /
        "#}),
        Platform::Windows => named::pwsh!(indoc::indoc! {r#"
            Remove-Item -Recurse -Force "C:\Android" -ErrorAction SilentlyContinue
            Get-PSDrive -PSProvider FileSystem | Format-Table -AutoSize
        "#}),
    }
}

/// Lifts Windows' 260-character path limit, which a cargo git checkout crosses:
///
/// ```text
/// path too long: 'C:/Users/runneradmin/.cargo/git/checkouts/python-environment-tools-.../
///   python-fastjsonschema-2.16.2-py310hca03da5_0.json'; class=Filesystem (30)
/// ```
///
/// That path is 261 characters. Upstream never sees it because their self-hosted runner
/// puts CARGO_HOME on a dev drive at a drive-letter root, which is 17 characters shorter
/// than the hosted runner's profile directory. Shortening the prefix would clear the limit
/// by a single character, so the limit is removed instead.
pub fn windows_enable_long_paths() -> Step<Run> {
    named::pwsh!(indoc::indoc! {r#"
        git config --global core.longpaths true
        New-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" `
            -Name "LongPathsEnabled" -Value 1 -PropertyType DWORD -Force | Out-Null
    "#})
}

pub fn clippy(platform: Platform, target: Option<&str>) -> Step<Run> {
    match platform {
        Platform::Windows => named::pwsh!("./script/clippy.ps1"),
        _ => match target {
            Some(target) => named::bash!(format!("./script/clippy --target {target}")),
            None => named::bash!("./script/clippy"),
        },
    }
}

pub fn install_rustup_target(target: &str) -> Step<Run> {
    named::bash!(format!("rustup target add {target}"))
}

/// Caches only the crate sources, never `target/`: the GitHub Actions cache is capped at
/// 10 GB per repository and a release `target/` dir is many times that, so caching it
/// would thrash rather than help. Cold compiles are the accepted cost of free runners.
pub fn cache_rust_dependencies_namespace() -> Step<Use> {
    named::uses!(
        "actions",
        "cache",
        "0057852bfaa89a56745cba8c7296529d2fc39830",
    )
    .add_with(("path", "~/.cargo/registry\n~/.cargo/git"))
    .add_with((
        "key",
        "cargo-${{ runner.os }}-${{ runner.arch }}-${{ hashFiles('**/Cargo.lock') }}",
    ))
    .add_with(("restore-keys", "cargo-${{ runner.os }}-${{ runner.arch }}-"))
}

pub fn setup_sccache(platform: Platform) -> Step<Run> {
    let step = match platform {
        Platform::Windows => named::pwsh!("./script/setup-sccache.ps1"),
        Platform::Linux | Platform::Mac => named::bash!("./script/setup-sccache"),
    };
    step.add_env(("R2_ACCOUNT_ID", vars::R2_ACCOUNT_ID))
        .add_env(("R2_ACCESS_KEY_ID", vars::R2_ACCESS_KEY_ID))
        .add_env(("R2_SECRET_ACCESS_KEY", vars::R2_SECRET_ACCESS_KEY))
        .add_env(("SCCACHE_BUCKET", SCCACHE_R2_BUCKET))
}

pub fn show_sccache_stats(platform: Platform) -> Step<Run> {
    match platform {
        // Use $env:RUSTC_WRAPPER (absolute path) because GITHUB_PATH changes
        // don't take effect until the next step in PowerShell.
        // Check if RUSTC_WRAPPER is set first (it won't be for fork PRs without secrets).
        Platform::Windows => {
            named::pwsh!("if ($env:RUSTC_WRAPPER) { & $env:RUSTC_WRAPPER --show-stats }; exit 0")
        }
        Platform::Linux | Platform::Mac => named::bash!("sccache --show-stats || true"),
    }
}

/// Bounded because this wedged once for 4h27m inside `apt-get update`, with the 360-minute
/// job ceiling as its only backstop. Installing the dependencies takes a couple of minutes,
/// so 20 turns an apt mirror stall into a quick, obvious failure that the next run clears.
pub fn setup_linux() -> Step<Run> {
    named::bash!("./script/linux").timeout_minutes(20u32)
}

/// Fetches the WASI SDK before anything needs it.
///
/// `extension_host`'s `test_extension_store_with_test_extension` compiles a real
/// extension, and compiling a grammar needs this SDK. Without this step the
/// *test* downloads it, inside its own 300s nextest budget and with no retry, so
/// a slow GitHub release turns into a failed test rather than a slow one -- which
/// is what took `run_tests_mac` down while `run_tests_linux` passed, Linux being
/// the only platform that already ran this.
///
/// Must come after `clear_target_dir_if_large`: the SDK lands in `target/`, and
/// clearing that afterwards would take it with it.
pub(crate) fn download_wasi_sdk() -> Step<Run> {
    named::bash!("./script/download-wasi-sdk")
}

pub(crate) fn install_linux_dependencies(job: Job) -> Job {
    job.add_step(setup_linux()).add_step(download_wasi_sdk())
}

pub fn script(name: &str) -> Step<Run> {
    if name.ends_with(".ps1") {
        Step::new(name).run(name).shell(PWSH_SHELL)
    } else {
        Step::new(name).run(name)
    }
}

pub struct NamedJob<J: JobType = RunJob> {
    pub name: String,
    pub job: Job<J>,
}

// impl NamedJob {
//     pub fn map(self, f: impl FnOnce(Job) -> Job) -> Self {
//         NamedJob {
//             name: self.name,
//             job: f(self.job),
//         }
//     }
// }

// Kept rather than removed: it still stops the workflows from running on someone else's
// fork and burning their quota.
pub(crate) const DEFAULT_REPOSITORY_OWNER_GUARD: &str = "(github.repository_owner == 'tgiap04')";

pub fn repository_owner_guard_expression(trigger_always: bool) -> Expression {
    Expression::new(format!(
        "{}{}",
        DEFAULT_REPOSITORY_OWNER_GUARD,
        trigger_always.then_some(" && always()").unwrap_or_default()
    ))
}

pub trait CommonJobConditions: Sized {
    fn with_repository_owner_guard(self) -> Self;
}

impl CommonJobConditions for Job {
    fn with_repository_owner_guard(self) -> Self {
        self.cond(repository_owner_guard_expression(false))
    }
}

pub(crate) fn release_job(deps: &[&NamedJob]) -> Job {
    dependant_job(deps)
        .with_repository_owner_guard()
        .timeout_minutes(RELEASE_JOB_TIMEOUT_MINUTES)
}

/// Lets a job create a release, attach an asset to one, or move a tag.
///
/// Upstream declares this nowhere, because an organisation-wide setting hands every
/// workflow a writable token. A plain repository gets GitHub's own default instead --
/// `default_workflow_permissions: read` -- under which `gh release create` fails with
/// `HTTP 403: Resource not accessible by integration` and nothing in the job output
/// says the word "permission". Asked for per job rather than per workflow, so a test or
/// bundle job never carries write it has no use for.
pub(crate) fn writes_to_releases(job: Job) -> Job {
    job.permissions(Permissions::default().contents(Level::Write))
}

/// GitHub's own hard ceiling for a hosted job. Upstream used 60, which is generous on a
/// 32-core machine and fatal on a 4-core one: a cold Zode build gets cut at minute 60 and
/// looks exactly like a hang.
pub(crate) const RELEASE_JOB_TIMEOUT_MINUTES: u32 = 360;

pub(crate) fn dependant_job(deps: &[&NamedJob]) -> Job {
    let job = Job::default();
    if deps.len() > 0 {
        job.needs(deps.iter().map(|j| j.name.clone()).collect::<Vec<_>>())
    } else {
        job
    }
}

impl FluentBuilder for Job {}
impl FluentBuilder for Workflow {}
impl FluentBuilder for Input {}
impl<T> FluentBuilder for Step<T> {}

/// A helper trait for building complex objects with imperative conditionals in a fluent style.
/// Copied from GPUI to avoid adding GPUI as dependency
/// todo(ci) just put this in gh-workflow
#[allow(unused)]
pub trait FluentBuilder {
    /// Imperatively modify self with the given closure.
    fn map<U>(self, f: impl FnOnce(Self) -> U) -> U
    where
        Self: Sized,
    {
        f(self)
    }

    /// Conditionally modify self with the given closure.
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self
    where
        Self: Sized,
    {
        self.map(|this| if condition { then(this) } else { this })
    }

    /// Conditionally modify self with the given closure.
    fn when_else(
        self,
        condition: bool,
        then: impl FnOnce(Self) -> Self,
        else_fn: impl FnOnce(Self) -> Self,
    ) -> Self
    where
        Self: Sized,
    {
        self.map(|this| if condition { then(this) } else { else_fn(this) })
    }

    /// Conditionally unwrap and modify self with the given closure, if the given option is Some.
    fn when_some<T>(self, option: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self
    where
        Self: Sized,
    {
        self.map(|this| {
            if let Some(value) = option {
                then(this, value)
            } else {
                this
            }
        })
    }
    /// Conditionally unwrap and modify self with the given closure, if the given option is None.
    fn when_none<T>(self, option: &Option<T>, then: impl FnOnce(Self) -> Self) -> Self
    where
        Self: Sized,
    {
        self.map(|this| if option.is_some() { this } else { then(this) })
    }
}

// (janky) helper to generate steps with a name that corresponds
// to the name of the calling function.
pub mod named {
    use super::*;

    /// The path of the function this expands in, from just after `workflows`.
    ///
    /// Compile-time, and that is the whole point of it. This used to be read
    /// from a runtime backtrace, which needs two things codegen does not
    /// promise: the frame must survive, and its name must be recoverable. CI
    /// lowers `[profile.dev] debug` to `"limited"` in `.cargo/ci-config.toml`,
    /// and at that level a private function with a single caller has no
    /// resolvable name -- so four jobs in `release.yml` came out under a mangled
    /// hash (`h1b546bfb5948ff19` and friends) instead of their own names. Every
    /// local run passed, because a developer's build keeps full debug info.
    ///
    /// `type_name` of a function item declared right here gives the enclosing
    /// path plus `::probe`, which is a fact about the source rather than about
    /// the build.
    #[macro_export]
    macro_rules! __workflow_path {
        () => {{
            fn probe() {}
            fn path_of<T>(_: T) -> &'static str {
                ::std::any::type_name::<T>()
            }
            let full = path_of(probe);
            $crate::tasks::workflows::steps::named::after_workflows(
                full.strip_suffix("::probe").unwrap_or(full),
            )
        }};
    }
    pub use crate::__workflow_path as path;

    /// Trims a module path to the part after `workflows`.
    ///
    /// Kept identical to what the backtrace version produced, so no generated
    /// name changes: `cargo xtask workflows` must leave `.github/workflows/`
    /// byte-for-byte as it found it.
    pub fn after_workflows(path: &str) -> String {
        path.split("::")
            .skip_while(|segment| *segment != "workflows")
            .skip(1)
            .collect::<Vec<_>>()
            .join("::")
    }

    /// Returns a uses step named after the enclosing function.
    #[macro_export]
    macro_rules! __named_uses {
        ($($arg:tt)*) => {
            $crate::tasks::workflows::steps::named::uses_named(
                $crate::tasks::workflows::steps::named::path!(),
                $($arg)*
            )
        };
    }
    pub use crate::__named_uses as uses;

    /// Returns a bash-script step named after the enclosing function.
    #[macro_export]
    macro_rules! __named_bash {
        ($($arg:tt)*) => {
            $crate::tasks::workflows::steps::named::bash_named(
                $crate::tasks::workflows::steps::named::path!(),
                $($arg)*
            )
        };
    }
    pub use crate::__named_bash as bash;

    /// Returns a pwsh-script step named after the enclosing function.
    #[macro_export]
    macro_rules! __named_pwsh {
        ($($arg:tt)*) => {
            $crate::tasks::workflows::steps::named::pwsh_named(
                $crate::tasks::workflows::steps::named::path!(),
                $($arg)*
            )
        };
    }
    pub use crate::__named_pwsh as pwsh;

    /// Runs the command in either powershell or bash, depending on platform.
    #[macro_export]
    macro_rules! __named_run {
        ($($arg:tt)*) => {
            $crate::tasks::workflows::steps::named::run_named(
                $crate::tasks::workflows::steps::named::path!(),
                $($arg)*
            )
        };
    }
    pub use crate::__named_run as run;

    /// Returns a Workflow named after the enclosing module.
    #[macro_export]
    macro_rules! __named_workflow {
        () => {
            $crate::tasks::workflows::steps::named::workflow_named(
                $crate::tasks::workflows::steps::named::path!(),
            )
        };
    }
    pub use crate::__named_workflow as workflow;

    /// Returns a Job named after the enclosing function.
    #[macro_export]
    macro_rules! __named_job {
        ($($arg:tt)*) => {
            $crate::tasks::workflows::steps::named::job_named(
                $crate::tasks::workflows::steps::named::path!(),
                $($arg)*
            )
        };
    }
    pub use crate::__named_job as job;

    pub fn uses_named(name: String, owner: &str, repo: &str, ref_: &str) -> Step<Use> {
        Step::new(name).uses(owner, repo, ref_)
    }

    pub fn bash_named(name: String, script: impl AsRef<str>) -> Step<Run> {
        Step::new(name).run(script.as_ref())
    }

    pub fn pwsh_named(name: String, script: &str) -> Step<Run> {
        Step::new(name).run(script).shell(PWSH_SHELL)
    }

    pub fn run_named(name: String, platform: Platform, script: &str) -> Step<Run> {
        match platform {
            Platform::Windows => Step::new(name).run(script).shell(PWSH_SHELL),
            Platform::Linux | Platform::Mac => Step::new(name).run(script),
        }
    }

    pub fn workflow_named(function_path: String) -> Workflow {
        Workflow::default()
            .name(
                function_path
                    .split("::")
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .skip(1)
                    .rev()
                    .collect::<Vec<_>>()
                    .join("::"),
            )
            .defaults(Defaults::default().run(RunDefaults::default().shell(BASH_SHELL)))
    }

    /// (note job names may not contain `::`)
    pub fn job_named<J: JobType>(function_path: String, job: Job<J>) -> NamedJob<J> {
        NamedJob {
            name: function_path
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned(),
            job,
        }
    }
}

pub fn git_checkout(ref_name: &dyn std::fmt::Display) -> Step<Run> {
    named::bash!(r#"git fetch origin "$REF_NAME" && git checkout "$REF_NAME""#)
        .add_env(("REF_NAME", ref_name.to_string()))
}

/// Non-exhaustive list of the permissions to be set for a GitHub app token.
///
/// See https://github.com/actions/create-github-app-token?tab=readme-ov-file#permission-permission-name
/// and beyond for a full list of available permissions.
#[allow(unused)]
pub(crate) enum TokenPermissions {
    Contents,
    Issues,
    PullRequests,
    Workflows,
}

impl TokenPermissions {
    pub fn environment_name(&self) -> &'static str {
        match self {
            TokenPermissions::Contents => "permission-contents",
            TokenPermissions::Issues => "permission-issues",
            TokenPermissions::PullRequests => "permission-pull-requests",
            TokenPermissions::Workflows => "permission-workflows",
        }
    }
}

pub(crate) struct GenerateAppToken<'a> {
    job_name: String,
    app_id: &'a str,
    app_secret: &'a str,
    repository_target: Option<RepositoryTarget>,
    permissions: Option<Vec<(TokenPermissions, Level)>>,
}

impl<'a> GenerateAppToken<'a> {
    pub fn for_repository(self, repository_target: RepositoryTarget) -> Self {
        Self {
            repository_target: Some(repository_target),
            ..self
        }
    }

    pub fn with_permissions(self, permissions: impl Into<Vec<(TokenPermissions, Level)>>) -> Self {
        Self {
            permissions: Some(permissions.into()),
            ..self
        }
    }
}

impl<'a> From<GenerateAppToken<'a>> for (Step<Use>, StepOutput) {
    fn from(token: GenerateAppToken<'a>) -> Self {
        let step = Step::new(token.job_name)
            .uses(
                "actions",
                "create-github-app-token",
                "f8d387b68d61c58ab83c6c016672934102569859",
            )
            .id("generate-token")
            .add_with(
                Input::default()
                    .add("app-id", token.app_id)
                    .add("private-key", token.app_secret)
                    .when_some(
                        token.repository_target,
                        |input,
                         RepositoryTarget {
                             owner,
                             repositories,
                         }| {
                            input
                                .when_some(owner, |input, owner| input.add("owner", owner))
                                .when_some(repositories, |input, repositories| {
                                    input.add("repositories", repositories)
                                })
                        },
                    )
                    .when_some(token.permissions, |input, permissions| {
                        permissions
                            .into_iter()
                            .fold(input, |input, (permission, level)| {
                                input.add(
                                    permission.environment_name(),
                                    serde_json::to_value(&level).unwrap_or_default(),
                                )
                            })
                    }),
            );

        let generated_token = StepOutput::new(&step, "token");
        (step, generated_token)
    }
}

pub(crate) struct RepositoryTarget {
    owner: Option<String>,
    repositories: Option<String>,
}

impl RepositoryTarget {
    pub fn new<T: ToString>(owner: T, repositories: &[&str]) -> Self {
        Self {
            owner: Some(owner.to_string()),
            repositories: Some(repositories.join("\n")),
        }
    }

    pub fn current() -> Self {
        Self {
            owner: None,
            repositories: None,
        }
    }
}

pub(crate) fn generate_token<'a>(
    app_id_source: &'a str,
    app_secret_source: &'a str,
) -> GenerateAppToken<'a> {
    generate_token_with_job_name(named::path!(), app_id_source, app_secret_source)
}

pub fn authenticate_as_zippy() -> GenerateAppToken<'static> {
    generate_token_with_job_name(
        named::path!(),
        vars::ZED_ZIPPY_APP_ID,
        vars::ZED_ZIPPY_APP_PRIVATE_KEY,
    )
}

/// Takes the caller's name rather than reading it back off the stack, for the
/// reason given on `named::path!`.
fn generate_token_with_job_name<'a>(
    job_name: String,
    app_id_source: &'a str,
    app_secret_source: &'a str,
) -> GenerateAppToken<'a> {
    GenerateAppToken {
        job_name,
        app_id: app_id_source,
        app_secret: app_secret_source,
        repository_target: None,
        permissions: None,
    }
}
