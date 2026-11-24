//! A tray icon on Linux and the BSDs through StatusNotifierItem, the D-Bus
//! protocol KDE, Xfce, Cinnamon, Ubuntu's GNOME and most panels implement.
//! Stock GNOME has no tray at all; there [`SniTray::host_present`] stays false
//! and callers fall back to OS notifications.
//!
//! No GTK: the item is a plain `zbus` object, so this crate also builds and
//! tests on macOS.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context as _, Result};
use futures::StreamExt as _;
use smol::channel::Sender;

/// One image of the icon: width, height and ARGB32 pixels in network byte
/// order, as `IconPixmap` carries them.
pub type Pixmap = (i32, i32, Vec<u8>);

/// What the user did to the item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// A click, at screen coordinates the host chose to report. Wayland hosts
    /// commonly report 0, 0, since a client cannot be told where it is there.
    Activate { x: i32, y: i32 },
}

/// Decodes `png` and scales it to each of `sizes`, as ARGB32 big-endian
/// pixmaps.
pub fn png_to_pixmaps(png: &[u8], sizes: &[u32]) -> Result<Vec<Pixmap>> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .context("the tray icon is not a PNG")?;
    Ok(sizes
        .iter()
        .map(|&size| {
            let scaled = image
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            let mut argb = Vec::with_capacity(scaled.as_raw().len());
            for pixel in scaled.pixels() {
                let [red, green, blue, alpha] = pixel.0;
                argb.extend_from_slice(&[alpha, red, green, blue]);
            }
            (size as i32, size as i32, argb)
        })
        .collect())
}

struct Item {
    id: String,
    title: String,
    pixmaps: Vec<Pixmap>,
    events: Sender<TrayEvent>,
}

impl Item {
    fn send(&self, x: i32, y: i32) {
        if let Err(error) = self.events.try_send(TrayEvent::Activate { x, y }) {
            log::debug!("tray click dropped: {error}");
        }
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, x: i32, y: i32) {
        self.send(x, y);
    }

    /// Right click. The panel is the only thing this item opens, so it opens
    /// that too, the same as a left click.
    fn secondary_activate(&self, x: i32, y: i32) {
        self.send(x, y);
    }

    fn context_menu(&self, x: i32, y: i32) {
        self.send(x, y);
    }

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        &self.id
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        &self.title
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        self.pixmaps.clone()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        (String::new(), Vec::new(), self.title.clone(), String::new())
    }

    /// No DBusMenu: a click opens the app's own panel.
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn status_notifier_host_registered(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn status_notifier_host_unregistered(&self) -> zbus::Result<()>;
}

const ITEM_PATH: &str = "/StatusNotifierItem";

/// The item while it is shown. Dropping it removes the icon: the connection
/// closes, and the watcher drops items whose owner left the bus.
pub struct SniTray {
    _connection: zbus::Connection,
    host_present: Arc<AtomicBool>,
    _watch: smol::Task<()>,
}

impl SniTray {
    /// Exports the item and registers it with the watcher, now and whenever a
    /// watcher (re)appears. Clicks arrive on `events`.
    pub async fn new(
        id: &str,
        title: &str,
        pixmaps: Vec<Pixmap>,
        events: Sender<TrayEvent>,
    ) -> Result<Self> {
        let item = Item {
            id: id.to_string(),
            title: title.to_string(),
            pixmaps,
            events,
        };
        let service = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let connection = zbus::connection::Builder::session()?
            .name(service.as_str())?
            .serve_at(ITEM_PATH, item)?
            .build()
            .await
            .context("could not export the tray item on the session bus")?;

        let host_present = Arc::new(AtomicBool::new(false));
        let watch = smol::spawn(watch_watcher(
            connection.clone(),
            service,
            host_present.clone(),
        ));
        Ok(Self {
            _connection: connection,
            host_present,
            _watch: watch,
        })
    }

    /// Whether a panel that shows tray items is running right now.
    pub fn host_present(&self) -> bool {
        self.host_present.load(Ordering::Relaxed)
    }
}

/// Registers with the watcher and keeps `host_present` current, re-registering
/// whenever the watcher's owner changes (a panel restart).
async fn watch_watcher(
    connection: zbus::Connection,
    service: String,
    host_present: Arc<AtomicBool>,
) {
    let register = async |connection: &zbus::Connection| -> zbus::Result<()> {
        let watcher = StatusNotifierWatcherProxy::new(connection).await?;
        watcher.register_status_notifier_item(&service).await?;
        let present = watcher.is_status_notifier_host_registered().await?;
        host_present.store(present, Ordering::Relaxed);
        Ok(())
    };
    if let Err(error) = register(&connection).await {
        log::info!("no StatusNotifierWatcher, so no tray icon: {error}");
    }

    let Ok(bus) = zbus::fdo::DBusProxy::new(&connection).await else {
        return;
    };
    let Ok(owner_changes) = bus
        .receive_name_owner_changed_with_args(&[(0, "org.kde.StatusNotifierWatcher")])
        .await
    else {
        return;
    };
    enum Signal {
        WatcherOwner(bool),
        Host(bool),
    }
    let mut streams: Vec<futures::stream::BoxStream<'static, Signal>> = vec![
        owner_changes
            .map(|change| {
                Signal::WatcherOwner(
                    change
                        .args()
                        .map(|args| args.new_owner().is_some())
                        .unwrap_or(false),
                )
            })
            .boxed(),
    ];
    if let Ok(watcher) = StatusNotifierWatcherProxy::new(&connection).await {
        if let Ok(registered) = watcher.receive_status_notifier_host_registered().await {
            streams.push(registered.map(|_| Signal::Host(true)).boxed());
        }
        if let Ok(unregistered) = watcher.receive_status_notifier_host_unregistered().await {
            streams.push(unregistered.map(|_| Signal::Host(false)).boxed());
        }
    }
    let mut signals = futures::stream::select_all(streams);
    while let Some(signal) = signals.next().await {
        match signal {
            Signal::WatcherOwner(has_owner) => {
                host_present.store(false, Ordering::Relaxed);
                if has_owner && let Err(error) = register(&connection).await {
                    log::info!("could not register with the new StatusNotifierWatcher: {error}");
                }
            }
            Signal::Host(present) => host_present.store(present, Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn pixmaps_are_argb_in_network_byte_order_at_each_size() {
        let pixmaps = png_to_pixmaps(&png(4, 4, [10, 20, 30, 200]), &[2, 3]).unwrap();
        assert_eq!(pixmaps.len(), 2);
        let (width, height, bytes) = &pixmaps[0];
        assert_eq!((*width, *height), (2, 2));
        assert_eq!(bytes.len(), 2 * 2 * 4);
        assert_eq!(&bytes[..4], &[200, 10, 20, 30]);
        assert_eq!(pixmaps[1].2.len(), 3 * 3 * 4);
    }

    #[test]
    fn a_non_png_is_refused() {
        assert!(png_to_pixmaps(b"not an image", &[22]).is_err());
    }

    #[test]
    fn every_click_kind_sends_its_coordinates() {
        let (sender, receiver) = smol::channel::bounded(4);
        let item = Item {
            id: "zode".into(),
            title: "Zode".into(),
            pixmaps: Vec::new(),
            events: sender,
        };
        item.activate(10, 20);
        item.secondary_activate(30, 40);
        item.context_menu(50, 60);
        let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
        assert_eq!(
            events,
            [
                TrayEvent::Activate { x: 10, y: 20 },
                TrayEvent::Activate { x: 30, y: 40 },
                TrayEvent::Activate { x: 50, y: 60 },
            ]
        );
    }
}
