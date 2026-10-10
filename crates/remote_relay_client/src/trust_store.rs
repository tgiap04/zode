//! The devices this machine has agreed to be controlled by.
//!
//! A pin is the whole of the trust: a session is only ever opened for a device
//! whose public key was compared by a person and written here, and the key in
//! the pin — not whatever the relay says the device's key is — is what the
//! encrypted channel is built from.

use db::kvp::KeyValueStore;
use gpui::{App, AppContext as _, Task};
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

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrustError {
    #[error("too many devices are already trusted")]
    TooMany,
    /// The saved pins could not be read, so writing now would erase them.
    #[error("the saved trusted devices could not be read, so nothing can be added until they can")]
    Unreadable,
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

/// Pins for one account. Held in memory, written through on every change.
pub struct TrustStore {
    user_id: String,
    role: TrustRole,
    pins: Vec<PinnedDevice>,
    /// The saved pins could not be read or understood. They are still in the
    /// database, so nothing is written until the next start can read them.
    unreadable: bool,
    /// The latest write. Replacing it is safe because every write carries the
    /// full list, so a superseded one has nothing the newer lacks.
    pending_write: Option<Task<()>>,
}

impl TrustStore {
    pub fn empty(user_id: String) -> Self {
        Self::empty_for(user_id, TrustRole::Controllers)
    }

    pub fn empty_for(user_id: String, role: TrustRole) -> Self {
        Self {
            user_id,
            role,
            pins: Vec::new(),
            unreadable: false,
            pending_write: None,
        }
    }

    /// Reads the saved pins. A store that cannot be read or parsed trusts
    /// nothing, which is the safe way to be wrong, and refuses to write, so
    /// that the pins it could not read are not erased by the next pairing.
    pub fn load(user_id: String, cx: &App) -> Task<Self> {
        Self::load_for(user_id, TrustRole::Controllers, cx)
    }

    pub fn load_for(user_id: String, role: TrustRole, cx: &App) -> Task<Self> {
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&user_id, role);
        cx.background_spawn(async move {
            let (pins, unreadable) = match kvp.read_kvp(&key) {
                Ok(None) => (Vec::new(), false),
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
                        let lost_some = pins.len() != total;
                        if lost_some {
                            log::warn!("some trusted devices could not be understood");
                        }
                        (pins, lost_some)
                    }
                    Err(error) => {
                        log::warn!("the trusted devices could not be parsed: {error}");
                        (Vec::new(), true)
                    }
                },
                Err(error) => {
                    log::warn!("could not read the trusted devices: {error}");
                    (Vec::new(), true)
                }
            };
            Self {
                user_id,
                role,
                pins,
                unreadable,
                pending_write: None,
            }
        })
    }

    /// Whether the saved pins could not be read. While so, nothing can be
    /// pinned.
    pub fn is_unreadable(&self) -> bool {
        self.unreadable
    }

    /// Whether `device_id` could be pinned now, so a ceremony that ends in a
    /// pin is not run for a pin that cannot be written.
    pub fn check_can_pin(&self, device_id: &str) -> Result<(), TrustError> {
        if self.unreadable {
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

    pub fn forget(&mut self, device_id: &str, cx: &App) -> bool {
        let before = self.pins.len();
        self.pins.retain(|pin| pin.device_id != device_id);
        let removed = self.pins.len() != before;
        if removed {
            self.persist(cx);
        }
        removed
    }

    pub fn forget_all(&mut self, cx: &App) {
        if !self.pins.is_empty() {
            self.pins.clear();
            self.persist(cx);
        }
    }

    fn persist(&mut self, cx: &App) {
        if self.unreadable {
            return;
        }
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
                return;
            }
        };
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&self.user_id, self.role);
        self.pending_write = Some(cx.background_spawn(async move {
            if let Err(error) = kvp.write_kvp(key, value).await {
                log::error!("the trusted devices could not be saved: {error}");
            }
        }));
    }

    /// Resolves once the latest write has reached the database.
    pub fn flush(&mut self) -> Task<()> {
        self.pending_write.take().unwrap_or_else(|| Task::ready(()))
    }
}

impl Drop for TrustStore {
    /// A store dropped right after a change -- remote control switched off the
    /// moment after a device was trusted, or forgotten -- must not lose that
    /// change. Dropping a task cancels it, so the last write is let go on its
    /// own instead.
    fn drop(&mut self) {
        if let Some(write) = self.pending_write.take() {
            write.detach();
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

    #[gpui::test]
    async fn a_pin_that_cannot_be_understood_blocks_writing_the_rest(cx: &mut TestAppContext) {
        init_test(cx);
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let key = storage_key("half-readable", TrustRole::Controllers);
        let stored = r#"[{"device_id":"a","public_key":"!!","name":"A","paired_at":1}]"#;
        kvp.write_kvp(key.clone(), stored.into()).await.unwrap();
        let mut store = cx
            .update(|cx| TrustStore::load("half-readable".into(), cx))
            .await;
        assert!(store.is_unreadable());
        assert_eq!(
            cx.update(|cx| store.pin(device("b", 2), cx)),
            Err(TrustError::Unreadable)
        );
        store.flush().await;
        assert_eq!(kvp.read_kvp(&key).unwrap().as_deref(), Some(stored));
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
