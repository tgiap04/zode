//! Native Xcode projects, on a simulator only: a physical iPhone needs the
//! app signed for it, which Zode does not set up. Both probes run
//! `xcodebuild`, which resolves packages and may run project code, so they
//! run only once the caller has passed the worktree trust check.

use std::time::Duration;

use serde_json::Value;

use crate::host::{HostOs, HostSystem, ToolInvocation, ToolRunner};
use crate::input_validation::{validate_app_path, validate_identifier, validate_xcode_name};
use crate::ios_simulators::XCRUN;
use crate::run_app::{
    AppProject, Framework, PlannedRun, RunPlan, RunRefusal, RunStep, TargetPlatform,
    XcodeContainer, ready_device, run_label,
};
use crate::{Device, DeviceKind, DeviceSnapshot};

pub(crate) const XCODEBUILD: &str = "/usr/bin/xcodebuild";
/// The first probe of a workspace with Swift packages resolves them over the
/// network, which takes minutes on a slow connection.
const PROBE_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) async fn plan_xcode(
    project: &AppProject,
    container: &XcodeContainer,
    device: &Device,
    snapshot: &DeviceSnapshot,
    host: &dyn HostSystem,
    runner: &dyn ToolRunner,
    scheme: Option<&str>,
) -> Result<PlannedRun, RunRefusal> {
    if host.os() != HostOs::MacOs {
        return Err(unsupported("Xcode projects run on a Mac only."));
    }
    if device.kind == DeviceKind::IosPhysical {
        return Err(unsupported(
            "Xcode projects run on a simulator only: an iPhone needs the app signed for it, \
             which Zode does not set up. Run it on this iPhone from Xcode.",
        ));
    }
    let ready = ready_device(device, snapshot)?;
    if ready.platform != TargetPlatform::Ios {
        return Err(RunRefusal::PlatformMissing {
            framework: Framework::IosXcode,
            platform: ready.platform,
        });
    }
    validate_xcode_name("Xcode project", &container.file_name)?;
    let container_path = project
        .directory
        .join(&container.file_name)
        .to_string_lossy()
        .into_owned();
    let flag = container.flag();
    let scheme = match scheme {
        Some(scheme) => scheme.to_string(),
        None => {
            let mut schemes = list_schemes(runner, flag, &container_path).await?;
            match schemes.len() {
                0 => {
                    return Err(unsupported(format!(
                        "{} has no scheme that builds an app.",
                        container.file_name
                    )));
                }
                1 => schemes.remove(0),
                _ => return Ok(PlannedRun::ChooseScheme(schemes)),
            }
        }
    };
    validate_xcode_name("scheme", &scheme)?;
    let destination = format!("id={}", ready.run_id);
    let args = |extra: &[&str]| -> Vec<String> {
        [flag, container_path.as_str(), "-scheme", scheme.as_str()]
            .iter()
            .chain(
                [
                    "-configuration",
                    "Debug",
                    "-destination",
                    destination.as_str(),
                ]
                .iter(),
            )
            .chain(extra)
            .map(|arg| arg.to_string())
            .collect()
    };
    let mut settings_args = vec!["-showBuildSettings".to_string(), "-json".to_string()];
    settings_args.extend(args(&[]));
    let product = read_app_product(runner, settings_args, &scheme).await?;
    validate_app_path(&product.path)?;
    validate_identifier("bundle id", &product.bundle_id)?;

    // The build names the container relative to the project's directory,
    // where the task runs.
    let mut build_args = args(&["build"]);
    build_args[1] = container.file_name.clone();
    let udid = ready.run_id.clone();
    Ok(PlannedRun::Ready(RunPlan {
        label: run_label(project, &ready.device),
        steps: vec![
            RunStep {
                program: XCODEBUILD.to_string(),
                args: build_args,
            },
            RunStep {
                program: XCRUN.to_string(),
                args: vec![
                    "simctl".into(),
                    "install".into(),
                    udid.clone(),
                    product.path,
                ],
            },
            RunStep {
                program: XCRUN.to_string(),
                args: vec![
                    "simctl".into(),
                    "launch".into(),
                    "--terminate-running-process".into(),
                    udid,
                    product.bundle_id,
                ],
            },
        ],
        env: Vec::new(),
        cwd: project.directory.clone(),
        confirmation: None,
        exclusive_per_project: false,
        needs_metro: false,
        open_installed_android_app: None,
    }))
}

fn unsupported(reason: impl Into<String>) -> RunRefusal {
    RunRefusal::Unsupported {
        reason: reason.into(),
    }
}

/// Stdout only: xcodebuild writes notes such as "Writing error result
/// bundle" to stderr, and may print a line before the JSON.
async fn run_xcodebuild(runner: &dyn ToolRunner, args: Vec<String>) -> Result<Value, RunRefusal> {
    let failed = |message: String| RunRefusal::ProjectToolFailed {
        tool: "xcodebuild",
        message,
    };
    let invocation = ToolInvocation::new(XCODEBUILD, args).with_timeout(PROBE_TIMEOUT);
    let output = runner
        .run(invocation)
        .await
        .map_err(|failure| failed(failure.to_string()))?;
    if !output.success() {
        return Err(failed(output.failure_message()));
    }
    serde_json::from_str(json_start(&output.stdout))
        .map_err(|error| failed(format!("its output was not the expected JSON: {error}")))
}

/// From the first line that opens the JSON: a bracket at the start of a line
/// from which the rest parses. Preamble lines hold brackets too, such as
/// `xcodebuild[1234:5678]`, or a destination warning's
/// `{ platform:iOS Simulator, id:... }` on a line of its own.
fn json_start(stdout: &str) -> &str {
    let mut offset = 0;
    for line in stdout.split_inclusive('\n') {
        if line.trim_start().starts_with(['{', '['])
            && serde_json::from_str::<serde::de::IgnoredAny>(&stdout[offset..]).is_ok()
        {
            return &stdout[offset..];
        }
        offset += line.len();
    }
    stdout
}

/// CocoaPods' `Pods-*` schemes build libraries, never the app.
async fn list_schemes(
    runner: &dyn ToolRunner,
    flag: &str,
    container_path: &str,
) -> Result<Vec<String>, RunRefusal> {
    let args = ["-list", "-json", flag, container_path]
        .iter()
        .map(|arg| arg.to_string())
        .collect();
    let listing = run_xcodebuild(runner, args).await?;
    let schemes = listing
        .get("workspace")
        .or_else(|| listing.get("project"))
        .and_then(|container| container.get("schemes"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(schemes
        .iter()
        .filter_map(Value::as_str)
        .filter(|scheme| !scheme.starts_with("Pods-"))
        .map(str::to_string)
        .collect())
}

struct AppProduct {
    target: String,
    path: String,
    bundle_id: String,
}

/// Read before the build: Xcode already knows where the build will put the
/// app. A scheme that also builds extensions lists them too; only `.app`
/// products count, and among several the one named like the scheme.
async fn read_app_product(
    runner: &dyn ToolRunner,
    args: Vec<String>,
    scheme: &str,
) -> Result<AppProduct, RunRefusal> {
    let settings = run_xcodebuild(runner, args).await?;
    let mut products: Vec<AppProduct> = settings
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let setting = |name: &str| entry.get("buildSettings")?.get(name)?.as_str();
            if setting("WRAPPER_EXTENSION")? != "app" {
                return None;
            }
            Some(AppProduct {
                target: entry.get("target")?.as_str()?.to_string(),
                path: format!(
                    "{}/{}",
                    setting("TARGET_BUILD_DIR")?,
                    setting("FULL_PRODUCT_NAME")?
                ),
                bundle_id: setting("PRODUCT_BUNDLE_IDENTIFIER")?.to_string(),
            })
        })
        .collect();
    if products.len() == 1 {
        return Ok(products.remove(0));
    }
    if let Some(index) = products.iter().position(|product| product.target == scheme) {
        return Ok(products.swap_remove(index));
    }
    if products.is_empty() {
        return Err(unsupported(format!("The scheme {scheme} builds no app.")));
    }
    let targets: Vec<&str> = products
        .iter()
        .map(|product| product.target.as_str())
        .collect();
    Err(unsupported(format!(
        "The scheme {scheme} builds several apps ({}), and none is named like the scheme.",
        targets.join(", ")
    )))
}
