use std::{
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::RestartManager::{
            CCH_RM_SESSION_KEY, RmEndSession, RmGetList, RmRegisterResources, RmShutdown,
            RmStartSession,
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::{PCWSTR, PWSTR},
};

use crate::windows_impl::WM_JOB_UPDATED;

/// The app binary's name inside the install directory.
///
/// `script/bundle-windows.ps1` copies `zode.exe` to `Zode.exe` and `zed.iss` installs that
/// name, so this is the file the update has to move aside and replace. It was `Zed.exe`
/// here for as long as the helper went unrun: every job below failed to find that file,
/// rolled back, and reported success at rolling back.
///
/// The CLI beside it is `bin\zode.exe` with the `bin\zode` shell shim; installs from
/// before that rename still carry `bin\zed.exe` and `bin\zed`, which the job list retires.
pub(crate) const APP_EXE: &str = "Zode.exe";

/// The app binary of a real Zed installed into the same directory. While it is there, the
/// `bin\zed.exe` and `bin\zed` beside it may be that Zed's CLI, so neither the job list nor
/// the handle release treats them as the pre-rename Zode CLI.
const ZED_EXE: &str = "Zed.exe";

pub(crate) struct Job {
    pub apply: Box<dyn Fn(&Path) -> Result<()> + Send + Sync>,
    pub rollback: Box<dyn Fn(&Path) -> Result<()> + Send + Sync>,
}

impl Job {
    pub fn mkdir(name: &'static Path) -> Self {
        Job {
            apply: Box::new(move |app_dir| {
                let dir = app_dir.join(name);
                std::fs::create_dir_all(&dir)
                    .context(format!("Failed to create directory {}", dir.display()))
            }),
            rollback: Box::new(move |app_dir| {
                let dir = app_dir.join(name);
                std::fs::remove_dir_all(&dir)
                    .context(format!("Failed to remove directory {}", dir.display()))
            }),
        }
    }

    pub fn mkdir_if_exists(name: &'static Path, check: &'static Path) -> Self {
        Job {
            apply: Box::new(move |app_dir| {
                let dir = app_dir.join(name);
                let check = app_dir.join(check);

                if check.exists() {
                    std::fs::create_dir_all(&dir)
                        .context(format!("Failed to create directory {}", dir.display()))?
                }
                Ok(())
            }),
            rollback: Box::new(move |app_dir| {
                let dir = app_dir.join(name);

                if dir.exists() {
                    std::fs::remove_dir_all(&dir)
                        .context(format!("Failed to remove directory {}", dir.display()))?
                }

                Ok(())
            }),
        }
    }

    pub fn move_file(filename: &'static Path, new_filename: &'static Path) -> Self {
        Job {
            apply: Box::new(move |app_dir| {
                let old_file = app_dir.join(filename);
                let new_file = app_dir.join(new_filename);
                log::info!(
                    "Moving file: {}->{}",
                    old_file.display(),
                    new_file.display()
                );

                std::fs::rename(&old_file, new_file)
                    .context(format!("Failed to move file {}", old_file.display()))
            }),
            rollback: Box::new(move |app_dir| {
                let old_file = app_dir.join(filename);
                let new_file = app_dir.join(new_filename);
                log::info!(
                    "Rolling back file move: {}->{}",
                    old_file.display(),
                    new_file.display()
                );

                std::fs::rename(&new_file, &old_file).context(format!(
                    "Failed to rollback file move {}->{}",
                    new_file.display(),
                    old_file.display()
                ))
            }),
        }
    }

    pub fn move_if_exists(filename: &'static Path, new_filename: &'static Path) -> Self {
        Self::move_if_exists_unless(filename, new_filename, None)
    }

    /// Like `move_if_exists`, but leaves the file where it is when `blocker` exists in the
    /// app directory.
    pub fn move_if_exists_unless(
        filename: &'static Path,
        new_filename: &'static Path,
        blocker: Option<&'static Path>,
    ) -> Self {
        Job {
            apply: Box::new(move |app_dir| {
                let old_file = app_dir.join(filename);
                let new_file = app_dir.join(new_filename);

                if let Some(blocker) = blocker.map(|blocker| app_dir.join(blocker))
                    && blocker.exists()
                {
                    log::info!(
                        "Leaving {} in place: {} exists",
                        old_file.display(),
                        blocker.display()
                    );
                    return Ok(());
                }

                if old_file.exists() {
                    log::info!(
                        "Moving file: {}->{}",
                        old_file.display(),
                        new_file.display()
                    );

                    std::fs::rename(&old_file, new_file)
                        .context(format!("Failed to move file {}", old_file.display()))?;
                }

                Ok(())
            }),
            rollback: Box::new(move |app_dir| {
                let old_file = app_dir.join(filename);
                let new_file = app_dir.join(new_filename);

                if new_file.exists() {
                    log::info!(
                        "Rolling back file move: {}->{}",
                        old_file.display(),
                        new_file.display()
                    );

                    std::fs::rename(&new_file, &old_file).context(format!(
                        "Failed to rollback file move {}->{}",
                        new_file.display(),
                        old_file.display()
                    ))?
                }

                Ok(())
            }),
        }
    }

    pub fn rmdir_nofail(filename: &'static Path) -> Self {
        Job {
            apply: Box::new(move |app_dir| {
                let filename = app_dir.join(filename);
                log::info!("Removing file: {}", filename.display());
                if let Err(e) = std::fs::remove_dir_all(&filename) {
                    log::warn!("Failed to remove directory: {}", e);
                }

                Ok(())
            }),
            rollback: Box::new(move |app_dir| {
                let filename = app_dir.join(filename);
                anyhow::bail!(
                    "Delete operations cannot be rolled back, file: {}",
                    filename.display()
                )
            }),
        }
    }
}

pub(crate) static JOBS: LazyLock<[Job; 26]> = LazyLock::new(|| {
    fn p(value: &str) -> &Path {
        Path::new(value)
    }
    // A real Zed installed into the same directory names its CLI `bin\zed.exe` too, so the
    // pre-rename CLI is only retired when the directory holds no `Zed.exe`.
    let zed_shares_this_dir = Some(p(ZED_EXE));
    [
        // Move old files
        // Not deleting because installing new files can fail
        Job::mkdir(p("old")),
        Job::move_file(p("Zode.exe"), p("old\\Zode.exe")),
        Job::mkdir(p("old\\bin")),
        Job::move_if_exists_unless(
            p("bin\\zed.exe"),
            p("old\\bin\\zed.exe"),
            zed_shares_this_dir,
        ),
        Job::move_if_exists_unless(p("bin\\zed"), p("old\\bin\\zed"), zed_shares_this_dir),
        Job::move_if_exists(p("bin\\zode.exe"), p("old\\bin\\zode.exe")),
        Job::move_if_exists(p("bin\\zode"), p("old\\bin\\zode")),
        Job::move_if_exists(
            p("bin\\remote_server.exe"),
            p("old\\bin\\remote_server.exe"),
        ),
        //
        // TODO: remove after a few weeks once everyone is on the new version and this file never exists
        Job::move_if_exists(p("OpenConsole.exe"), p("old\\OpenConsole.exe")),
        Job::mkdir(p("old\\x64")),
        Job::mkdir(p("old\\arm64")),
        Job::move_if_exists(p("x64\\OpenConsole.exe"), p("old\\x64\\OpenConsole.exe")),
        Job::move_if_exists(
            p("arm64\\OpenConsole.exe"),
            p("old\\arm64\\OpenConsole.exe"),
        ),
        //
        Job::move_file(p("conpty.dll"), p("old\\conpty.dll")),
        // Copy new files
        Job::move_file(p("install\\Zode.exe"), p("Zode.exe")),
        Job::move_file(p("install\\bin\\zode.exe"), p("bin\\zode.exe")),
        Job::move_file(p("install\\bin\\zode"), p("bin\\zode")),
        Job::move_file(
            p("install\\bin\\remote_server.exe"),
            p("bin\\remote_server.exe"),
        ),
        //
        Job::mkdir_if_exists(p("x64"), p("install\\x64")),
        Job::mkdir_if_exists(p("arm64"), p("install\\arm64")),
        Job::move_if_exists(
            p("install\\x64\\OpenConsole.exe"),
            p("x64\\OpenConsole.exe"),
        ),
        Job::move_if_exists(
            p("install\\arm64\\OpenConsole.exe"),
            p("arm64\\OpenConsole.exe"),
        ),
        //
        Job::move_file(p("install\\conpty.dll"), p("conpty.dll")),
        // Cleanup installer and updates folder
        Job::rmdir_nofail(p("updates")),
        Job::rmdir_nofail(p("install")),
        // Cleanup old installation
        Job::rmdir_nofail(p("old")),
    ]
});

/// Files that commonly get locked by Explorer or other processes during an update.
///
/// The pre-rename CLI is listed only when the job list retires it, that is, when no real
/// Zed shares the directory. Otherwise Restart Manager would be asked to shut down the
/// processes holding that Zed's own CLI.
fn files_to_release(app_dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        app_dir.join(APP_EXE),
        app_dir.join("bin\\zode.exe"),
        app_dir.join("bin\\zode"),
    ];
    if !app_dir.join(ZED_EXE).exists() {
        files.extend([app_dir.join("bin\\zed.exe"), app_dir.join("bin\\zed")]);
    }
    files.extend([
        app_dir.join("bin\\remote_server.exe"),
        app_dir.join("conpty.dll"),
    ]);
    files
}

/// Attempts to use Windows Restart Manager to release file handles held by other processes
/// (e.g., Explorer.exe) on the files we need to move during the update.
///
/// This is a best-effort operation - if it fails, we'll still try the update and rely on
/// the retry logic.
fn release_file_handles(app_dir: &Path) -> Result<()> {
    let files = files_to_release(app_dir);

    log::info!("Attempting to release file handles using Restart Manager...");

    let mut session: u32 = 0;
    let mut session_key = [0u16; CCH_RM_SESSION_KEY as usize + 1];

    // Start a Restart Manager session
    let err = unsafe {
        RmStartSession(
            &mut session,
            Some(0),
            PWSTR::from_raw(session_key.as_mut_ptr()),
        )
    };
    if err.is_err() {
        anyhow::bail!("RmStartSession failed: {err:?}");
    }

    // Ensure we end the session when done
    let _session_guard = scopeguard::guard(session, |s| {
        let _ = unsafe { RmEndSession(s) };
    });

    // Convert paths to wide strings for Windows API
    let wide_paths: Vec<Vec<u16>> = files
        .iter()
        .filter(|p| p.exists())
        .map(|p| {
            OsStr::new(p)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        })
        .collect();

    if wide_paths.is_empty() {
        log::info!("No files to release handles for");
        return Ok(());
    }

    let pcwstr_paths: Vec<PCWSTR> = wide_paths
        .iter()
        .map(|p| PCWSTR::from_raw(p.as_ptr()))
        .collect();

    // Register the files we want to modify
    let err = unsafe { RmRegisterResources(session, Some(&pcwstr_paths), None, None) };
    if err.is_err() {
        anyhow::bail!("RmRegisterResources failed: {err:?}");
    }

    // Check if any processes are using these files
    let mut needed: u32 = 0;
    let mut count: u32 = 0;
    let mut reboot_reasons: u32 = 0;
    let _ = unsafe { RmGetList(session, &mut needed, &mut count, None, &mut reboot_reasons) };

    if needed == 0 {
        log::info!("No processes are holding handles to the files");
        return Ok(());
    }

    log::info!(
        "{} process(es) are holding handles to the files, requesting release...",
        needed
    );

    // Request processes to release their handles
    // RmShutdown with flags=0 asks applications to release handles gracefully
    // For Explorer, this typically releases icon cache handles without closing Explorer
    let err = unsafe { RmShutdown(session, 0, None) };
    if err.is_err() {
        anyhow::bail!("RmShutdown failed: {:?}", err);
    }

    log::info!("Successfully requested handle release");
    Ok(())
}

pub(crate) fn perform_update(app_dir: &Path, hwnd: Option<isize>, launch: bool) -> Result<()> {
    run_jobs(&*JOBS, app_dir, hwnd, launch)
}

fn run_jobs(jobs: &[Job], app_dir: &Path, hwnd: Option<isize>, launch: bool) -> Result<()> {
    let hwnd = hwnd.map(|ptr| HWND(ptr as _));

    // Try to release file handles before starting the update
    if let Err(e) = release_file_handles(app_dir) {
        log::warn!("Restart Manager failed (will continue anyway): {}", e);
    }

    let mut last_successful_job = None;
    'outer: for (i, job) in jobs.iter().enumerate() {
        let start = Instant::now();
        loop {
            if start.elapsed().as_secs() > 2 {
                log::error!("Timed out, rolling back");
                break 'outer;
            }
            match (job.apply)(app_dir) {
                Ok(_) => {
                    last_successful_job = Some(i);
                    unsafe { PostMessageW(hwnd, WM_JOB_UPDATED, WPARAM(0), LPARAM(0))? };
                    break;
                }
                Err(err) => match err.downcast_ref::<std::io::Error>() {
                    Some(io_err) => match io_err.kind() {
                        std::io::ErrorKind::NotFound => {
                            log::error!("Operation failed with file not found, aborting: {}", err);
                            break 'outer;
                        }
                        _ => {
                            log::error!("Operation failed (retrying): {}", err);
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    },
                    None => {
                        log::error!("Operation failed with unexpected error, aborting: {}", err);
                        break 'outer;
                    }
                },
            }
        }
    }

    if last_successful_job
        .map(|job| job != jobs.len() - 1)
        .unwrap_or(true)
    {
        let Some(last_successful_job) = last_successful_job else {
            anyhow::bail!("Autoupdate failed, nothing to rollback");
        };

        for job in jobs.iter().take(last_successful_job + 1).rev() {
            if let Err(e) = (job.rollback)(app_dir) {
                anyhow::bail!(
                    "Job rollback failed, the app might be left in an inconsistent state: ({:?})",
                    e
                );
            }
        }

        anyhow::bail!("Autoupdate failed, rollback successful");
    }

    if launch {
        #[allow(clippy::disallowed_methods, reason = "doesn't run in the main binary")]
        if let Err(error) = std::process::Command::new(app_dir.join(APP_EXE)).spawn() {
            // The update itself has already succeeded at this point, so this is not a
            // rollback case -- but the user is left staring at a closed editor, and a
            // silent failure here is indistinguishable from the app simply never coming
            // back. Say so in the log the dialog surfaces.
            log::error!("update installed, but relaunching {APP_EXE} failed: {error:#}");
        }
    }
    log::info!("Update completed successfully");
    Ok(())
}

#[cfg(test)]
mod test {
    use std::{path::Path, sync::LazyLock, time::Duration};

    use anyhow::Context as _;

    use super::{JOBS, Job, files_to_release, run_jobs};

    static SIMULATED_JOBS: LazyLock<[Job; 9]> = LazyLock::new(|| {
        fn p(value: &str) -> &Path {
            Path::new(value)
        }
        [
            Job {
                apply: Box::new(|_| {
                    std::thread::sleep(Duration::from_millis(1000));
                    if let Ok(config) = std::env::var("ZED_AUTO_UPDATE") {
                        match config.as_str() {
                            "err1" => {
                                Err(std::io::Error::other("Simulated error")).context("Anyhow!")
                            }
                            "err2" => Ok(()),
                            _ => panic!("Unknown ZED_AUTO_UPDATE value: {}", config),
                        }
                    } else {
                        Ok(())
                    }
                }),
                rollback: Box::new(|_| {
                    unsafe { std::env::set_var("ZED_AUTO_UPDATE_RB", "rollback1") };
                    Ok(())
                }),
            },
            Job::mkdir(p("test1")),
            Job::mkdir_if_exists(p("test_exists"), p("test1")),
            Job::mkdir_if_exists(p("test_missing"), p("dont")),
            Job {
                apply: Box::new(|folder| {
                    std::fs::write(folder.join("test1/test"), "test")?;
                    Ok(())
                }),
                rollback: Box::new(|folder| {
                    std::fs::remove_file(folder.join("test1/test"))?;
                    Ok(())
                }),
            },
            Job::move_file(p("test1/test"), p("test1/moved")),
            Job::move_if_exists(p("test1/test"), p("test1/noop")),
            Job {
                apply: Box::new(|_| {
                    std::thread::sleep(Duration::from_millis(1000));
                    if let Ok(config) = std::env::var("ZED_AUTO_UPDATE") {
                        match config.as_str() {
                            "err1" => Ok(()),
                            "err2" => {
                                Err(std::io::Error::other("Simulated error")).context("Anyhow!")
                            }
                            _ => panic!("Unknown ZED_AUTO_UPDATE value: {}", config),
                        }
                    } else {
                        Ok(())
                    }
                }),
                rollback: Box::new(|_| Ok(())),
            },
            Job::rmdir_nofail(p("test1/nofolder")),
        ]
    });

    #[test]
    fn test_perform_update() {
        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        assert!(run_jobs(&*SIMULATED_JOBS, app_dir, None, false).is_ok());

        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        // Simulate a timeout
        unsafe { std::env::set_var("ZED_AUTO_UPDATE", "err1") };
        let ret = run_jobs(&*SIMULATED_JOBS, app_dir, None, false);
        assert!(
            ret.is_err_and(|e| e.to_string().as_str() == "Autoupdate failed, nothing to rollback")
        );

        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        // Simulate a timeout
        unsafe { std::env::set_var("ZED_AUTO_UPDATE", "err2") };
        let ret = run_jobs(&*SIMULATED_JOBS, app_dir, None, false);
        assert!(
            ret.is_err_and(|e| e.to_string().as_str() == "Autoupdate failed, rollback successful")
        );
        assert!(std::env::var("ZED_AUTO_UPDATE_RB").is_ok_and(|e| e == "rollback1"));
    }

    fn write(app_dir: &Path, relative: &str, contents: &str) {
        let path = app_dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn read(app_dir: &Path, relative: &str) -> String {
        std::fs::read_to_string(app_dir.join(relative))
            .unwrap_or_else(|error| panic!("reading {relative}: {error}"))
    }

    /// The layout a 0.1.5 install leaves behind, with an update staged in `install\` the
    /// way `zed.iss` stages one.
    fn stage_update_over_0_1_5_install(app_dir: &Path) {
        write(app_dir, "Zode.exe", "old app");
        write(app_dir, "bin\\zed.exe", "old cli");
        write(app_dir, "bin\\zed", "old shim");
        write(app_dir, "bin\\remote_server.exe", "old server");
        write(app_dir, "x64\\OpenConsole.exe", "old console");
        write(app_dir, "conpty.dll", "old conpty");

        write(app_dir, "install\\Zode.exe", "new app");
        write(app_dir, "install\\bin\\zode.exe", "new cli");
        write(app_dir, "install\\bin\\zode", "new shim");
        write(app_dir, "install\\bin\\remote_server.exe", "new server");
        write(app_dir, "install\\x64\\OpenConsole.exe", "new console");
        write(app_dir, "install\\conpty.dll", "new conpty");
        write(app_dir, "updates\\versions.txt", "0.1.6");
    }

    #[test]
    fn the_job_list_upgrades_a_0_1_5_layout() {
        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        stage_update_over_0_1_5_install(app_dir);

        run_jobs(&*JOBS, app_dir, None, false).unwrap();

        assert_eq!(read(app_dir, "Zode.exe"), "new app");
        assert_eq!(read(app_dir, "bin\\zode.exe"), "new cli");
        assert_eq!(read(app_dir, "bin\\zode"), "new shim");
        assert_eq!(read(app_dir, "bin\\remote_server.exe"), "new server");
        assert_eq!(read(app_dir, "x64\\OpenConsole.exe"), "new console");
        assert_eq!(read(app_dir, "conpty.dll"), "new conpty");
        for gone in ["bin\\zed.exe", "bin\\zed", "old", "install", "updates"] {
            assert!(!app_dir.join(gone).exists(), "{gone} should be gone");
        }
    }

    #[test]
    fn a_dir_shared_with_zed_keeps_its_zed_cli() {
        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        stage_update_over_0_1_5_install(app_dir);
        write(app_dir, "Zed.exe", "real zed");

        run_jobs(&*JOBS, app_dir, None, false).unwrap();

        assert_eq!(read(app_dir, "Zed.exe"), "real zed");
        assert_eq!(read(app_dir, "bin\\zed.exe"), "old cli");
        assert_eq!(read(app_dir, "bin\\zed"), "old shim");
        assert_eq!(read(app_dir, "bin\\zode.exe"), "new cli");
        assert_eq!(read(app_dir, "Zode.exe"), "new app");
    }

    #[test]
    fn handle_release_leaves_a_real_zeds_cli_alone() {
        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        let zed_cli = [app_dir.join("bin\\zed.exe"), app_dir.join("bin\\zed")];

        let files = files_to_release(app_dir);
        for file in &zed_cli {
            assert!(
                files.contains(file),
                "{} is the old Zode CLI",
                file.display()
            );
        }

        write(app_dir, "Zed.exe", "real zed");

        let files = files_to_release(app_dir);
        for file in &zed_cli {
            assert!(!files.contains(file), "{} may be Zed's CLI", file.display());
        }
        assert!(files.contains(&app_dir.join("bin\\zode.exe")));
        assert!(files.contains(&app_dir.join("Zode.exe")));
    }

    #[test]
    fn a_missing_new_cli_rolls_back_to_the_old_install() {
        let app_dir = tempfile::tempdir().unwrap();
        let app_dir = app_dir.path();
        stage_update_over_0_1_5_install(app_dir);
        std::fs::remove_file(app_dir.join("install\\bin\\zode.exe")).unwrap();

        let error = run_jobs(&*JOBS, app_dir, None, false).unwrap_err();

        assert_eq!(error.to_string(), "Autoupdate failed, rollback successful");
        assert_eq!(read(app_dir, "Zode.exe"), "old app");
        assert_eq!(read(app_dir, "bin\\zed.exe"), "old cli");
        assert_eq!(read(app_dir, "bin\\zed"), "old shim");
        assert_eq!(read(app_dir, "bin\\remote_server.exe"), "old server");
        assert_eq!(read(app_dir, "x64\\OpenConsole.exe"), "old console");
        assert_eq!(read(app_dir, "conpty.dll"), "old conpty");
        assert_eq!(read(app_dir, "install\\Zode.exe"), "new app");
        assert!(!app_dir.join("bin\\zode.exe").exists());
        assert!(!app_dir.join("old").exists());
    }
}
