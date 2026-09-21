//! The two `UNUserNotificationCenterDelegate` method bodies, registered as
//! selectors on `GPUIApplicationDelegate` in `platform.rs`. Split out of
//! `notifications.rs` to keep that file under its line budget.

use super::{UN_PRESENTATION_OPTION_BANNER, UN_PRESENTATION_OPTION_SOUND, ns_string_to_string};
use crate::{MacPlatform, platform::get_mac_platform};
use block::Block;
use cocoa::{base::id, foundation::NSUInteger};
use dispatch2::DispatchQueue;
use objc::{
    msg_send,
    runtime::{Object, Sel},
    sel, sel_impl,
};
use std::os::raw::c_void;

/// `userNotificationCenter:willPresentNotification:withCompletionHandler:`.
/// Must call the completion handler with `Banner | Sound`, or the banner
/// never appears while zode is frontmost -- the exact case this feature
/// exists for.
pub(crate) extern "C" fn will_present_notification(
    _this: &mut Object,
    _: Sel,
    _center: id,
    _notification: id,
    completion_handler: id,
) {
    unsafe {
        let block = completion_handler as *mut Block<(NSUInteger,), ()>;
        let options = UN_PRESENTATION_OPTION_BANNER | UN_PRESENTATION_OPTION_SOUND;
        (*block).call((options,));
    }
}

/// `userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:`.
/// Runs on the main thread already, but still hops through the foreground
/// executor's queue before touching the registered callback, since that
/// callback reaches entities (same reentrancy hazard `quit()` and
/// `on_thermal_state_change` already guard against in platform.rs).
pub(crate) extern "C" fn did_receive_notification_response(
    this: &mut Object,
    _: Sel,
    _center: id,
    response: id,
    completion_handler: id,
) {
    let identifier = unsafe {
        let notification: id = msg_send![response, notification];
        let request: id = msg_send![notification, request];
        ns_string_to_string(msg_send![request, identifier])
    };

    unsafe {
        let platform = get_mac_platform(this);
        let context = Box::into_raw(Box::new((platform as *const MacPlatform, identifier)));
        DispatchQueue::main().exec_async_f(context as *mut c_void, invoke_activation_callback);

        let block = completion_handler as *mut Block<(), ()>;
        (*block).call(());
    }

    extern "C" fn invoke_activation_callback(context: *mut c_void) {
        let (platform_ptr, identifier) =
            *unsafe { Box::from_raw(context as *mut (*const MacPlatform, String)) };
        let platform = unsafe { &*platform_ptr };
        let mut lock = platform.0.lock();
        if let Some(mut callback) = lock.on_notification_activated.take() {
            drop(lock);
            callback(identifier);
            platform
                .0
                .lock()
                .on_notification_activated
                .get_or_insert(callback);
        }
    }
}
