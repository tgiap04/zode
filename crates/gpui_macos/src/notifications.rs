//! `UNUserNotificationCenter` FFI: the bundle-identity guard, posting a
//! notification, and the two `UNUserNotificationCenterDelegate` method
//! bodies. Registered as selectors on `GPUIApplicationDelegate` in
//! `platform.rs`; this file owns only the bodies.
//!
//! Measured, not assumed (see `macos-probe-results.md`): an unbundled process
//! has a nil `NSBundle.mainBundle.bundleIdentifier`, and
//! `+[UNUserNotificationCenter currentNotificationCenter]` raises an
//! `NSInternalInconsistencyException` for that case *inside a `dispatch_once`
//! callout*, which libdispatch does not let unwind back through -- the
//! process aborts no matter what wraps the call on the Rust side. There is no
//! catchable failure mode here, so `supported()` is not a defensive nicety:
//! every path that can reach the center, including delegate registration in
//! `run()`, must check it first and unconditionally.

mod delegate;

pub(crate) use delegate::{did_receive_notification_response, will_present_notification};

use crate::ns_string;
use block::ConcreteBlock;
use cocoa::{
    base::{BOOL, NO, id, nil},
    foundation::NSUInteger,
};
use gpui::Notification;
use objc::{class, msg_send, sel, sel_impl};
use std::{ffi::CStr, os::raw::c_char, sync::OnceLock};

// `UNAuthorizationOptions` / `UNNotificationPresentationOptions` bitmask
// values. These are `NS_OPTIONS` macros in the SDK headers, not exported
// symbols, so -- same as the `K_IOPM_ASSERTION_LEVEL_ON` pattern in
// platform.rs -- they have to be written out by hand.
const UN_AUTHORIZATION_OPTION_SOUND: NSUInteger = 1 << 1;
const UN_AUTHORIZATION_OPTION_ALERT: NSUInteger = 1 << 2;
const UN_PRESENTATION_OPTION_SOUND: NSUInteger = 1 << 1;
const UN_PRESENTATION_OPTION_BANNER: NSUInteger = 1 << 4;

// `UNUserNotificationCenter` and its content/request classes are resolved at
// runtime via `class!`; this file calls no C symbol the framework exports.
// The block exists only so the framework is actually linked into the binary
// -- without it, `objc_getClass` finds no `UNUserNotificationCenter` and
// every message-send below is undefined behavior.
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

/// Whether this process can ever reach the notification center: a non-nil
/// bundle identifier and a `.app` bundle path. Cached because it cannot
/// change over the process lifetime, and cheap enough that callers never
/// need to cache it themselves.
pub(crate) fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| unsafe {
        let bundle: id = msg_send![class!(NSBundle), mainBundle];
        let bundle_identifier: id = msg_send![bundle, bundleIdentifier];
        if bundle_identifier == nil {
            return false;
        }
        let bundle_path: id = msg_send![bundle, bundlePath];
        if bundle_path == nil {
            return false;
        }
        ns_string_to_string(bundle_path).ends_with(".app")
    })
}

unsafe fn ns_string_to_string(value: id) -> String {
    unsafe {
        let utf8: *const c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}

/// Requests notification authorization the first time any post is attempted,
/// so a user who never runs an agent is never asked for a permission the app
/// has not yet needed. Fire-and-forget: the completion handler is genuinely
/// asynchronous (measured: still pending after 3s while the system prompt is
/// up), so this never blocks the caller.
fn request_authorization_once() {
    static REQUESTED: OnceLock<()> = OnceLock::new();
    REQUESTED.get_or_init(|| unsafe {
        let center: id = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let options = UN_AUTHORIZATION_OPTION_ALERT | UN_AUTHORIZATION_OPTION_SOUND;
        let block = ConcreteBlock::new(move |granted: BOOL, error: id| {
            if granted == NO {
                let reason = if error != nil {
                    ns_string_to_string(msg_send![error, localizedDescription])
                } else {
                    "denied".to_string()
                };
                log::error!(
                    "notification authorization denied ({reason}); enable in System Settings \
                     \u{2192} Notifications to see zode notifications"
                );
            }
        });
        let block = block.copy();
        let _: () =
            msg_send![center, requestAuthorizationWithOptions: options completionHandler: block];
    });
}

/// Posts `notification` to the OS notification center, or does nothing where
/// this process has no bundle identity to post under. The guard is applied
/// here rather than asked of callers, because the nil-bundle case described
/// above aborts the process instead of failing.
pub(crate) fn post(notification: Notification) {
    if !supported() {
        return;
    }

    request_authorization_once();

    unsafe {
        let center: id = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let content: id = msg_send![class!(UNMutableNotificationContent), new];
        let _: () = msg_send![content, setTitle: ns_string(&notification.title)];
        let _: () = msg_send![content, setBody: ns_string(&notification.body)];
        let user_info: id = msg_send![class!(NSDictionary), dictionaryWithObject: ns_string(&notification.id) forKey: ns_string("id")];
        let _: () = msg_send![content, setUserInfo: user_info];
        let request: id = msg_send![
            class!(UNNotificationRequest),
            requestWithIdentifier: ns_string(&notification.id)
            content: content
            trigger: nil
        ];

        let block = ConcreteBlock::new(move |error: id| {
            if error != nil {
                let description = ns_string_to_string(msg_send![error, localizedDescription]);
                log::error!("failed to post notification: {description}");
            }
        });
        let block = block.copy();
        let _: () = msg_send![center, addNotificationRequest: request withCompletionHandler: block];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbundled_test_binary_is_not_supported() {
        // `cargo test` produces a bare binary with no bundle identifier --
        // exactly the case that aborts the process if reached unguarded (see
        // module docs). Asserting `false` here is the regression test for
        // that guard, not a formality.
        assert!(!supported());
    }
}
