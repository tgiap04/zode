use gh_workflow::{Event, Expression, Push, Run, Step, Use, Workflow, ctx::Context};
use indoc::formatdoc;

use crate::tasks::workflows::{
    run_bundling::{bundle_linux, bundle_mac, bundle_windows, upload_artifact},
    run_tests,
    runners::{self, Arch, Platform},
    steps::{self, FluentBuilder, NamedJob, dependant_job, named, release_job},
    vars::{self, StepOutput, assets},
};

const CURRENT_ACTION_RUN_URL: &str =
    "${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}";

pub(crate) fn release() -> Workflow {
    // Gate on Linux only. Upstream blocks bundling on tests plus clippy across all three
    // platforms, which costs six extra jobs; on a 4-core hosted runner that is hours added
    // to every release. This is a deliberate reduction in coverage, not an optimisation:
    // a Windows-only or macOS-only regression will no longer stop a release. `run_tests`
    // still covers all three platforms on pull requests.
    let linux_tests = run_tests::run_platform_tests_no_filter(Platform::Linux);
    let linux_clippy = run_tests::clippy(Platform::Linux, None);
    let check_scripts = run_tests::check_scripts();

    let create_draft_release = create_draft_release();

    let gate: &[&NamedJob] = &[&linux_tests, &linux_clippy, &check_scripts];

    let bundle = ReleaseBundleJobs {
        linux_aarch64: bundle_linux(Arch::AARCH64, None, gate),
        linux_x86_64: bundle_linux(Arch::X86_64, None, gate),
        mac_aarch64: bundle_mac(Arch::AARCH64, None, gate),
        mac_x86_64: bundle_mac(Arch::X86_64, None, gate),
        windows_aarch64: bundle_windows(Arch::AARCH64, None, gate),
        windows_x86_64: bundle_windows(Arch::X86_64, None, gate),
    };

    let upload_release_assets = upload_release_assets(&[&create_draft_release], &bundle);
    let validate_release_assets = validate_release_assets(&[&upload_release_assets]);

    named::workflow()
        .on(Event::default().push(Push::default().tags(vec!["v*".to_string()])))
        .concurrency(vars::one_workflow_per_non_main_branch())
        .add_env(("CARGO_TERM_COLOR", "always"))
        .add_env(("RUST_BACKTRACE", "1"))
        .add_job(linux_tests.name, linux_tests.job)
        .add_job(linux_clippy.name, linux_clippy.job)
        .add_job(check_scripts.name, check_scripts.job)
        .add_job(create_draft_release.name, create_draft_release.job)
        .map(|mut workflow| {
            for job in bundle.into_jobs() {
                workflow = workflow.add_job(job.name, job.job);
            }
            workflow
        })
        .add_job(upload_release_assets.name, upload_release_assets.job)
        .add_job(validate_release_assets.name, validate_release_assets.job)
}

pub(crate) struct ReleaseBundleJobs {
    pub linux_aarch64: NamedJob,
    pub linux_x86_64: NamedJob,
    pub mac_aarch64: NamedJob,
    pub mac_x86_64: NamedJob,
    pub windows_aarch64: NamedJob,
    pub windows_x86_64: NamedJob,
}

impl ReleaseBundleJobs {
    pub fn jobs(&self) -> Vec<&NamedJob> {
        vec![
            &self.linux_aarch64,
            &self.linux_x86_64,
            &self.mac_aarch64,
            &self.mac_x86_64,
            &self.windows_aarch64,
            &self.windows_x86_64,
        ]
    }

    pub fn into_jobs(self) -> Vec<NamedJob> {
        vec![
            self.linux_aarch64,
            self.linux_x86_64,
            self.mac_aarch64,
            self.mac_x86_64,
            self.windows_aarch64,
            self.windows_x86_64,
        ]
    }
}

pub(crate) fn create_sentry_release() -> Step<Use> {
    named::uses(
        "getsentry",
        "action-release",
        "526942b68292201ac6bbb99b9a0747d4abee354c", // v3
    )
    .add_env(("SENTRY_ORG", "zed-dev"))
    .add_env(("SENTRY_PROJECT", "zed"))
    .add_env(("SENTRY_AUTH_TOKEN", vars::SENTRY_AUTH_TOKEN))
    .add_with(("environment", "production"))
}

pub(crate) const COMPLIANCE_REPORT_PATH: &str = "compliance-report-${GITHUB_REF_NAME}.md";
pub(crate) const COMPLIANCE_REPORT_ARTIFACT_PATH: &str =
    "compliance-report-${{ github.ref_name }}.md";
pub(crate) const COMPLIANCE_STEP_ID: &str = "run-compliance-check";
const NEEDS_REVIEW_PULLS_URL: &str = "https://github.com/zed-industries/zed/pulls?q=is%3Apr+is%3Aclosed+label%3A%22PR+state%3Aneeds+review%22";

/// Only the scheduled variant survives here: this fork's release path no longer runs a
/// compliance check, because it needs a GitHub App and a Slack webhook that belong to
/// upstream. `compliance_check.rs` still uses this.
pub(crate) enum ComplianceContext {
    Scheduled { tag_source: StepOutput },
}

impl ComplianceContext {
    fn tag_source(&self) -> Option<&StepOutput> {
        match self {
            ComplianceContext::Scheduled { tag_source } => Some(tag_source),
        }
    }
}

pub(crate) fn add_compliance_steps(
    job: gh_workflow::Job,
    context: ComplianceContext,
) -> (gh_workflow::Job, StepOutput) {
    fn run_compliance_check(context: &ComplianceContext) -> (Step<Run>, StepOutput) {
        let job = named::bash(
            formatdoc! {r#"
                cargo xtask compliance version {target} --report-path "{COMPLIANCE_REPORT_PATH}"
                "#,
                target = if context.tag_source().is_some() { r#""$LATEST_TAG" --branch main"# } else { r#""$GITHUB_REF_NAME""# },
            }
        )
        .id(COMPLIANCE_STEP_ID)
        .add_env(("GITHUB_APP_ID", vars::ZED_ZIPPY_APP_ID))
        .add_env(("GITHUB_APP_KEY", vars::ZED_ZIPPY_APP_PRIVATE_KEY))
        .when_some(context.tag_source(), |step, tag_source| {
            step.add_env(("LATEST_TAG", tag_source.to_string()))
        })
        .continue_on_error(true);

        let result = StepOutput::new_unchecked(&job, "outcome");
        (job, result)
    }

    let upload_step =
        upload_artifact(COMPLIANCE_REPORT_ARTIFACT_PATH).if_condition(Expression::new("always()"));

    let (success_prefix, failure_prefix) = (
        "✅ Scheduled compliance check passed",
        "⚠️ Scheduled compliance check failed",
    );

    let script = formatdoc! {r#"
        if [ "$COMPLIANCE_OUTCOME" == "success" ]; then
            STATUS="{success_prefix} for $COMPLIANCE_TAG"
            MESSAGE=$(printf "%s\n\nReport: %s" "$STATUS" "$ARTIFACT_URL")
        else
            STATUS="{failure_prefix} for $COMPLIANCE_TAG"
            MESSAGE=$(printf "%s\n\nReport: %s\nPRs needing review: %s" "$STATUS" "$ARTIFACT_URL" "{NEEDS_REVIEW_PULLS_URL}")
        fi

        curl -X POST -H 'Content-type: application/json' \
            --data "$(jq -n --arg text "$MESSAGE" '{{"text": $text}}')" \
            "$SLACK_WEBHOOK"
        "#,
    };

    let notification_step = Step::new("send_compliance_slack_notification")
        .run(&script)
        .if_condition(Expression::new("always()"))
        .add_env(("SLACK_WEBHOOK", vars::SLACK_WEBHOOK_WORKFLOW_FAILURES))
        .add_env((
            "COMPLIANCE_OUTCOME",
            format!("${{{{ steps.{COMPLIANCE_STEP_ID}.outcome }}}}"),
        ))
        .add_env((
            "COMPLIANCE_TAG",
            match &context {
                ComplianceContext::Scheduled { tag_source } => tag_source.to_string(),
            },
        ))
        .add_env((
            "ARTIFACT_URL",
            format!("{CURRENT_ACTION_RUN_URL}#artifacts"),
        ));

    let (compliance_step, check_result) = run_compliance_check(&context);

    (
        job.add_step(compliance_step)
            .add_step(upload_step)
            .add_step(notification_step),
        check_result,
    )
}

/// Guards against the failure mode where a bundle succeeds but its artifact filename does
/// not match what the release expects. `assets::all()` is the single source of truth for
/// both this check and `prep_release_artifacts`.
fn validate_release_assets(deps: &[&NamedJob]) -> NamedJob {
    let expected_assets: Vec<String> = assets::all().iter().map(|a| format!("\"{a}\"")).collect();
    let expected_assets_json = format!("[{}]", expected_assets.join(", "));

    let validation_script = formatdoc! {r#"
        EXPECTED_ASSETS='{expected_assets_json}'
        TAG="$GITHUB_REF_NAME"

        ACTUAL_ASSETS=$(gh release view "$TAG" --repo="$GITHUB_REPOSITORY" --json assets -q '[.assets[].name]')

        MISSING_ASSETS=$(echo "$EXPECTED_ASSETS" | jq -r --argjson actual "$ACTUAL_ASSETS" '. - $actual | .[]')

        if [ -n "$MISSING_ASSETS" ]; then
            echo "Error: The following assets are missing from the release:"
            echo "$MISSING_ASSETS"
            exit 1
        fi

        echo "All expected assets are present in the release."
        "#,
    };

    named::job(
        dependant_job(deps).runs_on(runners::LINUX_SMALL).add_step(
            named::bash(&validation_script).add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN)),
        ),
    )
}

pub(crate) fn download_workflow_artifacts() -> Step<Use> {
    named::uses(
        "actions",
        "download-artifact",
        "018cc2cf5baa6db3ef3c5f8a56943fffe632ef53", // v6.0.0
    )
    .add_with(("path", "./artifacts/"))
}

pub(crate) fn prep_release_artifacts() -> Step<Run> {
    let mut script_lines = vec!["mkdir -p release-artifacts/\n".to_string()];
    for asset in assets::all() {
        let mv_command = format!("mv ./artifacts/{asset}/{asset} release-artifacts/{asset}");
        script_lines.push(mv_command)
    }

    named::bash(&script_lines.join("\n"))
}

fn upload_release_assets(deps: &[&NamedJob], bundle: &ReleaseBundleJobs) -> NamedJob {
    let mut deps = deps.to_vec();
    deps.extend(bundle.jobs());

    named::job(
        dependant_job(&deps)
            .runs_on(runners::LINUX_MEDIUM)
            .add_step(download_workflow_artifacts())
            .add_step(steps::script("ls -lR ./artifacts"))
            .add_step(prep_release_artifacts())
            .add_step(
                steps::script("gh release upload \"$GITHUB_REF_NAME\" --repo=\"$GITHUB_REPOSITORY\" release-artifacts/*")
                    .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN)),
            ),
    )
}

fn create_draft_release() -> NamedJob {
    // Built from this repository's own git log rather than `script/draft-release-notes`.
    // That generator resolves every commit to a pull request in `zed-industries/zed` and
    // writes links into that repository -- for a fork it produces notes that look right
    // and point at the wrong project. Nothing here can fail on a first release: with no
    // prior tag it simply says so.
    fn generate_release_notes() -> Step<Run> {
        named::bash(indoc::indoc! {r#"
            prior_tag=$(git describe --tags --abbrev=0 "${GITHUB_REF_NAME}^" 2>/dev/null || true)
            {
                if [ -n "$prior_tag" ]; then
                    git log --no-merges --pretty='- %s' "${prior_tag}..${GITHUB_REF_NAME}"
                    echo
                    echo "**Full changelog**: ${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/compare/${prior_tag}...${GITHUB_REF_NAME}"
                else
                    echo "- First release."
                fi
                echo
                echo "Not code-signed, and there is no in-app updater. See the README before installing."
            } > target/release-notes.md
            cat target/release-notes.md
        "#})
    }

    fn create_release() -> Step<Run> {
        named::bash("script/create-draft-release target/release-notes.md")
            .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN))
    }

    named::job(
        release_job(&[])
            .runs_on(runners::LINUX_SMALL)
            // Full history, not a fixed depth: `git describe --tags` has to be able to
            // reach the previous tag, and a shallow clone does not carry the tags.
            .add_step(
                steps::checkout_repo()
                    .with_full_history()
                    .with_ref(Context::github().ref_()),
            )
            .add_step(steps::script("script/determine-release-channel"))
            .add_step(steps::script("mkdir -p target/"))
            .add_step(generate_release_notes())
            .add_step(create_release()),
    )
}

pub(crate) fn notify_on_failure(deps: &[&NamedJob]) -> NamedJob {
    let failure_message = format!("❌ ${{{{ github.workflow }}}} failed: {CURRENT_ACTION_RUN_URL}");

    let mut job = dependant_job(deps)
        .runs_on(runners::LINUX_SMALL)
        .cond(Expression::new("failure()"));

    for step in notify_slack(MessageType::Static(failure_message)) {
        job = job.add_step(step);
    }
    named::job(job)
}

pub(crate) enum MessageType {
    Static(String),
}

fn notify_slack(message: MessageType) -> Vec<Step<Run>> {
    match message {
        MessageType::Static(message) => vec![send_slack_message(message)],
    }
}

fn send_slack_message(message: String) -> Step<Run> {
    named::bash(
        r#"curl -X POST -H 'Content-type: application/json' --data "$(jq -n --arg text "$SLACK_MESSAGE" '{"text": $text}')" "$SLACK_WEBHOOK""#
    )
    .add_env(("SLACK_WEBHOOK", vars::SLACK_WEBHOOK_WORKFLOW_FAILURES))
    .add_env(("SLACK_MESSAGE", message))
}
