use gh_workflow::*;

use crate::tasks::workflows::steps::{CommonJobConditions, NamedJob, named};

use super::{runners, steps, vars};

/// Generates the danger.yml workflow
pub fn danger() -> Workflow {
    let danger = danger_job();

    named::workflow!()
        .on(
            // `develop` as well as `main`: feature branches land in `develop` first, so a
            // check that only watches `main` never sees the pull request that introduces
            // the change.
            Event::default().pull_request(
                PullRequest::default()
                    .add_branch("main")
                    .add_branch("develop")
                    .types([
                        PullRequestType::Opened,
                        PullRequestType::Synchronize,
                        PullRequestType::Reopened,
                        PullRequestType::Edited,
                    ]),
            ),
        )
        .add_job(danger.name, danger.job)
}

fn danger_job() -> NamedJob {
    pub fn install_deps() -> Step<Run> {
        named::bash!("pnpm install --dir script/danger")
    }

    // Talks to api.github.com directly with the workflow's own token. Upstream routes
    // everything through `danger-proxy.zed.dev`, which exists so Danger can authenticate
    // for pull requests opened from forks; that proxy refuses requests for any other
    // repository, so here it fails outright. The trade-off is that Danger will not be able
    // to comment on a pull request opened from a fork, where GITHUB_TOKEN is read-only.
    pub fn run() -> Step<Run> {
        named::bash!("pnpm run --dir script/danger danger ci")
            .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN))
    }

    NamedJob {
        name: "danger".to_string(),
        job: Job::default()
            .with_repository_owner_guard()
            .permissions(
                Permissions::default()
                    .contents(Level::Read)
                    .pull_requests(Level::Write),
            )
            .runs_on(runners::LINUX_SMALL)
            .add_step(steps::checkout_repo())
            .add_step(steps::setup_pnpm())
            .add_step(
                steps::setup_node()
                    .add_with(("cache", "pnpm"))
                    .add_with(("cache-dependency-path", "script/danger/pnpm-lock.yaml")),
            )
            .add_step(install_deps())
            .add_step(run()),
    }
}
