use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};

use crate::{Outcome, Support};

/// Device owner authentication rather than biometrics alone.
///
/// `DeviceOwnerAuthenticationWithBiometrics` refuses outright on a Mac with no
/// Touch ID, which is most desktops and every machine with the lid shut. This
/// policy falls back to the login password, so the check is available
/// everywhere rather than only on the hardware that happens to have a sensor.
const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

fn context() -> Retained<LAContext> {
    // SAFETY: `LAContext` has a plain designated initialiser and no arguments.
    unsafe { LAContext::new() }
}

pub(crate) fn support() -> Support {
    // SAFETY: the policy is a valid constant and the context was just built.
    let usable = unsafe { context().canEvaluatePolicy_error(POLICY) }.is_ok();

    if usable {
        Support::Available {
            method: "Touch ID or your login password",
        }
    } else {
        // Reached when the machine has no biometrics AND no password set, or
        // when the process is not permitted to ask. Either way the honest
        // answer is that nothing will be verified.
        Support::Unsupported {
            reason: "this Mac cannot verify it is you right now",
        }
    }
}

pub(crate) async fn authenticate(reason: &str) -> Outcome {
    if !support().is_available() {
        return Outcome::Unsupported;
    }

    let (sender, receiver) = futures::channel::oneshot::channel::<bool>();
    // The block is handed to AppKit and invoked once, from a thread we do not
    // choose — the framework documents the reply block as having to be
    // sendable, which is why this is an `Arc<Mutex<..>>` and not an `Rc`.
    //
    // A `oneshot::Sender` is consumed by sending, so it lives behind the lock
    // for the block to take out. A second invocation — which must not happen,
    // but is not ours to guarantee — finds `None` and is dropped rather than
    // panicking inside a framework callback.
    let sender = Arc::new(Mutex::new(Some(sender)));

    let block = RcBlock::new({
        let sender = sender.clone();
        move |success: Bool, _error: *mut NSError| {
            let taken = sender.lock().ok().and_then(|mut slot| slot.take());
            if let Some(sender) = taken {
                let _ = sender.send(success.as_bool());
            }
        }
    });

    let localized = NSString::from_str(reason);
    // SAFETY: the policy is valid, the string outlives the call, and the block
    // is retained by AppKit for the duration of the prompt.
    unsafe {
        context().evaluatePolicy_localizedReason_reply(POLICY, &localized, &block);
    }

    match receiver.await {
        Ok(true) => Outcome::Confirmed,
        // A dropped sender means the prompt went away without answering — the
        // app was quit, or the session ended. Treated as a refusal, because
        // the one thing it is not is a confirmation.
        Ok(false) | Err(_) => Outcome::Declined,
    }
}
