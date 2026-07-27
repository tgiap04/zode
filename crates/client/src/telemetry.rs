mod event_coalescer;

use crate::TelemetrySettings;
use clock::SystemClock;
use futures::Future;
use gpui::{App, Task};
use parking_lot::Mutex;
use regex::Regex;
use settings::{Settings, SettingsStore};
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Instant;
use std::{sync::Arc, time::Duration};
use worktree::{UpdatedEntriesSet, WorktreeId};

use self::event_coalescer::EventCoalescer;

/// Tracks the machine-level identifiers and settings that the surviving
/// `telemetry::event!` call sites read. Those events are discarded rather than
/// recorded — there is neither an upload path nor a local event log in this
/// fork, so this type holds no event state.
pub struct Telemetry {
    state: Arc<Mutex<TelemetryState>>,
}

struct TelemetryState {
    settings: TelemetrySettings,
    system_id: Option<Arc<str>>,       // Per system
    installation_id: Option<Arc<str>>, // Per app installation (different for dev, nightly, preview, and stable)
    event_coalescer: EventCoalescer,
    worktrees_with_project_type_events_sent: HashSet<WorktreeId>,
}

static DOTNET_PROJECT_FILES_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(global\.json|Directory\.Build\.props|.*\.(csproj|fsproj|vbproj|sln))$").unwrap()
});

#[cfg(target_os = "macos")]
static MACOS_VERSION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\s*\(Build [^)]*[0-9]\))").unwrap());

pub fn os_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        format!("Linux {}", gpui::guess_compositor())
    }
    #[cfg(target_os = "freebsd")]
    {
        format!("FreeBSD {}", gpui::guess_compositor())
    }

    #[cfg(target_os = "windows")]
    {
        "Windows".to_string()
    }
}

/// Note: This might do blocking IO! Only call from background threads
pub fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSProcessInfo;
        let process_info = NSProcessInfo::processInfo();
        let version_nsstring = process_info.operatingSystemVersionString();
        // "Version 15.6.1 (Build 24G90)" -> "15.6.1 (Build 24G90)"
        let version_string = version_nsstring.to_string().replace("Version ", "");
        // "15.6.1 (Build 24G90)" -> "15.6.1"
        // "26.0.0 (Build 25A5349a)" -> unchanged (Beta or Rapid Security Response; ends with letter)
        MACOS_VERSION_REGEX
            .replace_all(&version_string, "")
            .to_string()
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use std::path::Path;

        let content = if let Ok(file) = std::fs::read_to_string(&Path::new("/etc/os-release")) {
            file
        } else if let Ok(file) = std::fs::read_to_string(&Path::new("/usr/lib/os-release")) {
            file
        } else if let Ok(file) = std::fs::read_to_string(&Path::new("/var/run/os-release")) {
            file
        } else {
            log::error!(
                "Failed to load /etc/os-release, /usr/lib/os-release, or /var/run/os-release"
            );
            "".to_string()
        };
        let mut name = "unknown";
        let mut version = "unknown";

        for line in content.lines() {
            match line.split_once('=') {
                Some(("ID", val)) => name = val.trim_matches('"'),
                Some(("VERSION_ID", val)) => version = val.trim_matches('"'),
                _ => {}
            }
        }

        format!("{} {}", name, version)
    }

    #[cfg(target_os = "windows")]
    {
        let mut info = unsafe { std::mem::zeroed() };
        let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut info) };
        if status.is_ok() {
            semver::Version::new(
                info.dwMajorVersion as _,
                info.dwMinorVersion as _,
                info.dwBuildNumber as _,
            )
            .to_string()
        } else {
            "unknown".to_string()
        }
    }
}

impl Telemetry {
    pub fn new(clock: Arc<dyn SystemClock>, cx: &mut App) -> Arc<Self> {
        let state = Arc::new(Mutex::new(TelemetryState {
            settings: *TelemetrySettings::get_global(cx),
            system_id: None,
            installation_id: None,
            event_coalescer: EventCoalescer::new(clock),
            worktrees_with_project_type_events_sent: HashSet::new(),
        }));

        cx.observe_global::<SettingsStore>({
            let state = state.clone();

            move |cx| {
                let mut state = state.lock();
                state.settings = *TelemetrySettings::get_global(cx);
            }
        })
        .detach();

        let this = Arc::new(Self { state });

        // We should only ever have one instance of Telemetry, leak the subscription to keep it alive
        // rather than store in TelemetryState, complicating spawn as subscriptions are not Send
        std::mem::forget(cx.on_app_quit({
            let this = this.clone();
            move |_| this.shutdown_telemetry()
        }));

        this
    }

    #[cfg(any(test, feature = "test-support"))]
    fn shutdown_telemetry(self: &Arc<Self>) -> impl Future<Output = ()> + use<> {
        Task::ready(())
    }

    // Skip calling this function in tests.
    // TestAppContext ends up calling this function on shutdown and it panics when trying to find the TelemetrySettings
    #[cfg(not(any(test, feature = "test-support")))]
    fn shutdown_telemetry(self: &Arc<Self>) -> impl Future<Output = ()> + use<> {
        telemetry::event!("App Closed");
        // TODO: close final edit period and make sure it's sent
        Task::ready(())
    }

    pub fn start(self: &Arc<Self>, system_id: Option<String>, installation_id: Option<String>) {
        let mut state = self.state.lock();
        state.system_id = system_id.map(|id| id.into());
        state.installation_id = installation_id.map(|id| id.into());
    }

    pub fn metrics_enabled(self: &Arc<Self>) -> bool {
        self.state.lock().settings.metrics
    }

    pub fn diagnostics_enabled(self: &Arc<Self>) -> bool {
        self.state.lock().settings.diagnostics
    }

    pub fn log_edit_event(self: &Arc<Self>, environment: &'static str, is_via_ssh: bool) {
        static LAST_EVENT_TIME: Mutex<Option<Instant>> = Mutex::new(None);

        let mut state = self.state.lock();
        let period_data = state.event_coalescer.log_event(environment);
        drop(state);

        if let Some(mut last_event) = LAST_EVENT_TIME.try_lock() {
            let current_time = std::time::Instant::now();
            let last_time = last_event.get_or_insert(current_time);

            if current_time.duration_since(*last_time) > Duration::from_secs(60 * 10) {
                *last_time = current_time;
            } else {
                return;
            }

            if let Some((start, end, environment)) = period_data {
                let duration = end
                    .saturating_duration_since(start)
                    .min(Duration::from_secs(60 * 60 * 24))
                    .as_millis() as i64;

                telemetry::event!(
                    "Editor Edited",
                    duration = duration,
                    environment = environment,
                    is_via_ssh = is_via_ssh
                );
            }
        }
    }

    pub fn report_discovered_project_type_events(
        self: &Arc<Self>,
        worktree_id: WorktreeId,
        updated_entries_set: &UpdatedEntriesSet,
    ) {
        let Some(project_types) = self.detect_project_types(worktree_id, updated_entries_set)
        else {
            return;
        };

        for project_type in project_types {
            telemetry::event!("Project Opened", project_type = project_type);
        }
    }

    fn detect_project_types(
        self: &Arc<Self>,
        worktree_id: WorktreeId,
        updated_entries_set: &UpdatedEntriesSet,
    ) -> Option<Vec<String>> {
        let mut state = self.state.lock();

        if state
            .worktrees_with_project_type_events_sent
            .contains(&worktree_id)
        {
            return None;
        }

        let mut project_types: HashSet<&str> = HashSet::new();

        for (path, _, _) in updated_entries_set.iter() {
            let Some(file_name) = path.file_name() else {
                continue;
            };

            let project_type = match file_name {
                "pnpm-lock.yaml" => Some("pnpm"),
                "yarn.lock" => Some("yarn"),
                "package.json" => Some("node"),
                _ if DOTNET_PROJECT_FILES_REGEX.is_match(file_name) => Some("dotnet"),
                _ => None,
            };

            if let Some(project_type) = project_type {
                project_types.insert(project_type);
            };
        }

        if !project_types.is_empty() {
            state
                .worktrees_with_project_type_events_sent
                .insert(worktree_id);
        }

        let mut project_types: Vec<_> = project_types.into_iter().map(String::from).collect();
        project_types.sort();
        Some(project_types)
    }

    pub fn system_id(self: &Arc<Self>) -> Option<Arc<str>> {
        self.state.lock().system_id.clone()
    }

    pub fn installation_id(self: &Arc<Self>) -> Option<Arc<str>> {
        self.state.lock().installation_id.clone()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use clock::FakeSystemClock;

    use gpui::TestAppContext;
    use util::rel_path::RelPath;
    use worktree::{PathChange, ProjectEntryId, WorktreeId};

    #[gpui::test]
    fn test_project_discovery_does_not_double_report(cx: &mut gpui::TestAppContext) {
        init_test(cx);

        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = cx.update(|cx| Telemetry::new(clock.clone(), cx));
        let worktree_id = 1;

        // Scan of empty worktree finds nothing
        test_project_discovery_helper(telemetry.clone(), vec![], Some(vec![]), worktree_id);

        // Files added, second scan of worktree 1 finds project type
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["package.json"],
            Some(vec!["node"]),
            worktree_id,
        );

        // Third scan of worktree does not double report, as we already reported
        test_project_discovery_helper(telemetry, vec!["package.json"], None, worktree_id);
    }

    #[gpui::test]
    fn test_pnpm_project_discovery(cx: &mut gpui::TestAppContext) {
        init_test(cx);

        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = cx.update(|cx| Telemetry::new(clock.clone(), cx));

        test_project_discovery_helper(
            telemetry,
            vec!["package.json", "pnpm-lock.yaml"],
            Some(vec!["node", "pnpm"]),
            1,
        );
    }

    #[gpui::test]
    fn test_yarn_project_discovery(cx: &mut gpui::TestAppContext) {
        init_test(cx);

        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = cx.update(|cx| Telemetry::new(clock.clone(), cx));

        test_project_discovery_helper(
            telemetry,
            vec!["package.json", "yarn.lock"],
            Some(vec!["node", "yarn"]),
            1,
        );
    }

    #[gpui::test]
    fn test_dotnet_project_discovery(cx: &mut gpui::TestAppContext) {
        init_test(cx);

        let clock = Arc::new(FakeSystemClock::new());
        let telemetry = cx.update(|cx| Telemetry::new(clock.clone(), cx));

        // Using different worktrees, as production code blocks from reporting a
        // project type for the same worktree multiple times

        test_project_discovery_helper(
            telemetry.clone(),
            vec!["global.json"],
            Some(vec!["dotnet"]),
            1,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["Directory.Build.props"],
            Some(vec!["dotnet"]),
            2,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.csproj"],
            Some(vec!["dotnet"]),
            3,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.fsproj"],
            Some(vec!["dotnet"]),
            4,
        );
        test_project_discovery_helper(
            telemetry.clone(),
            vec!["file.vbproj"],
            Some(vec!["dotnet"]),
            5,
        );
        test_project_discovery_helper(telemetry.clone(), vec!["file.sln"], Some(vec!["dotnet"]), 6);

        // Each worktree should only send a single project type event, even when
        // encountering multiple files associated with that project type
        test_project_discovery_helper(
            telemetry,
            vec!["global.json", "Directory.Build.props"],
            Some(vec!["dotnet"]),
            7,
        );
    }

    // TODO:
    // Test settings
    // Update FakeHTTPClient to keep track of the number of requests and assert on it

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    fn test_project_discovery_helper(
        telemetry: Arc<Telemetry>,
        file_paths: Vec<&str>,
        expected_project_types: Option<Vec<&str>>,
        worktree_id_num: usize,
    ) {
        let worktree_id = WorktreeId::from_usize(worktree_id_num);
        let entries: Vec<_> = file_paths
            .into_iter()
            .enumerate()
            .filter_map(|(i, path)| {
                Some((
                    Arc::from(RelPath::unix(path).ok()?),
                    ProjectEntryId::from_proto(i as u64 + 1),
                    PathChange::Added,
                ))
            })
            .collect();
        let updated_entries: UpdatedEntriesSet = Arc::from(entries.as_slice());

        let detected_project_types = telemetry.detect_project_types(worktree_id, &updated_entries);

        let expected_project_types =
            expected_project_types.map(|types| types.iter().map(|&t| t.to_string()).collect());

        assert_eq!(detected_project_types, expected_project_types);
    }
}
