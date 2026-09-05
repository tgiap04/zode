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
use database::install::InstallProgress;
use futures::{StreamExt as _, select_biased};
use gpui::{Context, Window};
use ui::prelude::*;

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

            this.update_in(cx, |this, window, cx| match outcome {
                Ok(_) => {
                    // The registry is what every other path reads, so it is
                    // updated before anything acts on the driver being here.
                    driver_registry::refresh(&id, cx);
                    this.reload_engines(cx);
                    let engine = CatalogueEntry {
                        installed: true,
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
    /// Nothing half-installed survives: the archive is unpacked into a staging
    /// directory and moved into place in one rename, so stopping before that
    /// leaves the store as it was.
    pub(crate) fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Step::Downloading { engine, .. } = &self.step {
            driver_registry::installer(cx).forget(&engine.driver);
        }
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
