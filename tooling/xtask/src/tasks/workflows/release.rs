use gh_workflow::{
    Container, Env, Event, Expression, Push, Run, Step, Use, Workflow, ctx::Context,
};
use indoc::formatdoc;

use crate::tasks::workflows::{
    bump_patch_version::TAG_TOKEN,
    run_bundling::{bundle_linux, bundle_mac, bundle_windows, upload_artifact},
    run_tests,
    runners::{self, Arch, Platform, ReleaseChannel},
    steps::{self, FluentBuilder, NamedJob, dependant_job, named, release_job},
    vars::{self, StepOutput, assets},
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

    // The first thing a release finds out, and the cheapest: whether it can sign the
    // driver manifest at all. Nothing is built from a tag whose drivers cannot be
    // published, but a missing or wrong secret otherwise surfaces in the last job, after
    // hours of bundling and with installers already attached to the release. It is in the
    // gate, so no upload and no bump happens without it, and the draft release waits for
    // it so that a tag that fails it leaves nothing behind.
    let signing_key = check_driver_signing_key();
    let create_draft_release = create_draft_release(&[&signing_key]);

    let gate: &[&NamedJob] = &[&linux_tests, &linux_clippy, &check_scripts, &signing_key];
    let ungated: &[&NamedJob] = &[];

    // Behind the same gate as the upload, and for the same reason: a tag that fails it
    // produces no release, and develop's version must not move to one that was never
    // shipped. Nothing waits on the bump, so a failed or slow one still holds nothing
    // back.
    let mut bump_deps = vec![&create_draft_release];
    bump_deps.extend_from_slice(gate);
    let bump_manifest_on_develop = bump_manifest_on_develop(&bump_deps);

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

    let remote_server_x86_64 = bundle_remote_server_linux(Arch::X86_64);
    let remote_server_aarch64 = bundle_remote_server_linux(Arch::AARCH64);

    let mut upload_deps = vec![&create_draft_release];
    upload_deps.extend_from_slice(gate);
    upload_deps.extend([&remote_server_x86_64, &remote_server_aarch64]);
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
        .add_job(signing_key.name, signing_key.job)
        .add_job(create_draft_release.name, create_draft_release.job)
        .add_job(bump_manifest_on_develop.name, bump_manifest_on_develop.job)
        .map(|mut workflow| {
            for job in bundle.into_jobs() {
                workflow = workflow.add_job(job.name, job.job);
            }
            workflow
        })
        .add_job(remote_server_x86_64.name, remote_server_x86_64.job)
        .add_job(remote_server_aarch64.name, remote_server_aarch64.job)
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

/// Builds the statically linked remote server the app installs on a WSL or SSH host.
///
/// A job of its own rather than a step of `bundle_linux`: the musl target needs a
/// second full target directory, which inside a bundle job roughly doubled its time and
/// disk. Here it runs beside the bundles and, like them, ungated -- the upload is what
/// waits for the gate. Musl and static because the gnu build links the glibc of the
/// build image, which older WSL distributions do not have.
///
/// No secrets: nothing in the remote server reads the ones `bundle_envs` hands the app.
fn bundle_remote_server_linux(arch: Arch) -> NamedJob {
    NamedJob {
        name: format!("bundle_remote_server_linux_{arch}"),
        job: dependant_job(&[])
            .timeout_minutes(steps::RELEASE_JOB_TIMEOUT_MINUTES)
            .runs_on(arch.linux_bundler())
            // The image `bundle_linux` uses, so `script/linux` runs where it is known to.
            .container(Container::new("ubuntu:22.04"))
            .envs(vars::release_profile_envs())
            .add_env(Env::new("CC", "clang"))
            .add_env(Env::new("CXX", "clang++"))
            .add_env(Env::new("DEBIAN_FRONTEND", "noninteractive"))
            .add_step(steps::bootstrap_container())
            .add_step(steps::checkout_repo())
            // `remote_server` compiles its channel in from `crates/zed/RELEASE_CHANNEL`,
            // and a server built as dev reports a commit where the app expects a version.
            .add_step(steps::script("script/determine-release-channel"))
            .add_step(steps::free_disk_space(Platform::Linux))
            .add_step(steps::setup_linux())
            .add_step(steps::script("./script/bundle-remote-server-linux"))
            .add_step(upload_artifact(&format!(
                "target/{}",
                assets::remote_server_linux(arch)
            ))),
    }
}

/// Guards against the failure mode where a bundle succeeds but its artifact filename does
/// not match what the release expects. `assets::all()` is the single source of truth for
/// both this check and `prep_release_artifacts`.
///
/// Driver archives and the manifest are deliberately absent here: they no longer reach the
/// GitHub release at all, so there is nothing of theirs for this job to validate. Their
/// upload is checked by `script/publish-drivers-to-web` itself, which re-fetches the
/// published manifest and counts its entries before the job can succeed.
fn validate_release_assets(deps: &[&NamedJob]) -> NamedJob {
    let expected_assets: Vec<String> = assets::all()
        .into_iter()
        .chain(assets::remote_server_release_assets())
        .map(|asset| format!("\"{asset}\""))
        .collect();
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

/// Unchanged from before this phase: still gathers every asset, including the driver
/// archives, into one `release-artifacts/` directory.
///
/// `release_nightly.rs` also calls this, and that workflow's rolling prerelease still
/// globs `release-artifacts/*` wholesale with no driver-manifest or web-upload step of its
/// own. Splitting driver archives out here would silently drop them from the nightly
/// build with no equivalent published anywhere -- a behavior change out of scope for this
/// phase, which owns `release.yml` only. `prep_upload_release_and_driver_artifacts` below
/// is the split version, used solely by `release.yml`'s `upload_release_assets`.
pub(crate) fn prep_release_artifacts() -> Step<Run> {
    let mut script_lines = vec!["mkdir -p release-artifacts/\n".to_string()];
    for asset in assets::all() {
        let mv_command = format!("mv ./artifacts/{asset}/{asset} release-artifacts/{asset}");
        script_lines.push(mv_command)
    }

    // The driver archives arrive as one artifact directory per platform rather
    // than one per file: six jobs each package four drivers, and naming
    // twenty-four separate uploads would be twenty-four more chances for a
    // filename to drift out of step with what the release expects.
    for target in assets::DRIVER_TARGETS {
        let artifact = assets::drivers_artifact(target);
        script_lines.push(format!("mv ./artifacts/{artifact}/* release-artifacts/"));
    }

    named::bash!(&script_lines.join("\n"))
}

/// The `release.yml`-only split of `prep_release_artifacts` above: the six app assets go
/// into `release-artifacts/`, exactly as before, but the driver archives land in their own
/// `driver-artifacts/` directory instead. The remote server archives go to
/// `release-artifacts/` too; only this workflow builds them.
///
/// They must never be reachable through `release-artifacts/*`, because that is exactly the
/// glob `gh release upload` uses -- a driver archive left in that directory would still
/// reach the GitHub release despite `validate_release_assets` no longer checking for it.
pub(crate) fn prep_upload_release_and_driver_artifacts() -> Step<Run> {
    let mut script_lines = vec!["mkdir -p release-artifacts/ driver-artifacts/\n".to_string()];
    for asset in assets::all() {
        let mv_command = format!("mv ./artifacts/{asset}/{asset} release-artifacts/{asset}");
        script_lines.push(mv_command)
    }

    for target in assets::DRIVER_TARGETS {
        let artifact = assets::drivers_artifact(target);
        script_lines.push(format!("mv ./artifacts/{artifact}/* driver-artifacts/"));
    }

    for arch in [Arch::X86_64, Arch::AARCH64] {
        let asset = assets::remote_server_linux(arch);
        script_lines.push(format!(
            "mv ./artifacts/{asset}/{asset} release-artifacts/{asset}"
        ));
    }

    named::bash!(&script_lines.join("\n"))
}

/// Writes the manifest the app reads before it downloads a remote server, from the
/// archives already in `release-artifacts/` -- the bytes `gh release upload` publishes
/// next, so the checksums describe exactly what the app will download.
fn build_remote_server_manifest() -> Step<Run> {
    named::bash!("script/build-remote-server-manifest release-artifacts \"${GITHUB_REF_NAME#v}\"")
}

/// Writes the manifest the app reads to find its drivers.
///
/// Built here rather than in the six bundle jobs because it describes all of
/// them at once, and its checksums are taken from the very bytes about to be
/// uploaded -- a checksum that travelled separately from what it describes is
/// one more thing that can disagree, and detecting disagreement is its job.
/// Reads from `driver-artifacts/`, the same directory
/// `script/publish-drivers-to-web` uploads from, so the checksum is always
/// computed from the exact bytes that reach the backend.
pub(crate) fn build_driver_manifest() -> Step<Run> {
    named::bash!("script/build-driver-manifest driver-artifacts \"${GITHUB_REF_NAME#v}\"")
}

/// Where `cargo build --package xtask` leaves the binary.
const XTASK_BINARY: &str = "target/debug/xtask";

/// Compiles the release tooling, in a step of its own that holds no secret.
///
/// Building runs every build script and proc macro in xtask's dependency tree, so any
/// step that compiles is a step that runs code from the whole tree with its environment.
/// The steps that hold the signing key and the upload token therefore never compile:
/// they run what this one built. `--locked` so the tree that is built is the one the
/// lockfile names, and not whatever a fresh resolve would pick.
fn build_xtask() -> Step<Run> {
    named::bash!("cargo build --package xtask --locked")
}

/// Fails the release at its start unless it can sign the driver manifest.
///
/// Runs the binary `build_xtask` made, which is why the secret is in this step's
/// environment and not in the build's.
fn check_driver_signing_key() -> NamedJob {
    named::job!(
        release_job(&[])
            .runs_on(runners::LINUX_SMALL)
            .add_step(steps::checkout_repo())
            .add_step(steps::cache_rust_dependencies_namespace())
            .add_step(build_xtask())
            .add_step(
                named::bash!(format!("{XTASK_BINARY} driver-manifest check-key"))
                    .add_env(("ZODE_DRIVER_SIGNING_KEY", vars::ZODE_DRIVER_SIGNING_KEY)),
            ),
    )
}

/// Hands the driver archives and the signed manifest to the web backend. The step's
/// env carries the secret token, the signing key and the base URL variable rather than
/// the script resolving them itself, keeping every credential this job touches visible
/// in one place in the generated YAML.
///
/// `ZODE_DRIVER_SIGNING_KEY` is the base64 of the PKCS#8 Ed25519 private key whose public
/// half is built into the app. Without it the script fails before uploading anything, and
/// the app refuses an unsigned manifest, so there is no release without it.
///
/// The script signs with the binary `build_xtask` made, named by `ZODE_XTASK_BIN`, so it
/// compiles nothing while it holds those secrets.
fn publish_drivers_to_web() -> Step<Run> {
    named::bash!("script/publish-drivers-to-web driver-artifacts \"${GITHUB_REF_NAME#v}\"")
        .add_env(("ZODE_DRIVER_UPLOAD_TOKEN", vars::ZODE_DRIVER_UPLOAD_TOKEN))
        .add_env(("ZODE_DRIVER_UPLOAD_URL", vars::ZODE_DRIVER_UPLOAD_URL))
        .add_env(("ZODE_DRIVER_SIGNING_KEY", vars::ZODE_DRIVER_SIGNING_KEY))
        .add_env(("ZODE_XTASK_BIN", XTASK_BINARY))
}

fn upload_release_assets(deps: &[&NamedJob], bundle: &ReleaseBundleJobs) -> NamedJob {
    let mut deps = deps.to_vec();
    deps.extend(bundle.jobs());

    named::job!(
        steps::writes_to_releases(dependant_job(&deps))
            .runs_on(runners::LINUX_MEDIUM)
            // Checked out for `script/build-driver-manifest` and
            // `script/publish-drivers-to-web`. Every other step here works on
            // downloaded artifacts alone, which is why this job had no
            // checkout until the manifest needed one.
            .add_step(steps::checkout_repo())
            // The signer is built from source, from a cold target directory, before
            // anything is downloaded or uploaded: a build that fails here costs nothing,
            // and the steps that hold secrets further down compile nothing.
            .add_step(steps::cache_rust_dependencies_namespace())
            .add_step(build_xtask())
            .add_step(download_workflow_artifacts())
            .add_step(steps::script("ls -lR ./artifacts"))
            .add_step(prep_upload_release_and_driver_artifacts())
            .add_step(build_driver_manifest())
            .add_step(build_remote_server_manifest())
            // `--clobber`: a run that fails after this step -- in the publish, say --
            // leaves the installers attached, and without it the re-run fails here on the
            // assets that are already there, before it ever reaches the step that failed.
            .add_step(
                steps::script("gh release upload \"$GITHUB_REF_NAME\" --repo=\"$GITHUB_REPOSITORY\" --clobber release-artifacts/*")
                    .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN)),
            )
            .add_step(publish_drivers_to_web()),
    )
}

fn create_draft_release(deps: &[&NamedJob]) -> NamedJob {
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
    // the tag commit then names a different version than the release built from it. Warn,
    // never fail -- releasing straight from a tag without a bump commit is a deliberate
    // property of this fork (see script/determine-release-channel). Develop does not stay
    // behind: `bump_manifest_on_develop` moves its manifest up to the tag afterwards.
    fn warn_on_version_divergence() -> Step<Run> {
        named::bash!(indoc::indoc! {r#"
            # The first `version =` in the manifest is the one under [package].
            manifest_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' crates/zed/Cargo.toml | head -1)
            if [ "$manifest_version" = "$RELEASE_VERSION" ]; then
                echo "tag and crates/zed/Cargo.toml agree on ${RELEASE_VERSION}"
            else
                echo "::warning::tag ${GITHUB_REF_NAME} builds as ${RELEASE_VERSION} but crates/zed/Cargo.toml on this commit says ${manifest_version}; the binary reports the tag, and for a stable tag bump_manifest_on_develop moves develop's manifest up to it"
            fi
        "#})
    }

    fn create_release() -> Step<Run> {
        named::bash!("script/create-draft-release target/release-notes.md")
            .add_env(("GITHUB_TOKEN", vars::GITHUB_TOKEN))
    }

    named::job!(
        steps::writes_to_releases(release_job(deps))
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

/// Moves develop's manifest up to a stable tag, so a build from develop reports the
/// version that was last shipped rather than the one before it.
///
/// Waits for the release gate (`deps`), so a tag that fails it moves nothing: the script
/// never moves develop down, which makes a bump for a release that was never made
/// permanent until somebody reverts the bot commit by hand. Nothing waits on the bump,
/// though: it is bookkeeping about a release, and a failed or slow bump must never hold
/// one back.
///
/// Pushed with plain `git push` rather than a commit-signing action. Those build the
/// commit from the remote's head at the moment they run, so a develop push that landed
/// since checkout would lose its changes to these two files, and two tags racing could
/// stack the older version on top of the newer. A non-forced push is a compare-and-swap
/// instead: if develop moved, git rejects it, the job fails loudly, and a re-run decides
/// again from scratch. The cost is no "Verified" badge on the bot commit.
fn bump_manifest_on_develop(deps: &[&NamedJob]) -> NamedJob {
    fn sync_manifest_to_tag() -> Step<Run> {
        named::bash!("script/sync-zode-manifest-to-release-tag \"$GITHUB_REF_NAME\"").id("sync")
    }

    // The version comes in through the environment, never interpolated into the script:
    // it is derived from a tag name, which is text somebody else chose.
    fn commit_and_push(changed: &StepOutput, version: &StepOutput) -> Step<Run> {
        named::bash!(indoc::indoc! {r#"
            git config user.name "github-actions[bot]"
            git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
            git commit -m "chore: bump zode to $VERSION" -- crates/zed/Cargo.toml Cargo.lock
            git push origin HEAD:refs/heads/develop
        "#})
        .if_condition(Expression::new(format!("{} == 'true'", changed.expr())))
        .add_env(("VERSION", version.to_string()))
    }

    let sync_step = sync_manifest_to_tag();
    // The script appends both outputs itself, so they cannot be found in its command line.
    let changed = StepOutput::new_unchecked(&sync_step, "changed");
    let version = StepOutput::new_unchecked(&sync_step, "version");

    named::job!(
        steps::writes_to_releases(release_job(deps))
            // Two lines edited and one push: a run that takes longer is stuck on
            // the network, and the release default would let it hold a runner for
            // hours.
            .timeout_minutes(10u32)
            .runs_on(runners::LINUX_SMALL)
            // The checkout's persisted credentials are what `git push` uses, so this is
            // where the token that may start CI on develop has to be set.
            .add_step(
                steps::checkout_repo()
                    .with_ref("develop")
                    .with_token_expression(TAG_TOKEN),
            )
            .add_step(sync_step)
            .add_step(commit_and_push(&changed, &version)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::workflows::run_bundling::run_bundling;
    use serde_yaml::Value;

    const GATE: [&str; 4] = [
        "run_tests_linux",
        "clippy_linux",
        "check_scripts",
        "check_driver_signing_key",
    ];
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

    fn repo_file(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
    }

    /// The driver ids live in three places that cannot see each other: the app
    /// decides which drivers it knows, a shell script decides which get
    /// packaged, and this workflow decides which get uploaded to the web
    /// backend.
    ///
    /// Drift between them fails in the least useful order. An id the app knows
    /// but nothing packages becomes an engine whose Download button always
    /// fails, discovered by a user. An id packaged but not listed here uploads
    /// unvalidated. Neither shows up until after a release is cut, which is why
    /// it is asserted here instead. This test used to read `BUILT_IN` against
    /// what the *GitHub release* validated; the coupling it protects has not
    /// moved, only its destination has -- the drivers now go to the web
    /// backend instead of a release asset.
    #[test]
    fn the_drivers_uploaded_to_the_web_are_the_ones_the_app_knows() {
        let registry = repo_file("crates/database_ui/src/driver_registry.rs");
        let built_in = registry
            .split_once("const BUILT_IN: &[(&str, &str)] = &[")
            .expect("`BUILT_IN` must still be the app's list of shipped drivers")
            .1
            .split_once("];")
            .expect("an unterminated BUILT_IN")
            .0;

        for id in assets::DRIVER_IDS {
            assert!(
                built_in.contains(&format!("(\"{id}\"")),
                "the pipeline uploads `{id}` to the web, but the app does not list it as a driver it knows"
            );
        }
        let listed = built_in.matches("(\"").count();
        assert_eq!(
            listed,
            assets::DRIVER_IDS.len(),
            "the app knows {listed} drivers but the pipeline uploads {}",
            assets::DRIVER_IDS.len()
        );
    }

    /// The same list again, this time against what actually gets packaged. A
    /// driver missing here is an asset `script/publish-drivers-to-web` waits
    /// for and never gets.
    #[test]
    fn every_uploaded_driver_is_one_the_bundle_scripts_package() {
        let packager = repo_file("script/package-database-drivers");
        let listed = packager
            .split_once("drivers=(")
            .expect("`drivers=(` must still be the packaging list")
            .1
            .split_once(')')
            .expect("an unterminated drivers=()")
            .0;

        let packaged: Vec<&str> = listed.split_whitespace().collect();
        assert_eq!(
            packaged,
            assets::DRIVER_IDS.to_vec(),
            "script/package-database-drivers and assets::DRIVER_IDS disagree"
        );
    }

    /// The manifest is what the app fetches first; without it every download
    /// fails with a message about the backend rather than about the packaging.
    /// Replaces the old `the_manifest_is_validated_like_any_other_asset`: the
    /// manifest is no longer validated as a *release* asset (see
    /// `no_driver_asset_reaches_the_github_release` below), but it is still
    /// exactly one more entry in the list the web upload sends, and the count
    /// this asserts is what `script/publish-drivers-to-web` itself checks
    /// against the backend's response after publishing.
    #[test]
    fn the_upload_list_is_one_archive_per_driver_per_platform_plus_the_manifest() {
        assert!(assets::all_drivers().contains(&assets::DRIVER_MANIFEST.to_string()));
        assert_eq!(
            assets::all_drivers().len(),
            assets::DRIVER_IDS.len() * assets::DRIVER_TARGETS.len() + 1,
            "one archive per driver per platform, plus the manifest"
        );
    }

    /// The formal statement of "drivers no longer reach the GitHub release":
    /// the two lists this workflow builds from must never overlap. A future
    /// edit that quietly re-adds `.chain(assets::all_drivers())` to
    /// `validate_release_assets`, or that renames a driver archive into
    /// `assets::all()`, breaks this test before it breaks a release.
    #[test]
    fn no_driver_asset_reaches_the_github_release() {
        let release_assets: std::collections::HashSet<String> =
            assets::all().into_iter().map(str::to_string).collect();
        let driver_assets: std::collections::HashSet<String> =
            assets::all_drivers().into_iter().collect();

        let overlap: Vec<&String> = release_assets.intersection(&driver_assets).collect();
        assert!(
            overlap.is_empty(),
            "these assets are both release assets and driver uploads: {overlap:?}"
        );

        let workflow = jobs_of(release());
        let validation_script = workflow["validate_release_assets"]["steps"][0]["run"]
            .as_str()
            .expect("validate_release_assets has a run step")
            .to_string();
        assert!(
            !validation_script.contains(assets::DRIVER_MANIFEST),
            "validate_release_assets still mentions the driver manifest: {validation_script}"
        );
    }

    /// The upload step this phase adds, and the guarantee that it reads from
    /// the driver directory while `gh release upload` reads from the release
    /// directory -- so the two globs can never collide even if someone changes
    /// the directory names later without noticing this test.
    #[test]
    fn the_upload_job_publishes_drivers_to_the_web() {
        let jobs = jobs_of(release());
        let steps = jobs["upload_release_assets"]["steps"]
            .as_sequence()
            .expect("upload_release_assets has steps");

        let run_commands: Vec<&str> = steps
            .iter()
            .filter_map(|step| step["run"].as_str())
            .collect();

        let publishes_to_web = run_commands
            .iter()
            .any(|command| command.contains("script/publish-drivers-to-web"));
        assert!(
            publishes_to_web,
            "upload_release_assets never calls script/publish-drivers-to-web: {run_commands:?}"
        );

        let gh_upload = run_commands
            .iter()
            .find(|command| command.contains("gh release upload"))
            .unwrap_or_else(|| panic!("no gh release upload step found: {run_commands:?}"));
        assert!(
            gh_upload.contains("release-artifacts/*"),
            "gh release upload does not glob release-artifacts/*: {gh_upload}"
        );
        assert!(
            gh_upload.contains("--clobber"),
            "a re-run after a partial release fails on the assets already attached: {gh_upload}"
        );
        assert!(
            !gh_upload.contains("driver-artifacts"),
            "gh release upload must not read from driver-artifacts/: {gh_upload}"
        );
    }

    /// The app refuses a manifest that is not signed, and the script refuses to publish
    /// one it cannot sign, so the secret has to reach the step that runs it -- through the
    /// environment, never spliced into the command, where it would be part of the script
    /// text.
    #[test]
    fn the_publish_step_is_given_the_signing_key_through_its_environment() {
        let jobs = jobs_of(release());
        let steps = jobs["upload_release_assets"]["steps"]
            .as_sequence()
            .expect("upload_release_assets has steps");
        let publish = steps
            .iter()
            .find(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|command| command.contains("script/publish-drivers-to-web"))
            })
            .expect("the job publishes the drivers");

        assert!(
            publish["env"]["ZODE_DRIVER_SIGNING_KEY"]
                .as_str()
                .is_some_and(|value| value.contains("secrets.ZODE_DRIVER_SIGNING_KEY")),
            "the publish step does not get the signing key: {:?}",
            publish["env"]
        );
        assert!(
            !publish["run"].as_str().unwrap_or_default().contains("${{"),
            "an expression interpolated into the script is injectable; pass it through env"
        );
        assert_eq!(
            publish["env"]["ZODE_XTASK_BIN"].as_str(),
            Some(XTASK_BINARY),
            "the script must sign with the binary built beforehand, not compile its own"
        );
    }

    /// Building runs every build script and proc macro in xtask's dependency tree, with
    /// the environment of the step that builds. So a step that holds the signing key or
    /// the upload token must compile nothing, and the compile must happen in a step
    /// before it that holds neither -- in every job of the release, not just the one that
    /// is there today.
    #[test]
    fn no_step_that_holds_a_deploy_secret_compiles_anything() {
        const SECRETS: [&str; 2] = ["ZODE_DRIVER_SIGNING_KEY", "ZODE_DRIVER_UPLOAD_TOKEN"];
        let jobs = jobs_of(release());
        let mut holders = 0;
        for (job_name, job) in jobs.as_mapping().expect("jobs is a mapping") {
            let Some(steps) = job["steps"].as_sequence() else {
                continue;
            };
            for (index, step) in steps.iter().enumerate() {
                let holds_secret = SECRETS.iter().any(|secret| !step["env"][*secret].is_null());
                if !holds_secret {
                    continue;
                }
                holders += 1;
                let command = step["run"].as_str().unwrap_or_default();
                assert!(
                    !command.contains("cargo"),
                    "{job_name:?} compiles in a step that holds a secret: {command}"
                );
                let built_before = steps[..index].iter().any(|earlier| {
                    earlier["run"].as_str().is_some_and(|command| {
                        command.contains("cargo build --package xtask --locked")
                    })
                });
                assert!(
                    built_before,
                    "{job_name:?} runs the release tooling without building it first"
                );
            }
            for step in steps {
                let command = step["run"].as_str().unwrap_or_default();
                if command.contains("cargo build --package xtask") {
                    assert!(
                        step["env"].is_null(),
                        "the step that compiles carries an environment: {:?}",
                        step["env"]
                    );
                }
            }
        }
        assert!(
            holders >= 2,
            "expected the publish and the check to hold secrets"
        );
    }

    /// The first thing a release finds out. Without it a missing or mistyped secret
    /// surfaces in the last job, after hours of bundling and with installers already
    /// attached to the release.
    #[test]
    fn the_release_checks_it_can_sign_before_anything_is_created_or_uploaded() {
        const CHECK: &str = "check_driver_signing_key";
        let jobs = jobs_of(release());

        assert!(
            needs_of(&jobs, CHECK).is_empty(),
            "the check must start at once, not behind {:?}",
            needs_of(&jobs, CHECK)
        );
        let steps = jobs[CHECK]["steps"]
            .as_sequence()
            .expect("the check has steps");
        let check = steps
            .iter()
            .find(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|command| command.contains("driver-manifest check-key"))
            })
            .expect("the job checks the key");
        assert!(
            check["env"]["ZODE_DRIVER_SIGNING_KEY"]
                .as_str()
                .is_some_and(|value| value.contains("secrets.ZODE_DRIVER_SIGNING_KEY")),
            "the check does not get the secret: {:?}",
            check["env"]
        );

        for dependent in [
            "create_draft_release",
            "upload_release_assets",
            "bump_manifest_on_develop",
        ] {
            assert!(
                needs_of(&jobs, dependent).contains(&CHECK.to_owned()),
                "{dependent} does not wait for the signing key check: {:?}",
                needs_of(&jobs, dependent)
            );
        }
        assert!(
            BUNDLES
                .iter()
                .all(|bundle| !needs_of(&jobs, bundle).contains(&CHECK.to_owned())),
            "the bundles must stay off the check's path"
        );
    }

    /// The bump is bookkeeping about the release, not part of it: if it could hold a job
    /// back, a develop push that raced it would be able to hold a release back too. It waits
    /// for the release gate all the same, because a tag that fails the gate makes no release
    /// and develop must not move to a version that was never shipped. And it must push
    /// without force, because the rejected push is what stops it from rewinding a develop
    /// that moved while the job ran.
    #[test]
    fn the_manifest_bump_follows_the_release_gate_and_blocks_nothing() {
        const BUMP: &str = "bump_manifest_on_develop";
        let jobs = jobs_of(release());
        let job = &jobs[BUMP];
        assert!(!job.is_null(), "release has no {BUMP} job");

        let waits_for = needs_of(&jobs, BUMP);
        for gate in GATE.into_iter().chain(["create_draft_release"]) {
            assert!(
                waits_for.contains(&gate.to_owned()),
                "{BUMP} does not wait for {gate}, so a tag that fails it still moves develop: {waits_for:?}"
            );
        }
        assert!(
            BUNDLES
                .iter()
                .all(|bundle| !waits_for.contains(&bundle.to_string())),
            "{BUMP} waits on a bundle, which takes hours: {waits_for:?}"
        );
        for (name, _) in jobs.as_mapping().expect("jobs is a mapping") {
            let name = name.as_str().expect("job names are strings");
            assert!(
                !needs_of(&jobs, name).iter().any(|need| need == BUMP),
                "{name} waits on {BUMP}, so a failed bump would hold a release back"
            );
        }

        assert_eq!(
            job["permissions"]["contents"].as_str(),
            Some("write"),
            "{BUMP} cannot push without contents: write"
        );
        assert!(
            job["if"]
                .as_str()
                .is_some_and(|guard| guard.contains("github.repository_owner")),
            "{BUMP} lost the repository owner guard: {:?}",
            job["if"]
        );

        let steps = job["steps"].as_sequence().expect("the job has steps");
        let checkout = steps
            .iter()
            .find(|step| {
                step["uses"]
                    .as_str()
                    .is_some_and(|uses| uses.starts_with("actions/checkout@"))
            })
            .expect("the job checks the repository out");
        assert_eq!(
            checkout["with"]["ref"].as_str(),
            Some("develop"),
            "the bump must start from develop, not from the tag"
        );
        assert_eq!(
            checkout["with"]["token"].as_str(),
            Some(TAG_TOKEN),
            "the push uses the checkout's credentials, so the checkout needs the release token"
        );

        let sync = steps
            .iter()
            .find(|step| step["id"].as_str() == Some("sync"))
            .expect("the script step has the id the push step reads");
        assert!(
            sync["run"].as_str().is_some_and(|command| {
                command.contains("script/sync-zode-manifest-to-release-tag")
                    && command.contains("\"$GITHUB_REF_NAME\"")
            }),
            "the sync step does not run the script on the tag: {:?}",
            sync["run"]
        );

        let push = steps
            .iter()
            .find(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|command| command.contains("git push"))
            })
            .expect("the job pushes");
        assert!(
            push["if"]
                .as_str()
                .is_some_and(|condition| condition.contains("steps.sync.outputs.changed == 'true'")),
            "the push is not guarded by the script's `changed` output: {:?}",
            push["if"]
        );
        let push_script = push["run"].as_str().expect("the push step has a script");
        assert!(
            push_script.contains("git push origin HEAD:refs/heads/develop"),
            "the push does not target develop: {push_script}"
        );
        assert!(
            !push_script.contains("--force"),
            "a forced push would rewind a develop that moved during the job: {push_script}"
        );
        assert!(
            !push_script.contains("${{"),
            "an expression interpolated into the script is injectable; pass it through env: {push_script}"
        );
        assert!(
            push["env"]["VERSION"]
                .as_str()
                .is_some_and(|version| version.contains("steps.sync.outputs.version")),
            "the commit message does not read the version the script reported: {:?}",
            push["env"]
        );
    }

    const REMOTE_SERVER_JOBS: [&str; 2] = [
        "bundle_remote_server_linux_x86_64",
        "bundle_remote_server_linux_aarch64",
    ];

    /// The remote server archives reach the release through the same upload as the
    /// installers, so that upload has to wait for the jobs that build them -- otherwise
    /// it runs, finds nothing to move, and the release ships without a remote server.
    #[test]
    fn the_upload_waits_for_both_remote_server_builds() {
        let jobs = jobs_of(release());
        let waits_for = needs_of(&jobs, "upload_release_assets");
        for job in REMOTE_SERVER_JOBS {
            assert!(
                waits_for.contains(&job.to_owned()),
                "upload_release_assets does not wait for {job}: {waits_for:?}"
            );
        }
    }

    /// Built beside the bundles, like them ungated: the upload is what waits for the
    /// gate. A remote server job that waited on a bundle would put an hour-long build
    /// back on the critical path for a binary that takes a fraction of that.
    #[test]
    fn the_remote_server_builds_start_at_once_and_upload_their_archive() {
        let jobs = jobs_of(release());
        for (job, arch) in REMOTE_SERVER_JOBS
            .into_iter()
            .zip([Arch::X86_64, Arch::AARCH64])
        {
            assert!(!jobs[job].is_null(), "release has no {job} job");
            assert!(
                needs_of(&jobs, job).is_empty(),
                "{job} waits on {:?}",
                needs_of(&jobs, job)
            );
            assert!(jobs[job]["if"].is_null(), "{job} carries a condition");
            let steps = jobs[job]["steps"].as_sequence().expect("the job has steps");
            assert!(
                steps.iter().any(|step| step["run"]
                    .as_str()
                    .is_some_and(|command| command.contains("script/determine-release-channel"))),
                "{job} builds the checked-in channel instead of the tag's"
            );
            assert!(
                steps.iter().any(|step| step["run"]
                    .as_str()
                    .is_some_and(|command| command.contains("script/bundle-remote-server-linux"))),
                "{job} never runs script/bundle-remote-server-linux"
            );
            let name = assets::remote_server_linux(arch);
            let upload = steps
                .iter()
                .find(|step| step["with"]["name"].as_str() == Some(name))
                .unwrap_or_else(|| panic!("{job} does not upload {name}"));
            assert_eq!(
                upload["with"]["if-no-files-found"].as_str(),
                Some("error"),
                "a missing archive must fail {job}, not upload nothing"
            );
        }
    }

    #[test]
    fn the_release_is_validated_for_the_remote_server_and_its_manifest() {
        let jobs = jobs_of(release());
        let validation_script = jobs["validate_release_assets"]["steps"][0]["run"]
            .as_str()
            .expect("validate_release_assets has a run step");
        for name in [
            "zed-remote-server-linux-x86_64.gz",
            "zed-remote-server-linux-aarch64.gz",
            "remote-server-manifest.json",
        ] {
            assert!(
                validation_script.contains(&format!("\"{name}\"")),
                "validate_release_assets does not expect {name}: {validation_script}"
            );
        }
    }

    /// The manifest has to be written from `release-artifacts/` before `gh release
    /// upload` globs that directory, or the release carries archives the app cannot
    /// verify and therefore refuses.
    #[test]
    fn the_remote_server_manifest_is_built_before_the_release_upload() {
        let jobs = jobs_of(release());
        let steps = jobs["upload_release_assets"]["steps"]
            .as_sequence()
            .expect("upload_release_assets has steps");
        let position = |needle: &str| {
            steps
                .iter()
                .position(|step| step["run"].as_str().is_some_and(|run| run.contains(needle)))
                .unwrap_or_else(|| panic!("no step runs {needle}"))
        };
        let moved = position(&format!(
            "release-artifacts/{}",
            assets::REMOTE_SERVER_LINUX_AARCH64
        ));
        let manifest = position("script/build-remote-server-manifest release-artifacts");
        let upload = position("gh release upload");
        assert!(
            moved < manifest && manifest < upload,
            "expected move ({moved}) < manifest ({manifest}) < upload ({upload})"
        );
    }

    /// The remote server is compiled from the whole dependency tree, so its jobs run
    /// every build script in it. None of them may hold a secret, not even the ones the
    /// app bundles carry.
    #[test]
    fn the_remote_server_builds_hold_no_secret() {
        let workflow = release().to_string().expect("workflow serialises to YAML");
        let parsed: Value = serde_yaml::from_str(&workflow).expect("generated YAML parses");
        for job in REMOTE_SERVER_JOBS {
            let text = serde_yaml::to_string(&parsed["jobs"][job]).expect("job serialises");
            assert!(!parsed["jobs"][job].is_null(), "release has no {job} job");
            assert!(
                !text.contains("secrets."),
                "{job} is handed a secret:\n{text}"
            );
            assert!(
                parsed["jobs"][job]["permissions"].is_null(),
                "{job} asks for permissions it has no use for"
            );
        }
    }

    /// The client asks for `zed-remote-server-{os}-{arch}.gz`, with the arch spelled as
    /// Rust spells it. These must agree byte for byte with the names the jobs upload.
    #[test]
    fn the_remote_server_asset_names_are_the_ones_the_client_asks_for() {
        for arch in [Arch::X86_64, Arch::AARCH64] {
            assert_eq!(
                assets::remote_server_linux(arch),
                format!("zed-remote-server-linux-{arch}.gz")
            );
        }
        assert!(
            assets::all()
                .iter()
                .all(|asset| !assets::remote_server_release_assets().contains(asset)),
            "the remote server leaked into the list the nightly and the bundles share"
        );
    }

    /// The script runs against a scratch checkout holding only the two files it edits.
    /// Unix only: the script is executed directly through its shebang, which Windows does
    /// not do, and the release job that runs it is on Linux.
    #[cfg(unix)]
    mod manifest_sync {
        use indoc::formatdoc;
        use std::{
            fs,
            path::{Path, PathBuf},
            process::Output,
        };

        struct Checkout {
            root: PathBuf,
        }

        impl Checkout {
            fn new(label: &str, manifest: &str, lockfile: &str) -> Self {
                let root = std::env::temp_dir()
                    .join(format!("zode-manifest-sync-{}-{label}", std::process::id()));
                // A directory left behind by a crashed run must not leak into this one.
                if root.exists() {
                    fs::remove_dir_all(&root).expect("clearing a stale scratch checkout");
                }
                fs::create_dir_all(root.join("crates/zed")).expect("creating the scratch checkout");
                // Built before the writes so `Drop` cleans up even if one of them fails.
                let checkout = Self { root };
                fs::write(checkout.manifest_path(), manifest).expect("writing the manifest");
                fs::write(checkout.lockfile_path(), lockfile).expect("writing the lockfile");
                fs::write(checkout.output_path(), "").expect("creating GITHUB_OUTPUT");
                checkout
            }

            fn manifest_path(&self) -> PathBuf {
                self.root.join("crates/zed/Cargo.toml")
            }

            fn lockfile_path(&self) -> PathBuf {
                self.root.join("Cargo.lock")
            }

            fn output_path(&self) -> PathBuf {
                self.root.join("github-output")
            }

            fn manifest(&self) -> String {
                fs::read_to_string(self.manifest_path()).expect("reading the manifest")
            }

            fn lockfile(&self) -> String {
                fs::read_to_string(self.lockfile_path()).expect("reading the lockfile")
            }

            /// The last value the script appended for `key`, as GitHub would read it.
            fn reported(&self, key: &str) -> Option<String> {
                let output = fs::read_to_string(self.output_path()).expect("reading GITHUB_OUTPUT");
                output
                    .lines()
                    .rev()
                    .find_map(|line| line.strip_prefix(&format!("{key}=")))
                    .map(str::to_owned)
            }
        }

        impl Drop for Checkout {
            fn drop(&mut self) {
                if let Err(error) = fs::remove_dir_all(&self.root) {
                    eprintln!("could not remove {}: {error}", self.root.display());
                }
            }
        }

        /// `decoy` is the version of the lines the script must leave alone. Giving it the
        /// value the real line has before the edit is what catches a replace-everything
        /// implementation: it would drag the decoy along.
        fn manifest(version: &str, decoy: &str) -> String {
            formatdoc! {r#"
                [package]
                name = "zed"
                version = "{version}"
                edition.workspace = true

                [dependencies.decoy]
                version = "{decoy}"
            "#}
        }

        fn lockfile(version: &str, decoy: &str, with_zode: bool) -> String {
            let zode = if with_zode {
                formatdoc! {r#"
                    [[package]]
                    name = "zode"
                    version = "{version}"
                    dependencies = [
                     "zode-db-mysql",
                    ]

                "#}
            } else {
                String::new()
            };
            let header = formatdoc! {r#"
                # This file is automatically @generated by Cargo.
                version = 4

                [[package]]
                name = "serde"
                version = "1.0.219"

            "#};
            let siblings = formatdoc! {r#"
                [[package]]
                name = "zode-db-mysql"
                version = "{decoy}"

                [[package]]
                name = "zode_account"
                version = "{decoy}"
            "#};
            format!("{header}{zode}{siblings}")
        }

        #[allow(
            clippy::disallowed_methods,
            reason = "A test of a shell script has to run it, and nothing else is on this thread"
        )]
        fn run_script(checkout: &Checkout, tag: &str) -> Output {
            let script = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../script/sync-zode-manifest-to-release-tag");
            std::process::Command::new(&script)
                .arg(tag)
                .current_dir(&checkout.root)
                .env("GITHUB_OUTPUT", checkout.output_path())
                .output()
                .unwrap_or_else(|error| panic!("running {}: {error}", script.display()))
        }

        fn describe(output: &Output) -> String {
            format!(
                "{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }

        /// The manifest is only ever lifted to a stable tag. A downgrade would make develop
        /// report an older version than the build before it, and a prerelease tag is not a
        /// version develop should adopt.
        #[test]
        fn the_manifest_sync_script_never_moves_backwards() {
            // (manifest version, tag, version the manifest ends at -- None if it must not move)
            let cases: [(&str, &str, Option<&str>); 11] = [
                ("0.1.1", "v0.1.5", Some("0.1.5")),
                ("0.1.5", "v0.1.5", None),
                ("0.1.6", "v0.1.5", None),
                // Numeric, not lexical: "9" sorts after "10" as text.
                ("0.1.9", "v0.1.10", Some("0.1.10")),
                ("0.1.10", "v0.1.9", None),
                // Past what bash arithmetic holds: a comparison that wrapped would
                // read the larger number as smaller.
                (
                    "0.1.5",
                    "v0.1.99999999999999999999",
                    Some("0.1.99999999999999999999"),
                ),
                ("0.1.99999999999999999999", "v0.1.5", None),
                // `sort -V` ranks the prerelease above the release; semver does not.
                ("0.1.5-pre", "v0.1.5", Some("0.1.5")),
                ("0.1.4", "v0.1.5-beta.1", None),
                ("0.1.4", "release-0.1.5", None),
                // Not valid semver; copied into the manifest it would stop cargo reading it.
                ("0.1.4", "v0.1.05", None),
            ];

            for (index, (manifest_version, tag, bumped_to)) in cases.into_iter().enumerate() {
                let before_manifest = manifest(manifest_version, manifest_version);
                let before_lockfile = lockfile(manifest_version, manifest_version, true);
                let checkout =
                    Checkout::new(&index.to_string(), &before_manifest, &before_lockfile);

                let output = run_script(&checkout, tag);
                let context = format!(
                    "manifest {manifest_version}, tag {tag}: {}",
                    describe(&output)
                );
                assert!(output.status.success(), "the script failed for {context}");

                match bumped_to {
                    Some(version) => {
                        assert_eq!(
                            checkout.reported("changed").as_deref(),
                            Some("true"),
                            "{context}"
                        );
                        assert_eq!(
                            checkout.reported("version").as_deref(),
                            Some(version),
                            "{context}"
                        );
                        assert_eq!(
                            checkout.manifest(),
                            manifest(version, manifest_version),
                            "only the package version may change in the manifest: {context}"
                        );
                        assert_eq!(
                            checkout.lockfile(),
                            lockfile(version, manifest_version, true),
                            "only zode's own stanza may change in the lockfile: {context}"
                        );
                    }
                    None => {
                        assert_eq!(
                            checkout.reported("changed").as_deref(),
                            Some("false"),
                            "{context}"
                        );
                        assert_eq!(checkout.manifest(), before_manifest, "{context}");
                        assert_eq!(checkout.lockfile(), before_lockfile, "{context}");
                    }
                }
            }

            // A lockfile with no `zode` stanza means the script cannot keep the two files in
            // step. It must fail and edit neither, rather than bump the manifest alone and
            // leave a lock that `cargo --locked` then rejects.
            let before_manifest = manifest("0.1.1", "0.1.1");
            let before_lockfile = lockfile("0.1.1", "0.1.1", false);
            let checkout = Checkout::new("no-stanza", &before_manifest, &before_lockfile);
            let output = run_script(&checkout, "v0.1.5");
            assert!(
                !output.status.success(),
                "a lockfile without zode must fail the script: {}",
                describe(&output)
            );
            assert_eq!(
                checkout.manifest(),
                before_manifest,
                "the manifest was edited anyway"
            );
            assert_eq!(
                checkout.lockfile(),
                before_lockfile,
                "the lockfile was edited anyway"
            );
        }
    }
}
