//! A host that never leaves the process, whose commands can be held open so a
//! test sees the panel between a click and the command's answer.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use devices::{
    DetachedTool, FakeHost, HostOs, HostSystem as _, ToolFailure, ToolInvocation, ToolOutput,
    ToolRunner,
};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use gpui::{Entity, TestAppContext, VisualTestContext, WeakEntity};
use parking_lot::Mutex;
use settings::Settings as _;

use crate::device_follow::FOLLOW_INTERVAL;
use crate::devices_panel::DevicesPanel;

pub(crate) const ADB: &str = "/sdk/platform-tools/adb";
pub(crate) const EMULATOR: &str = "/sdk/emulator/emulator";
pub(crate) const XCRUN: &str = "/usr/bin/xcrun";
pub(crate) const SIMULATOR_A: &str = "11111111-1111-1111-1111-111111111111";
pub(crate) const SIMULATOR_B: &str = "22222222-2222-2222-2222-222222222222";
pub(crate) const LOG_DIR: &str = "/logs/devices";

type Gate = (Vec<String>, Shared<oneshot::Receiver<()>>);

/// Answers like the host it wraps, but holds back the answer of any command a
/// test chose to hold. The command is recorded when it starts, as a slow
/// command would be.
pub(crate) struct GatedRunner {
    inner: Arc<FakeHost>,
    gates: Mutex<Vec<Gate>>,
}

impl GatedRunner {
    /// Keep the answer until the returned sender is used or dropped.
    pub(crate) fn hold(&self, args: &[&str]) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        self.gates.lock().push((
            args.iter().map(|arg| arg.to_string()).collect(),
            receiver.shared(),
        ));
        sender
    }
}

#[async_trait]
impl ToolRunner for GatedRunner {
    async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolFailure> {
        let gate = self
            .gates
            .lock()
            .iter()
            .find(|(args, _)| *args == invocation.args)
            .map(|(_, receiver)| receiver.clone());
        let result = self.inner.run(invocation).await;
        if let Some(gate) = gate {
            gate.await.ok();
        }
        result
    }

    fn spawn_detached(
        &self,
        invocation: ToolInvocation,
        log_file: &Path,
    ) -> anyhow::Result<DetachedTool> {
        self.inner.spawn_detached(invocation, log_file)
    }

    fn terminate_process_group(&self, process_id: u32) -> anyhow::Result<()> {
        self.inner.terminate_process_group(process_id)
    }
}

pub(crate) struct Rig {
    pub host: Arc<FakeHost>,
    pub runner: Arc<GatedRunner>,
    simulator_lists_scripted: AtomicUsize,
}

impl Rig {
    fn new(os: HostOs) -> Self {
        let host = Arc::new(FakeHost::new(os));
        let runner = Arc::new(GatedRunner {
            inner: host.clone(),
            gates: Mutex::new(Vec::new()),
        });
        Self {
            host,
            runner,
            simulator_lists_scripted: AtomicUsize::new(0),
        }
    }

    /// A Mac with Xcode, two simulators, and no Android SDK. Each call of
    /// `script_simulator_list` adds the answer for one read of the list.
    pub(crate) fn mac() -> Self {
        let rig = Self::new(HostOs::MacOs);
        let host = &rig.host;
        host.script_output(
            "/usr/bin/xcode-select",
            ["-p"],
            0,
            "/Applications/Xcode.app/Contents/Developer\n",
            "",
        );
        host.script_output(
            XCRUN,
            ["simctl", "list", "runtimes", "-j"],
            0,
            r#"{"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-18-0","name":"iOS 18.0","supportedDeviceTypes":[{"identifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-16","productFamily":"iPhone"}]}]}"#,
            "",
        );
        let json_path = host
            .create_temp_dir("devicectl")
            .expect("the fake host makes folders")
            .join("devicectl-devices.json");
        host.script_output(
            XCRUN,
            [
                "devicectl".to_string(),
                "list".to_string(),
                "devices".to_string(),
                "--json-output".to_string(),
                json_path.to_string_lossy().into_owned(),
                "-q".to_string(),
            ],
            1,
            "",
            "xcrun: error: unable to find utility \"devicectl\"",
        );
        for udid in [SIMULATOR_A, SIMULATOR_B] {
            host.script_output(XCRUN, ["simctl", "shutdown", udid], 0, "", "");
            host.script_output(XCRUN, ["simctl", "bootstatus", udid, "-b"], 0, "", "");
            host.script_output(
                "/usr/bin/open",
                ["-a", "Simulator", "--args", "-CurrentDeviceUDID", udid],
                0,
                "",
                "",
            );
        }
        rig
    }

    /// `simctl boot` succeeds for both simulators. Left out of [`Rig::mac`] so
    /// a test can script a failing boot first.
    pub(crate) fn mac_booting() -> Self {
        let rig = Self::mac();
        for udid in [SIMULATOR_A, SIMULATOR_B] {
            rig.host
                .script_output(XCRUN, ["simctl", "boot", udid], 0, "", "");
        }
        rig
    }

    /// One more read of the simulators. The first answer is the panel's own
    /// read of the whole list; every later one is also what following one
    /// simulator sees, which asks for that simulator alone.
    pub(crate) fn script_simulator_list(&self, state_a: &str, state_b: &str) {
        let entry = |udid: &str, name: &str, state: &str| {
            format!(
                r#"{{"udid":"{udid}","name":"{name}","state":"{state}","isAvailable":true,"deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-16"}}"#
            )
        };
        let list = |entries: Vec<String>| {
            format!(
                r#"{{"devices":{{"com.apple.CoreSimulator.SimRuntime.iOS-18-0":[{}]}}}}"#,
                entries.join(",")
            )
        };
        let entry_a = entry(SIMULATOR_A, "iPhone A", state_a);
        let entry_b = entry(SIMULATOR_B, "iPhone B", state_b);
        self.host.script_output(
            XCRUN,
            ["simctl", "list", "devices", "available", "-j"],
            0,
            &list(vec![entry_a.clone(), entry_b.clone()]),
            "",
        );
        if self
            .simulator_lists_scripted
            .fetch_add(1, Ordering::Relaxed)
            > 0
        {
            for (udid, entry) in [(SIMULATOR_A, entry_a), (SIMULATOR_B, entry_b)] {
                self.host.script_output(
                    XCRUN,
                    ["simctl", "list", "devices", "available", "-j", udid],
                    0,
                    &list(vec![entry]),
                    "",
                );
            }
        }
    }

    /// A Linux machine with an Android SDK, two AVDs and one USB phone.
    pub(crate) fn linux() -> Self {
        let rig = Self::new(HostOs::Linux);
        let host = &rig.host;
        host.set_environment_variable("ANDROID_HOME", "/sdk");
        host.add_file(ADB, "");
        host.add_file(EMULATOR, "");
        host.script_output(EMULATOR, ["-list-avds"], 0, "Pixel_8\nPixel_9\n", "");
        host.script_output(
            ADB,
            ["-s", "emulator-5554", "emu", "avd", "name"],
            0,
            "Pixel_8\r\nOK\r\n",
            "",
        );
        host.script_output(
            ADB,
            [
                "-s",
                "emulator-5554",
                "shell",
                "getprop",
                "sys.boot_completed",
            ],
            0,
            "1\n",
            "",
        );
        host.script_output(
            ADB,
            ["-s", "emulator-5554", "emu", "kill"],
            0,
            "OK: killing emulator, bye bye\r\n",
            "",
        );
        rig
    }

    /// One answer of `adb devices -l`; emulator lines optional.
    pub(crate) fn script_adb_devices(&self, lines: &str) {
        self.host.script_output(
            ADB,
            ["devices", "-l"],
            0,
            &format!("List of devices attached\n{lines}"),
            "",
        );
    }

    pub(crate) fn command_count(&self, program: &str, args: &[&str]) -> usize {
        self.host
            .invocations()
            .iter()
            .filter(|invocation| {
                invocation.program.as_path() == Path::new(program)
                    && invocation
                        .args
                        .iter()
                        .map(String::as_str)
                        .eq(args.iter().copied())
            })
            .count()
    }
}

pub(crate) fn init_test(cx: &mut TestAppContext) {
    zlog::init_test();
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        project::DisableAiSettings::register(cx);
        // The factory behind every `InputField` is installed here.
        editor::init(cx);
    });
}

struct KeptProject(#[allow(dead_code)] Entity<project::Project>);

impl gpui::Global for KeptProject {}

/// A panel over `rig`, in a window so it can be drawn, in a local project or a
/// shared (collab) one.
pub(crate) async fn panel_in_window<'a>(
    cx: &'a mut TestAppContext,
    rig: &Rig,
    shared_project: bool,
) -> (Entity<DevicesPanel>, &'a mut VisualTestContext) {
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    if shared_project {
        project.update(cx, |project, _| project.mark_as_collab_for_testing());
    }
    panel_for_project(cx, rig, project)
}

/// A panel over `rig` for a project the test built.
pub(crate) fn panel_for_project<'a>(
    cx: &'a mut TestAppContext,
    rig: &Rig,
    project: Entity<project::Project>,
) -> (Entity<DevicesPanel>, &'a mut VisualTestContext) {
    // The panel holds its project weakly, as the real one does; the workspace
    // that owns the project in the app is not here, so something must.
    cx.update(|cx| cx.set_global(KeptProject(project.clone())));
    let host = rig.host.clone();
    let runner = rig.runner.clone();
    cx.add_window_view(|_window, cx| {
        DevicesPanel::new(
            &project,
            WeakEntity::new_invalid(),
            host,
            runner,
            PathBuf::from(LOG_DIR),
            cx,
        )
    })
}

/// Moves the fake clock in the steps the follower itself uses, so a loop that
/// waits one interval per round gets to run every round.
pub(crate) fn advance(cx: &mut VisualTestContext, total: Duration) {
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        cx.executor().advance_clock(FOLLOW_INTERVAL);
        cx.run_until_parked();
        elapsed += FOLLOW_INTERVAL;
    }
}

pub(crate) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}
