//! What a project hosted here costs, measured on this host for a client that
//! can only see the `wsl.exe` or `ssh` processes it started.
//!
//! A project's processes are its language servers, which this server spawned,
//! and everything carrying the project's terminal marker -- see
//! `process_footprint::pids_carrying_marker`. This server itself is left out,
//! as the client leaves out the editor when it measures a local project.

use std::time::{Duration, Instant};

use collections::HashMap;
use gpui::{EntityId, SharedString};
use process_footprint::{FootprintRoots, Pid, ProjectFootprint, SysinfoProcessSampler};

/// How often the process table is enumerated to find a project's processes.
/// Between enumerations only the processes already found are sampled, which
/// is the split the client's own badge makes for the same cost reason.
const DISCOVERY_EVERY: Duration = Duration::from_secs(30);

/// A meter nobody has asked about for this long is dropped. Markers come from
/// clients, and one that reconnects with a new project must not leave the old
/// project's sampler here for the life of the server.
const IDLE_EVICTION: Duration = Duration::from_secs(120);

#[derive(Default)]
pub(crate) struct FootprintMeters {
    meters: HashMap<String, Meter>,
}

struct Meter {
    sampler: SysinfoProcessSampler,
    /// Every process the last pass attributed, fed back in as roots on the
    /// passes that do not enumerate. See `process_footprint::collect`.
    known: Vec<Pid>,
    last_discovery: Option<Instant>,
    last_used: Instant,
}

impl FootprintMeters {
    /// Blocking: enumerates and samples processes, so it belongs on a
    /// background thread.
    pub(crate) fn measure(
        &mut self,
        marker: &str,
        language_server_pids: Vec<Pid>,
        now: Instant,
    ) -> ProjectFootprint {
        self.meters
            .retain(|_, meter| now.saturating_duration_since(meter.last_used) < IDLE_EVICTION);
        let meter = self
            .meters
            .entry(marker.to_string())
            .or_insert_with(|| Meter {
                sampler: SysinfoProcessSampler::new(),
                known: Vec::new(),
                last_discovery: None,
                last_used: now,
            });
        meter.last_used = now;

        let discover = meter
            .last_discovery
            .is_none_or(|last| now.saturating_duration_since(last) >= DISCOVERY_EVERY);
        let mut roots = language_server_pids;
        if discover {
            roots.extend(process_footprint::pids_carrying_marker(marker));
            meter.last_discovery = Some(now);
        } else {
            roots.extend(meter.known.iter().copied());
        }
        roots.sort_unstable();
        roots.dedup();

        let project = FootprintRoots {
            key: EntityId::from(0u64),
            label: SharedString::default(),
            roots,
        };
        let Some((_, footprint, pids)) =
            process_footprint::collect(&mut meter.sampler, &[project], discover)
                .into_iter()
                .next()
        else {
            return ProjectFootprint::default();
        };
        meter.known = pids;
        footprint
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A `sleep` copied out of the system directories: macOS will not show
    /// the environment of a binary it ships.
    #[allow(
        clippy::disallowed_methods,
        reason = "a synchronous test with no executor for the spawn to block"
    )]
    fn marked_sleeper(marker: &str) -> std::process::Child {
        let system_sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .find(|path| std::path::Path::new(path).exists())
            .expect("sleep is on every unix test machine");
        let copy = std::env::temp_dir().join(format!("zode-meter-sleep-{}", std::process::id()));
        if !copy.exists() {
            std::fs::copy(system_sleep, &copy).expect("copying sleep must succeed");
        }
        std::process::Command::new(&copy)
            .arg("30")
            .env(process_footprint::MARKER_VARIABLE, marker)
            .spawn()
            .expect("the copied sleep must start")
    }

    /// Waits out the gap between spawn and exec, when the child still shows
    /// its parent's environment.
    fn measured_until_found(meters: &mut FootprintMeters, marker: &str) -> ProjectFootprint {
        let mut footprint = ProjectFootprint::default();
        for _ in 0..50 {
            footprint = meters.measure(marker, Vec::new(), Instant::now());
            if footprint.memory_bytes.is_some() {
                break;
            }
            meters.meters.clear();
            std::thread::sleep(Duration::from_millis(20));
        }
        footprint
    }

    #[test]
    fn a_terminal_carrying_the_marker_is_measured() {
        let marker = format!("meter-test-{}", std::process::id());
        let mut child = marked_sleeper(&marker);
        let mut meters = FootprintMeters::default();

        let footprint = measured_until_found(&mut meters, &marker);

        child.kill().ok();
        child.wait().ok();
        assert!(
            footprint.memory_bytes.is_some_and(|bytes| bytes > 0),
            "{footprint:?}"
        );
    }

    #[test]
    fn a_marker_nothing_carries_is_not_measured() {
        let mut meters = FootprintMeters::default();
        let footprint = meters.measure("no-process-has-this", Vec::new(), Instant::now());
        assert_eq!(footprint.memory_bytes, None, "nothing found is not zero");
    }

    #[test]
    fn an_idle_meter_is_dropped() {
        let mut meters = FootprintMeters::default();
        let start = Instant::now();
        meters.measure("first", Vec::new(), start);
        meters.measure("second", Vec::new(), start + IDLE_EVICTION);
        assert!(!meters.meters.contains_key("first"));
        assert!(meters.meters.contains_key("second"));
    }
}
