use std::path::Path;

use crate::tasks::workflows::{
    release::ReleaseBundleJobs,
    runners::{self, Arch, Platform, ReleaseChannel},
    steps::{FluentBuilder, NamedJob, dependant_job, named},
    vars::{assets, bundle_envs},
};

use super::steps;
use gh_workflow::*;
use indoc::indoc;

pub fn run_bundling() -> Workflow {
    let bundle = ReleaseBundleJobs {
        linux_aarch64: bundle_linux(Arch::AARCH64, None, &[]),
        linux_x86_64: bundle_linux(Arch::X86_64, None, &[]),
        mac_aarch64: bundle_mac(Arch::AARCH64, None, &[]),
        mac_x86_64: bundle_mac(Arch::X86_64, None, &[]),
        windows_aarch64: bundle_windows(Arch::AARCH64, None, &[]),
        windows_x86_64: bundle_windows(Arch::X86_64, None, &[]),
    };
    // No Nix jobs: they authenticate against upstream's Nix binary cache through a
    // Namespace-only cache action, so they cannot run here. This workflow's remaining
    // purpose is on-demand bundling via the `run-bundling` label.
    named::workflow()
        .on(Event::default().pull_request(
            PullRequest::default().types([PullRequestType::Labeled, PullRequestType::Synchronize]),
        ))
        .concurrency(
            Concurrency::new(Expression::new(
                "${{ github.workflow }}-${{ github.head_ref || github.ref }}",
            ))
            .cancel_in_progress(true),
        )
        .add_env(("CARGO_TERM_COLOR", "always"))
        .add_env(("RUST_BACKTRACE", "1"))
        .map(|mut workflow| {
            for job in bundle.into_jobs() {
                workflow = workflow.add_job(job.name, job.job);
            }
            workflow
        })
}

fn bundle_job(deps: &[&NamedJob]) -> Job {
    dependant_job(deps)
        .when(deps.len() == 0, |job|
            job.cond(Expression::new(
                indoc! {
                    r#"(github.event.action == 'labeled' && github.event.label.name == 'run-bundling') ||
                    (github.event.action == 'synchronize' && contains(github.event.pull_request.labels.*.name, 'run-bundling'))"#,
                })))
        .timeout_minutes(steps::RELEASE_JOB_TIMEOUT_MINUTES)
}

/// A nightly run is triggered by `schedule`, which GitHub always evaluates against the
/// default branch -- `develop` here. Without an explicit ref the nightly would quietly
/// build `develop` instead of `main`. A tag-triggered release wants the default behaviour
/// (the tag itself), so the release channel is the signal that distinguishes them.
fn checkout_for(release_channel: Option<ReleaseChannel>) -> steps::CheckoutStep {
    match release_channel {
        Some(ReleaseChannel::Nightly) => steps::checkout_repo().with_ref("main"),
        None => steps::checkout_repo(),
    }
}

pub(crate) fn bundle_mac(
    arch: Arch,
    release_channel: Option<ReleaseChannel>,
    deps: &[&NamedJob],
) -> NamedJob {
    pub fn bundle_mac(arch: Arch) -> Step<Run> {
        named::bash(&format!("./script/bundle-mac {arch}-apple-darwin"))
    }
    let platform = Platform::Mac;
    let artifact_name = match arch {
        Arch::X86_64 => assets::MAC_X86_64,
        Arch::AARCH64 => assets::MAC_AARCH64,
    };
    NamedJob {
        name: format!("bundle_mac_{arch}"),
        job: bundle_job(deps)
            .runs_on(arch.mac_bundler())
            .envs(bundle_envs(platform))
            .add_step(checkout_for(release_channel))
            .when_some(release_channel, |job, release_channel| {
                job.add_step(set_release_channel(platform, release_channel))
            })
            .add_step(steps::setup_node())
            .add_step(steps::free_disk_space(platform))
            .add_step(bundle_mac(arch))
            .add_step(upload_artifact(&format!(
                "target/{arch}-apple-darwin/release/{artifact_name}"
            ))),
    }
}

pub fn upload_artifact(path: &str) -> Step<Use> {
    let name = Path::new(path).file_name().unwrap().to_str().unwrap();
    Step::new(format!("@actions/upload-artifact {}", name))
        .uses(
            "actions",
            "upload-artifact",
            "330a01c490aca151604b8cf639adc76d48f6c5d4", // v5
        )
        // N.B. "name" is the name for the asset. The uploaded
        // file retains its filename.
        .add_with(("name", name))
        .add_with(("path", path))
        .add_with(("if-no-files-found", "error"))
}

pub(crate) fn bundle_linux(
    arch: Arch,
    release_channel: Option<ReleaseChannel>,
    deps: &[&NamedJob],
) -> NamedJob {
    let platform = Platform::Linux;
    let artifact_name = match arch {
        Arch::X86_64 => assets::LINUX_X86_64,
        Arch::AARCH64 => assets::LINUX_AARCH64,
    };
    NamedJob {
        name: format!("bundle_linux_{arch}"),
        job: bundle_job(deps)
            .runs_on(arch.linux_bundler())
            .envs(bundle_envs(platform))
            // Unversioned on purpose: upstream pinned clang-18, which exists only on the
            // 20.04 image its bundler used. On 24.04 the default `clang` already is 18.
            .add_env(Env::new("CC", "clang"))
            .add_env(Env::new("CXX", "clang++"))
            .add_step(checkout_for(release_channel))
            .when_some(release_channel, |job, release_channel| {
                job.add_step(set_release_channel(platform, release_channel))
            })
            .add_step(steps::free_disk_space(platform))
            .map(steps::install_linux_dependencies)
            .add_step(steps::script("./script/bundle-linux"))
            .add_step(upload_artifact(&format!("target/release/{artifact_name}"))),
    }
}

pub(crate) fn bundle_windows(
    arch: Arch,
    release_channel: Option<ReleaseChannel>,
    deps: &[&NamedJob],
) -> NamedJob {
    let platform = Platform::Windows;
    // No `working_directory` override: upstream pointed it at `$ZED_WORKSPACE`, which is a
    // machine-level variable on their self-hosted runner and resolves to empty here. The
    // script derives that path itself via `ParseZedWorkspace`.
    pub fn bundle_windows(arch: Arch) -> Step<Run> {
        match arch {
            Arch::X86_64 => named::pwsh("script/bundle-windows.ps1 -Architecture x86_64"),
            Arch::AARCH64 => named::pwsh("script/bundle-windows.ps1 -Architecture aarch64"),
        }
    }
    let artifact_name = match arch {
        Arch::X86_64 => assets::WINDOWS_X86_64,
        Arch::AARCH64 => assets::WINDOWS_AARCH64,
    };
    NamedJob {
        name: format!("bundle_windows_{arch}"),
        job: bundle_job(deps)
            .runs_on(runners::WINDOWS_DEFAULT)
            .envs(bundle_envs(platform))
            .add_step(checkout_for(release_channel))
            .when_some(release_channel, |job, release_channel| {
                job.add_step(set_release_channel(platform, release_channel))
            })
            .add_step(steps::free_disk_space(platform))
            .add_step(steps::windows_enable_long_paths())
            .add_step(bundle_windows(arch))
            .add_step(upload_artifact(&format!("target/{artifact_name}"))),
    }
}

fn set_release_channel(platform: Platform, release_channel: ReleaseChannel) -> Step<Run> {
    match release_channel {
        ReleaseChannel::Nightly => set_release_channel_to_nightly(platform),
    }
}

fn set_release_channel_to_nightly(platform: Platform) -> Step<Run> {
    match platform {
        Platform::Linux | Platform::Mac => named::bash(indoc::indoc! {r#"
            set -eu
            version=$(git rev-parse --short HEAD)
            echo "Publishing version: ${version} on release channel nightly"
            echo "nightly" > crates/zed/RELEASE_CHANNEL
        "#}),
        Platform::Windows => named::pwsh(indoc::indoc! {r#"
            $ErrorActionPreference = "Stop"
            $version = git rev-parse --short HEAD
            Write-Host "Publishing version: $version on release channel nightly"
            "nightly" | Set-Content -Path "crates/zed/RELEASE_CHANNEL"
        "#})
        .working_directory("${{ env.ZED_WORKSPACE }}"),
    }
}
