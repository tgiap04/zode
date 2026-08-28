use crate::tasks::workflows::{
    release::{ReleaseBundleJobs, download_workflow_artifacts, prep_release_artifacts},
    run_bundling::{bundle_linux, bundle_mac, bundle_windows},
    run_tests::{clippy_on_ref, run_platform_tests_no_filter_on_ref},
    runners::{Arch, Platform, ReleaseChannel},
    steps::{CommonJobConditions, DEFAULT_REPOSITORY_OWNER_GUARD, FluentBuilder, NamedJob},
    vars::StepOutput,
};

use super::{runners, steps, steps::named, vars};
use gh_workflow::*;

/// Generates the release_nightly.yml workflow
pub fn release_nightly() -> Workflow {
    let main_moved = main_moved_since_nightly();

    let mut style = check_style();
    // Linux, not Windows: upstream gated on Windows because that was their fastest
    // platform, but on GitHub-hosted runners Linux is.
    let mut tests = run_platform_tests_no_filter_on_ref(Platform::Linux, "main");
    let mut clippy_job = clippy_on_ref(Platform::Linux, None, Some("main"));
    let nightly = Some(ReleaseChannel::Nightly);

    // Only these three carry the condition. The bundles depend on them and GitHub skips a
    // job whose dependency was skipped, so the whole chain -- six bundles and the publish
    // step -- falls away without repeating the condition on each one.
    style.job = gate_on_main_moved(style.job, &main_moved);
    tests.job = gate_on_main_moved(tests.job, &main_moved);
    clippy_job.job = gate_on_main_moved(clippy_job.job, &main_moved);

    let gate: &[&NamedJob] = &[&style, &tests, &clippy_job];

    let bundle = ReleaseBundleJobs {
        linux_aarch64: bundle_linux(Arch::AARCH64, nightly, gate),
        linux_x86_64: bundle_linux(Arch::X86_64, nightly, gate),
        mac_aarch64: bundle_mac(Arch::AARCH64, nightly, gate),
        mac_x86_64: bundle_mac(Arch::X86_64, nightly, gate),
        windows_aarch64: bundle_windows(Arch::AARCH64, nightly, gate),
        windows_x86_64: bundle_windows(Arch::X86_64, nightly, gate),
    };

    let publish_nightly = publish_nightly_job(&bundle);

    named::workflow!()
        .on(Event::default()
            // Fire every day at 7:00am UTC (Roughly before EU workday and after US workday)
            .schedule([Schedule::new("0 7 * * *")])
            // Lets the whole path be exercised without waiting for the cron, which matters
            // because `schedule` only ever reads this file from the default branch.
            .workflow_dispatch(WorkflowDispatch::default()))
        .add_env(("CARGO_TERM_COLOR", "always"))
        .add_env(("RUST_BACKTRACE", "1"))
        .add_job(main_moved.name.clone(), main_moved.job)
        .add_job(style.name, style.job)
        .add_job(tests.name, tests.job)
        .add_job(clippy_job.name, clippy_job.job)
        .map(|mut workflow| {
            for job in bundle.into_jobs() {
                workflow = workflow.add_job(job.name, job.job);
            }
            workflow
        })
        .add_job(publish_nightly.name, publish_nightly.job)
}

const MAIN_MOVED_OUTPUT: &str = "moved";

/// The cron fires on a schedule, not on a change, so without this the six bundles would
/// rebuild an identical commit every day and replace the nightly assets with copies of
/// themselves. Compares `main` against wherever the `nightly` tag currently points.
fn main_moved_since_nightly() -> NamedJob {
    let compare = Step::new("compare")
        .run(indoc::indoc! {r#"
            previous=$(git rev-parse --verify --quiet refs/tags/nightly || true)
            current=$(git rev-parse HEAD)
            if [ "$previous" = "$current" ]; then
                echo "main is unchanged since the last nightly ($current); skipping the build."
                echo "moved=false" >> "$GITHUB_OUTPUT"
            else
                echo "main moved: ${previous:-<no nightly tag yet>} -> $current"
                echo "moved=true" >> "$GITHUB_OUTPUT"
            fi
        "#})
        .id("compare");

    let output = StepOutput::new(&compare, MAIN_MOVED_OUTPUT);

    named::job!(
        Job::default()
            .with_repository_owner_guard()
            .runs_on(runners::LINUX_SMALL)
            .outputs([(MAIN_MOVED_OUTPUT.to_owned(), output.to_string())])
            // Full history so the `nightly` tag is present to compare against.
            .add_step(steps::checkout_repo().with_full_history().with_ref("main"))
            .add_step(compare),
    )
}

/// Combines the owner guard with the changed check rather than replacing it: `cond` is a
/// single field, so setting it again would silently drop the guard.
///
/// `workflow_dispatch` always builds -- it exists to exercise this path on demand, which
/// is exactly when `main` has not moved.
fn gate_on_main_moved(job: Job, guard: &NamedJob) -> Job {
    let condition = format!(
        "{DEFAULT_REPOSITORY_OWNER_GUARD} && (needs.{name}.outputs.{MAIN_MOVED_OUTPUT} == 'true' || github.event_name == 'workflow_dispatch')",
        name = guard.name,
    );

    job.needs(vec![guard.name.clone()])
        .cond(Expression::new(condition))
}

/// Formatting only. Upstream also ran `script/clippy` here, on a mac runner, which on this
/// account would compile the workspace a second time for no new signal -- `clippy_linux`
/// already covers it. `cargo fmt --check` compiles nothing, so the 60-minute ceiling from
/// `release_job` stays appropriate here.
fn check_style() -> NamedJob {
    let job = release_job(&[])
        .runs_on(runners::LINUX_SMALL)
        .add_step(steps::checkout_repo().with_ref("main"))
        .add_step(steps::cargo_fmt());

    named::job!(job)
}

fn release_job(deps: &[&NamedJob]) -> Job {
    let job = Job::default()
        .with_repository_owner_guard()
        .timeout_minutes(60u32);
    if deps.len() > 0 {
        job.needs(deps.iter().map(|j| j.name.clone()).collect::<Vec<_>>())
    } else {
        job
    }
}

/// Publishes into a single rolling prerelease rather than upstream's DigitalOcean Spaces
/// bucket, so nothing external is needed and old builds do not accumulate.
fn publish_nightly_job(bundle: &ReleaseBundleJobs) -> NamedJob {
    fn move_nightly_tag() -> Step<Run> {
        named::bash!(indoc::indoc! {r#"
            if [ "$(git rev-parse nightly 2>/dev/null || true)" = "$(git rev-parse HEAD)" ]; then
              echo "Nightly tag already points to current commit. Skipping tagging."
              exit 0
            fi
            git config user.name github-actions
            git config user.email github-actions@github.com
            git tag -f nightly
            git push origin nightly --force
        "#})
    }

    // `--clobber` is what makes this roll: one release whose assets are replaced, instead
    // of a new release per run. The tag has to move first, or `gh release create` would
    // point the release at the previous commit.
    fn publish_rolling_prerelease() -> Step<Run> {
        named::bash!(indoc::indoc! {r#"
            gh release view nightly --repo="$GITHUB_REPOSITORY" \
              || gh release create nightly --repo="$GITHUB_REPOSITORY" \
                   --prerelease --target main --title "Nightly" \
                   --notes "Automated build from main. Unsigned, and it does not update itself."
            gh release upload nightly --repo="$GITHUB_REPOSITORY" --clobber release-artifacts/*
        "#})
        .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN))
    }

    NamedJob {
        name: "publish_nightly".to_owned(),
        // Moves the `nightly` tag as well as writing the release, and both need the same
        // write the default read-only token does not carry.
        job: steps::writes_to_releases(steps::release_job(&bundle.jobs()))
            .runs_on(runners::LINUX_MEDIUM)
            .add_step(steps::checkout_repo().with_full_history().with_ref("main"))
            .add_step(download_workflow_artifacts())
            .add_step(steps::script("ls -lR ./artifacts"))
            .add_step(prep_release_artifacts())
            .add_step(move_nightly_tag())
            .add_step(publish_rolling_prerelease()),
    }
}
