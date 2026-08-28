use gh_workflow::{Event, Push, Run, Step, Use, Workflow, ctx::Context};
use indoc::formatdoc;

use crate::tasks::workflows::{
    run_bundling::{bundle_linux, bundle_mac, bundle_windows},
    run_tests,
    runners::{self, Arch, Platform, ReleaseChannel},
    steps::{self, FluentBuilder, NamedJob, dependant_job, named, release_job},
    vars::{self, assets},
};

pub(crate) fn release() -> Workflow {
    // Gate on Linux only. Upstream blocks bundling on tests plus clippy across all three
    // platforms, which costs six extra jobs; on a 4-core hosted runner that is hours added
    // to every release. This is a deliberate reduction in coverage, not an optimisation:
    // a Windows-only or macOS-only regression will no longer stop a release. `run_tests`
    // still covers all three platforms on pull requests.
    //
    // These three run *beside* the bundles rather than in front of them, and
    // `upload_release_assets` is what waits for them. Measured on v0.1.1: the bundles all
    // started at t=47m because `run_tests_linux` took 47m, and the longest bundle then ran
    // to t=145m. Nothing in those 47 minutes was new information -- a `v*` tag is cut from
    // `main`, and `run_tests` has already run this exact tree on the push that put it
    // there. Moving the gate off the bundles' critical path and onto the upload's costs no
    // coverage: a bundle whose gate failed still produces an artifact, but the upload that
    // would attach it to the release is skipped, so no asset can reach a release from a
    // tree that failed. It costs runner minutes on a build that turns out to be doomed,
    // which on a public repository is free (see `runners.rs`).
    let linux_tests = run_tests::run_platform_tests_no_filter(Platform::Linux);
    let linux_clippy = run_tests::clippy(Platform::Linux, None);
    let check_scripts = run_tests::check_scripts();

    let create_draft_release = create_draft_release();

    let gate: &[&NamedJob] = &[&linux_tests, &linux_clippy, &check_scripts];
    let ungated: &[&NamedJob] = &[];

    // Every bundling job resolves the channel from the tag itself. The bundling
    // scripts read `crates/zed/RELEASE_CHANNEL`, and a job that leaves it alone
    // bundles the checked-in channel -- which is how a `v*` tag produced installers
    // named "Zode Dev" / "Zode Devel" on all three platforms.
    let channel = Some(ReleaseChannel::FromTag);

    let bundle = ReleaseBundleJobs {
        linux_aarch64: bundle_linux(Arch::AARCH64, channel, ungated),
        linux_x86_64: bundle_linux(Arch::X86_64, channel, ungated),
        mac_aarch64: bundle_mac(Arch::AARCH64, channel, ungated),
        mac_x86_64: bundle_mac(Arch::X86_64, channel, ungated),
        windows_aarch64: bundle_windows(Arch::AARCH64, channel, ungated),
        windows_x86_64: bundle_windows(Arch::X86_64, channel, ungated),
    };

    let mut upload_deps = vec![&create_draft_release];
    upload_deps.extend_from_slice(gate);
    let upload_release_assets = upload_release_assets(&upload_deps, &bundle);
    let validate_release_assets = validate_release_assets(&[&upload_release_assets]);

    named::workflow!()
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

/// Guards against the failure mode where a bundle succeeds but its artifact filename does
/// not match what the release expects. `assets::all()` is the single source of truth for
/// both this check and `prep_release_artifacts`.
fn validate_release_assets(deps: &[&NamedJob]) -> NamedJob {
    let expected_assets: Vec<String> = assets::all().iter().map(|a| format!("\"{a}\"")).collect();
    let expected_assets_json = format!("[{}]", expected_assets.join(", "));

    // The empty-`ACTUAL_ASSETS` guard is not defensive padding. Without it, a release this
    // job cannot read leaves the variable empty, `jq --argjson actual ""` dies on invalid
    // JSON, and the job reports a jq parse error -- which reads as a broken check rather
    // than an unreadable release, and sends the next reader looking in the wrong place.
    let validation_script = formatdoc! {r#"
        EXPECTED_ASSETS='{expected_assets_json}'
        TAG="$GITHUB_REF_NAME"

        ACTUAL_ASSETS=$(gh release view "$TAG" --repo="$GITHUB_REPOSITORY" --json assets -q '[.assets[].name]')

        if [ -z "$ACTUAL_ASSETS" ]; then
            echo "Error: could not read release $TAG. It exists only as a draft until someone"
            echo "publishes it, and a draft is invisible to a token without write on contents."
            exit 1
        fi

        MISSING_ASSETS=$(echo "$EXPECTED_ASSETS" | jq -r --argjson actual "$ACTUAL_ASSETS" '. - $actual | .[]')

        if [ -n "$MISSING_ASSETS" ]; then
            echo "Error: The following assets are missing from the release:"
            echo "$MISSING_ASSETS"
            exit 1
        fi

        echo "All expected assets are present in the release."
        "#,
    };

    // Write, for a job that only reads. GitHub shows a draft release solely to callers with
    // push access, so a `contents: read` token gets `release not found` for a release that
    // is plainly there -- which is exactly how v0.1.0 failed here after all six bundles and
    // the upload had already succeeded.
    named::job!(
        steps::writes_to_releases(dependant_job(deps))
            .runs_on(runners::LINUX_SMALL)
            .add_step(
                named::bash!(&validation_script).add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN)),
            ),
    )
}

pub(crate) fn download_workflow_artifacts() -> Step<Use> {
    named::uses!(
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

    named::bash!(&script_lines.join("\n"))
}

fn upload_release_assets(deps: &[&NamedJob], bundle: &ReleaseBundleJobs) -> NamedJob {
    let mut deps = deps.to_vec();
    deps.extend(bundle.jobs());

    named::job!(
        steps::writes_to_releases(dependant_job(&deps))
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
        named::bash!(indoc::indoc! {r#"
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
                echo "Not code-signed. See the README before installing."
            } > target/release-notes.md
            cat target/release-notes.md
        "#})
    }

    // The tag is the source of truth for a published version: `crates/zed/build.rs` stamps
    // RELEASE_VERSION into the binary, so a Cargo.toml that disagrees no longer yields a
    // build that misreports itself. A divergence is still worth saying out loud, because
    // the repo and the release then name different versions and whoever reads the manifest
    // next is misled. Warn, never fail -- releasing straight from a tag without a bump
    // commit is a deliberate property of this fork (see script/determine-release-channel).
    fn warn_on_version_divergence() -> Step<Run> {
        named::bash!(indoc::indoc! {r#"
            # The first `version =` in the manifest is the one under [package].
            manifest_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/zed/Cargo.toml | head -1)
            if [ "$manifest_version" = "$RELEASE_VERSION" ]; then
                echo "tag and crates/zed/Cargo.toml agree on ${RELEASE_VERSION}"
            else
                echo "::warning::tag ${GITHUB_REF_NAME} builds as ${RELEASE_VERSION} but crates/zed/Cargo.toml says ${manifest_version}; the binary reports the tag, so consider bumping the manifest to match"
            fi
        "#})
    }

    fn create_release() -> Step<Run> {
        named::bash!("script/create-draft-release target/release-notes.md")
            .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN))
    }

    named::job!(
        steps::writes_to_releases(release_job(&[]))
            .runs_on(runners::LINUX_SMALL)
            // Full history, not a fixed depth: `git describe --tags` has to be able to
            // reach the previous tag, and a shallow clone does not carry the tags.
            .add_step(
                steps::checkout_repo()
                    .with_full_history()
                    .with_ref(Context::github().ref_()),
            )
            .add_step(steps::script("script/determine-release-channel"))
            .add_step(warn_on_version_divergence())
            .add_step(steps::script("mkdir -p target/"))
            .add_step(generate_release_notes())
            .add_step(create_release()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::workflows::run_bundling::run_bundling;
    use serde_yaml::Value;

    const GATE: [&str; 3] = ["run_tests_linux", "clippy_linux", "check_scripts"];
    const BUNDLES: [&str; 6] = [
        "bundle_linux_aarch64",
        "bundle_linux_x86_64",
        "bundle_mac_aarch64",
        "bundle_mac_x86_64",
        "bundle_windows_aarch64",
        "bundle_windows_x86_64",
    ];

    /// Asserts against the serialised YAML rather than the builder objects: the YAML is
    /// what GitHub reads, and a job graph that is right in Rust but serialises wrong is
    /// still a broken release.
    fn jobs_of(workflow: Workflow) -> Value {
        let yaml = workflow.to_string().expect("workflow serialises to YAML");
        let parsed: Value = serde_yaml::from_str(&yaml).expect("generated YAML parses");
        parsed["jobs"].clone()
    }

    fn needs_of(jobs: &Value, job: &str) -> Vec<String> {
        assert!(!jobs[job].is_null(), "no job named {job}");
        match &jobs[job]["needs"] {
            Value::Sequence(items) => items
                .iter()
                .map(|item| item.as_str().unwrap_or_default().to_owned())
                .collect(),
            Value::String(only) => vec![only.clone()],
            _ => Vec::new(),
        }
    }

    /// `bundle_job` used to infer "this job is triggered by the `run-bundling` label" from
    /// `deps.is_empty()`. The two agreed only by coincidence -- every caller that passed a
    /// release channel also passed a gate. Taking the gate off the release bundles under
    /// that old rule stamped `if: github.event.action == 'labeled'` onto all six, which is
    /// false on a tag push: the release would have finished green with no asset attached
    /// and nothing in the log saying why.
    #[test]
    fn ungating_the_release_bundles_did_not_hand_them_a_label_condition() {
        let jobs = jobs_of(release());
        for bundle in BUNDLES {
            assert!(
                jobs[bundle]["if"].is_null(),
                "release bundle {bundle} carries a condition it should not have: {:?}",
                jobs[bundle]["if"]
            );
            assert!(
                needs_of(&jobs, bundle).is_empty(),
                "release bundle {bundle} still waits on {:?}, so it is back on the critical path",
                needs_of(&jobs, bundle)
            );
        }
    }

    /// The other half of that same rule. `run_bundling` is the on-demand workflow, and the
    /// label condition is the only thing stopping every pull request from bundling six
    /// targets.
    #[test]
    fn the_on_demand_bundling_workflow_keeps_its_label_condition() {
        let jobs = jobs_of(run_bundling());
        for bundle in BUNDLES {
            let condition = jobs[bundle]["if"].as_str().unwrap_or_default();
            assert!(
                condition.contains("run-bundling"),
                "on-demand bundle {bundle} lost its label gate: {condition:?}"
            );
        }
    }

    /// What makes ungating the bundles safe. They no longer wait for the gate, but the
    /// upload does -- and a skipped `needs` skips the upload -- so an artifact built from a
    /// tree that failed its tests never reaches the release.
    #[test]
    fn no_asset_reaches_a_release_whose_gate_failed() {
        let jobs = jobs_of(release());
        let waits_for = needs_of(&jobs, "upload_release_assets");
        for gate in GATE {
            assert!(
                waits_for.contains(&gate.to_owned()),
                "upload_release_assets does not wait for {gate}, so a failed gate cannot stop it: {waits_for:?}"
            );
        }
    }
}
