//! Finding a project's processes by a variable in their environment.
//!
//! A terminal of a remote project is started by the client through `wsl.exe`
//! or `ssh`, so on the host its shell is not a descendant of anything the
//! remote server owns and no parent chain leads back to the project. The
//! client therefore sets [`MARKER_VARIABLE`] in every such terminal's
//! environment, and children inherit it: an agent started from that shell
//! carries the same marker as the shell itself.

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::Pid;

/// The environment variable a remote project's terminals carry, set to that
/// project's marker.
pub const MARKER_VARIABLE: &str = "ZODE_FOOTPRINT_MARKER";

/// Every process whose environment sets [`MARKER_VARIABLE`] to `marker`.
///
/// A full enumeration that reads environments, so it belongs on the slow
/// discovery cadence. Only processes of this user expose their environment;
/// others are skipped, which is the boundary working as intended. macOS also
/// hides the environment of its own system binaries -- `/bin/zsh` on an SSH
/// host among them -- so there a shell itself is missed while what it starts,
/// an agent's `node` included, is found.
pub fn pids_carrying_marker(marker: &str) -> Vec<Pid> {
    if marker.is_empty() {
        return Vec::new();
    }
    let wanted = format!("{MARKER_VARIABLE}={marker}");
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_environ(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .filter(|(_, process)| {
            process
                .environ()
                .iter()
                .any(|variable| variable.as_os_str() == wanted.as_str())
        })
        .map(|(pid, _)| pid.as_u32())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `sleep` copied out of the system directories: macOS will not show
    /// the environment of a binary it ships, which is not what is under test.
    #[cfg(unix)]
    #[allow(
        clippy::disallowed_methods,
        reason = "a synchronous test with no executor for the spawn to block"
    )]
    fn sleeper(marker: Option<&str>) -> std::process::Child {
        let system_sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .find(|path| std::path::Path::new(path).exists())
            .expect("sleep is on every unix test machine");
        let copy = std::env::temp_dir().join(format!("zode-marker-sleep-{}", std::process::id()));
        if !copy.exists() {
            std::fs::copy(system_sleep, &copy).expect("copying sleep must succeed");
        }
        let mut command = std::process::Command::new(&copy);
        command.arg("30");
        if let Some(marker) = marker {
            command.env(MARKER_VARIABLE, marker);
        }
        command
            .spawn()
            .expect("sleep is on every unix test machine")
    }

    #[cfg(unix)]
    #[test]
    fn a_process_carrying_the_marker_is_found_and_others_are_not() {
        let marker = format!("marker-test-{}", std::process::id());
        let mut marked = sleeper(Some(&marker));
        let mut other = sleeper(Some("some-other-project"));
        let mut unmarked = sleeper(None);

        // A child that has forked but not yet exec'd still shows its parent's
        // environment, so the first look may come too early.
        let mut found = Vec::new();
        for _ in 0..50 {
            found = pids_carrying_marker(&marker);
            if found.contains(&marked.id()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        for child in [&mut marked, &mut other, &mut unmarked] {
            child.kill().ok();
            child.wait().ok();
        }
        assert!(found.contains(&marked.id()), "{found:?}");
        assert!(!found.contains(&other.id()), "another project's marker");
        assert!(!found.contains(&unmarked.id()), "no marker at all");
    }

    #[test]
    fn an_empty_marker_matches_nothing() {
        assert!(pids_carrying_marker("").is_empty());
    }
}
