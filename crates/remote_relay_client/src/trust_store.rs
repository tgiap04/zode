//! The devices this machine has agreed to be controlled by.
//!
//! A pin is the whole of the trust: a session is only ever opened for a device
//! whose public key was compared by a person and written here, and the key in
//! the pin — not whatever the relay says the device's key is — is what the
//! encrypted channel is built from.

use std::{future::Future, sync::Arc};

use db::kvp::KeyValueStore;
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{App, AppContext as _, BackgroundExecutor, Task};
use parking_lot::Mutex;
use remote_relay_protocol::KEY_LEN;
use serde::{Deserialize, Serialize};

use crate::device_identity::{decode_public_key, encode_public_key};

/// More pins than this is a sign something is wrong, not a bigger household.
/// The relay itself limits how many browsers one account may register.
pub const MAX_PINNED_DEVICES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedDevice {
    pub device_id: String,
    pub public_key: [u8; KEY_LEN],
    pub name: String,
    /// Seconds since the Unix epoch.
    pub paired_at: u64,
}

#[derive(Serialize, Deserialize)]
struct StoredPin {
    device_id: String,
    public_key: String,
    name: String,
    paired_at: u64,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum TrustError {
    #[error("too many devices are already trusted")]
    TooMany,
    /// The saved pins could not be read at all, so writing now would erase
    /// them, and forgetting one now would leave it trusted after a restart.
    #[error(
        "the saved trusted devices could not be read, so nothing can be changed until they can"
    )]
    Unreadable,
    /// The change holds in memory but did not reach the database, so after a
    /// restart the saved pins are the ones from before it.
    #[error("the trusted devices could not be saved: {0}")]
    NotSaved(String),
}

/// Which way a pin points. The two lists are stored apart and never consulted
/// for each other's purpose: a device this Zode has pinned as a host it may
/// control must not thereby become a device that may control this Zode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustRole {
    /// Devices allowed to control this Zode.
    Controllers,
    /// Zodes this one has paired with in order to control them.
    Hosts,
}

fn storage_key(user_id: &str, role: TrustRole) -> String {
    match role {
        TrustRole::Controllers => format!("remote_control_trust:{user_id}"),
        TrustRole::Hosts => format!("remote_control_hosts:{user_id}"),
    }
}

/// How much of the saved record could be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavedRecord {
    Intact,
    /// Some pins could not be understood and are left out. They are unusable
    /// whatever happens to them, so the next save writes the pins that were
    /// read and lets the others go.
    SomePinsLost,
    /// None of it could be read, so the database may hold any number of pins
    /// that are not known here. Nothing is written over them.
    Unreadable,
}

/// Pins for one account. Held in memory, written through on every change.
pub struct TrustStore {
    user_id: String,
    role: TrustRole,
    pins: Vec<PinnedDevice>,
    saved_record: SavedRecord,
    /// Resolves when the latest write has finished, whether or not it
    /// succeeded. Each write waits for the one before it, so the database sees
    /// the lists in the order they were made even when the writes run on
    /// different threads, and nothing a caller does with a flush can change
    /// that.
    latest_write: Option<Shared<oneshot::Receiver<()>>>,
    /// Runs the waiting that [`TrustStore::flush`] hands out as a task.
    executor: Option<BackgroundExecutor>,
    /// Why the latest write failed, if it did; cleared by the next that
    /// succeeds.
    write_error: Arc<Mutex<Option<TrustError>>>,
}

impl TrustStore {
    pub fn empty(user_id: String) -> Self {
        Self::empty_for(user_id, TrustRole::Controllers)
    }

    pub fn empty_for(user_id: String, role: TrustRole) -> Self {
        Self::with_record(user_id, role, Vec::new(), SavedRecord::Intact)
    }

    fn with_record(
        user_id: String,
        role: TrustRole,
        pins: Vec<PinnedDevice>,
        saved_record: SavedRecord,
    ) -> Self {
        Self {
            user_id,
            role,
            pins,
            saved_record,
            latest_write: None,
            executor: None,
            write_error: Arc::default(),
        }
    }

    /// Reads the saved pins. A record that cannot be read or parsed at all
    /// trusts nothing, which is the safe way to be wrong, and refuses every
    /// change, so that the pins it could not read are neither erased by the
    /// next pairing nor left behind by a forget.
    pub fn load(user_id: String, cx: &App) -> Task<Self> {
        Self::load_for(user_id, TrustRole::Controllers, cx)
    }

    pub fn load_for(user_id: String, role: TrustRole, cx: &App) -> Task<Self> {
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&user_id, role);
        cx.background_spawn(async move {
            let (pins, saved_record) = match kvp.read_kvp(&key) {
                Ok(None) => (Vec::new(), SavedRecord::Intact),
                Ok(Some(raw)) => match serde_json::from_str::<Vec<StoredPin>>(&raw) {
                    Ok(stored) => {
                        let total = stored.len();
                        let pins: Vec<PinnedDevice> = stored
                            .into_iter()
                            .filter_map(|stored| {
                                Some(PinnedDevice {
                                    public_key: decode_public_key(&stored.public_key)?,
                                    device_id: stored.device_id,
                                    name: stored.name,
                                    paired_at: stored.paired_at,
                                })
                            })
                            .take(MAX_PINNED_DEVICES)
                            .collect();
                        if pins.len() == total {
                            (pins, SavedRecord::Intact)
                        } else {
                            log::warn!("some trusted devices could not be understood");
                            (pins, SavedRecord::SomePinsLost)
                        }
                    }
                    Err(error) => {
                        log::warn!("the trusted devices could not be parsed: {error}");
                        (Vec::new(), SavedRecord::Unreadable)
                    }
                },
                Err(error) => {
                    log::warn!("could not read the trusted devices: {error}");
                    (Vec::new(), SavedRecord::Unreadable)
                }
            };
            Self::with_record(user_id, role, pins, saved_record)
        })
    }

    /// Whether any saved pin could not be read, so the list may be shorter
    /// than what the database holds. Only a record that could not be read at
    /// all blocks changes; see [`TrustError::Unreadable`].
    pub fn is_unreadable(&self) -> bool {
        self.saved_record != SavedRecord::Intact
    }

    /// Whether `device_id` could be pinned now, so a ceremony that ends in a
    /// pin is not run for a pin that cannot be written.
    pub fn check_can_pin(&self, device_id: &str) -> Result<(), TrustError> {
        if self.saved_record == SavedRecord::Unreadable {
            return Err(TrustError::Unreadable);
        }
        if self.get(device_id).is_none() && self.pins.len() >= MAX_PINNED_DEVICES {
            return Err(TrustError::TooMany);
        }
        Ok(())
    }

    pub fn devices(&self) -> &[PinnedDevice] {
        &self.pins
    }

    pub fn get(&self, device_id: &str) -> Option<&PinnedDevice> {
        self.pins.iter().find(|pin| pin.device_id == device_id)
    }

    /// Pins a device, replacing an earlier pin for the same id: pairing again
    /// is how a browser's key is rotated.
    pub fn pin(&mut self, device: PinnedDevice, cx: &App) -> Result<(), TrustError> {
        self.check_can_pin(&device.device_id)?;
        if let Some(existing) = self
            .pins
            .iter_mut()
            .find(|pin| pin.device_id == device.device_id)
        {
            *existing = device;
        } else {
            self.pins.push(device);
        }
        self.persist(cx);
        Ok(())
    }

    /// Stops trusting `device_id` and saves that. `Ok(false)` if it was not
    /// pinned. Fails with [`TrustError::Unreadable`], changing nothing, when
    /// the saved record could not be read: a forget that cannot be saved must
    /// not look as if it worked. Whether the write itself succeeded is told by
    /// [`TrustStore::flush_checked`].
    pub fn try_forget(&mut self, device_id: &str, cx: &App) -> Result<bool, TrustError> {
        if self.saved_record == SavedRecord::Unreadable {
            return Err(TrustError::Unreadable);
        }
        let before = self.pins.len();
        self.pins.retain(|pin| pin.device_id != device_id);
        let removed = self.pins.len() != before;
        if removed {
            self.persist(cx);
        }
        Ok(removed)
    }

    /// [`TrustStore::try_forget`] for a caller that only wants to know whether
    /// a pin went away. A forget that could not be saved is logged and counts
    /// as not done.
    pub fn forget(&mut self, device_id: &str, cx: &App) -> bool {
        match self.try_forget(device_id, cx) {
            Ok(removed) => removed,
            Err(error) => {
                log::error!("could not forget a trusted device: {error}");
                false
            }
        }
    }

    /// Stops trusting every device and saves that. Fails like
    /// [`TrustStore::try_forget`].
    pub fn try_forget_all(&mut self, cx: &App) -> Result<(), TrustError> {
        if self.saved_record == SavedRecord::Unreadable {
            return Err(TrustError::Unreadable);
        }
        if !self.pins.is_empty() {
            self.pins.clear();
            self.persist(cx);
        }
        Ok(())
    }

    /// [`TrustStore::try_forget_all`] for a caller with nobody to tell; the
    /// failure is logged.
    pub fn forget_all(&mut self, cx: &App) {
        if let Err(error) = self.try_forget_all(cx) {
            log::error!("could not forget the trusted devices: {error}");
        }
    }

    fn persist(&mut self, cx: &App) {
        let stored: Vec<StoredPin> = self
            .pins
            .iter()
            .map(|pin| StoredPin {
                device_id: pin.device_id.clone(),
                public_key: encode_public_key(&pin.public_key),
                name: pin.name.clone(),
                paired_at: pin.paired_at,
            })
            .collect();
        let value = match serde_json::to_string(&stored) {
            Ok(value) => value,
            Err(error) => {
                log::error!("the trusted devices could not be serialised: {error}");
                *self.write_error.lock() = Some(TrustError::NotSaved(error.to_string()));
                return;
            }
        };
        // What could not be read is dropped by this write.
        self.saved_record = SavedRecord::Intact;
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&self.user_id, self.role);
        let write_error = self.write_error.clone();
        let (finished, finishing) = oneshot::channel();
        let previous = self.latest_write.replace(finishing.shared());
        self.executor = Some(cx.background_executor().clone());
        // Detached, so that the write is neither cancelled by the store being
        // dropped right after a change -- remote control switched off the
        // moment after a device was forgotten -- nor by a caller letting go of
        // a flush.
        cx.background_spawn(async move {
            if let Some(previous) = previous {
                // A write that was cancelled before it finished has nothing
                // left to wait for, so how it ended does not matter here.
                previous.await.ok();
            }
            let outcome = match kvp.write_kvp(key, value).await {
                Ok(()) => None,
                Err(error) => {
                    log::error!("the trusted devices could not be saved: {error:#}");
                    Some(TrustError::NotSaved(format!("{error:#}")))
                }
            };
            *write_error.lock() = outcome;
            if finished.send(()).is_err() {
                log::debug!("nothing was waiting for the trusted devices to be saved");
            }
        })
        .detach();
    }

    /// Resolves once the latest write made so far has reached the database.
    /// Dropping the task does not stop the write, and flushing again, or
    /// changing a pin meanwhile, is fine.
    pub fn flush(&mut self) -> Task<()> {
        let (Some(latest_write), Some(executor)) = (&self.latest_write, &self.executor) else {
            return Task::ready(());
        };
        if latest_write.peek().is_some() {
            return Task::ready(());
        }
        let latest_write = latest_write.clone();
        executor.spawn(async move {
            // Cancelled and finished both mean there is nothing left to wait for.
            latest_write.await.ok();
        })
    }

    /// Resolves once the latest write made so far has finished, with the reason
    /// if it failed. A change whose write failed is still in effect in memory,
    /// and will be undone by the next start.
    pub fn flush_checked(&mut self) -> impl Future<Output = Result<(), TrustError>> + use<> {
        let write = self.flush();
        let write_error = self.write_error.clone();
        async move {
            write.await;
            write_error.lock().clone().map_or(Ok(()), Err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    fn device(id: &str, key_byte: u8) -> PinnedDevice {
        PinnedDevice {
            device_id: id.into(),
            public_key: [key_byte; KEY_LEN],
            name: format!("Browser {id}"),
            paired_at: 1_700_000_000,
        }
    }

    #[gpui::test]
    async fn pins_survive_a_reload(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("pins-survive".into());
        cx.update(|cx| {
            store.pin(device("a", 1), cx).unwrap();
            store.pin(device("b", 2), cx).unwrap();
        });
        store.flush().await;

        let reloaded = cx
            .update(|cx| TrustStore::load("pins-survive".into(), cx))
            .await;
        assert_eq!(reloaded.devices(), &[device("a", 1), device("b", 2)]);
        assert_eq!(reloaded.get("b"), Some(&device("b", 2)));
        assert_eq!(reloaded.get("missing"), None);
    }

    #[gpui::test]
    async fn pairing_again_replaces_the_key_and_forgetting_removes_it(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("replace-forget".into());
        cx.update(|cx| {
            store.pin(device("a", 1), cx).unwrap();
            store.pin(device("a", 9), cx).unwrap();
        });
        assert_eq!(store.devices(), &[device("a", 9)]);

        assert!(cx.update(|cx| store.forget("a", cx)));
        assert!(
            !cx.update(|cx| store.forget("a", cx)),
            "forgetting twice is a no-op"
        );
        store.flush().await;
        let reloaded = cx
            .update(|cx| TrustStore::load("replace-forget".into(), cx))
            .await;
        assert!(reloaded.devices().is_empty());
    }

    #[gpui::test]
    async fn accounts_do_not_share_pins(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("account-one".into());
        cx.update(|cx| store.pin(device("a", 1), cx).unwrap());
        store.flush().await;
        let other = cx
            .update(|cx| TrustStore::load("account-two".into(), cx))
            .await;
        assert!(other.devices().is_empty());
    }

    #[gpui::test]
    async fn hosts_pinned_for_control_never_become_devices_allowed_to_control(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let mut hosts = TrustStore::empty_for("roles".into(), TrustRole::Hosts);
        cx.update(|cx| hosts.pin(device("a-host", 1), cx).unwrap());
        hosts.flush().await;

        let controllers = cx.update(|cx| TrustStore::load("roles".into(), cx)).await;
        assert!(controllers.devices().is_empty());
        let reloaded = cx
            .update(|cx| TrustStore::load_for("roles".into(), TrustRole::Hosts, cx))
            .await;
        assert_eq!(reloaded.devices(), &[device("a-host", 1)]);
    }

    #[gpui::test]
    async fn the_number_of_pins_is_bounded(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("bounded".into());
        cx.update(|cx| {
            for index in 0..MAX_PINNED_DEVICES {
                store.pin(device(&index.to_string(), 1), cx).unwrap();
            }
            assert_eq!(
                store.pin(device("one-too-many", 1), cx),
                Err(TrustError::TooMany)
            );
            // Re-pairing a device that is already pinned needs no new room.
            store.pin(device("0", 5), cx).unwrap();
        });
        assert_eq!(store.devices().len(), MAX_PINNED_DEVICES);
    }

    #[gpui::test]
    async fn a_damaged_record_means_nothing_is_trusted(cx: &mut TestAppContext) {
        init_test(cx);
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        kvp.write_kvp(
            storage_key("damaged", TrustRole::Controllers),
            "{not json".into(),
        )
        .await
        .unwrap();
        let store = cx.update(|cx| TrustStore::load("damaged".into(), cx)).await;
        assert!(store.devices().is_empty());
    }

    #[gpui::test]
    async fn a_record_that_cannot_be_read_is_never_overwritten(cx: &mut TestAppContext) {
        init_test(cx);
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let key = storage_key("unreadable", TrustRole::Hosts);
        kvp.write_kvp(key.clone(), "{not json".into())
            .await
            .unwrap();

        let mut store = cx
            .update(|cx| TrustStore::load_for("unreadable".into(), TrustRole::Hosts, cx))
            .await;
        assert!(store.is_unreadable());
        assert_eq!(
            cx.update(|cx| store.pin(device("a", 1), cx)),
            Err(TrustError::Unreadable)
        );
        assert_eq!(store.check_can_pin("a"), Err(TrustError::Unreadable));
        assert!(store.devices().is_empty());
        store.flush().await;
        assert_eq!(
            kvp.read_kvp(&key).unwrap().as_deref(),
            Some("{not json"),
            "the saved pins were overwritten"
        );
    }

    /// Lets every write made so far reach the database. The writes run on the
    /// database's own thread, which parking the test does not wait for, but
    /// the queue it reads is in order, so a write made now lands behind them.
    async fn wait_for_queued_writes(cx: &mut TestAppContext) {
        cx.run_until_parked();
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        kvp.write_kvp("trust_store_test_sentinel".into(), "done".into())
            .await
            .unwrap();
    }

    const UNDECODABLE_PIN: &str =
        r#"{"device_id":"broken","public_key":"!!","name":"Broken","paired_at":1}"#;

    async fn save_record(
        cx: &mut TestAppContext,
        key: &str,
        devices: &[PinnedDevice],
        extra: &str,
    ) {
        let mut entries: Vec<String> = devices
            .iter()
            .map(|device| {
                serde_json::to_string(&StoredPin {
                    device_id: device.device_id.clone(),
                    public_key: encode_public_key(&device.public_key),
                    name: device.name.clone(),
                    paired_at: device.paired_at,
                })
                .unwrap()
            })
            .collect();
        entries.push(extra.to_string());
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        kvp.write_kvp(key.to_string(), format!("[{}]", entries.join(",")))
            .await
            .unwrap();
    }

    fn saved_ids(cx: &mut TestAppContext, key: &str) -> Vec<String> {
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let raw = kvp.read_kvp(key).unwrap().expect("a saved record");
        serde_json::from_str::<Vec<StoredPin>>(&raw)
            .expect("a readable record")
            .into_iter()
            .map(|pin| pin.device_id)
            .collect()
    }

    #[gpui::test]
    async fn a_pin_is_saved_beside_the_readable_ones_when_another_cannot_be_understood(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let key = storage_key("half-readable", TrustRole::Controllers);
        save_record(cx, &key, &[device("a", 1)], UNDECODABLE_PIN).await;

        let mut store = cx
            .update(|cx| TrustStore::load("half-readable".into(), cx))
            .await;
        assert!(store.is_unreadable(), "the loss is noticed");
        assert_eq!(store.devices(), &[device("a", 1)]);
        assert_eq!(store.check_can_pin("b"), Ok(()));
        cx.update(|cx| store.pin(device("b", 2), cx)).unwrap();
        store.flush_checked().await.unwrap();

        assert_eq!(saved_ids(cx, &key), ["a", "b"]);
        assert!(!store.is_unreadable(), "what could not be read is gone");
        let reloaded = cx
            .update(|cx| TrustStore::load("half-readable".into(), cx))
            .await;
        assert!(!reloaded.is_unreadable());
        assert_eq!(reloaded.devices(), &[device("a", 1), device("b", 2)]);
    }

    #[gpui::test]
    async fn forgetting_is_saved_when_another_pin_cannot_be_understood(cx: &mut TestAppContext) {
        init_test(cx);
        let key = storage_key("forget-half", TrustRole::Controllers);
        save_record(cx, &key, &[device("a", 1), device("b", 2)], UNDECODABLE_PIN).await;
        let mut store = cx
            .update(|cx| TrustStore::load("forget-half".into(), cx))
            .await;
        assert!(store.is_unreadable());

        assert_eq!(cx.update(|cx| store.try_forget("a", cx)), Ok(true));
        store.flush_checked().await.unwrap();

        assert_eq!(
            saved_ids(cx, &key),
            ["b"],
            "the forgotten device must not come back at the next start"
        );
        let reloaded = cx
            .update(|cx| TrustStore::load("forget-half".into(), cx))
            .await;
        assert_eq!(reloaded.devices(), &[device("b", 2)]);
    }

    #[gpui::test]
    async fn forgetting_everything_is_saved_when_a_pin_cannot_be_understood(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let key = storage_key("forget-all-half", TrustRole::Hosts);
        save_record(cx, &key, &[device("a", 1), device("b", 2)], UNDECODABLE_PIN).await;
        let mut store = cx
            .update(|cx| TrustStore::load_for("forget-all-half".into(), TrustRole::Hosts, cx))
            .await;

        assert_eq!(cx.update(|cx| store.try_forget_all(cx)), Ok(()));
        store.flush_checked().await.unwrap();

        assert!(saved_ids(cx, &key).is_empty());
        let reloaded = cx
            .update(|cx| TrustStore::load_for("forget-all-half".into(), TrustRole::Hosts, cx))
            .await;
        assert!(reloaded.devices().is_empty());
        assert!(!reloaded.is_unreadable());
    }

    #[gpui::test]
    async fn a_forget_in_a_record_that_cannot_be_read_says_so_and_changes_nothing(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let key = storage_key("forget-unreadable", TrustRole::Controllers);
        kvp.write_kvp(key.clone(), "{not json".into())
            .await
            .unwrap();
        let mut store = cx
            .update(|cx| TrustStore::load("forget-unreadable".into(), cx))
            .await;

        assert_eq!(
            cx.update(|cx| store.try_forget("a", cx)),
            Err(TrustError::Unreadable)
        );
        assert_eq!(
            cx.update(|cx| store.try_forget_all(cx)),
            Err(TrustError::Unreadable)
        );
        assert!(
            !cx.update(|cx| store.forget("a", cx)),
            "the plain forget must not report a success it did not have"
        );
        cx.update(|cx| store.forget_all(cx));
        store.flush_checked().await.unwrap();
        assert_eq!(kvp.read_kvp(&key).unwrap().as_deref(), Some("{not json"));
    }

    #[gpui::test]
    async fn a_forget_is_saved_even_when_the_store_is_dropped_at_once(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("dropped".into());
        cx.update(|cx| {
            store.pin(device("a", 1), cx).unwrap();
            store.pin(device("b", 2), cx).unwrap();
        });
        store.flush().await;

        cx.update(|cx| store.forget("a", cx));
        drop(store);
        wait_for_queued_writes(cx).await;

        let reloaded = cx.update(|cx| TrustStore::load("dropped".into(), cx)).await;
        assert_eq!(reloaded.devices(), &[device("b", 2)]);
    }

    #[gpui::test]
    async fn changes_made_one_after_another_are_saved_in_that_order(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("ordered".into());
        cx.update(|cx| {
            store.pin(device("a", 1), cx).unwrap();
            store.pin(device("b", 2), cx).unwrap();
            store.forget("a", cx);
            store.pin(device("a", 3), cx).unwrap();
            store.forget("b", cx);
        });
        store.flush_checked().await.unwrap();

        let reloaded = cx.update(|cx| TrustStore::load("ordered".into(), cx)).await;
        assert_eq!(reloaded.devices(), &[device("a", 3)]);
    }

    #[gpui::test]
    async fn a_write_that_fails_is_reported_and_leaves_the_forget_in_memory(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let mut store = TrustStore::empty("failing".into());
        cx.update(|cx| store.pin(device("a", 1), cx).unwrap());
        store.flush_checked().await.unwrap();

        let connection = cx.update(|cx| db::AppDatabase::global(cx).clone());
        connection
            .write(|connection| {
                connection
                    .exec("DROP TABLE kv_store")
                    .and_then(|mut statement| statement())
            })
            .await
            .unwrap();

        assert!(cx.update(|cx| store.forget("a", cx)));
        assert!(matches!(
            store.flush_checked().await,
            Err(TrustError::NotSaved(_))
        ));
        assert!(
            matches!(store.flush_checked().await, Err(TrustError::NotSaved(_))),
            "asking again does not forget the failure"
        );
        assert!(store.devices().is_empty());
    }

    #[gpui::test(iterations = 30)]
    async fn changes_made_while_a_flush_is_pending_are_saved_after_it(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("flush-then-change".into());
        cx.update(|cx| store.pin(device("a", 1), cx).unwrap());
        let flushing = store.flush_checked();

        cx.update(|cx| {
            store.pin(device("b", 2), cx).unwrap();
            store.forget("a", cx);
        });
        flushing.await.unwrap();
        store.flush_checked().await.unwrap();

        let reloaded = cx
            .update(|cx| TrustStore::load("flush-then-change".into(), cx))
            .await;
        assert_eq!(
            reloaded.devices(),
            &[device("b", 2)],
            "the last change must be the one that is saved"
        );
    }

    #[gpui::test]
    async fn letting_go_of_a_flush_does_not_cancel_the_write(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty("flush-dropped".into());
        cx.update(|cx| store.pin(device("a", 1), cx).unwrap());
        drop(store.flush());
        wait_for_queued_writes(cx).await;

        let reloaded = cx
            .update(|cx| TrustStore::load("flush-dropped".into(), cx))
            .await;
        assert_eq!(reloaded.devices(), &[device("a", 1)]);
    }

    #[gpui::test]
    async fn a_missing_record_is_a_normal_empty_store(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = cx.update(|cx| TrustStore::load("fresh".into(), cx)).await;
        assert!(!store.is_unreadable());
        cx.update(|cx| store.pin(device("a", 1), cx)).unwrap();
    }

    #[gpui::test]
    async fn the_limit_is_known_before_anything_is_pinned(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = TrustStore::empty_for("limit".into(), TrustRole::Hosts);
        cx.update(|cx| {
            for index in 0..MAX_PINNED_DEVICES {
                store.pin(device(&index.to_string(), 1), cx).unwrap();
            }
        });
        assert_eq!(store.check_can_pin("new"), Err(TrustError::TooMany));
        assert_eq!(store.check_can_pin("0"), Ok(()));
    }
}
