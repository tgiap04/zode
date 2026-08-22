use gh_workflow::*;

use crate::tasks::workflows::{
    runners,
    steps::{self, CheckoutStep, CommonJobConditions, named},
    vars::{StepOutput, WorkflowInput},
};

/// The token the commit and the tag are made with.
///
/// `GITHUB_TOKEN` can do both, but GitHub deliberately does not start a workflow run
/// from anything that token pushed -- so the tag would appear and `release` would
/// never fire. Set `RELEASE_TAG_TOKEN` to a PAT (or app token) with `contents: write`
/// to get the release chained automatically; without it the bump still lands and the
/// tag still exists, and the release has to be started by hand.
const TAG_TOKEN: &str = "${{ secrets.RELEASE_TAG_TOKEN || secrets.GITHUB_TOKEN }}";

pub fn bump_patch_version() -> Workflow {
    let branch = WorkflowInput::string("branch", None).description("Branch name to run on");
    let prerelease = WorkflowInput::bool("prerelease", Some(false))
        .description("Tag as a prerelease (v0.1.2-pre) instead of a full release (v0.1.2)");
    let bump_patch_version_job = run_bump_patch_version(&branch, &prerelease);
    named::workflow()
        .on(Event::default().workflow_dispatch(
            WorkflowDispatch::default()
                .add_input(branch.name, branch.input())
                .add_input(prerelease.name, prerelease.input()),
        ))
        .concurrency(
            Concurrency::new(Expression::new(format!(
                "${{{{ github.workflow }}}}-{branch}"
            )))
            .cancel_in_progress(true),
        )
        .add_job(bump_patch_version_job.name, bump_patch_version_job.job)
}

fn run_bump_patch_version(
    branch: &WorkflowInput,
    prerelease: &WorkflowInput,
) -> steps::NamedJob {
    fn checkout_branch(branch: &WorkflowInput) -> CheckoutStep {
        steps::checkout_repo()
            .with_token_expression(TAG_TOKEN)
            .with_ref(branch.to_string())
    }

    /// Bumps the patch version and reports the tag to cut.
    ///
    /// Upstream read `crates/zed/RELEASE_CHANNEL` here and refused to run unless it
    /// said `stable` or `preview`. That gate cannot pass in this fork: the file says
    /// `dev` on every branch, because the channel is derived from the *tag* --
    /// `script/determine-release-channel` reads its shape, and the bundling jobs write
    /// the file from that. Reading the file to decide the tag inverted the one rule
    /// this fork actually has, so the caller says which kind of tag they want instead.
    fn bump_version(prerelease: &WorkflowInput) -> Step<Run> {
        named::bash(indoc::indoc! {r#"
            if [[ "$PRERELEASE" == "true" ]]; then
              tag_suffix="-pre"
            else
              tag_suffix=""
            fi

            which cargo-set-version > /dev/null || cargo install cargo-edit -f --no-default-features --features "set-version"
            version="$(cargo set-version -p zode --bump patch 2>&1 | sed 's/.* //')"
            echo "version=$version" >> "$GITHUB_OUTPUT"
            echo "tag_suffix=$tag_suffix" >> "$GITHUB_OUTPUT"
        "#})
        // Through the environment, never interpolated into the script: a workflow
        // input is attacker-controlled text, and the generator lints for exactly this.
        .add_env(("PRERELEASE", prerelease.to_string()))
        .id("bump-version")
    }

    fn commit_changes(version: &StepOutput, branch: &WorkflowInput) -> Step<Use> {
        named::uses(
            "IAreKyleW00t",
            "verified-bot-commit",
            "126a6a11889ab05bcff72ec2403c326cd249b84c", // v2.3.0
        )
        .id("commit")
        .add_with((
            "message",
            format!("Bump to {version} for @${{{{ github.actor }}}}"),
        ))
        .add_with(("ref", format!("refs/heads/{branch}")))
        .add_with(("files", "**"))
        .add_with(("token", TAG_TOKEN))
    }

    fn create_version_tag(
        version: &StepOutput,
        tag_suffix: &StepOutput,
        commit_sha: &StepOutput,
    ) -> Step<Use> {
        named::uses(
            "actions",
            "github-script",
            "f28e40c7f34bde8b3046d885e986cb6290c5673b", // v7
        )
        .with(
            Input::default()
                .add(
                    "script",
                    indoc::formatdoc! {r#"
                        github.rest.git.createRef({{
                            owner: context.repo.owner,
                            repo: context.repo.repo,
                            ref: 'refs/tags/v{version}{tag_suffix}',
                            sha: '{commit_sha}'
                        }})
                    "#},
                )
                .add("github-token", TAG_TOKEN),
        )
    }

    let bump_version_step = bump_version(prerelease);
    let version = StepOutput::new(&bump_version_step, "version");
    let tag_suffix = StepOutput::new(&bump_version_step, "tag_suffix");
    let commit_step = commit_changes(&version, branch);
    let commit_sha = StepOutput::new_unchecked(&commit_step, "commit");

    named::job(
        // Both the commit and the tag are writes, and a plain repository's default
        // token is read-only -- see `writes_to_releases`.
        steps::writes_to_releases(Job::default())
            .with_repository_owner_guard()
            .runs_on(runners::LINUX_XL)
            .add_step(checkout_branch(branch))
            .add_step(bump_version_step)
            .add_step(commit_step)
            .add_step(create_version_tag(&version, &tag_suffix, &commit_sha)),
    )
}
