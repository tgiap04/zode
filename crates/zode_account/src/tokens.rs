use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Refresh this long before the access token actually expires.
///
/// Without a margin, a request issued at the last moment can arrive after the
/// token it carries has died — the clock advances between deciding to send and
/// the server deciding to trust.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// What the keychain holds for a signed-in account.
///
/// `SystemTime` rather than an `Instant`: this is written to disk and read back
/// in a later process, and a monotonic clock means nothing across a reboot. The
/// cost is that a user who moves their clock backwards gets one unnecessary
/// refresh, which is the cheap direction to be wrong in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: SystemTime,
}

impl StoredTokens {
    pub fn new(access_token: String, refresh_token: String, expires_in: Duration) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at: SystemTime::now() + expires_in,
        }
    }

    /// Whether the access token should be exchanged before being used again.
    pub fn needs_refresh(&self, now: SystemTime) -> bool {
        match self.expires_at.duration_since(now) {
            Ok(remaining) => remaining <= REFRESH_MARGIN,
            // `duration_since` fails when `expires_at` is in the past, which is
            // precisely the case that most needs a refresh.
            Err(_) => true,
        }
    }
}

impl std::fmt::Display for StoredTokens {
    /// Prints the shape, never the values.
    ///
    /// `Debug` is derived because the struct is compared in tests, and a
    /// derived `Debug` will happily print both tokens. This exists so that any
    /// log line reaching for a human-readable form gets the redacted one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "StoredTokens {{ access: <redacted>, refresh: <redacted> }}"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed clock. The base is far enough from the epoch that every offset
    /// a test reaches for stays positive — including the backwards-clock case,
    /// which is the whole point of one of them.
    const BASE_SECS: u64 = 1_000_000;

    fn at(offset_secs: i64) -> SystemTime {
        let secs = BASE_SECS
            .checked_add_signed(offset_secs)
            .expect("test offset moved the clock out of range");
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn tokens_expiring_at(expires_at: SystemTime) -> StoredTokens {
        StoredTokens {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at,
        }
    }

    #[test]
    fn a_token_with_plenty_of_life_left_is_left_alone() {
        let tokens = tokens_expiring_at(at(900));
        assert!(!tokens.needs_refresh(at(0)));
    }

    #[test]
    fn a_token_inside_the_margin_is_refreshed_early() {
        let tokens = tokens_expiring_at(at(30));
        assert!(tokens.needs_refresh(at(0)));
    }

    #[test]
    fn the_margin_boundary_itself_refreshes() {
        let tokens = tokens_expiring_at(at(60));
        assert!(tokens.needs_refresh(at(0)));
    }

    #[test]
    fn an_expired_token_refreshes() {
        let tokens = tokens_expiring_at(at(-1));
        assert!(tokens.needs_refresh(at(0)));
    }

    #[test]
    fn a_clock_that_moved_backwards_refreshes_rather_than_panicking() {
        // `duration_since` returns Err here. The wrong answer would be to
        // subtract and underflow.
        let tokens = tokens_expiring_at(at(-5_000));
        assert!(tokens.needs_refresh(at(0)));
    }

    #[test]
    fn display_never_prints_a_token() {
        let tokens = StoredTokens {
            access_token: "super-secret-access".into(),
            refresh_token: "super-secret-refresh".into(),
            expires_at: at(900),
        };
        let rendered = tokens.to_string();
        assert!(!rendered.contains("super-secret-access"));
        assert!(!rendered.contains("super-secret-refresh"));
    }

    #[test]
    fn round_trips_through_json_so_the_keychain_can_hold_it() {
        let tokens = tokens_expiring_at(at(900));
        let encoded = serde_json::to_vec(&tokens).unwrap();
        let decoded: StoredTokens = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(tokens, decoded);
    }
}
