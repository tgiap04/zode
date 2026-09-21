//! Asking the operating system to confirm that the person at the keyboard is
//! the account owner.
//!
//! Used before an environment value is revealed on screen or written to disk.
//! It is a second factor for the seconds a laptop is unlocked and unattended —
//! not a replacement for the encryption, which has already done its work by
//! the time anything reaches this crate.
//!
//! **The one rule: never pretend.** Where the platform offers nothing, this
//! crate reports [`Support::Unsupported`] and the caller says so on screen. A
//! dialog that asks for a password this process then ignores is worse than no
//! dialog: it teaches a user to trust a prompt that protects nothing, and it
//! makes the product's security claims false in a way that is easy to
//! demonstrate.

#[cfg(target_os = "macos")]
mod macos;

/// Whether this machine can verify who is using it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// The platform can ask. `method` is the phrase to put in front of a user.
    Available { method: &'static str },
    /// It cannot, and the user is entitled to know why.
    Unsupported { reason: &'static str },
}

impl Support {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// One sentence for the interface, in the user's terms rather than the
    /// platform's.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Available { method } => method,
            Self::Unsupported { reason } => reason,
        }
    }
}

/// What came back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The platform confirmed the person.
    Confirmed,
    /// The person cancelled, or failed the check.
    Declined,
    /// Nothing was asked, because nothing could be.
    ///
    /// Distinct from [`Outcome::Confirmed`] on purpose. A caller that treats
    /// them the same has silently decided that "we could not check" means
    /// "the check passed", which is the decision this type exists to force
    /// into the open.
    Unsupported,
}

/// What this machine can do.
pub fn support() -> Support {
    #[cfg(target_os = "macos")]
    {
        macos::support()
    }
    #[cfg(target_os = "windows")]
    {
        // Windows Hello is reachable through `UserConsentVerifier`, and it is
        // not wired up yet. Reported honestly rather than defaulted to
        // "available" — see the module comment.
        Support::Unsupported {
            reason: "Zode cannot yet verify it is you on Windows",
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // No uniform equivalent exists. polkit is the nearest thing and is not
        // present on every desktop, so a check here would succeed on some
        // machines and silently do nothing on others.
        Support::Unsupported {
            reason: "this system offers no way for Zode to verify it is you",
        }
    }
}

/// Asks, if asking is possible.
///
/// `reason` is shown to the user by the platform and should name the specific
/// thing about to happen — "reveal DATABASE_URL", not "continue".
pub async fn authenticate(reason: &str) -> Outcome {
    #[cfg(target_os = "macos")]
    {
        macos::authenticate(reason).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = reason;
        Outcome::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_platform_that_cannot_ask_never_answers_confirmed() {
        // Only the `Unsupported` branch is driven here. Calling `authenticate`
        // on a Mac that CAN ask opens a real Touch ID dialog, and a test suite
        // that pops a system prompt is a test suite nobody runs — the first
        // draft of this test did exactly that and took three seconds waiting
        // for a human. The prompting path is `asks_the_platform_for_real`
        // below, behind `#[ignore]`.
        if support().is_available() {
            return;
        }
        assert_eq!(
            futures::executor::block_on(authenticate("run the test suite")),
            Outcome::Unsupported,
        );
    }

    /// The real prompt. Run deliberately:
    /// `cargo test -p os_auth -- --ignored --nocapture`
    #[test]
    #[ignore = "opens a system authentication dialog and waits for a person"]
    fn asks_the_platform_for_real() {
        let outcome = futures::executor::block_on(authenticate("run the Zode test suite"));
        println!("support: {:?}\noutcome: {outcome:?}", support());
        assert_ne!(
            outcome,
            Outcome::Unsupported,
            "a platform that says it can ask must not answer Unsupported"
        );
    }

    #[test]
    fn unsupported_is_not_confirmed() {
        // Stated as a test because the whole design rests on callers not
        // collapsing these two.
        assert_ne!(Outcome::Unsupported, Outcome::Confirmed);
    }

    #[test]
    fn every_support_answer_has_something_to_show_a_user() {
        assert!(!support().describe().is_empty());
    }
}
