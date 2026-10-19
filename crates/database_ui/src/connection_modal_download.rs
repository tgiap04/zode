//! Fetching a driver from inside the dialog that needs it.
//!
//! Zode bundles no drivers, so an engine nobody has connected to before has no
//! driver on the machine. That used to end the dialog: `Continue` set an error
//! saying an extension might provide one -- which was never true of the four
//! engines Zode itself publishes drivers for, and left the person who wanted a
//! database looking for an extension instead.
//!
//! Now it is a step. The download runs where the decision was made, and when it
//! lands the dialog carries on into the same form an already-installed engine
//! would have reached. Kept out of `connection_modal.rs`, which is long enough
//! that adding a fourth screen to it would be the wrong kind of convenient.

use crate::connection_modal::{ConnectionModal, Step};
use crate::driver_registry::{self, CatalogueEntry};
use database::install::{DriverInstaller, InstallProgress};
use futures::{StreamExt as _, select_biased};
use gpui::{Context, Window};
use std::sync::Arc;
use ui::prelude::*;

/// A place in line for a download that other windows may be waiting on too,
/// given up if this is dropped before the answer came.
///
/// The installer counts the callers waiting and stops a download only when the
/// last of them leaves, so every way a window can stop waiting has to say so:
/// pressing Cancel, but also Esc, a click outside, or the window closing, none
/// of which go through Cancel. Tying it to the task that waits means they all
/// do, and a Cancel pressed afterwards still counts for something.
struct Waiting {
    installer: Arc<DriverInstaller>,
    id: String,
    answered: bool,
}

impl Waiting {
    /// The download finished, one way or the other: nothing is left to give up.
    fn answered(&mut self) {
        self.answered = true;
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if !self.answered {
            self.installer.forget(&self.id);
        }
    }
}

impl ConnectionModal {
    /// Downloads the driver for `engine`, then continues into its form.
    ///
    /// Success walks the same path an installed engine takes -- no second
    /// click, because there was never a second decision.
    pub(crate) fn download_driver(
        &mut self,
        engine: CatalogueEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = engine.driver.to_string();
        let installer = driver_registry::installer(cx);
        // Registered before the install is asked for, so a second window
        // joining a download already running still sees the bar move.
        let mut progress = installer.watch(&id);
        let install = installer.install(&id);
        let mut waiting = Waiting {
            installer,
            id: id.clone(),
            answered: false,
        };

        self.error = None;
        self.step = Step::Downloading {
            engine: engine.clone(),
            progress: InstallProgress::FetchingManifest,
        };
        cx.notify();

        self.set_task(cx.spawn_in(window, async move |this, cx| {
            let mut install = std::pin::pin!(install);
            let outcome = loop {
                select_biased! {
                    step = progress.next() => {
                        let Some(step) = step else { continue };
                        this.update(cx, |this, cx| {
                            if let Step::Downloading { progress, .. } = &mut this.step {
                                *progress = step;
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                    outcome = install => break outcome,
                }
            };
            waiting.answered();

            this.update_in(cx, |this, window, cx| match outcome {
                Ok(_) => {
                    // The registry is what every other path reads, so it is
                    // updated before anything acts on the driver being here.
                    driver_registry::refresh(&id, cx);
                    this.reload_engines(cx);
                    let engine = CatalogueEntry {
                        installed: true,
                        downloaded: true,
                        hand_placed: false,
                        ..engine
                    };
                    this.open(engine, window, cx);
                }
                Err(error) => {
                    this.step = Step::Unreachable {
                        engine,
                        message: format!("{error:#}").into(),
                    };
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Abandons a download and goes back to the engine list.
    ///
    /// Dropping the task is what gives up this window's place in the download
    /// (see [`Waiting`]), and what stops it if nobody else is waiting. Nothing
    /// half-installed survives: the archive is unpacked into a staging directory
    /// and moved into place in one rename, and what a stopped download had
    /// written is removed with it.
    pub(crate) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        self.set_task(None);
        self.step = Step::PickEngine;
        cx.notify();
    }

    /// Tries the failed download again.
    ///
    /// Reachable only from [`Step::Unreachable`] on an engine that is still not
    /// installed -- a driver that failed to *start* is a different failure, and
    /// offering to download one already on disk would not fix it.
    pub(crate) fn retry_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Unreachable { engine, .. } = &self.step else {
            return;
        };
        let engine = engine.clone();
        if engine.installed {
            return;
        }
        self.download_driver(engine, window, cx);
    }

    /// Deletes the driver an engine has in the download store, after asking.
    ///
    /// Asked about because the driver may serve more than the engine clicked:
    /// CockroachDB and PostgreSQL share one, and removing it for one removes it
    /// for both. A driver Zode downloaded is a click from coming back, so that
    /// is all that is asked. One with no marker may have been placed by hand --
    /// or downloaded by an earlier Zode, which Zode cannot tell -- so the
    /// question names what is about to be deleted and says it may not be
    /// downloadable again.
    pub(crate) fn remove_driver(
        &mut self,
        engine: CatalogueEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !engine.downloaded {
            return;
        }
        let driver = engine.driver.clone();
        let driver_name = driver_registry::global(cx)
            .read(cx)
            .get(&driver)
            .map(|descriptor| descriptor.name.to_string())
            .unwrap_or_else(|| engine.name.to_string());
        let (question, detail) = if engine.hand_placed {
            let directory = driver_registry::store_root(cx).join(driver.as_str());
            (
                format!(
                    "Remove the {driver_name} driver from {}?",
                    directory.display()
                ),
                format!(
                    "Everything in {} is deleted. Zode cannot tell whether it downloaded this \
                     driver or you put it there yourself; if you did, it cannot be downloaded \
                     again. Every engine that connects through it will need a driver again. \
                     Connections already open keep working until they are closed.",
                    directory.display()
                ),
            )
        } else {
            (
                format!("Remove the downloaded {driver_name} driver?"),
                "Every engine that connects through it will need it downloaded again. \
                 Connections already open keep working until they are closed."
                    .to_string(),
            )
        };
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &question,
            Some(&detail),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let removal = this.update(cx, |_, cx| driver_registry::remove_download(driver, cx));
            let Ok(removal) = removal else {
                return;
            };
            let outcome = removal.await;
            this.update(cx, |this, cx| {
                match outcome {
                    Ok(()) => this.error = None,
                    Err(error) => {
                        this.error = Some(
                            format!("Could not remove the {driver_name} driver: {error:#}").into(),
                        )
                    }
                }
                this.reload_engines(cx);
            })
            .ok();
        })
        .detach();
    }

    /// What the progress line says, and how far along the bar is.
    ///
    /// `None` for the fraction while the size is unknown: a bar that invents a
    /// position is worse than one that admits it has none.
    pub(crate) fn download_status(progress: &InstallProgress) -> (SharedString, Option<f32>) {
        match progress {
            InstallProgress::FetchingManifest => ("Looking up the driver…".into(), None),
            InstallProgress::Downloading {
                received,
                total: Some(total),
            } if *total > 0 => (
                format!(
                    "Downloading… {} of {}",
                    megabytes(*received),
                    megabytes(*total)
                )
                .into(),
                Some((*received as f32 / *total as f32).clamp(0., 1.)),
            ),
            InstallProgress::Downloading { received, .. } => (
                format!("Downloading… {}", megabytes(*received)).into(),
                None,
            ),
            InstallProgress::Verifying => ("Checking what arrived…".into(), Some(1.)),
            InstallProgress::Unpacking => ("Unpacking…".into(), Some(1.)),
        }
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_with_no_size_to_go_on_claims_no_position() {
        let (label, fraction) = ConnectionModal::download_status(&InstallProgress::Downloading {
            received: 1_048_576,
            total: None,
        });
        assert_eq!(label, "Downloading… 1.0 MB");
        assert_eq!(fraction, None);
    }

    #[test]
    fn a_bar_with_a_size_reports_how_far_along_it_is() {
        let (label, fraction) = ConnectionModal::download_status(&InstallProgress::Downloading {
            received: 2_097_152,
            total: Some(4_194_304),
        });
        assert_eq!(label, "Downloading… 2.0 MB of 4.0 MB");
        assert_eq!(fraction, Some(0.5));
    }

    /// A release that published a zero size must not make the bar divide by it.
    #[test]
    fn a_size_of_zero_is_treated_as_no_size() {
        let (_, fraction) = ConnectionModal::download_status(&InstallProgress::Downloading {
            received: 10,
            total: Some(0),
        });
        assert_eq!(fraction, None);
    }
}
