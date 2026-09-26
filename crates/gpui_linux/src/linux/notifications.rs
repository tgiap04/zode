use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::Arc,
};

use futures::StreamExt as _;
use gpui::{BackgroundExecutor, ForegroundExecutor, Notification, SharedString};
use parking_lot::Mutex;

/// Caps how many in-flight notifications this session tracks for
/// replace-on-repost and click routing. Bounded per `CLAUDE.md`'s rule
/// against unbounded growth: a long-lived agent session posts one of these
/// per finished tab, and nothing here proactively clears an entry beyond
/// what the daemon itself reports closed, so eviction on both
/// `NotificationClosed` and this cap is what keeps the map from growing
/// forever across a session the daemon never tells us about.
const MAX_TRACKED_NOTIFICATIONS: usize = 64;

/// `org.freedesktop.Notifications`, the desktop-notification convention every
/// mainstream Linux notification daemon implements -- `mako`, `dunst`,
/// `swaync` and the rest -- on X11 and Wayland alike, mirroring the
/// `org.freedesktop.ScreenSaver` convention this crate already relies on for
/// `inhibit_screensaver` in `platform.rs`.
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

/// Tracks the D-Bus notification id returned for each of our own
/// [`Notification::id`]s, in both directions: forward so a repost for the
/// same id can pass `replaces_id` instead of stacking a second banner,
/// backward so an `ActionInvoked`/`NotificationClosed` signal (which only
/// carries the D-Bus id) can be routed back to the id our caller knows.
#[derive(Default)]
struct ReplaceIdMap {
    dbus_id_by_our_id: HashMap<SharedString, u32>,
    our_id_by_dbus_id: HashMap<u32, SharedString>,
    /// Insertion order, oldest first, so the cap evicts the longest-idle
    /// entry rather than an arbitrary one.
    order: VecDeque<SharedString>,
}

/// Everything `Platform::post_notification`, `Platform::on_notification_activated`
/// and the listener that connects them need. Held outside the `LinuxClient`
/// generic in `platform.rs` because none of it depends on which backend (X11,
/// Wayland, headless) is running.
#[derive(Default, Clone)]
pub(crate) struct NotificationState {
    replace_ids: Arc<Mutex<ReplaceIdMap>>,
    /// `Rc<RefCell<..>>`, not `Arc<Mutex<..>>`: the registered callback may
    /// close over non-`Send` state (window or entity handles), so it can
    /// never be moved into the background listener task below. Only the
    /// foreground hop task spawned by `start_notification_listener` ever
    /// touches it.
    activated_callback: Rc<RefCell<Option<Box<dyn FnMut(String)>>>>,
    /// Sentinel for "the listener task is already running": `on_notification_activated`
    /// can be called more than once (re-registering a handler), and the
    /// architecture is one long-lived listener, not one per registration.
    listener_started: Rc<Cell<bool>>,
}

/// Returns the D-Bus id to pass as `replaces_id` for `our_id`, or `0` (the
/// protocol's "this is new" sentinel) when nothing has been posted for it yet.
fn lookup_replaces_id(map: &ReplaceIdMap, our_id: &SharedString) -> u32 {
    map.dbus_id_by_our_id.get(our_id).copied().unwrap_or(0)
}

/// Records that `our_id` most recently posted as `dbus_id`, evicting the
/// oldest tracked entry once the map exceeds `MAX_TRACKED_NOTIFICATIONS`.
fn record_replace_id(map: &mut ReplaceIdMap, our_id: SharedString, dbus_id: u32) {
    if let Some(previous_dbus_id) = map.dbus_id_by_our_id.insert(our_id.clone(), dbus_id) {
        map.our_id_by_dbus_id.remove(&previous_dbus_id);
    } else {
        map.order.push_back(our_id.clone());
    }
    map.our_id_by_dbus_id.insert(dbus_id, our_id);

    while map.order.len() > MAX_TRACKED_NOTIFICATIONS {
        let Some(oldest) = map.order.pop_front() else {
            break;
        };
        if let Some(dbus_id) = map.dbus_id_by_our_id.remove(&oldest) {
            map.our_id_by_dbus_id.remove(&dbus_id);
        }
    }
}

/// Resolves a D-Bus notification id (as reported by `ActionInvoked`) back to
/// the id our caller originally posted with.
fn resolve_replace_id(map: &ReplaceIdMap, dbus_id: u32) -> Option<SharedString> {
    map.our_id_by_dbus_id.get(&dbus_id).cloned()
}

/// Drops the tracked entry for `dbus_id`, called on `NotificationClosed` so
/// the map does not hold banners the daemon has already dismissed.
fn evict_replace_id(map: &mut ReplaceIdMap, dbus_id: u32) {
    let Some(our_id) = map.our_id_by_dbus_id.remove(&dbus_id) else {
        return;
    };
    map.dbus_id_by_our_id.remove(&our_id);
    map.order.retain(|id| id != &our_id);
}

/// Posts `notification` on `org.freedesktop.Notifications`, entirely on
/// `background_executor` -- there is nothing for `Platform::post_notification`'s
/// caller to act on if this fails, so the call never blocks the caller and a
/// refused post simply logs and disappears.
pub(crate) fn deliver_notification(
    state: &NotificationState,
    notification: Notification,
    background_executor: BackgroundExecutor,
) {
    let replace_ids = state.replace_ids.clone();
    background_executor
        .spawn(async move {
            let connection = match zbus::Connection::session().await {
                Ok(connection) => connection,
                Err(error) => {
                    log::error!("cannot post notification: no session bus: {error}");
                    return;
                }
            };
            let proxy = match NotificationsProxy::new(&connection).await {
                Ok(proxy) => proxy,
                Err(error) => {
                    log::error!(
                        "cannot post notification: no org.freedesktop.Notifications: {error}"
                    );
                    return;
                }
            };

            let replaces_id = lookup_replaces_id(&replace_ids.lock(), &notification.id);
            let dbus_id = match proxy
                .notify(
                    "Zode",
                    replaces_id,
                    "",
                    &notification.title,
                    &notification.body,
                    &["default", "Open"],
                    HashMap::new(),
                    -1,
                )
                .await
            {
                Ok(id) => id,
                Err(error) => {
                    log::error!("the notification daemon refused the post: {error}");
                    return;
                }
            };
            record_replace_id(&mut replace_ids.lock(), notification.id, dbus_id);
        })
        .detach();
}

/// Stores the callback invoked when the user activates a posted notification,
/// and makes sure the listener that can call it is running.
pub(crate) fn register_notification_activated_callback(
    state: &NotificationState,
    callback: Box<dyn FnMut(String)>,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
) {
    *state.activated_callback.borrow_mut() = Some(callback);
    start_notification_listener(state, background_executor, foreground_executor);
}

/// Starts the single long-lived listener for `ActionInvoked` (routed to the
/// registered callback) and `NotificationClosed` (evicts the id map entry),
/// the first time a callback is registered. A no-op on any later call:
/// re-registering a handler must not spawn a second listener.
fn start_notification_listener(
    state: &NotificationState,
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
) {
    if state.listener_started.replace(true) {
        return;
    }

    let replace_ids = state.replace_ids.clone();
    let (activation_tx, mut activation_rx) = futures::channel::mpsc::unbounded::<String>();

    background_executor
        .spawn(async move {
            let connection = match zbus::Connection::session().await {
                Ok(connection) => connection,
                Err(error) => {
                    log::error!(
                        "cannot listen for notification activations: no session bus: {error}"
                    );
                    return;
                }
            };
            let proxy = match NotificationsProxy::new(&connection).await {
                Ok(proxy) => proxy,
                Err(error) => {
                    log::error!(
                        "cannot listen for notification activations: no org.freedesktop.Notifications: {error}"
                    );
                    return;
                }
            };
            let mut action_invoked = match proxy.receive_action_invoked().await {
                Ok(stream) => stream,
                Err(error) => {
                    log::error!("cannot subscribe to ActionInvoked: {error}");
                    return;
                }
            };
            let mut notification_closed = match proxy.receive_notification_closed().await {
                Ok(stream) => stream,
                Err(error) => {
                    log::error!("cannot subscribe to NotificationClosed: {error}");
                    return;
                }
            };

            loop {
                futures::select_biased! {
                    signal = action_invoked.next() => {
                        let Some(signal) = signal else { break; };
                        let args = match signal.args() {
                            Ok(args) => args,
                            Err(error) => {
                                log::error!("malformed ActionInvoked signal: {error}");
                                continue;
                            }
                        };
                        if args.action_key() != "default" {
                            continue;
                        }
                        let Some(our_id) = resolve_replace_id(&replace_ids.lock(), *args.id()) else {
                            continue;
                        };
                        if let Err(error) = activation_tx.unbounded_send(our_id.to_string()) {
                            log::debug!("dropped a notification activation: {error}");
                        }
                    }
                    signal = notification_closed.next() => {
                        let Some(signal) = signal else { break; };
                        let args = match signal.args() {
                            Ok(args) => args,
                            Err(error) => {
                                log::error!("malformed NotificationClosed signal: {error}");
                                continue;
                            }
                        };
                        evict_replace_id(&mut replace_ids.lock(), *args.id());
                    }
                }
            }
        })
        .detach();

    let activated_callback = state.activated_callback.clone();
    foreground_executor
        .spawn(async move {
            while let Some(id) = activation_rx.next().await {
                if let Some(callback) = activated_callback.borrow_mut().as_mut() {
                    callback(id);
                }
            }
        })
        .detach();
}
