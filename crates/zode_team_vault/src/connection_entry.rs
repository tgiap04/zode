use serde::{Deserialize, Serialize};
use zeroize::{Zeroize as _, Zeroizing};

use crate::TeamCryptoError;

/// A shared database connection, as it travels inside a sealed vault entry.
///
/// The shape of `database_ui`'s `ConnectionConfig`, plus the password only
/// when the sharer ticked "Include password". The URL is treated as secret as
/// well: people do paste credentials into them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionEntry {
    pub name: String,
    pub driver: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl ConnectionEntry {
    pub fn to_json(&self) -> Result<Zeroizing<Vec<u8>>, TeamCryptoError> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|error| TeamCryptoError::Malformed(error.to_string()))
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, TeamCryptoError> {
        // The serde error is not forwarded: it can quote the input, and the
        // input is a connection string.
        serde_json::from_slice(bytes)
            .map_err(|_| TeamCryptoError::Malformed("not a shared connection".into()))
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        self.url.zeroize();
        if let Some(password) = self.password.as_mut() {
            password.zeroize();
        }
    }
}

impl std::fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("name", &self.name)
            .field("driver", &self.driver)
            .field("url", &"<redacted>")
            .field("has_password", &self.password.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(password: Option<&str>) -> ConnectionEntry {
        ConnectionEntry {
            name: "staging".into(),
            driver: "postgres".into(),
            url: "postgres://app@db.internal:5432/app".into(),
            password: password.map(Into::into),
        }
    }

    #[test]
    fn a_connection_round_trips_with_and_without_its_password() {
        for entry in [shared(None), shared(Some("hunter2"))] {
            let json = entry.to_json().unwrap();
            assert_eq!(ConnectionEntry::from_json(&json).unwrap(), entry);
        }
    }

    #[test]
    fn no_password_means_no_password_field_on_the_wire() {
        let json = shared(None).to_json().unwrap();
        assert!(!String::from_utf8_lossy(&json).contains("password"));
    }

    #[test]
    fn debug_and_errors_never_show_the_secret_parts() {
        let rendered = format!("{:?}", shared(Some("hunter2")));
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(!rendered.contains("db.internal"), "{rendered}");

        let error = ConnectionEntry::from_json(br#"{"name":1,"url":"postgres://u:hunter2@h"}"#)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("hunter2"), "{error}");
    }
}
