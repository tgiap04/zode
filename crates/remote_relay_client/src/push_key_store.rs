//! The push subscription keys of the devices allowed to control this Zode, so
//! that it can push to them while they are not connected.
//!
//! Kept beside the pins of [`crate::TrustStore`] and only ever for a pinned
//! device: the host prunes what is not pinned when it loads the keys, and
//! forgets a device's keys whenever it forgets the device. Losing them is not
//! a loss of trust, only of pushes until the browser subscribes again, so a
//! record that cannot be read is simply started over.

use std::{collections::BTreeMap, future::Future};

use db::kvp::KeyValueStore;
use futures::{FutureExt as _, channel::oneshot, future::Shared};
use gpui::{App, AppContext as _, Task};
use serde::{Deserialize, Serialize};

use crate::{trust_store::MAX_PINNED_DEVICES, web_push_crypto::PushSubscriptionKeys};

fn storage_key(user_id: &str) -> String {
    format!("remote_control_push_keys:{user_id}")
}

#[derive(Serialize, Deserialize)]
struct StoredKeys {
    p256dh: String,
    auth: String,
}

/// Push keys for one account, by device id. Held in memory, written through on
/// every change, in the order the changes were made.
pub struct PushKeyStore {
    user_id: String,
    keys: BTreeMap<String, PushSubscriptionKeys>,
    /// The saved record could not be read, so it may hold keys not known here:
    /// changes are kept in memory and never written over it.
    unreadable: bool,
    latest_write: Option<Shared<oneshot::Receiver<()>>>,
}

impl PushKeyStore {
    pub fn empty(user_id: String) -> Self {
        Self {
            user_id,
            keys: BTreeMap::new(),
            unreadable: false,
            latest_write: None,
        }
    }

    pub fn load(user_id: String, cx: &App) -> Task<Self> {
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&user_id);
        cx.background_spawn(async move { Self::from_read(user_id, kvp.read_kvp(&key)) })
    }

    /// The store a read of the saved record comes to. A record that cannot be
    /// parsed is started over, but one that could not be read at all may hold
    /// keys this one does not know, so nothing is written over it.
    fn from_read(user_id: String, read: anyhow::Result<Option<String>>) -> Self {
        let mut unreadable = false;
        let stored = match read {
            Ok(Some(raw)) => serde_json::from_str::<BTreeMap<String, StoredKeys>>(&raw)
                .map_err(|error| log::warn!("the push keys could not be parsed: {error}"))
                .unwrap_or_default(),
            Ok(None) => BTreeMap::new(),
            Err(error) => {
                log::warn!("could not read the push keys: {error}");
                unreadable = true;
                BTreeMap::new()
            }
        };
        let keys = stored
            .into_iter()
            .filter_map(|(device_id, stored)| {
                Some((
                    device_id,
                    PushSubscriptionKeys::decode(&stored.p256dh, &stored.auth)?,
                ))
            })
            .take(MAX_PINNED_DEVICES)
            .collect();
        Self {
            user_id,
            keys,
            unreadable,
            latest_write: None,
        }
    }

    pub fn get(&self, device_id: &str) -> Option<&PushSubscriptionKeys> {
        self.keys.get(device_id)
    }

    pub fn device_ids(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(String::as_str)
    }

    /// Keeps `keys` for `device_id`, replacing what it had. `false`, changing
    /// nothing, if as many devices as can be pinned already have keys.
    pub fn set(&mut self, device_id: &str, keys: PushSubscriptionKeys, cx: &App) -> bool {
        if !self.keys.contains_key(device_id) && self.keys.len() >= MAX_PINNED_DEVICES {
            return false;
        }
        if self.keys.get(device_id) == Some(&keys) {
            return true;
        }
        self.keys.insert(device_id.to_string(), keys);
        self.persist(cx);
        true
    }

    /// `true` if `device_id` had keys.
    pub fn forget(&mut self, device_id: &str, cx: &App) -> bool {
        let removed = self.keys.remove(device_id).is_some();
        if removed {
            self.persist(cx);
        }
        removed
    }

    /// Forgets `device_id`'s keys only if they are still `keys`: an answer
    /// about keys the device has since replaced says nothing about the new
    /// ones. `true` if they were forgotten.
    pub fn forget_if_still(
        &mut self,
        device_id: &str,
        keys: &PushSubscriptionKeys,
        cx: &App,
    ) -> bool {
        if self.keys.get(device_id) != Some(keys) {
            return false;
        }
        self.forget(device_id, cx)
    }

    /// Forgets the keys of every device `keep` says no to.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool, cx: &App) {
        let before = self.keys.len();
        self.keys.retain(|device_id, _| keep(device_id));
        if self.keys.len() != before {
            self.persist(cx);
        }
    }

    fn persist(&mut self, cx: &App) {
        if self.unreadable {
            return;
        }
        let stored: BTreeMap<&str, StoredKeys> = self
            .keys
            .iter()
            .map(|(device_id, keys)| {
                let (p256dh, auth) = keys.encode();
                (device_id.as_str(), StoredKeys { p256dh, auth })
            })
            .collect();
        let value = match serde_json::to_string(&stored) {
            Ok(value) => value,
            Err(error) => {
                log::error!("the push keys could not be serialised: {error}");
                return;
            }
        };
        let kvp = KeyValueStore::global(cx);
        let key = storage_key(&self.user_id);
        let (finished, finishing) = oneshot::channel();
        let previous = self.latest_write.replace(finishing.shared());
        // Detached, so that a change made just before remote control is
        // switched off is still saved; each write waits for the one before it.
        cx.background_spawn(async move {
            if let Some(previous) = previous {
                previous.await.ok();
            }
            if let Err(error) = kvp.write_kvp(key, value).await {
                log::error!("the push keys could not be saved: {error:#}");
            }
            if finished.send(()).is_err() {
                log::debug!("nothing was waiting for the push keys to be saved");
            }
        })
        .detach();
    }

    /// Resolves once the latest write made so far has finished.
    pub fn flush(&self) -> impl Future<Output = ()> + use<> {
        let latest_write = self.latest_write.clone();
        async move {
            if let Some(latest_write) = latest_write {
                latest_write.await.ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_push_crypto::test_keys;
    use gpui::TestAppContext;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
    }

    fn keys(seed: u8) -> PushSubscriptionKeys {
        test_keys(seed)
    }

    async fn reload(user_id: &str, cx: &mut TestAppContext) -> PushKeyStore {
        cx.update(|cx| PushKeyStore::load(user_id.into(), cx)).await
    }

    #[gpui::test]
    async fn keys_survive_a_reload_and_a_forget_survives_too(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = PushKeyStore::empty("survive".into());
        cx.update(|cx| {
            assert!(store.set("a", keys(1), cx));
            assert!(store.set("b", keys(2), cx));
            assert!(store.set("a", keys(3), cx), "subscribing again replaces");
        });
        store.flush().await;
        let reloaded = reload("survive", cx).await;
        assert_eq!(reloaded.get("a"), Some(&keys(3)));
        assert_eq!(reloaded.get("b"), Some(&keys(2)));

        cx.update(|cx| {
            assert!(store.forget("a", cx));
            assert!(!store.forget("a", cx), "forgetting twice is a no-op");
        });
        store.flush().await;
        let reloaded = reload("survive", cx).await;
        assert_eq!(reloaded.get("a"), None);
        assert_eq!(reloaded.device_ids().collect::<Vec<_>>(), vec!["b"]);
    }

    #[gpui::test]
    async fn only_the_keys_an_answer_was_about_are_forgotten(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = PushKeyStore::empty("forget-if".into());
        cx.update(|cx| {
            store.set("a", keys(1), cx);
            store.set("a", keys(2), cx);
            assert!(!store.forget_if_still("a", &keys(1), cx));
            assert_eq!(store.get("a"), Some(&keys(2)));
            assert!(store.forget_if_still("a", &keys(2), cx));
            assert_eq!(store.get("a"), None);
        });
    }

    #[gpui::test]
    async fn keys_of_devices_no_longer_pinned_are_pruned(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = PushKeyStore::empty("prune".into());
        cx.update(|cx| {
            store.set("kept", keys(1), cx);
            store.set("unpinned", keys(2), cx);
            store.retain(|device_id| device_id == "kept", cx);
        });
        store.flush().await;
        let reloaded = reload("prune", cx).await;
        assert_eq!(reloaded.device_ids().collect::<Vec<_>>(), vec!["kept"]);
    }

    #[gpui::test]
    async fn accounts_do_not_share_keys_and_a_damaged_record_starts_over(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = PushKeyStore::empty("one".into());
        cx.update(|cx| store.set("a", keys(1), cx));
        store.flush().await;
        assert_eq!(reload("two", cx).await.device_ids().count(), 0);

        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        kvp.write_kvp(storage_key("damaged"), "{not json".into())
            .await
            .unwrap();
        assert_eq!(reload("damaged", cx).await.device_ids().count(), 0);

        kvp.write_kvp(
            storage_key("bad-key"),
            r#"{"a":{"p256dh":"short","auth":"short"}}"#.into(),
        )
        .await
        .unwrap();
        assert_eq!(reload("bad-key", cx).await.get("a"), None);
    }

    #[gpui::test]
    async fn a_record_that_could_not_be_read_is_never_written_over(cx: &mut TestAppContext) {
        init_test(cx);
        let kvp = cx.update(|cx| KeyValueStore::global(cx));
        let key = storage_key("unreadable");
        kvp.write_kvp(key.clone(), "kept as it was".into())
            .await
            .unwrap();

        let mut store = PushKeyStore::from_read(
            "unreadable".into(),
            Err(anyhow::anyhow!("the database is locked")),
        );
        cx.update(|cx| {
            assert!(store.set("a", keys(1), cx), "kept in memory all the same");
            store.forget("a", cx);
            store.set("b", keys(2), cx);
        });
        store.flush().await;
        cx.run_until_parked();
        assert_eq!(store.get("b"), Some(&keys(2)));
        assert_eq!(
            kvp.read_kvp(&key).unwrap().as_deref(),
            Some("kept as it was")
        );
    }

    #[gpui::test]
    async fn no_more_devices_have_keys_than_can_be_pinned(cx: &mut TestAppContext) {
        init_test(cx);
        let mut store = PushKeyStore::empty("bounded".into());
        cx.update(|cx| {
            for index in 0..MAX_PINNED_DEVICES {
                assert!(store.set(&index.to_string(), keys(1), cx));
            }
            assert!(!store.set("one-too-many", keys(1), cx));
            assert!(
                store.set("0", keys(2), cx),
                "a device with keys may replace them"
            );
        });
        assert_eq!(store.device_ids().count(), MAX_PINNED_DEVICES);
    }
}
